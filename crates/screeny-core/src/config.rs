use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    /// Seconds between captures. Clamped to at least 5.
    pub interval_seconds: u64,
    pub format: ImageFormat,
    /// JPEG quality 1-100 (ignored for PNG).
    pub jpeg_quality: u8,
    /// Delete captures older than this many days. None = keep forever.
    pub retention_days: Option<u32>,
    /// Start capturing as soon as the app launches.
    pub start_on_launch: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            interval_seconds: 20,
            format: ImageFormat::Jpeg,
            jpeg_quality: 80,
            retention_days: Some(30),
            start_on_launch: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// Implicit TLS (SMTPS), typically port 465.
    Ssl,
    /// STARTTLS upgrade, typically port 587.
    Starttls,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmailConfig {
    pub enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub security: SmtpSecurity,
    /// SMTP login user. The password lives in the OS keychain, never here.
    pub username: String,
    pub from: String,
    pub to: String,
    /// Screenshots bundled per email. Raise to stay under provider send
    /// limits (e.g. Gmail's ~500 emails/day).
    pub batch_size: u32,
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 465,
            security: SmtpSecurity::Ssl,
            username: String::new(),
            from: String::new(),
            to: String::new(),
            batch_size: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelsConfig {
    pub email: EmailConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub capture: CaptureConfig,
    pub channels: ChannelsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            capture: CaptureConfig::default(),
            channels: ChannelsConfig::default(),
        }
    }
}

impl Config {
    /// Returns a copy with invalid values clamped into range.
    pub fn sanitized(&self) -> Config {
        let capture = CaptureConfig {
            interval_seconds: self.capture.interval_seconds.max(5),
            jpeg_quality: self.capture.jpeg_quality.clamp(1, 100),
            ..self.capture.clone()
        };
        let email = EmailConfig {
            batch_size: self.channels.email.batch_size.clamp(1, 200),
            smtp_host: self.channels.email.smtp_host.trim().to_string(),
            username: self.channels.email.username.trim().to_string(),
            from: self.channels.email.from.trim().to_string(),
            to: self.channels.email.to.trim().to_string(),
            ..self.channels.email.clone()
        };
        Config {
            version: CONFIG_VERSION,
            capture,
            channels: ChannelsConfig { email },
        }
    }

    pub fn load_or_default(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let raw = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&raw).map_err(|e| {
            CoreError::Config(format!("invalid config file {}: {e}", path.display()))
        })?;
        Ok(config.sanitized())
    }

    /// Atomic write: write to a temp file in the same directory, then rename.
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| CoreError::Config("config path has no parent directory".into()))?;
        fs::create_dir_all(parent)?;
        let tmp: PathBuf = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.sanitized())?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = Config {
            version: CONFIG_VERSION,
            capture: CaptureConfig {
                interval_seconds: 45,
                format: ImageFormat::Png,
                jpeg_quality: 90,
                retention_days: None,
                start_on_launch: false,
            },
            channels: ChannelsConfig {
                email: EmailConfig {
                    enabled: true,
                    smtp_host: "mail.example.com".into(),
                    smtp_port: 587,
                    security: SmtpSecurity::Starttls,
                    username: "user".into(),
                    from: "a@example.com".into(),
                    to: "b@example.com".into(),
                    batch_size: 30,
                },
            },
        };
        config.save(&path).unwrap();
        let loaded = Config::load_or_default(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = Config::load_or_default(&dir.path().join("nope.json")).unwrap();
        assert_eq!(loaded, Config::default());
    }

    #[test]
    fn sanitize_clamps_out_of_range_values() {
        let config = Config {
            version: CONFIG_VERSION,
            capture: CaptureConfig {
                interval_seconds: 0,
                jpeg_quality: 250,
                ..CaptureConfig::default()
            },
            channels: ChannelsConfig {
                email: EmailConfig {
                    batch_size: 0,
                    ..EmailConfig::default()
                },
            },
        };
        let clean = config.sanitized();
        assert_eq!(clean.capture.interval_seconds, 5);
        assert_eq!(clean.capture.jpeg_quality, 100);
        assert_eq!(clean.channels.email.batch_size, 1);
    }

    #[test]
    fn unknown_fields_and_partial_config_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"capture": {"interval_seconds": 60}, "future_field": 1}"#,
        )
        .unwrap();
        let loaded = Config::load_or_default(&path).unwrap();
        assert_eq!(loaded.capture.interval_seconds, 60);
        assert_eq!(loaded.capture.format, ImageFormat::Jpeg);
    }
}

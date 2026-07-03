//! SMTP email sink via lettre. Works with any provider: implicit TLS
//! (SMTPS, 465) or STARTTLS (587). Credentials come from the OS keychain.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Local;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::{ContentMode, EmailConfig, SmtpSecurity};
use crate::error::{CoreError, Result};
use crate::secrets::{SecretStore, SMTP_PASSWORD};
use crate::sinks::{DeliveryItem, Sink, SinkKind};

pub struct EmailSink {
    config: EmailConfig,
    secrets: Arc<dyn SecretStore>,
}

impl EmailSink {
    pub fn new(config: EmailConfig, secrets: Arc<dyn SecretStore>) -> EmailSink {
        EmailSink { config, secrets }
    }

    fn err(message: impl Into<String>) -> CoreError {
        CoreError::Delivery {
            sink: "email".into(),
            message: message.into(),
        }
    }

    fn transport(&self, password: String) -> Result<AsyncSmtpTransport<Tokio1Executor>> {
        let builder = match self.config.security {
            SmtpSecurity::Ssl => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.smtp_host)
            }
            SmtpSecurity::Starttls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.smtp_host)
            }
        }
        .map_err(|e| Self::err(format!("smtp setup: {e}")))?;

        Ok(builder
            .port(self.config.smtp_port)
            .credentials(Credentials::new(self.config.username.clone(), password))
            .build())
    }

    async fn password(&self) -> Result<String> {
        let secrets = self.secrets.clone();
        let found = tokio::task::spawn_blocking(move || secrets.get(SMTP_PASSWORD))
            .await
            .map_err(|e| Self::err(format!("keychain task: {e}")))??;
        // Gmail shows app passwords with spaces; the stored value may too.
        found
            .map(|p| p.replace(' ', ""))
            .filter(|p| !p.is_empty())
            .ok_or_else(|| Self::err("no SMTP password saved — set one in Settings"))
    }

    fn validate(&self) -> Result<()> {
        if self.config.smtp_host.is_empty() {
            return Err(Self::err("SMTP host is empty"));
        }
        if self.config.username.is_empty() {
            return Err(Self::err("SMTP username is empty"));
        }
        if self.config.from.is_empty() || self.config.to.is_empty() {
            return Err(Self::err("from/to addresses must be set"));
        }
        Ok(())
    }

    fn body_text(&self, batch: &[DeliveryItem], stamp: &str) -> String {
        let mut body = format!(
            "Automated screen archive from Screeny.\nCaptured: {stamp}\nScreenshots in this batch: {}\n",
            batch.len()
        );
        if self.config.content != ContentMode::Image {
            for item in batch {
                let time = item
                    .capture
                    .taken_at
                    .get(11..19)
                    .unwrap_or(&item.capture.taken_at);
                body.push_str(&format!(
                    "\n===== {} ({}) =====\n",
                    time, item.capture.monitor
                ));
                match &item.analysis {
                    Some(analysis) => {
                        if !analysis.description.is_empty() {
                            body.push_str(&format!("Summary: {}\n", analysis.description));
                        }
                        if !analysis.ocr_text.is_empty() {
                            let mut ocr = analysis.ocr_text.as_str();
                            if ocr.len() > 4000 {
                                let mut cut = 4000;
                                while !ocr.is_char_boundary(cut) {
                                    cut -= 1;
                                }
                                ocr = &ocr[..cut];
                            }
                            body.push_str(&format!("On-screen text:\n{ocr}\n"));
                        }
                    }
                    None => body.push_str("(no AI analysis available for this capture)\n"),
                }
            }
        }
        body
    }

    fn build_message(&self, batch: &[DeliveryItem], images: Vec<Vec<u8>>) -> Result<Message> {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let suffix = if batch.len() > 1 {
            format!(" ({} shots)", batch.len())
        } else {
            String::new()
        };

        let mut multipart =
            MultiPart::mixed().singlepart(SinglePart::plain(self.body_text(batch, &stamp)));

        if self.config.content != ContentMode::Analysis {
            for (item, bytes) in batch.iter().zip(images) {
                let row = &item.capture;
                let filename = Path::new(&row.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("capture_{}.jpg", row.id));
                let mime = if filename.ends_with(".png") {
                    "image/png"
                } else {
                    "image/jpeg"
                };
                let content_type =
                    ContentType::parse(mime).map_err(|e| Self::err(format!("mime: {e}")))?;
                multipart =
                    multipart.singlepart(Attachment::new(filename).body(bytes, content_type));
            }
        }

        Message::builder()
            .from(
                self.config
                    .from
                    .parse()
                    .map_err(|e| Self::err(format!("invalid from address: {e}")))?,
            )
            .to(self
                .config
                .to
                .parse()
                .map_err(|e| Self::err(format!("invalid to address: {e}")))?)
            .subject(format!("Screeny — {stamp}{suffix}"))
            .multipart(multipart)
            .map_err(|e| Self::err(format!("build message: {e}")))
    }
}

#[async_trait]
impl Sink for EmailSink {
    fn kind(&self) -> SinkKind {
        SinkKind::Email
    }

    fn batch_size(&self) -> usize {
        self.config.batch_size.max(1) as usize
    }

    async fn deliver(&self, batch: &[DeliveryItem]) -> Result<()> {
        self.validate()?;
        let mut images = Vec::with_capacity(batch.len());
        if self.config.content != ContentMode::Analysis {
            for item in batch {
                let path = &item.capture.path;
                let bytes = tokio::fs::read(path)
                    .await
                    .map_err(|e| Self::err(format!("read {path}: {e}")))?;
                images.push(bytes);
            }
        }
        let message = self.build_message(batch, images)?;
        let transport = self.transport(self.password().await?)?;
        transport
            .send(message)
            .await
            .map_err(|e| Self::err(format!("smtp send: {e}")))?;
        Ok(())
    }

    async fn test(&self) -> Result<()> {
        self.validate()?;
        let message = Message::builder()
            .from(
                self.config
                    .from
                    .parse()
                    .map_err(|e| Self::err(format!("invalid from address: {e}")))?,
            )
            .to(self
                .config
                .to
                .parse()
                .map_err(|e| Self::err(format!("invalid to address: {e}")))?)
            .subject("Screeny test message")
            .body(String::from(
                "Screeny email delivery is configured correctly. 🎉",
            ))
            .map_err(|e| Self::err(format!("build message: {e}")))?;
        let transport = self.transport(self.password().await?)?;
        transport
            .send(message)
            .await
            .map_err(|e| Self::err(format!("smtp send: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;

    fn sink(config: EmailConfig) -> EmailSink {
        EmailSink::new(config, Arc::new(MemoryStore::default()))
    }

    fn valid_config() -> EmailConfig {
        EmailConfig {
            enabled: true,
            username: "user@example.com".into(),
            from: "user@example.com".into(),
            to: "inbox@example.com".into(),
            ..EmailConfig::default()
        }
    }

    #[test]
    fn validate_rejects_missing_fields() {
        assert!(sink(EmailConfig::default()).validate().is_err());
        assert!(sink(valid_config()).validate().is_ok());
    }

    fn item(id: i64, path: &str, analysis: Option<crate::llm::Analysis>) -> DeliveryItem {
        DeliveryItem {
            capture: crate::store::CaptureRow {
                id,
                taken_at: format!("2026-07-02T10:00:0{id}Z"),
                path: path.into(),
                monitor: "M".into(),
                width: 10,
                height: 10,
                status: "captured".into(),
                description: None,
                delivery_summary: None,
            },
            analysis,
        }
    }

    fn analysis() -> crate::llm::Analysis {
        crate::llm::Analysis {
            model: "moondream".into(),
            ocr_text: "SECRET_OCR_LINE".into(),
            description: "A code editor with tests.".into(),
            latency_ms: 500,
        }
    }

    #[test]
    fn builds_multipart_message_with_attachments() {
        let sink = sink(valid_config());
        let items = vec![item(1, "shot_1.jpg", None), item(2, "shot_2.png", None)];
        let message = sink
            .build_message(&items, vec![vec![1, 2, 3], vec![4, 5, 6]])
            .unwrap();
        let raw = String::from_utf8_lossy(&message.formatted()).into_owned();
        assert!(raw.contains("(2 shots)"));
        assert!(raw.contains("shot_1.jpg"));
        assert!(raw.contains("image/png"));
        // image mode: no analysis text
        assert!(!raw.contains("Summary:"));
    }

    #[test]
    fn analysis_mode_includes_text_and_skips_attachments() {
        let sink = sink(EmailConfig {
            content: ContentMode::Analysis,
            ..valid_config()
        });
        let items = vec![item(1, "shot_1.jpg", Some(analysis()))];
        let message = sink.build_message(&items, vec![]).unwrap();
        let raw = String::from_utf8_lossy(&message.formatted()).into_owned();
        assert!(raw.contains("A code editor with tests."));
        assert!(raw.contains("SECRET_OCR_LINE"));
        assert!(!raw.contains("Content-Disposition: attachment"));
    }

    #[test]
    fn both_mode_includes_text_and_attachments() {
        let sink = sink(EmailConfig {
            content: ContentMode::Both,
            ..valid_config()
        });
        let items = vec![item(1, "shot_1.jpg", Some(analysis()))];
        let message = sink.build_message(&items, vec![vec![7, 7]]).unwrap();
        let raw = String::from_utf8_lossy(&message.formatted()).into_owned();
        assert!(raw.contains("Summary: A code editor with tests."));
        assert!(raw.contains("shot_1.jpg"));
    }

    #[tokio::test]
    async fn deliver_without_password_fails_clearly() {
        let sink = sink(valid_config());
        let err = sink.password().await.unwrap_err().to_string();
        assert!(err.contains("no SMTP password saved"), "got: {err}");
    }
}

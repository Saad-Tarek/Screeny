use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use crate::error::Result;

/// Compute (and create) the on-disk location for a capture taken at `when`:
/// `<root>/captures/YYYY/MM/DD/screen_HHMMSS_mmm.<ext>`
pub fn image_path_for(root: &Path, when: DateTime<Local>, ext: &str) -> Result<PathBuf> {
    let dir = root
        .join("captures")
        .join(when.format("%Y").to_string())
        .join(when.format("%m").to_string())
        .join(when.format("%d").to_string());
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("screen_{}.{ext}", when.format("%H%M%S_%3f"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn builds_dated_path_and_creates_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let when = Local.with_ymd_and_hms(2026, 7, 2, 15, 4, 5).unwrap();
        let path = image_path_for(tmp.path(), when, "jpg").unwrap();
        assert!(path.parent().unwrap().is_dir());
        let s = path.to_string_lossy().replace('\\', "/");
        assert!(s.contains("captures/2026/07/02/"), "got {s}");
        assert!(s.ends_with(".jpg"));
    }
}

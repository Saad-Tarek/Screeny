//! Filesystem/database reconciliation. The archive must not keep referencing
//! images that no longer exist on disk (e.g. the user deleted them manually);
//! stale rows show up as broken thumbnails and dead "latest capture" targets.

use std::path::Path;

use crate::error::Result;
use crate::store::Store;

/// Remove capture rows whose image file is gone from disk. Returns how many
/// rows were removed. Blocking (one `stat` per capture) — call from a
/// blocking-friendly context.
pub fn remove_stale_captures(store: &Store) -> Result<usize> {
    let stale: Vec<i64> = store
        .capture_paths()?
        .into_iter()
        .filter(|(_, path)| !Path::new(path).exists())
        .map(|(id, _)| id)
        .collect();
    store.delete_captures(&stale)?;
    Ok(stale.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::NewCapture;
    use chrono::Utc;

    fn capture(path: &str) -> NewCapture {
        NewCapture {
            taken_at: Utc::now(),
            path: path.into(),
            monitor: "Display 1".into(),
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn removes_rows_for_missing_files_and_keeps_existing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().unwrap();

        let present = dir.path().join("present.jpg");
        std::fs::write(&present, b"jpeg").unwrap();
        store
            .insert_capture(&capture(present.to_str().unwrap()))
            .unwrap();
        store
            .insert_capture(&capture(dir.path().join("deleted.jpg").to_str().unwrap()))
            .unwrap();

        let removed = remove_stale_captures(&store).unwrap();

        assert_eq!(removed, 1);
        let remaining = store.capture_paths().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1, present.to_str().unwrap());
    }

    #[test]
    fn empty_store_is_a_no_op() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(remove_stale_captures(&store).unwrap(), 0);
    }
}

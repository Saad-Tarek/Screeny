use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::store::migrations;

/// Thread-safe wrapper around the SQLite database. Queries are short-lived,
/// so a single mutex-guarded connection is sufficient at Screeny's write rate
/// (a few rows per minute).
pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct NewCapture {
    pub taken_at: DateTime<Utc>,
    pub path: String,
    pub monitor: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureRow {
    pub id: i64,
    pub taken_at: String,
    pub path: String,
    pub monitor: String,
    pub width: u32,
    pub height: u32,
    pub status: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Store> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA synchronous = NORMAL;",
        )?;
        migrations::migrate(&conn)?;
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_capture(&self, new: &NewCapture) -> Result<CaptureRow> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let taken_at = new.taken_at.to_rfc3339();
        conn.execute(
            "INSERT INTO captures (taken_at, path, monitor, width, height, status)
             VALUES (?1, ?2, ?3, ?4, ?5, 'captured')",
            params![taken_at, new.path, new.monitor, new.width, new.height],
        )?;
        let id = conn.last_insert_rowid();
        Ok(CaptureRow {
            id,
            taken_at,
            path: new.path.clone(),
            monitor: new.monitor.clone(),
            width: new.width,
            height: new.height,
            status: "captured".into(),
        })
    }

    /// Newest-first page. Pass `before_id` from the previous page's last row
    /// to paginate.
    pub fn list_captures(&self, limit: u32, before_id: Option<i64>) -> Result<Vec<CaptureRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, taken_at, path, monitor, width, height, status
             FROM captures
             WHERE (?1 IS NULL OR id < ?1)
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![before_id, limit], |r| {
                Ok(CaptureRow {
                    id: r.get(0)?,
                    taken_at: r.get(1)?,
                    path: r.get(2)?,
                    monitor: r.get(3)?,
                    width: r.get(4)?,
                    height: r.get(5)?,
                    status: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn capture_count(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let count = conn.query_row("SELECT COUNT(*) FROM captures", [], |r| r.get(0))?;
        Ok(count)
    }

    /// Delete capture rows older than the cutoff and return the image paths
    /// the caller should remove from disk.
    pub fn prune_older_than(&self, cutoff: DateTime<Utc>) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let cutoff = cutoff.to_rfc3339();
        let mut stmt = conn.prepare("SELECT path FROM captures WHERE taken_at < ?1")?;
        let paths = stmt
            .query_map(params![cutoff], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        conn.execute("DELETE FROM captures WHERE taken_at < ?1", params![cutoff])?;
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn capture_at(taken_at: DateTime<Utc>, path: &str) -> NewCapture {
        NewCapture {
            taken_at,
            path: path.into(),
            monitor: "Display 1".into(),
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn insert_and_list_newest_first() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        for i in 0..5 {
            store
                .insert_capture(&capture_at(now, &format!("shot_{i}.jpg")))
                .unwrap();
        }
        let page = store.list_captures(3, None).unwrap();
        assert_eq!(page.len(), 3);
        assert_eq!(page[0].path, "shot_4.jpg");
        let next = store.list_captures(3, Some(page[2].id)).unwrap();
        assert_eq!(next.len(), 2);
        assert_eq!(next[1].path, "shot_0.jpg");
        assert_eq!(store.capture_count().unwrap(), 5);
    }

    #[test]
    fn prune_removes_old_rows_and_returns_paths() {
        let store = Store::open_in_memory().unwrap();
        let now = Utc::now();
        store
            .insert_capture(&capture_at(now - Duration::days(40), "old.jpg"))
            .unwrap();
        store.insert_capture(&capture_at(now, "new.jpg")).unwrap();

        let removed = store.prune_older_than(now - Duration::days(30)).unwrap();
        assert_eq!(removed, vec!["old.jpg".to_string()]);
        assert_eq!(store.capture_count().unwrap(), 1);
    }
}

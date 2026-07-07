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
    /// AI description when analyzed (populated by list/search queries).
    #[serde(default)]
    pub description: Option<String>,
    /// Latest delivery outcome per sink, e.g. "email:sent,telegram:failed"
    /// (populated by list/search queries).
    #[serde(default)]
    pub delivery_summary: Option<String>,
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
            description: None,
            delivery_summary: None,
        })
    }

    /// Store one capture's analysis and index it for full-text search.
    pub fn insert_analysis(&self, capture_id: i64, analysis: &crate::llm::Analysis) -> Result<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO analyses
                 (capture_id, model, ocr_text, description, latency_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                capture_id,
                analysis.model,
                analysis.ocr_text,
                analysis.description,
                analysis.latency_ms as i64,
                Utc::now().to_rfc3339(),
            ],
        )?;
        tx.execute(
            "INSERT INTO captures_fts (rowid, ocr_text, description) VALUES (?1, ?2, ?3)",
            params![capture_id, analysis.ocr_text, analysis.description],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<CaptureRow> {
        Ok(CaptureRow {
            id: r.get(0)?,
            taken_at: r.get(1)?,
            path: r.get(2)?,
            monitor: r.get(3)?,
            width: r.get(4)?,
            height: r.get(5)?,
            status: r.get(6)?,
            description: r.get(7)?,
            delivery_summary: r.get(8)?,
        })
    }

    /// Correlated subquery selecting the latest delivery status per sink.
    const DELIVERY_SUMMARY_SQL: &'static str = "(
        SELECT group_concat(d.sink || ':' || d.status)
        FROM deliveries d
        WHERE d.capture_id = c.id
          AND d.id = (SELECT MAX(d2.id) FROM deliveries d2
                      WHERE d2.capture_id = c.id AND d2.sink = d.sink)
    )";

    /// Newest-first page. Pass `before_id` from the previous page's last row
    /// to paginate.
    pub fn list_captures(&self, limit: u32, before_id: Option<i64>) -> Result<Vec<CaptureRow>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(&format!(
            "SELECT c.id, c.taken_at, c.path, c.monitor, c.width, c.height, c.status,
                    a.description, {} AS delivery_summary
             FROM captures c
             LEFT JOIN analyses a ON a.capture_id = c.id
             WHERE (?1 IS NULL OR c.id < ?1)
             ORDER BY c.id DESC
             LIMIT ?2",
            Self::DELIVERY_SUMMARY_SQL
        ))?;
        let rows = stmt
            .query_map(params![before_id, limit], Self::row_from)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Full-text search over OCR text + descriptions. Each term is quoted so
    /// user input can't break the FTS5 query syntax.
    pub fn search_captures(&self, query: &str, limit: u32) -> Result<Vec<CaptureRow>> {
        let fts_query = query
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(&format!(
            "SELECT c.id, c.taken_at, c.path, c.monitor, c.width, c.height, c.status,
                    a.description, {} AS delivery_summary
             FROM captures_fts f
             JOIN captures c ON c.id = f.rowid
             LEFT JOIN analyses a ON a.capture_id = c.id
             WHERE captures_fts MATCH ?1
             ORDER BY c.id DESC
             LIMIT ?2",
            Self::DELIVERY_SUMMARY_SQL
        ))?;
        let rows = stmt
            .query_map(params![fts_query, limit], Self::row_from)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn capture_count(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let count = conn.query_row("SELECT COUNT(*) FROM captures", [], |r| r.get(0))?;
        Ok(count)
    }

    /// Record the outcome of one delivery attempt for a batch of captures.
    pub fn record_delivery(
        &self,
        capture_ids: &[i64],
        sink: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO deliveries (capture_id, sink, status, error, attempted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let now = Utc::now().to_rfc3339();
            for id in capture_ids {
                stmt.execute(params![id, sink, status, error, now])?;
            }
        }
        tx.commit()?;
        Ok(())
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

    /// Fetch one capture's stored analysis, if any.
    pub fn get_analysis(&self, capture_id: i64) -> Result<Option<crate::llm::Analysis>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT model, ocr_text, description, latency_ms
             FROM analyses WHERE capture_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![capture_id], |r| {
            Ok(crate::llm::Analysis {
                model: r.get(0)?,
                ocr_text: r.get(1)?,
                description: r.get(2)?,
                latency_ms: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    /// (id, path) of every capture, for filesystem reconciliation.
    pub fn capture_paths(&self) -> Result<Vec<(i64, String)>> {
        let conn = self.conn.lock().expect("store mutex poisoned");
        let mut stmt = conn.prepare("SELECT id, path FROM captures")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Delete the given captures along with their FTS index entries; analyses
    /// and deliveries follow via ON DELETE CASCADE.
    pub fn delete_captures(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().expect("store mutex poisoned");
        let tx = conn.transaction()?;
        {
            // Contentless FTS5 only deletes a row when given the original
            // column values; they live in the analyses table.
            let mut fts = tx.prepare(
                "INSERT INTO captures_fts (captures_fts, rowid, ocr_text, description)
                 SELECT 'delete', capture_id, ocr_text, description
                 FROM analyses WHERE capture_id = ?1",
            )?;
            let mut del = tx.prepare("DELETE FROM captures WHERE id = ?1")?;
            for id in ids {
                fts.execute(params![id])?;
                del.execute(params![id])?;
            }
        }
        tx.commit()?;
        Ok(())
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
    fn analysis_round_trip_and_fts_search() {
        let store = Store::open_in_memory().unwrap();
        let row = store
            .insert_capture(&capture_at(Utc::now(), "a.jpg"))
            .unwrap();
        store
            .insert_analysis(
                row.id,
                &crate::llm::Analysis {
                    model: "moondream".into(),
                    ocr_text: "cargo build --release".into(),
                    description: "A terminal running a Rust build.".into(),
                    latency_ms: 900,
                },
            )
            .unwrap();

        // list_captures surfaces the description
        let listed = store.list_captures(10, None).unwrap();
        assert_eq!(
            listed[0].description.as_deref(),
            Some("A terminal running a Rust build.")
        );

        // search hits OCR text and description; misses unrelated terms
        assert_eq!(store.search_captures("cargo release", 10).unwrap().len(), 1);
        assert_eq!(store.search_captures("terminal", 10).unwrap().len(), 1);
        assert!(store.search_captures("spreadsheet", 10).unwrap().is_empty());
        // hostile input must not break FTS syntax
        assert!(store
            .search_captures("\"unbalanced OR (", 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn delivery_summary_reflects_latest_status_per_sink() {
        let store = Store::open_in_memory().unwrap();
        let row = store
            .insert_capture(&capture_at(Utc::now(), "a.jpg"))
            .unwrap();
        store
            .record_delivery(&[row.id], "email", "failed", Some("boom"))
            .unwrap();
        store
            .record_delivery(&[row.id], "email", "sent", None)
            .unwrap();
        store
            .record_delivery(&[row.id], "telegram", "failed", Some("nope"))
            .unwrap();

        let listed = store.list_captures(1, None).unwrap();
        let summary = listed[0].delivery_summary.as_deref().unwrap();
        assert!(summary.contains("email:sent"), "got {summary}");
        assert!(summary.contains("telegram:failed"), "got {summary}");
        assert!(!summary.contains("email:failed"), "got {summary}");
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

    #[test]
    fn capture_paths_returns_id_path_pairs() {
        let store = Store::open_in_memory().unwrap();
        let a = store
            .insert_capture(&capture_at(Utc::now(), "a.jpg"))
            .unwrap();
        let b = store
            .insert_capture(&capture_at(Utc::now(), "b.jpg"))
            .unwrap();
        let paths = store.capture_paths().unwrap();
        assert_eq!(paths, vec![(a.id, "a.jpg".into()), (b.id, "b.jpg".into())]);
    }

    #[test]
    fn delete_captures_removes_rows_deliveries_and_fts_entries() {
        let store = Store::open_in_memory().unwrap();
        let keep = store
            .insert_capture(&capture_at(Utc::now(), "keep.jpg"))
            .unwrap();
        let gone = store
            .insert_capture(&capture_at(Utc::now(), "gone.jpg"))
            .unwrap();
        for row in [&keep, &gone] {
            store
                .insert_analysis(
                    row.id,
                    &crate::llm::Analysis {
                        model: "m".into(),
                        ocr_text: format!("token_{}", row.id),
                        description: "A screen.".into(),
                        latency_ms: 1,
                    },
                )
                .unwrap();
        }
        store
            .record_delivery(&[gone.id], "email", "sent", None)
            .unwrap();

        store.delete_captures(&[gone.id]).unwrap();

        assert_eq!(store.capture_count().unwrap(), 1);
        let listed = store.list_captures(10, None).unwrap();
        assert_eq!(listed[0].path, "keep.jpg");
        // FTS entry for the deleted capture is gone; the kept one still hits.
        assert!(store
            .search_captures(&format!("token_{}", gone.id), 10)
            .unwrap()
            .is_empty());
        assert_eq!(
            store
                .search_captures(&format!("token_{}", keep.id), 10)
                .unwrap()
                .len(),
            1
        );
        // Deleting nothing is a no-op.
        store.delete_captures(&[]).unwrap();
        assert_eq!(store.capture_count().unwrap(), 1);
    }
}

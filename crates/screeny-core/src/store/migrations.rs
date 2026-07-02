use rusqlite::Connection;

use crate::error::Result;

/// Applies schema migrations based on `PRAGMA user_version`.
pub fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS captures (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                taken_at  TEXT NOT NULL,
                path      TEXT NOT NULL,
                monitor   TEXT NOT NULL,
                width     INTEGER NOT NULL,
                height    INTEGER NOT NULL,
                status    TEXT NOT NULL DEFAULT 'captured'
            );
            CREATE INDEX IF NOT EXISTS idx_captures_taken_at ON captures(taken_at);

            CREATE TABLE IF NOT EXISTS analyses (
                capture_id  INTEGER PRIMARY KEY REFERENCES captures(id) ON DELETE CASCADE,
                model       TEXT NOT NULL,
                ocr_text    TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                latency_ms  INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS deliveries (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                capture_id   INTEGER NOT NULL REFERENCES captures(id) ON DELETE CASCADE,
                sink         TEXT NOT NULL,
                status       TEXT NOT NULL,
                error        TEXT,
                attempted_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_deliveries_capture ON deliveries(capture_id);

            CREATE VIRTUAL TABLE IF NOT EXISTS captures_fts USING fts5(
                ocr_text, description, content=''
            );

            PRAGMA user_version = 1;
            "#,
        )?;
    }
    Ok(())
}

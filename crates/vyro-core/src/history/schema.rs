//! SQLite schema for the history store.
//!
//! Three tiers keyed by `(metric, ts)`: raw 1 Hz samples, 10 s aggregates and 1 min
//! aggregates. `WITHOUT ROWID` keeps rows clustered by metric so range queries and per-metric
//! pruning are primary-key range scans.

use rusqlite::Connection;

pub const VERSION: i32 = 1;

const CREATE: &str = "
CREATE TABLE IF NOT EXISTS metric (
    id  INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS raw (
    metric INTEGER NOT NULL,
    ts     INTEGER NOT NULL,
    v      REAL    NOT NULL,
    PRIMARY KEY (metric, ts)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS agg10s (
    metric INTEGER NOT NULL,
    ts     INTEGER NOT NULL,
    avg    REAL    NOT NULL,
    min    REAL    NOT NULL,
    max    REAL    NOT NULL,
    PRIMARY KEY (metric, ts)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS agg1m (
    metric INTEGER NOT NULL,
    ts     INTEGER NOT NULL,
    avg    REAL    NOT NULL,
    min    REAL    NOT NULL,
    max    REAL    NOT NULL,
    PRIMARY KEY (metric, ts)
) WITHOUT ROWID;
";

/// Applies pragmas and creates tables. A database written by a newer schema is refused
/// rather than guessed at.
pub fn init(conn: &Connection) -> rusqlite::Result<()> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > VERSION {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISMATCH),
            Some(format!(
                "history schema v{version} is newer than supported v{VERSION}"
            )),
        ));
    }
    // auto_vacuum only takes effect before the first table exists.
    conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    let _mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(CREATE)?;
    conn.pragma_update(None, "user_version", VERSION)?;
    Ok(())
}

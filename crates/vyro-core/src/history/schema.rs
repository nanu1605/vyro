//! SQLite schema for the history store.
//!
//! Three tiers keyed by `(metric, ts)`: raw 1 Hz samples, 10 s aggregates and 1 min
//! aggregates. `WITHOUT ROWID` keeps rows clustered by metric so range queries and per-metric
//! pruning are primary-key range scans. `event` keeps health changes and fired alerts (JSON per
//! row, by kind) so they survive a restart.
//!
//! v2 (P15) adds game sessions for the replay: `game_session` (the summary as JSON; `ended` is
//! empty while it runs; `timeline` 0 once its samples are gone), `game_sample` (one row per
//! tick, readings as scaled integers, see `gaming::replay::COLUMNS`) and `game_event` (its
//! markers). A v1 file gains the tables in place.
//!
//! v3 (P19) adds the PC change history: `pc_inventory` (the last inventory seen, as JSON; one
//! row) and `hardware_change` (one row per change, kept by count rather than age). Later 0.4
//! tables join v3 the same way, since `init` creates missing tables on every open: `digest`
//! (P16) keeps a week of minute digests for the intelligence engine (see `intel::digest`).

use rusqlite::Connection;

pub const VERSION: i32 = 3;

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
CREATE TABLE IF NOT EXISTS event (
    id   INTEGER PRIMARY KEY,
    kind TEXT    NOT NULL,
    ts   INTEGER NOT NULL,
    json TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS event_kind_ts ON event(kind, ts);
CREATE TABLE IF NOT EXISTS agg1m (
    metric INTEGER NOT NULL,
    ts     INTEGER NOT NULL,
    avg    REAL    NOT NULL,
    min    REAL    NOT NULL,
    max    REAL    NOT NULL,
    PRIMARY KEY (metric, ts)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS game_session (
    id       INTEGER PRIMARY KEY,
    started  INTEGER NOT NULL,
    ended    INTEGER,
    timeline INTEGER NOT NULL DEFAULT 1,
    json     TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS game_session_started ON game_session(started);
CREATE TABLE IF NOT EXISTS game_sample (
    session      INTEGER NOT NULL,
    t            INTEGER NOT NULL,
    fps          INTEGER,
    fps_min      INTEGER,
    frame_ms     INTEGER,
    frame_max_ms INTEGER,
    cpu_usage    INTEGER,
    gpu_usage    INTEGER,
    cpu_temp     INTEGER,
    gpu_temp     INTEGER,
    cpu_power    INTEGER,
    gpu_power    INTEGER,
    PRIMARY KEY (session, t)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS game_event (
    id      INTEGER PRIMARY KEY,
    session INTEGER NOT NULL,
    t       INTEGER NOT NULL,
    json    TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS game_event_session ON game_event(session, t);
CREATE TABLE IF NOT EXISTS pc_inventory (
    id   INTEGER PRIMARY KEY,
    ts   INTEGER NOT NULL,
    json TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS hardware_change (
    id       INTEGER PRIMARY KEY,
    ts       INTEGER NOT NULL,
    category TEXT    NOT NULL,
    json     TEXT    NOT NULL
);
CREATE TABLE IF NOT EXISTS digest (
    ts   INTEGER PRIMARY KEY,
    json TEXT    NOT NULL
);
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

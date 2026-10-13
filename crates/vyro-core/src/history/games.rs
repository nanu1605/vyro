//! Game sessions for the replay (P15): a row per session (its summary as JSON), one sample per
//! tick and the session's markers. Samples and markers are buffered and written with the
//! history's own batch; the running session's summary is refreshed with each batch, so a crash
//! loses at most one batch of it.

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use super::{Error, History, Result};
use crate::gaming::markers::GameEvent;
use crate::gaming::replay::{self, downsample, GameSample, GameTimeline, COLUMNS};
use crate::gaming::{GameSession, MAX_SESSIONS, MIN_SESSION_SECS};

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))
}

/// A stored session: the columns win over what its JSON says.
fn session(id: i64, ended: Option<i64>, timeline: bool, json: &str) -> Option<GameSession> {
    let mut s: GameSession = serde_json::from_str(json).ok()?;
    s.id = id;
    s.ended = ended;
    s.has_timeline = timeline;
    Some(s)
}

/// Writes buffered samples, markers and the running session's summary (inside `flush`).
pub(super) fn write(
    tx: &Transaction<'_>,
    samples: &[(i64, GameSample)],
    events: &[(i64, GameEvent)],
    summary: Option<&(i64, GameSession)>,
) -> Result<()> {
    if !samples.is_empty() {
        let names: Vec<&str> = COLUMNS.iter().map(|(n, _)| *n).collect();
        let sql = format!(
            "INSERT OR REPLACE INTO game_sample(session, t, {}) VALUES (?1, ?2, {})",
            names.join(", "),
            (3..3 + COLUMNS.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut st = tx.prepare_cached(&sql)?;
        for (session, s) in samples {
            let values = s.values();
            let mut row: Vec<Option<i64>> = vec![Some(*session), Some(s.t)];
            row.extend(
                values
                    .iter()
                    .zip(COLUMNS)
                    .map(|(v, (_, scale))| replay::encode(*v, scale)),
            );
            st.execute(rusqlite::params_from_iter(row))?;
        }
    }
    if !events.is_empty() {
        let mut st =
            tx.prepare_cached("INSERT INTO game_event(session, t, json) VALUES (?1, ?2, ?3)")?;
        for (session, e) in events {
            st.execute(params![session, e.t, to_json(e)?])?;
        }
    }
    if let Some((id, s)) = summary {
        tx.execute(
            "UPDATE game_session SET json = ?2 WHERE id = ?1 AND ended IS NULL",
            params![id, to_json(s)?],
        )?;
    }
    Ok(())
}

/// Keeps the newest [`MAX_SESSIONS`] finished sessions (inside `prune`).
pub(super) fn prune(tx: &Transaction<'_>) -> Result<()> {
    const OLD: &str = "SELECT id FROM game_session WHERE ended IS NOT NULL \
                       ORDER BY started DESC, id DESC LIMIT -1 OFFSET ?1";
    let keep = i64::try_from(MAX_SESSIONS).unwrap_or(i64::MAX);
    tx.execute(
        &format!("DELETE FROM game_sample WHERE session IN ({OLD})"),
        [keep],
    )?;
    tx.execute(
        &format!("DELETE FROM game_event WHERE session IN ({OLD})"),
        [keep],
    )?;
    tx.execute(
        &format!("DELETE FROM game_session WHERE id IN ({OLD})"),
        [keep],
    )?;
    Ok(())
}

/// The finished session with a replay that started first: `(id, started)`.
pub(super) fn oldest_replay(conn: &Connection) -> Result<Option<(i64, i64)>> {
    conn.query_row(
        "SELECT id, started FROM game_session WHERE timeline = 1 AND ended IS NOT NULL \
         ORDER BY started, id LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// Drops a session's samples and markers; its summary stays in the list.
pub(super) fn drop_replay(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM game_sample WHERE session = ?1", [id])?;
    conn.execute("DELETE FROM game_event WHERE session = ?1", [id])?;
    conn.execute("UPDATE game_session SET timeline = 0 WHERE id = ?1", [id])?;
    Ok(())
}

fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM game_sample WHERE session = ?1", [id])?;
    conn.execute("DELETE FROM game_event WHERE session = ?1", [id])?;
    conn.execute("DELETE FROM game_session WHERE id = ?1", [id])?;
    Ok(())
}

impl History {
    /// Starts a session's record; returns its id.
    pub fn game_begin(&mut self, summary: &GameSession) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO game_session(started, json) VALUES (?1, ?2)",
            params![summary.started, to_json(summary)?],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// One tick of session `id`; written with the next batch.
    pub fn game_sample(&mut self, id: i64, sample: GameSample) {
        self.pending_samples.push((id, sample));
    }

    /// A marker of session `id`; written with the next batch.
    pub fn game_event(&mut self, id: i64, event: GameEvent) {
        self.pending_events.push((id, event));
    }

    /// The running session's summary so far; written with the next batch.
    pub fn game_update(&mut self, id: i64, summary: &GameSession) {
        self.pending_summary = Some((id, summary.clone()));
    }

    /// Closes session `id` at `ended` (unix seconds) with its final summary.
    pub fn game_finish(&mut self, id: i64, ended: i64, summary: &GameSession) -> Result<()> {
        if self.pending_summary.as_ref().is_some_and(|(s, _)| *s == id) {
            self.pending_summary = None;
        }
        self.flush()?;
        self.conn.execute(
            "UPDATE game_session SET ended = ?2, json = ?3 WHERE id = ?1",
            params![id, ended, to_json(summary)?],
        )?;
        Ok(())
    }

    /// Forgets session `id` (too short to keep).
    pub fn game_discard(&mut self, id: i64) -> Result<()> {
        self.pending_samples.retain(|(s, _)| *s != id);
        self.pending_events.retain(|(s, _)| *s != id);
        if self.pending_summary.as_ref().is_some_and(|(s, _)| *s == id) {
            self.pending_summary = None;
        }
        let tx = self.conn.transaction()?;
        delete(&tx, id)?;
        tx.commit()
    }

    /// Finished sessions, newest first.
    pub fn game_sessions(&self, limit: usize) -> Result<Vec<GameSession>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, ended, timeline, json FROM game_session WHERE ended IS NOT NULL \
             ORDER BY started DESC, id DESC LIMIT ?1",
        )?;
        let rows = st.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, ended, timeline, json) = row?;
            out.extend(session(id, ended, timeline, &json));
        }
        Ok(out)
    }

    /// Session `id` with its samples (at most `max_points`) and markers; `None` if unknown.
    pub fn game_timeline(&mut self, id: i64, max_points: usize) -> Result<Option<GameTimeline>> {
        self.flush()?;
        let row: Option<(Option<i64>, bool, String)> = self
            .conn
            .query_row(
                "SELECT ended, timeline, json FROM game_session WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some(summary) = row.and_then(|(ended, tl, json)| session(id, ended, tl, &json)) else {
            return Ok(None);
        };
        let names: Vec<&str> = COLUMNS.iter().map(|(n, _)| *n).collect();
        let mut st = self.conn.prepare_cached(&format!(
            "SELECT t, {} FROM game_sample WHERE session = ?1 ORDER BY t",
            names.join(", ")
        ))?;
        let samples = st
            .query_map([id], |r| {
                let mut v = [None; 10];
                for (i, (_, scale)) in COLUMNS.iter().enumerate() {
                    v[i] = r.get::<_, Option<f64>>(i + 1)?.map(|x| x / scale);
                }
                Ok(GameSample::from_values(r.get(0)?, v))
            })?
            .collect::<Result<Vec<_>>>()?;
        let (samples, step_ms) = downsample(samples, max_points);
        let mut st = self
            .conn
            .prepare_cached("SELECT json FROM game_event WHERE session = ?1 ORDER BY t, id")?;
        let events = st
            .query_map([id], |r| r.get::<_, String>(0))?
            .filter_map(|j| j.ok().and_then(|j| serde_json::from_str(&j).ok()))
            .collect();
        Ok(Some(GameTimeline {
            session: summary,
            step_ms,
            samples,
            events,
        }))
    }

    /// Adds sessions logged before the replay (0.2's `games.json`): summaries only.
    pub fn game_import(&mut self, sessions: &[GameSession]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO game_session(started, ended, timeline, json) VALUES (?1, ?2, 0, ?3)",
            )?;
            for s in sessions {
                st.execute(params![s.started, s.started + s.secs, to_json(s)?])?;
            }
        }
        tx.commit()
    }

    /// Closes sessions left open by a crash at their last sample, from the summary of the last
    /// batch; drops those too short to keep.
    pub(super) fn game_close_open(&mut self) -> Result<()> {
        let open: Vec<(i64, i64, String)> = {
            let mut st = self
                .conn
                .prepare("SELECT id, started, json FROM game_session WHERE ended IS NULL")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            rows.collect::<Result<_>>()?
        };
        for (id, started, json) in open {
            let last: Option<i64> = self.conn.query_row(
                "SELECT MAX(t) FROM game_sample WHERE session = ?1",
                [id],
                |r| r.get(0),
            )?;
            match serde_json::from_str::<GameSession>(&json) {
                Ok(s) if s.secs >= MIN_SESSION_SECS => {
                    let ended = started + last.map_or(s.secs, |ms| ms / 1000);
                    self.conn.execute(
                        "UPDATE game_session SET ended = ?2 WHERE id = ?1",
                        params![id, ended],
                    )?;
                    log::info!("gaming: closed {} (cut short)", s.app);
                }
                _ => delete(&self.conn, id)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::HistoryConfig;
    use super::*;
    use crate::gaming::markers::{Marker, Part};

    const T0: i64 = 1_700_000_000;

    fn history() -> History {
        History::open_in_memory(HistoryConfig::default()).unwrap()
    }

    fn summary(app: &str, started: i64, secs: i64) -> GameSession {
        GameSession {
            app: app.into(),
            started,
            secs,
            avg_fps: Some(120.0),
            ..GameSession::default()
        }
    }

    fn sample(t: i64) -> GameSample {
        GameSample {
            t,
            fps: Some(141.6),
            frame_max_ms: Some(9.25),
            gpu_temp: Some(64.0),
            ..GameSample::default()
        }
    }

    /// A finished session of `secs` seconds with a sample per second.
    fn play(h: &mut History, app: &str, started: i64, secs: i64) -> i64 {
        let s = summary(app, started, secs);
        let id = h.game_begin(&s).unwrap();
        for i in 0..secs {
            h.game_sample(id, sample(i * 1000));
        }
        h.game_finish(id, started + secs, &s).unwrap();
        id
    }

    #[test]
    fn a_session_round_trips_with_its_samples_and_markers() {
        let mut h = history();
        let s = summary("Game.exe", T0, 60);
        let id = h.game_begin(&s).unwrap();
        assert!(
            h.game_sessions(10).unwrap().is_empty(),
            "not listed while running"
        );
        for i in 0..60 {
            h.game_sample(id, sample(i * 1000));
        }
        h.game_event(
            id,
            GameEvent {
                t: 12_000,
                marker: Marker::Thermal { part: Part::Gpu },
            },
        );
        h.game_finish(id, T0 + 61, &s).unwrap();

        let list = h.game_sessions(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].id, list[0].ended, list[0].has_timeline),
            (id, Some(T0 + 61), true)
        );
        let tl = h.game_timeline(id, 1500).unwrap().unwrap();
        assert_eq!(tl.session.app, "Game.exe");
        assert_eq!((tl.samples.len(), tl.step_ms), (60, 1000));
        assert_eq!(tl.samples[3], sample(3000));
        assert_eq!(tl.events.len(), 1);
        assert!(h.game_timeline(id + 1, 1500).unwrap().is_none());
    }

    #[test]
    fn samples_wait_for_the_batch_and_a_short_session_is_forgotten() {
        let mut h = history();
        let id = h.game_begin(&summary("Launcher.exe", T0, 0)).unwrap();
        h.game_sample(id, sample(0));
        let rows = |h: &History| -> i64 {
            h.conn
                .query_row("SELECT COUNT(*) FROM game_sample", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(rows(&h), 0, "buffered");
        h.flush().unwrap();
        assert_eq!(rows(&h), 1);
        h.game_sample(id, sample(1000));
        h.game_discard(id).unwrap();
        h.flush().unwrap();
        assert_eq!(rows(&h), 0);
        assert!(h.game_timeline(id, 10).unwrap().is_none());
    }

    #[test]
    fn a_session_cut_short_is_closed_at_its_last_sample() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        let (long, short) = {
            let mut h = History::open(&path, HistoryConfig::default()).unwrap();
            let long = h.game_begin(&summary("Long.exe", T0, 0)).unwrap();
            for i in 0..90 {
                h.game_sample(long, sample(i * 1000));
            }
            h.game_update(long, &summary("Long.exe", T0, 88));
            let short = h.game_begin(&summary("Short.exe", T0, 0)).unwrap();
            h.game_sample(short, sample(0));
            h.flush().unwrap();
            // No finish: the service stops here.
            (long, short)
        };
        let mut h = History::open(&path, HistoryConfig::default()).unwrap();
        let list = h.game_sessions(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].id, list[0].secs), (long, 88));
        assert_eq!(list[0].ended, Some(T0 + 89));
        assert!(h.game_timeline(short, 10).unwrap().is_none());
    }

    #[test]
    fn only_the_newest_hundred_sessions_are_kept() {
        let mut h = history();
        for i in 0..105 {
            play(&mut h, &format!("g{i}.exe"), T0 + i * 100, 2);
        }
        h.prune(T0 + 20_000).unwrap();
        let list = h.game_sessions(1000).unwrap();
        assert_eq!(list.len(), MAX_SESSIONS);
        assert_eq!(list[0].app, "g104.exe");
        assert_eq!(list[99].app, "g5.exe");
        let samples: i64 = h
            .conn
            .query_row("SELECT COUNT(*) FROM game_sample", [], |r| r.get(0))
            .unwrap();
        assert_eq!(samples, 200);
    }

    #[test]
    fn over_the_cap_the_oldest_replay_goes_before_newer_history() {
        let cfg = HistoryConfig {
            max_bytes: 400 * 1024,
            prune_interval_secs: i64::MAX,
            ..HistoryConfig::default()
        };
        let mut h = History::open_in_memory(cfg).unwrap();
        // An old, long session, then half an hour of history for 20 sensors.
        let old = play(&mut h, "Old.exe", T0 - 86_400, 7_200);
        let keys: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
        for t in 0..1800 {
            h.record(T0 + t, keys.iter().map(|k| (k.as_str(), t as f64)))
                .unwrap();
        }
        let new = play(&mut h, "New.exe", T0 + 1800, 600);
        h.flush().unwrap();
        assert!(h.used_bytes().unwrap() > 400 * 1024, "test must start over");

        h.prune(T0 + 2400).unwrap();
        assert!(h.used_bytes().unwrap() <= 400 * 1024);
        let list = h.game_sessions(10).unwrap();
        let by = |id| list.iter().find(|s| s.id == id).unwrap();
        assert!(!by(old).has_timeline, "the oldest replay went first");
        assert_eq!(by(old).app, "Old.exe", "its summary stays");
        assert!(by(new).has_timeline);
    }

    #[test]
    fn imported_sessions_are_summaries_only() {
        let mut h = history();
        h.game_import(&[summary("Old.exe", T0, 3600)]).unwrap();
        let list = h.game_sessions(10).unwrap();
        assert_eq!(list[0].ended, Some(T0 + 3600));
        assert!(!list[0].has_timeline);
    }

    #[test]
    fn readings_are_stored_as_small_integers() {
        let mut h = history();
        let id = play(&mut h, "Game.exe", T0, 1);
        let (fps, frame): (i64, i64) = h
            .conn
            .query_row(
                "SELECT fps, frame_max_ms FROM game_sample WHERE session = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((fps, frame), (1416, 925));
    }
}

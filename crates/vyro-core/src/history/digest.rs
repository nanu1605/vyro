//! Minute digests for the intelligence engine (P16): one row a minute, as JSON, kept as long
//! as the 1-minute history. Buffered and written with the history's own batch.

use rusqlite::{params, Transaction};

use super::{Error, History, Result};
use crate::gaming::markers::GameEvent;
use crate::intel::digest::MinuteDigest;

/// Writes buffered digests (inside `flush`).
pub(super) fn write(tx: &Transaction<'_>, digests: &[MinuteDigest]) -> Result<()> {
    if digests.is_empty() {
        return Ok(());
    }
    let mut st = tx.prepare_cached("INSERT OR REPLACE INTO digest(ts, json) VALUES (?1, ?2)")?;
    for d in digests {
        let json =
            serde_json::to_string(d).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))?;
        st.execute(params![d.ts, json])?;
    }
    Ok(())
}

impl History {
    /// Keeps a closed minute's digest; written with the next batch.
    pub fn add_digest(&mut self, digest: MinuteDigest) {
        self.pending_digests.push(digest);
    }

    /// Digests of the minutes from `from` to `to` (unix seconds, both included), oldest first.
    /// Rows that no longer parse are skipped.
    pub fn digests(&mut self, from: i64, to: i64) -> Result<Vec<MinuteDigest>> {
        self.flush()?;
        let mut st = self
            .conn
            .prepare_cached("SELECT json FROM digest WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts")?;
        let rows = st.query_map(params![from, to], |r| r.get::<_, String>(0))?;
        Ok(rows
            .filter_map(|r| r.ok().and_then(|j| serde_json::from_str(&j).ok()))
            .collect())
    }

    /// A logged session's markers, in order (none once its replay is gone).
    pub fn game_events(&mut self, id: i64) -> Result<Vec<GameEvent>> {
        self.flush()?;
        let mut st = self
            .conn
            .prepare_cached("SELECT json FROM game_event WHERE session = ?1 ORDER BY t, id")?;
        let rows = st.query_map([id], |r| r.get::<_, String>(0))?;
        Ok(rows
            .filter_map(|r| r.ok().and_then(|j| serde_json::from_str(&j).ok()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intel::digest::Busy;
    use crate::profiles::Profile;
    use crate::HistoryConfig;

    const T0: i64 = 1_700_000_000 - 1_700_000_000 % 60;

    fn digest(ts: i64) -> MinuteDigest {
        MinuteDigest {
            ts,
            profile: Some(Profile::Desktop),
            game: None,
            bound: None,
            throttle: Default::default(),
            busy: vec![Busy {
                name: "MsMpEng.exe".into(),
                cpu: 12.5,
                gpu: 0.0,
            }],
        }
    }

    #[test]
    fn keeps_a_week_of_digests_and_reads_a_span() {
        let mut h = History::open_in_memory(HistoryConfig::default()).unwrap();
        for i in 0..5 {
            h.add_digest(digest(T0 + i * 60));
        }
        let got = h.digests(T0 + 60, T0 + 180).unwrap();
        assert_eq!(
            got.iter().map(|d| d.ts).collect::<Vec<_>>(),
            [T0 + 60, T0 + 120, T0 + 180]
        );
        assert_eq!(got[0], digest(T0 + 60));
        // A week later the first ones are gone with the 1-minute history.
        h.prune(T0 + 7 * 86_400 + 90).unwrap();
        let left = h.digests(0, i64::MAX).unwrap();
        assert_eq!(left.first().map(|d| d.ts), Some(T0 + 120));
    }
}

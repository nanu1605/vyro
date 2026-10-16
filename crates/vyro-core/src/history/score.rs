//! Daily Performance Scores (P17): one row a day, kept a year. A result without a score (too
//! little data) is not kept.

use rusqlite::{params, OptionalExtension};

use super::{Error, History, Result};
use crate::score::{PerformanceScore, ScorePoint};

impl History {
    /// Keeps a worked-out score (one with a value).
    pub fn add_score(&mut self, s: &PerformanceScore) -> Result<()> {
        let Some(score) = s.score else {
            return Ok(());
        };
        self.flush()?;
        let json =
            serde_json::to_string(s).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO score(ts, score, formula, json) VALUES (?1, ?2, ?3, ?4)",
            params![s.ts, score, s.formula, json],
        )?;
        Ok(())
    }

    /// Stored scores since `since`, oldest first.
    pub fn scores(&mut self, since: i64) -> Result<Vec<ScorePoint>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT ts, score, formula FROM score WHERE ts >= ?1 ORDER BY ts")?;
        let rows = st.query_map([since], |r| {
            Ok(ScorePoint {
                ts: r.get(0)?,
                score: r.get(1)?,
                formula: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// When the last score was kept.
    pub fn last_score_ts(&mut self) -> Result<Option<i64>> {
        self.conn
            .query_row("SELECT MAX(ts) FROM score", [], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()
            .map(Option::flatten)
    }

    /// The last kept result in full.
    pub fn last_score(&mut self) -> Result<Option<PerformanceScore>> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT json FROM score ORDER BY ts DESC LIMIT 1", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{ScoreBand, FORMULA_VERSION, KEEP_SECS, WINDOW_SECS};
    use crate::HistoryConfig;

    fn result(ts: i64, score: Option<u32>) -> PerformanceScore {
        PerformanceScore {
            ts,
            formula: FORMULA_VERSION,
            score,
            band: score.map(ScoreBand::of),
            coverage: 1.0,
            window_secs: WINDOW_SECS,
            inputs: Vec::new(),
        }
    }

    #[test]
    fn keeps_a_year_of_daily_scores() {
        let mut h = History::open_in_memory(HistoryConfig::default()).unwrap();
        let t0 = 1_800_000_000;
        assert_eq!(h.last_score_ts().unwrap(), None);
        h.add_score(&result(t0, None)).unwrap();
        assert_eq!(h.last_score_ts().unwrap(), None, "no score, nothing kept");
        for d in 0..3 {
            h.add_score(&result(
                t0 + d * 86_400,
                Some(90 + u32::try_from(d).unwrap()),
            ))
            .unwrap();
        }
        let pts = h.scores(0).unwrap();
        assert_eq!(
            pts.iter().map(|p| p.score).collect::<Vec<_>>(),
            [90, 91, 92]
        );
        assert_eq!(h.last_score_ts().unwrap(), Some(t0 + 2 * 86_400));
        assert_eq!(h.last_score().unwrap().unwrap().score, Some(92));
        // A year on, the first is gone.
        h.prune(t0 + KEEP_SECS + 3600).unwrap();
        assert_eq!(h.scores(0).unwrap().len(), 2);
    }
}

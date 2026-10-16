//! Benchmark runs (P20): one `benchmark_runs` row per run (the whole result as JSON) and one
//! `benchmark_metrics` row per metric, for the run list. The newest [`KEEP_RUNS`] are kept.

use rusqlite::{params, OptionalExtension};

use super::{Error, History, Result};
use crate::bench::{BenchMode, BenchRun, RunStatus, RunSummary, SummaryMetric, KEEP_RUNS};

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))
}

fn word<T: serde::Serialize>(v: &T) -> Result<String> {
    Ok(to_json(v)?.trim_matches('"').to_owned())
}

fn parse<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_str(&format!("\"{s}\"")).ok()
}

impl History {
    /// Keeps a run; returns its id. The oldest runs past [`KEEP_RUNS`] go.
    pub fn add_benchmark(&mut self, run: &BenchRun) -> Result<i64> {
        self.flush()?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO benchmark_runs(ts, method, mode, status, busy, errors, json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.ts,
                run.method,
                word(&run.mode)?,
                word(&run.status)?,
                run.busy,
                run.errors.total(),
                to_json(run)?
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut st = tx.prepare_cached(
                "INSERT OR REPLACE INTO benchmark_metrics(run_id, key, value, spread) \
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (_, m) in run.metrics() {
                if m.value.is_finite() {
                    st.execute(params![id, m.key, m.value, m.spread])?;
                }
            }
        }
        let keep = i64::try_from(KEEP_RUNS).unwrap_or(i64::MAX);
        tx.execute(
            "DELETE FROM benchmark_runs WHERE id NOT IN \
             (SELECT id FROM benchmark_runs ORDER BY id DESC LIMIT ?1)",
            params![keep],
        )?;
        tx.execute(
            "DELETE FROM benchmark_metrics WHERE run_id NOT IN (SELECT id FROM benchmark_runs)",
            [],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Every kept run, newest first, with its metrics.
    pub fn benchmark_runs(&mut self) -> Result<Vec<RunSummary>> {
        let mut runs: Vec<RunSummary> = {
            let mut st = self.conn.prepare_cached(
                "SELECT id, ts, method, mode, status, busy, errors FROM benchmark_runs \
                 ORDER BY id DESC",
            )?;
            let rows = st.query_map([], |r| {
                Ok(RunSummary {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    method: r.get(2)?,
                    mode: parse(&r.get::<_, String>(3)?).unwrap_or(BenchMode::Quick),
                    status: parse(&r.get::<_, String>(4)?).unwrap_or(RunStatus::Stopped),
                    busy: r.get(5)?,
                    errors: r.get(6)?,
                    metrics: Vec::new(),
                })
            })?;
            rows.collect::<Result<_>>()?
        };
        let mut st = self.conn.prepare_cached(
            "SELECT run_id, key, value, spread FROM benchmark_metrics ORDER BY run_id, key",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                SummaryMetric {
                    key: r.get(1)?,
                    value: r.get(2)?,
                    spread: r.get(3)?,
                },
            ))
        })?;
        for row in rows {
            let (id, m) = row?;
            if let Some(run) = runs.iter_mut().find(|r| r.id == id) {
                run.metrics.push(m);
            }
        }
        Ok(runs)
    }

    /// One run in full.
    pub fn benchmark_run(&mut self, id: i64) -> Result<Option<BenchRun>> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT json FROM benchmark_runs WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(json
            .and_then(|j| serde_json::from_str::<BenchRun>(&j).ok())
            .map(|mut run| {
                run.id = id;
                run
            }))
    }

    /// Forgets a run; false when there was none.
    pub fn delete_benchmark(&mut self, id: i64) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let n = tx.execute("DELETE FROM benchmark_runs WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM benchmark_metrics WHERE run_id = ?1", [id])?;
        tx.commit()?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{AreaId, AreaResult, BenchErrors, BenchSetup, Better, Metric};
    use crate::HistoryConfig;

    fn run(ts: i64, pts: f64) -> BenchRun {
        BenchRun {
            id: 0,
            ts,
            method: 1,
            mode: BenchMode::Full,
            status: RunStatus::Completed,
            reason: None,
            secs: 780.0,
            busy: false,
            notes: Vec::new(),
            setup: BenchSetup::default(),
            areas: vec![AreaResult {
                metrics: vec![
                    Metric::measured(
                        "cpu.multi",
                        "Points",
                        "pts",
                        Better::Higher,
                        vec![pts, pts + 10.0, pts - 10.0],
                    ),
                    Metric::derived("cpu.scaling", "Scaling", "×", Better::Shown, 7.5),
                ],
                missing: None,
                ..AreaResult::missing(AreaId::CpuMulti, "")
            }],
            errors: BenchErrors {
                calc: 2,
                ..BenchErrors::default()
            },
        }
    }

    #[test]
    fn keeps_runs_whole_and_by_metric() {
        let mut h = History::open_in_memory(HistoryConfig::default()).unwrap();
        let a = h.add_benchmark(&run(1_800_000_000, 40_000.0)).unwrap();
        let b = h.add_benchmark(&run(1_800_086_400, 41_000.0)).unwrap();
        let list = h.benchmark_runs().unwrap();
        assert_eq!(
            list.iter().map(|r| r.id).collect::<Vec<_>>(),
            [b, a],
            "newest first"
        );
        assert_eq!(
            (list[0].mode, list[0].status, list[0].errors),
            (BenchMode::Full, RunStatus::Completed, 2)
        );
        let m = &list[0].metrics;
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].key.as_str(), m[0].value), ("cpu.multi", 41_000.0));
        assert!(m[0].spread.is_some() && m[1].spread.is_none());
        let full = h.benchmark_run(a).unwrap().unwrap();
        assert_eq!((full.id, full.secs), (a, 780.0));
        assert!(h.delete_benchmark(a).unwrap());
        assert!(!h.delete_benchmark(a).unwrap());
        assert_eq!(h.benchmark_run(a).unwrap(), None);
        assert_eq!(h.benchmark_runs().unwrap().len(), 1);
    }

    #[test]
    fn keeps_the_newest_hundred() {
        let mut h = History::open_in_memory(HistoryConfig::default()).unwrap();
        let mut last = 0;
        for i in 0..(KEEP_RUNS as i64 + 5) {
            last = h.add_benchmark(&run(1_800_000_000 + i, 1000.0)).unwrap();
        }
        let list = h.benchmark_runs().unwrap();
        assert_eq!(list.len(), KEEP_RUNS);
        assert_eq!(list[0].id, last);
        let metric_rows: i64 = h
            .conn
            .query_row("SELECT COUNT(*) FROM benchmark_metrics", [], |r| r.get(0))
            .unwrap();
        assert_eq!(metric_rows, 2 * KEEP_RUNS as i64);
    }
}

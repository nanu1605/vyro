//! Diagnostics reports (P21): one `diagnostic_runs` row per report (the whole report as JSON)
//! and one `diagnostic_findings` row per check. The newest [`KEEP_REPORTS`] are kept.

use rusqlite::{params, OptionalExtension};

use super::{Error, History, Result};
use crate::diag::{CheckStatus, DiagReport, DiagSummary, KEEP_REPORTS};

fn word<T: serde::Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)
        .map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))?
        .trim_matches('"')
        .to_owned())
}

impl History {
    /// Keeps a report; returns its id. The oldest past [`KEEP_REPORTS`] go.
    pub fn add_diagnostic(&mut self, r: &DiagReport) -> Result<i64> {
        self.flush()?;
        let json =
            serde_json::to_string(r).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO diagnostic_runs(ts, version, attention, unavailable, json) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                r.ts,
                r.version,
                u32::try_from(r.count(CheckStatus::Attention)).unwrap_or(u32::MAX),
                u32::try_from(r.count(CheckStatus::Unavailable)).unwrap_or(u32::MAX),
                json
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut st = tx.prepare_cached(
                "INSERT OR REPLACE INTO diagnostic_findings(run_id, check_id, status, summary) \
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for c in &r.checks {
                st.execute(params![id, word(&c.id)?, word(&c.status)?, c.summary])?;
            }
        }
        let keep = i64::try_from(KEEP_REPORTS).unwrap_or(i64::MAX);
        tx.execute(
            "DELETE FROM diagnostic_runs WHERE id NOT IN \
             (SELECT id FROM diagnostic_runs ORDER BY id DESC LIMIT ?1)",
            params![keep],
        )?;
        tx.execute(
            "DELETE FROM diagnostic_findings WHERE run_id NOT IN (SELECT id FROM diagnostic_runs)",
            [],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// Every kept report, newest first.
    pub fn diagnostic_runs(&mut self) -> Result<Vec<DiagSummary>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, ts, version, attention, unavailable FROM diagnostic_runs ORDER BY id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(DiagSummary {
                id: r.get(0)?,
                ts: r.get(1)?,
                version: r.get(2)?,
                attention: r.get(3)?,
                unavailable: r.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn diagnostic_run(&mut self, id: i64) -> Result<Option<DiagReport>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM diagnostic_runs WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json
            .and_then(|j| serde_json::from_str::<DiagReport>(&j).ok())
            .map(|mut r| {
                r.id = id;
                r
            }))
    }

    /// Forgets a report; false when there was none.
    pub fn delete_diagnostic(&mut self, id: i64) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let n = tx.execute("DELETE FROM diagnostic_runs WHERE id = ?1", [id])?;
        tx.execute("DELETE FROM diagnostic_findings WHERE run_id = ?1", [id])?;
        tx.commit()?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::{CheckId, DiagCheck, CHECKS_VERSION};
    use crate::HistoryConfig;

    fn report(ts: i64, attention: bool) -> DiagReport {
        DiagReport {
            id: 0,
            ts,
            version: CHECKS_VERSION,
            app_version: "0.4.0".into(),
            engine: "Vyro service 0.4.0".into(),
            pc: Vec::new(),
            checks: CheckId::ALL
                .iter()
                .map(|id| DiagCheck {
                    id: *id,
                    status: if attention && *id == CheckId::Storage {
                        CheckStatus::Attention
                    } else {
                        CheckStatus::Pass
                    },
                    summary: "ok".into(),
                    advice: None,
                    citations: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn keeps_reports_and_their_findings() {
        let mut h = History::open_in_memory(HistoryConfig::default()).unwrap();
        let a = h.add_diagnostic(&report(1_800_000_000, false)).unwrap();
        let b = h.add_diagnostic(&report(1_800_000_600, true)).unwrap();
        let list = h.diagnostic_runs().unwrap();
        assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), [b, a]);
        assert_eq!((list[0].attention, list[1].attention), (1, 0));
        let rows: i64 = h
            .conn
            .query_row(
                "SELECT COUNT(*) FROM diagnostic_findings WHERE run_id = ?1 AND status = 'attention'",
                [b],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(h.diagnostic_run(b).unwrap().unwrap().id, b);
        assert!(h.delete_diagnostic(a).unwrap());
        assert_eq!(h.diagnostic_run(a).unwrap(), None);
        for i in 0..(KEEP_REPORTS as i64 + 3) {
            h.add_diagnostic(&report(1_800_001_000 + i, false)).unwrap();
        }
        assert_eq!(h.diagnostic_runs().unwrap().len(), KEEP_REPORTS);
        let findings: i64 = h
            .conn
            .query_row("SELECT COUNT(*) FROM diagnostic_findings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(findings, (KEEP_REPORTS * CheckId::ALL.len()) as i64);
    }
}

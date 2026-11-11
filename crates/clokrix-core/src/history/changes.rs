//! PC change history (P19): the last inventory seen and the changes found since tracking
//! started. Changes are kept by count (the newest [`MAX_CHANGES`]), never by age, and the size
//! cap leaves them alone: they are a few hundred bytes each.

use rusqlite::{params, OptionalExtension};

use super::{Error, History, Result};
use crate::changes::{self, HardwareChange, Known, PcInventory, MAX_CHANGES};

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))
}

impl History {
    fn known(&self) -> Result<Option<Known>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM pc_inventory ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        // A row that no longer parses starts tracking afresh rather than failing every time.
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    /// Compares `inventory` with the last one seen, stores both the new state and the changes
    /// in one transaction, and returns the changes (with their ids).
    pub fn record_inventory(
        &mut self,
        inventory: PcInventory,
        ts: i64,
    ) -> Result<Vec<HardwareChange>> {
        let prev = self.known()?;
        let (mut found, known) = changes::record(prev, inventory, ts);
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM pc_inventory", [])?;
        tx.execute(
            "INSERT INTO pc_inventory(ts, json) VALUES (?1, ?2)",
            params![ts, to_json(&known)?],
        )?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO hardware_change(ts, category, json) VALUES (?1, ?2, ?3)",
            )?;
            for c in &mut found {
                let category = to_json(&c.category)?;
                st.execute(params![c.ts, category.trim_matches('"'), to_json(c)?])?;
                c.id = tx.last_insert_rowid();
            }
        }
        if !found.is_empty() {
            tx.execute(
                "DELETE FROM hardware_change WHERE id <= (SELECT id FROM hardware_change
                     ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                [MAX_CHANGES],
            )?;
        }
        tx.commit()?;
        Ok(found)
    }

    /// The newest `limit` changes, newest first. Rows that no longer parse are skipped.
    pub fn hardware_changes(&self, limit: i64) -> Result<Vec<HardwareChange>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT id, json FROM hardware_change ORDER BY id DESC LIMIT ?1")?;
        let rows = st.query_map([limit], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        Ok(rows
            .filter_map(|r| {
                let (id, json) = r.ok()?;
                let mut c: HardwareChange = serde_json::from_str(&json).ok()?;
                c.id = id;
                Some(c)
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::changes::{ChangeCategory, ChangeKind, ComponentClass, Field, InventoryItem};
    use crate::history::{History, HistoryConfig};
    use crate::info::LocalOnly;

    use super::*;

    fn inv(driver: &str) -> PcInventory {
        PcInventory {
            items: vec![
                InventoryItem {
                    class: ComponentClass::Gpu,
                    key: "gpu:10de:2d04:00000000#0".into(),
                    serial: None,
                    slot: None,
                    name: "NVIDIA GeForce RTX 5060 Ti".into(),
                    identity: "NVIDIA GeForce RTX 5060 Ti|16384".into(),
                    fields: vec![Field {
                        name: "Driver".into(),
                        value: driver.into(),
                        note: None,
                        category: ChangeCategory::Driver,
                    }],
                },
                InventoryItem {
                    class: ComponentClass::Drive,
                    key: "drive:Samsung SSD 990 PRO 2TB".into(),
                    serial: Some(LocalOnly("S6Z2NJ0W123456".into())),
                    slot: None,
                    name: "Samsung SSD 990 PRO 2TB (1.82 TB, NVMe)".into(),
                    identity: "Samsung SSD 990 PRO 2TB|1863".into(),
                    fields: vec![],
                },
            ],
        }
    }

    #[test]
    fn changes_are_kept_across_reopen_newest_first_without_serials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        {
            let mut h = History::open(&path, HistoryConfig::default()).unwrap();
            let first = h.record_inventory(inv("32.0.15.6636"), 100).unwrap();
            assert_eq!(first.len(), 1);
            assert_eq!(first[0].kind, ChangeKind::Tracking);
            assert!(h
                .record_inventory(inv("32.0.15.6636"), 200)
                .unwrap()
                .is_empty());
        }
        let mut h = History::open(&path, HistoryConfig::default()).unwrap();
        let ch = h.record_inventory(inv("32.0.15.7216"), 300).unwrap();
        assert_eq!(ch.len(), 1);
        assert!(ch[0].id > first_id(&h));
        let all = h.hardware_changes(10).unwrap();
        let kinds: Vec<_> = all.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [ChangeKind::Changed, ChangeKind::Tracking]);
        assert_eq!(all[0].after.as_deref(), Some("32.0.15.7216"));
        assert_eq!(all[0].id, ch[0].id);
        // Nothing stored holds the serial, and the category column is filterable text.
        let dump: Vec<String> = h
            .conn
            .prepare("SELECT category || json FROM hardware_change UNION ALL SELECT json FROM pc_inventory")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(dump.iter().all(|row| !row.contains("S6Z2NJ0W123456")));
        assert!(dump[0].starts_with("hardware{") && dump[1].starts_with("driver{"));
        assert_eq!(
            h.conn
                .query_row("SELECT count(*) FROM pc_inventory", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    fn first_id(h: &History) -> i64 {
        h.conn
            .query_row("SELECT min(id) FROM hardware_change", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn the_oldest_changes_go_past_the_limit_and_pruning_leaves_them() {
        let mut h = History::open_in_memory(HistoryConfig::default()).unwrap();
        for i in 0..(MAX_CHANGES + 5) {
            h.record_inventory(inv(&format!("v{i}")), 1000 + i).unwrap();
        }
        let n: i64 = h
            .conn
            .query_row("SELECT count(*) FROM hardware_change", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, MAX_CHANGES);
        let newest = h.hardware_changes(1).unwrap();
        assert_eq!(
            newest[0].after.as_deref(),
            Some(format!("v{}", MAX_CHANGES + 4).as_str())
        );
        // History retention is by age; the change history is not.
        h.prune(1000 + 400 * 86_400).unwrap();
        assert_eq!(
            h.hardware_changes(MAX_CHANGES).unwrap().len(),
            MAX_CHANGES as usize
        );
    }
}

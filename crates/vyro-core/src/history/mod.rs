//! Durable time-series store for the 5 min – 7 day history ranges (spec §2.11).
//!
//! Readings arrive once per tick. Raw samples and closed 10 s / 1 min buckets are buffered in
//! memory and written in one transaction every `flush_interval_secs`, so the disk sees a short
//! burst every few seconds rather than a write per sample. Each range is served from the
//! coarsest tier that covers it and downsampled to at most [`MAX_POINTS`] points.

mod schema;

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use rusqlite::Error;
pub type Result<T> = rusqlite::Result<T>;

/// Events kept per kind (health changes, fired alerts); older ones are pruned first.
pub const MAX_EVENTS_PER_KIND: i64 = 1000;

/// Upper bound on points returned by [`History::query`].
pub const MAX_POINTS: i64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum HistoryRange {
    #[serde(rename = "5m")]
    M5,
    #[serde(rename = "30m")]
    M30,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "6h")]
    H6,
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "7d")]
    D7,
}

impl HistoryRange {
    pub const ALL: [Self; 6] = [Self::M5, Self::M30, Self::H1, Self::H6, Self::H24, Self::D7];

    pub fn secs(self) -> i64 {
        match self {
            Self::M5 => 5 * 60,
            Self::M30 => 30 * 60,
            Self::H1 => 3600,
            Self::H6 => 6 * 3600,
            Self::H24 => 24 * 3600,
            Self::D7 => 7 * 24 * 3600,
        }
    }

    fn tier(self) -> Tier {
        match self {
            Self::M5 | Self::M30 => Tier::Raw,
            Self::H1 | Self::H6 | Self::H24 => Tier::Agg10s,
            Self::D7 => Tier::Agg1m,
        }
    }

    /// Bucket width for queries: a multiple of the tier's resolution that keeps the series
    /// within [`MAX_POINTS`].
    pub fn step_secs(self) -> i64 {
        let res = self.tier().resolution();
        let needed = (self.secs() + MAX_POINTS - 1) / MAX_POINTS;
        let step = needed.max(res);
        (step + res - 1) / res * res
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Raw,
    Agg10s,
    Agg1m,
}

impl Tier {
    fn resolution(self) -> i64 {
        match self {
            Self::Raw => 1,
            Self::Agg10s => 10,
            Self::Agg1m => 60,
        }
    }

    fn table(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Agg10s => "agg10s",
            Self::Agg1m => "agg1m",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryConfig {
    pub raw_retention_secs: i64,
    pub agg10s_retention_secs: i64,
    pub agg1m_retention_secs: i64,
    /// Cap on live data in the database file; oldest data is dropped first.
    pub max_bytes: u64,
    pub flush_interval_secs: i64,
    pub prune_interval_secs: i64,
}

impl Default for HistoryConfig {
    /// Spec §12.6: raw ≤ 30 min, 10 s avg ≤ 24 h, 1 min avg ≤ 7 days, ≤ 200 MB.
    fn default() -> Self {
        Self {
            raw_retention_secs: 30 * 60,
            agg10s_retention_secs: 24 * 3600,
            agg1m_retention_secs: 7 * 24 * 3600,
            max_bytes: 200 * 1024 * 1024,
            flush_interval_secs: 10,
            prune_interval_secs: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistorySeries {
    pub metric: String,
    pub range: HistoryRange,
    pub step_secs: i64,
    pub points: Vec<HistoryPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryPoint {
    /// Bucket start, unix seconds.
    pub ts: i64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    start: i64,
    sum: f64,
    count: u32,
    min: f64,
    max: f64,
}

impl Bucket {
    fn new(start: i64, v: f64) -> Self {
        Self {
            start,
            sum: v,
            count: 1,
            min: v,
            max: v,
        }
    }

    fn add(&mut self, v: f64) {
        self.sum += v;
        self.count += 1;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
    }

    fn row(&self, metric: i64) -> AggRow {
        AggRow {
            metric,
            ts: self.start,
            avg: self.sum / f64::from(self.count),
            min: self.min,
            max: self.max,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AggRow {
    metric: i64,
    ts: i64,
    avg: f64,
    min: f64,
    max: f64,
}

#[derive(Debug)]
struct MetricState {
    id: i64,
    b10: Option<Bucket>,
    b60: Option<Bucket>,
}

/// Adds `v` to the open bucket, closing it into `out` when `ts` falls in a new bucket.
fn roll(
    slot: &mut Option<Bucket>,
    width: i64,
    metric: i64,
    ts: i64,
    v: f64,
    out: &mut Vec<AggRow>,
) {
    let start = ts.div_euclid(width) * width;
    match slot {
        Some(b) if b.start == start => b.add(v),
        Some(b) => {
            out.push(b.row(metric));
            *slot = Some(Bucket::new(start, v));
        }
        None => *slot = Some(Bucket::new(start, v)),
    }
}

pub struct History {
    conn: Connection,
    cfg: HistoryConfig,
    metrics: HashMap<String, MetricState>,
    pending_raw: Vec<(i64, i64, f64)>,
    pending_10s: Vec<AggRow>,
    pending_1m: Vec<AggRow>,
    last_flush: Option<i64>,
    last_prune: Option<i64>,
}

impl History {
    pub fn open(path: &Path, cfg: HistoryConfig) -> Result<Self> {
        Self::with_connection(Connection::open(path)?, cfg)
    }

    pub fn open_in_memory(cfg: HistoryConfig) -> Result<Self> {
        Self::with_connection(Connection::open_in_memory()?, cfg)
    }

    fn with_connection(conn: Connection, cfg: HistoryConfig) -> Result<Self> {
        schema::init(&conn)?;
        Ok(Self {
            conn,
            cfg,
            metrics: HashMap::new(),
            pending_raw: Vec::new(),
            pending_10s: Vec::new(),
            pending_1m: Vec::new(),
            last_flush: None,
            last_prune: None,
        })
    }

    /// Records one tick of readings taken at `ts` (unix seconds). Flushes and prunes on their
    /// configured cadence.
    pub fn record<'a>(
        &mut self,
        ts: i64,
        values: impl IntoIterator<Item = (&'a str, f64)>,
    ) -> Result<()> {
        for (key, v) in values {
            if !v.is_finite() {
                continue;
            }
            let id = self.metric_id(key)?;
            self.pending_raw.push((id, ts, v));
            if let Some(state) = self.metrics.get_mut(key) {
                roll(&mut state.b10, 10, id, ts, v, &mut self.pending_10s);
                roll(&mut state.b60, 60, id, ts, v, &mut self.pending_1m);
            }
        }
        if ts - *self.last_flush.get_or_insert(ts) >= self.cfg.flush_interval_secs {
            self.flush()?;
            self.last_flush = Some(ts);
        }
        if ts - *self.last_prune.get_or_insert(ts) >= self.cfg.prune_interval_secs {
            self.prune(ts)?;
            self.last_prune = Some(ts);
        }
        Ok(())
    }

    fn metric_id(&mut self, key: &str) -> Result<i64> {
        if let Some(state) = self.metrics.get(key) {
            return Ok(state.id);
        }
        self.conn
            .execute("INSERT OR IGNORE INTO metric(key) VALUES (?1)", [key])?;
        let id = self
            .conn
            .query_row("SELECT id FROM metric WHERE key = ?1", [key], |r| r.get(0))?;
        self.metrics.insert(
            key.to_owned(),
            MetricState {
                id,
                b10: None,
                b60: None,
            },
        );
        Ok(id)
    }

    /// Writes buffered samples and closed buckets. Buffers are cleared even on failure so a
    /// broken disk cannot grow memory without bound.
    pub fn flush(&mut self) -> Result<()> {
        let raw = std::mem::take(&mut self.pending_raw);
        let agg10 = std::mem::take(&mut self.pending_10s);
        let agg60 = std::mem::take(&mut self.pending_1m);
        if raw.is_empty() && agg10.is_empty() && agg60.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut st =
                tx.prepare_cached("INSERT OR REPLACE INTO raw(metric, ts, v) VALUES (?1, ?2, ?3)")?;
            for (metric, ts, v) in &raw {
                st.execute(params![metric, ts, v])?;
            }
            for (tier, rows) in [(Tier::Agg10s, &agg10), (Tier::Agg1m, &agg60)] {
                let sql = format!(
                    "INSERT OR REPLACE INTO {}(metric, ts, avg, min, max) VALUES (?1, ?2, ?3, ?4, ?5)",
                    tier.table()
                );
                let mut st = tx.prepare_cached(&sql)?;
                for r in rows {
                    st.execute(params![r.metric, r.ts, r.avg, r.min, r.max])?;
                }
            }
        }
        tx.commit()
    }

    /// Stores events of one kind (e.g. `"health"`, `"alert"`) right away; they are rare.
    pub fn log_events<T: Serialize>(&mut self, kind: &str, ts: i64, items: &[T]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        {
            let mut st =
                tx.prepare_cached("INSERT INTO event(kind, ts, json) VALUES (?1, ?2, ?3)")?;
            for item in items {
                let json = serde_json::to_string(item)
                    .map_err(|e| Error::ToSqlConversionFailure(Box::new(e)))?;
                st.execute(params![kind, ts, json])?;
            }
        }
        tx.commit()
    }

    /// The newest `limit` events of `kind`, oldest first. Rows that no longer parse are skipped.
    pub fn events<T: for<'de> Deserialize<'de>>(&self, kind: &str, limit: i64) -> Result<Vec<T>> {
        let mut st = self.conn.prepare_cached(
            "SELECT json FROM event WHERE kind = ?1 ORDER BY ts DESC, id DESC LIMIT ?2",
        )?;
        let rows = st.query_map(params![kind, limit], |r| r.get::<_, String>(0))?;
        let mut out: Vec<T> = rows
            .filter_map(|r| r.ok().and_then(|j| serde_json::from_str(&j).ok()))
            .collect();
        out.reverse();
        Ok(out)
    }

    /// Closes all open buckets (partial) and flushes. Call on shutdown.
    pub fn flush_all(&mut self) -> Result<()> {
        for state in self.metrics.values_mut() {
            if let Some(b) = state.b10.take() {
                self.pending_10s.push(b.row(state.id));
            }
            if let Some(b) = state.b60.take() {
                self.pending_1m.push(b.row(state.id));
            }
        }
        self.flush()
    }

    /// Returns `metric` over `range` ending at `now`, bucketed to [`HistoryRange::step_secs`].
    /// Unknown metrics yield an empty series.
    pub fn query(&mut self, metric: &str, range: HistoryRange, now: i64) -> Result<HistorySeries> {
        self.flush()?;
        let step = range.step_secs();
        let mut series = HistorySeries {
            metric: metric.to_owned(),
            range,
            step_secs: step,
            points: Vec::new(),
        };
        let id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM metric WHERE key = ?1", [metric], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(id) = id else {
            return Ok(series);
        };
        let tier = range.tier();
        let sql = match tier {
            Tier::Raw => "SELECT (ts / ?3) * ?3 AS b, AVG(v), MIN(v), MAX(v) FROM raw \
                          WHERE metric = ?1 AND ts > ?2 GROUP BY b ORDER BY b"
                .to_owned(),
            _ => format!(
                "SELECT (ts / ?3) * ?3 AS b, AVG(avg), MIN(min), MAX(max) FROM {} \
                 WHERE metric = ?1 AND ts > ?2 GROUP BY b ORDER BY b",
                tier.table()
            ),
        };
        let mut st = self.conn.prepare_cached(&sql)?;
        let rows = st.query_map(params![id, now - range.secs(), step], |r| {
            Ok(HistoryPoint {
                ts: r.get(0)?,
                avg: r.get(1)?,
                min: r.get(2)?,
                max: r.get(3)?,
            })
        })?;
        series.points = rows.collect::<Result<_>>()?;
        Ok(series)
    }

    /// Drops data past each tier's retention, then enforces the size cap.
    pub fn prune(&mut self, now: i64) -> Result<()> {
        let tiers = [
            (Tier::Raw, self.cfg.raw_retention_secs),
            (Tier::Agg10s, self.cfg.agg10s_retention_secs),
            (Tier::Agg1m, self.cfg.agg1m_retention_secs),
        ];
        let ids: Vec<i64> = self.metrics.values().map(|m| m.id).collect();
        let tx = self.conn.transaction()?;
        for (tier, retention) in tiers {
            let sql = format!(
                "DELETE FROM {} WHERE metric = ?1 AND ts <= ?2",
                tier.table()
            );
            let mut st = tx.prepare_cached(&sql)?;
            // Per-metric deletes are primary-key range scans; no separate ts index needed.
            for id in &ids {
                st.execute(params![id, now - retention])?;
            }
        }
        // Events: same 7-day window as the longest tier, and a cap per kind.
        tx.execute(
            "DELETE FROM event WHERE ts <= ?1",
            params![now - self.cfg.agg1m_retention_secs],
        )?;
        tx.execute(
            "DELETE FROM event WHERE id IN (SELECT id FROM (SELECT id, ROW_NUMBER() OVER \
             (PARTITION BY kind ORDER BY ts DESC, id DESC) AS n FROM event) WHERE n > ?1)",
            params![MAX_EVENTS_PER_KIND],
        )?;
        tx.commit()?;
        self.enforce_size_cap()
    }

    /// Bytes held by live pages (excludes free pages awaiting vacuum).
    pub fn used_bytes(&self) -> Result<u64> {
        let page_size: i64 = self
            .conn
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        let pages: i64 = self
            .conn
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let free: i64 = self
            .conn
            .pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        Ok(u64::try_from(page_size * (pages - free)).unwrap_or(0))
    }

    fn enforce_size_cap(&mut self) -> Result<()> {
        // Trim the oldest eighth of the coarsest non-empty tier until under the cap. The coarse
        // tiers hold the oldest data, so this drops history oldest-first.
        let mut trimmed = false;
        for _ in 0..256 {
            if self.used_bytes()? <= self.cfg.max_bytes {
                break;
            }
            let mut deleted = false;
            for tier in [Tier::Agg1m, Tier::Agg10s, Tier::Raw] {
                let table = tier.table();
                let span: (Option<i64>, Option<i64>) = self.conn.query_row(
                    &format!("SELECT MIN(ts), MAX(ts) FROM {table}"),
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if let (Some(oldest), Some(newest)) = span {
                    let slice = ((newest - oldest) / 8).max(tier.resolution());
                    self.conn.execute(
                        &format!("DELETE FROM {table} WHERE ts < ?1"),
                        [oldest + slice],
                    )?;
                    deleted = true;
                    break;
                }
            }
            if !deleted {
                break;
            }
            trimmed = true;
        }
        if trimmed {
            self.conn.execute_batch("PRAGMA incremental_vacuum;")?;
        }
        Ok(())
    }
}

impl Drop for History {
    fn drop(&mut self) {
        if let Err(e) = self.flush_all() {
            log::warn!("history: final flush failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_700_000_000; // multiple of 60 → bucket-aligned

    fn history() -> History {
        History::open_in_memory(HistoryConfig::default()).unwrap()
    }

    #[test]
    fn events_persist_newest_last_and_are_pruned() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct E {
            n: i64,
        }
        let mut h = history();
        h.log_events("health", T0, &[E { n: 1 }, E { n: 2 }])
            .unwrap();
        h.log_events("alert", T0 + 1, &[E { n: 9 }]).unwrap();
        h.log_events("health", T0 + 2, &[E { n: 3 }]).unwrap();
        assert_eq!(
            h.events::<E>("health", 2).unwrap(),
            [E { n: 2 }, E { n: 3 }]
        );
        assert_eq!(h.events::<E>("alert", 10).unwrap(), [E { n: 9 }]);
        let many: Vec<E> = (0..MAX_EVENTS_PER_KIND + 5).map(|n| E { n }).collect();
        h.log_events("alert", T0 + 3, &many).unwrap();
        h.prune(T0 + 10).unwrap();
        assert_eq!(
            h.events::<E>("alert", 10_000).unwrap().len(),
            usize::try_from(MAX_EVENTS_PER_KIND).unwrap()
        );
        h.prune(T0 + 8 * 24 * 3600).unwrap();
        assert!(
            h.events::<E>("health", 10).unwrap().is_empty(),
            "past 7 days"
        );
    }

    #[test]
    fn step_keeps_every_range_within_max_points_and_aligned_to_tier() {
        for range in HistoryRange::ALL {
            let step = range.step_secs();
            assert!(range.secs() / step <= MAX_POINTS, "{range:?}");
            assert_eq!(step % range.tier().resolution(), 0, "{range:?}");
        }
        assert_eq!(HistoryRange::M5.step_secs(), 1);
        assert_eq!(HistoryRange::H1.step_secs(), 10);
        assert_eq!(HistoryRange::D7.step_secs(), 1020);
    }

    #[test]
    fn raw_samples_round_trip() {
        let mut h = history();
        for i in 0..120 {
            h.record(T0 + i, [("cpu.usage", i as f64)]).unwrap();
        }
        let s = h.query("cpu.usage", HistoryRange::M5, T0 + 119).unwrap();
        assert_eq!(s.step_secs, 1);
        assert_eq!(s.points.len(), 120);
        assert_eq!(s.points[0].ts, T0);
        assert_eq!(s.points[119].avg, 119.0);
    }

    #[test]
    fn ten_second_buckets_aggregate_avg_min_max() {
        let mut h = history();
        // Two full 10 s buckets of 0..9 and 10..19, then one sample to close the second.
        for i in 0..21 {
            h.record(T0 + i, [("gpu.temp", i as f64)]).unwrap();
        }
        let s = h.query("gpu.temp", HistoryRange::H1, T0 + 20).unwrap();
        assert_eq!(s.step_secs, 10);
        assert_eq!(s.points.len(), 2);
        let p = s.points[1];
        assert_eq!((p.ts, p.avg, p.min, p.max), (T0 + 10, 14.5, 10.0, 19.0));
    }

    #[test]
    fn minute_buckets_serve_seven_day_range_and_flush_all_writes_partial() {
        let mut h = history();
        for i in 0..90 {
            h.record(T0 + i, [("power", 100.0 + (i % 2) as f64)])
                .unwrap();
        }
        h.flush_all().unwrap();
        let s = h.query("power", HistoryRange::D7, T0 + 90).unwrap();
        assert_eq!(
            s.points.len(),
            1,
            "both minute buckets fall in one 1020 s step"
        );
        assert_eq!(s.points[0].min, 100.0);
        assert_eq!(s.points[0].max, 101.0);
    }

    #[test]
    fn metrics_are_isolated_and_unknown_metric_is_empty() {
        let mut h = history();
        h.record(T0, [("a", 1.0), ("b", 2.0)]).unwrap();
        assert_eq!(
            h.query("a", HistoryRange::M5, T0).unwrap().points[0].avg,
            1.0
        );
        assert_eq!(
            h.query("b", HistoryRange::M5, T0).unwrap().points[0].avg,
            2.0
        );
        assert!(h
            .query("nope", HistoryRange::M5, T0)
            .unwrap()
            .points
            .is_empty());
    }

    #[test]
    fn non_finite_values_are_skipped() {
        let mut h = history();
        h.record(T0, [("a", f64::NAN), ("a", f64::INFINITY)])
            .unwrap();
        assert!(h
            .query("a", HistoryRange::M5, T0)
            .unwrap()
            .points
            .is_empty());
    }

    #[test]
    fn prune_drops_data_past_retention() {
        let cfg = HistoryConfig {
            raw_retention_secs: 60,
            ..HistoryConfig::default()
        };
        let mut h = History::open_in_memory(cfg).unwrap();
        for i in 0..300 {
            h.record(T0 + i, [("a", 1.0)]).unwrap();
        }
        h.flush().unwrap();
        h.prune(T0 + 299).unwrap();
        let s = h.query("a", HistoryRange::M5, T0 + 299).unwrap();
        assert_eq!(s.points.len(), 60);
        assert_eq!(s.points[0].ts, T0 + 240);
    }

    #[test]
    fn size_cap_trims_oldest_data_first() {
        let cfg = HistoryConfig {
            max_bytes: 160 * 1024,
            prune_interval_secs: i64::MAX,
            ..HistoryConfig::default()
        };
        let mut h = History::open_in_memory(cfg).unwrap();
        let keys: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
        for t in 0..1800 {
            h.record(T0 + t, keys.iter().map(|k| (k.as_str(), t as f64)))
                .unwrap();
        }
        h.flush().unwrap();
        assert!(
            h.used_bytes().unwrap() > 160 * 1024,
            "test must start over the cap"
        );

        h.prune(T0 + 1799).unwrap();
        assert!(h.used_bytes().unwrap() <= 160 * 1024);
        let s = h.query("m0", HistoryRange::M30, T0 + 1799).unwrap();
        let newest = s.points.last().unwrap();
        assert!(newest.ts >= T0 + 1797, "newest data survives");
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        {
            let mut h = History::open(&path, HistoryConfig::default()).unwrap();
            for i in 0..5 {
                h.record(T0 + i, [("a", i as f64)]).unwrap();
            }
        } // Drop flushes.
        let mut h = History::open(&path, HistoryConfig::default()).unwrap();
        assert_eq!(
            h.query("a", HistoryRange::M5, T0 + 5).unwrap().points.len(),
            5
        );
    }
}

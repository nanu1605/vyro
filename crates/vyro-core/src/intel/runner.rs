//! The intelligence engine inside the engine: builds the minute digest from the poller's
//! ticks, works out the findings on demand (kept a minute) and in the background every ten
//! minutes on a short-lived thread, and hands the app the keys of the findings worth a look.
//! The same pass works out the usual readings "above usual" alert rules compare with (P18).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use super::digest::{DigestBuilder, MinuteDigest};
use super::{analyze, wanted, Inputs, InsightBrief, Insights, SeriesMap};
use crate::alerts::usual::{load_of, usual, Usual};
use crate::changes::MAX_CHANGES;
use crate::fan::{self, FanEvent};
use crate::gaming::MAX_SESSIONS;
use crate::history::History;
use crate::profiles::Profile;
use crate::registry::{Registry, Unit};
use crate::snapshot::SensorSnapshot;
use crate::{lock, HistoryRange};

/// Findings asked for within this long of the last pass are the same findings.
pub const CACHE_SECS: i64 = 60;
/// A background pass runs at most this often…
pub const BACKGROUND_SECS: i64 = 600;
/// …the first one this long after start, once the PC has settled.
const FIRST_PASS_SECS: i64 = 120;
/// Game sessions whose markers are read for background load.
const MARKED_SESSIONS: usize = 5;

pub struct Intel {
    registry: Arc<Mutex<Registry>>,
    history: Option<Arc<Mutex<History>>>,
    digest: Mutex<DigestBuilder>,
    profile: Mutex<Option<Profile>>,
    latest: Mutex<Option<Insights>>,
    brief: Mutex<InsightBrief>,
    busy: AtomicBool,
    /// When the last pass started (unix seconds).
    last_pass: AtomicI64,
    /// Sensors alert rules compare with their usual reading, and those readings.
    alert_sensors: Mutex<Vec<String>>,
    usual: Mutex<HashMap<String, Usual>>,
}

impl Intel {
    pub fn new(
        registry: Arc<Mutex<Registry>>,
        history: Option<Arc<Mutex<History>>>,
        now: i64,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            history,
            digest: Mutex::new(DigestBuilder::new()),
            profile: Mutex::new(None),
            latest: Mutex::new(None),
            brief: Mutex::new(InsightBrief::default()),
            busy: AtomicBool::new(false),
            last_pass: AtomicI64::new(now - BACKGROUND_SECS + FIRST_PASS_SECS),
            alert_sensors: Mutex::new(Vec::new()),
            usual: Mutex::new(HashMap::new()),
        })
    }

    /// The profile the app has put in force.
    pub fn set_profile(&self, profile: Profile) {
        *lock(&self.profile) = Some(profile);
    }

    /// Every tick, on the poller thread: adds the snapshot to the minute digest (returning
    /// a minute once it closes, for the history store) and starts a background pass when one
    /// is due.
    pub fn tick(self: &Arc<Self>, snap: &SensorSnapshot) -> Option<MinuteDigest> {
        let profile = *lock(&self.profile);
        let closed = lock(&self.digest).add(snap, profile);
        let due = snap.ts - self.last_pass.load(Ordering::Relaxed) >= BACKGROUND_SECS;
        if due && self.history.is_some() && !self.busy.swap(true, Ordering::AcqRel) {
            self.last_pass.store(snap.ts, Ordering::Relaxed);
            let me = Arc::clone(self);
            let now = snap.ts;
            let spawned = std::thread::Builder::new()
                .name("vyro-intel".into())
                .spawn(move || {
                    match me.compute(now) {
                        Ok(i) => me.keep(i),
                        Err(e) => log::warn!("insights: {e}"),
                    }
                    me.busy.store(false, Ordering::Release);
                });
            if let Err(e) = spawned {
                log::warn!("insights: no thread: {e}");
                self.busy.store(false, Ordering::Release);
            }
        }
        closed
    }

    /// The sensors alert rules compare with their usual reading. New ones are worked out
    /// right away (a few week queries); the rest keep theirs until the next pass.
    pub fn set_alert_sensors(&self, ids: Vec<String>, now: i64) {
        let new: Vec<String> = {
            let known = lock(&self.usual);
            ids.iter()
                .filter(|id| !known.contains_key(*id))
                .cloned()
                .collect()
        };
        *lock(&self.alert_sensors) = ids.clone();
        let mut fresh = HashMap::new();
        if !new.is_empty() && self.history.is_some() {
            match self.compute_usual(&new, now) {
                Ok(u) => fresh = u,
                Err(e) => log::warn!("alerts: usual readings: {e}"),
            }
        }
        let mut usual = lock(&self.usual);
        usual.retain(|id, _| ids.contains(id));
        usual.extend(fresh);
    }

    /// `sensor`'s usual reading now, as of the last pass; `read` gives the current load.
    pub fn usual_now(&self, sensor: &str, read: impl Fn(&str) -> Option<f64>) -> Option<f64> {
        lock(&self.usual).get(sensor)?.now(read)
    }

    /// `sensor`'s usual reading now, worked out afresh (the alert editor asks).
    pub fn usual_for(&self, sensor: &str, now: i64) -> Result<Option<f64>, String> {
        let u = self.compute_usual(&[sensor.to_owned()], now)?;
        let reg = lock(&self.registry);
        Ok(u.get(sensor)
            .and_then(|u| u.now(|id| reg.get(id).and_then(|s| s.value))))
    }

    /// The week's history of `ids` (and the loads their temperatures follow) and their usual
    /// readings.
    fn compute_usual(&self, ids: &[String], now: i64) -> Result<HashMap<String, Usual>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        let celsius: HashMap<&str, bool> = {
            let reg = lock(&self.registry);
            ids.iter()
                .map(|id| {
                    let c = reg.get(id).is_some_and(|s| s.unit == Unit::Celsius);
                    (id.as_str(), c)
                })
                .collect()
        };
        let mut week = SeriesMap::new();
        for id in ids {
            let load = celsius
                .get(id.as_str())
                .copied()
                .unwrap_or(false)
                .then(|| load_of(id))
                .flatten();
            for metric in std::iter::once(id.clone()).chain(load) {
                if week.contains_key(&metric) {
                    continue;
                }
                let s = lock(history)
                    .query(&metric, HistoryRange::D7, now)
                    .map_err(|e| e.to_string())?;
                week.insert(metric, s.points);
            }
        }
        Ok(ids
            .iter()
            .filter_map(|id| {
                let c = celsius.get(id.as_str()).copied().unwrap_or(false);
                usual(&week, id, c, now).map(|u| (id.clone(), u))
            })
            .collect())
    }

    /// The keys of the findings worth a look, as of the last pass.
    pub fn brief(&self) -> InsightBrief {
        lock(&self.brief).clone()
    }

    /// The findings, from the last pass if it is recent enough.
    pub fn insights(&self, now: i64) -> Result<Insights, String> {
        if let Some(i) = lock(&self.latest)
            .as_ref()
            .filter(|i| now >= i.ts && now - i.ts < CACHE_SECS)
        {
            return Ok(i.clone());
        }
        self.last_pass.store(now, Ordering::Relaxed);
        let i = self.compute(now)?;
        self.keep(i.clone());
        Ok(i)
    }

    fn keep(&self, i: Insights) {
        let ids = lock(&self.alert_sensors).clone();
        if !ids.is_empty() {
            match self.compute_usual(&ids, i.ts) {
                Ok(u) => *lock(&self.usual) = u,
                Err(e) => log::warn!("alerts: usual readings: {e}"),
            }
        }
        *lock(&self.brief) = i.brief();
        *lock(&self.latest) = Some(i);
    }

    /// Reads what the analysis needs from the history store (a lock per step, so ticks are
    /// not held up) and works out the findings.
    fn compute(&self, now: i64) -> Result<Insights, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        let err = |e: rusqlite::Error| e.to_string();
        let tree = lock(&self.registry).tree();
        let ids = wanted(&tree);
        let names: HashMap<String, String> = tree
            .groups
            .iter()
            .flat_map(|g| g.sensors.iter())
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let mut windows: [SeriesMap; 3] = Default::default();
        let ranges = [HistoryRange::D7, HistoryRange::H24, HistoryRange::H1];
        for (map, range) in windows.iter_mut().zip(ranges) {
            let mut h = lock(history);
            for id in &ids {
                let s = h.query(id, range, now).map_err(err)?;
                map.insert(id.clone(), s.points);
            }
        }
        let week_ago = now - HistoryRange::D7.secs();
        let digests = lock(history).digests(week_ago, now).map_err(err)?;
        let (sessions, markers, changes, fan_events) = {
            let mut h = lock(history);
            let sessions = h.game_sessions(MAX_SESSIONS).map_err(err)?;
            let mut markers = HashMap::new();
            for s in sessions
                .iter()
                .filter(|s| s.started >= week_ago && s.has_timeline)
                .take(MARKED_SESSIONS)
            {
                markers.insert(s.id, h.game_events(s.id).map_err(err)?);
            }
            let changes = h.hardware_changes(MAX_CHANGES).map_err(err)?;
            let fan_events: Vec<FanEvent> = h.events(fan::EVENT_KIND, 1000).map_err(err)?;
            (sessions, markers, changes, fan_events)
        };
        let [week, day, hour] = &windows;
        let profile = *lock(&self.profile);
        Ok(analyze(&Inputs {
            now,
            week,
            day,
            hour,
            names: &names,
            digests: &digests,
            sessions: &sessions,
            markers: &markers,
            changes: &changes,
            fan_events: &fan_events,
            profile,
        }))
    }
}

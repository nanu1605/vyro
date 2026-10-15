//! The intelligence engine inside the engine: builds the minute digest from the poller's
//! ticks, works out the findings on demand (kept a minute) and in the background every ten
//! minutes on a short-lived thread, and hands the app the keys of the findings worth a look.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use super::digest::{DigestBuilder, MinuteDigest};
use super::{analyze, wanted, Inputs, InsightBrief, Insights, SeriesMap};
use crate::changes::MAX_CHANGES;
use crate::fan::{self, FanEvent};
use crate::gaming::MAX_SESSIONS;
use crate::history::History;
use crate::profiles::Profile;
use crate::registry::Registry;
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

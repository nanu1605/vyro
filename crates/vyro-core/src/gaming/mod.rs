//! Gaming (P10): frame rate and frame times of the game in the foreground, from PresentMon's
//! ETW capture. The app names the process to capture ([`CaptureTarget`]); the engine runs the
//! capture, puts the live figures in the snapshot ([`GameSnapshot`]) and the `game.*` sensors,
//! and keeps a summary of every session ([`GameSession`]). With a history store, every tick of
//! a session and its markers are kept too, for the replay (P15, [`replay`], [`markers`]).

pub mod auto;
pub mod capture;
pub mod csv;
pub mod foreground;
pub mod markers;
pub mod overlay;
pub mod replay;
pub mod stats;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::history::History;
use crate::profiles::Profile;
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;
use crate::{lock, unix_now};
use capture::{Capture, End, Launch};
use csv::Frame;
use markers::{Detector, Frames, GameEvent};
use replay::GameSample;
use stats::{Totals, Window};

/// Game sessions kept.
pub const MAX_SESSIONS: usize = 100;
/// Shorter sessions (a launcher, a loading screen) are not kept.
pub const MIN_SESSION_SECS: i64 = 30;
/// No frame for this long: the game is paused, minimized or loading.
const STALE: Duration = Duration::from_secs(3);
pub const SESSIONS_FILE: &str = "games.json";
/// `games.json` once its sessions are in the history store.
pub const IMPORTED_FILE: &str = "games.json.imported";

/// The process the app wants captured (the fullscreen app in the foreground).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CaptureTarget {
    pub pid: u32,
    /// Executable name, shown until the first frame names it.
    pub name: String,
}

/// What the app knows about the game it asked to capture: whether it is in front, the profile
/// in force and the overlay. Sent whenever any of it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameContext {
    /// The game is the foreground app (not alt-tabbed away).
    pub in_front: bool,
    /// Profile in force.
    pub profile: Profile,
    /// The game chose it (automatic game profiles).
    pub auto: bool,
    /// The companion overlay shows over the game.
    pub overlay: bool,
}

/// What limits the frame rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Bound {
    /// The GPU is busy for (nearly) the whole frame.
    Gpu,
    /// The CPU side of the game is.
    Cpu,
    /// Neither: V-Sync or a frame limiter holds the rate.
    Capped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CaptureState {
    /// Started, no frames yet.
    Starting,
    Capturing,
    /// No frames for a few seconds (paused, minimized, loading).
    Idle,
    /// Capture could not run; see `detail`.
    Unavailable,
}

/// The captured game, every tick while there is one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameSnapshot {
    pub state: CaptureState,
    /// Why capture is unavailable.
    pub detail: Option<String>,
    pub app: String,
    pub pid: u32,
    /// Graphics API ("DXGI", "Vulkan", …) and present mode, once frames arrive.
    pub runtime: Option<String>,
    pub present_mode: Option<String>,
    /// Over the last second.
    pub fps: Option<f64>,
    pub frame_ms: Option<f64>,
    /// Over the last ten seconds: frame rate at the 99th / 99.9th percentile frame time.
    pub low1_fps: Option<f64>,
    pub low01_fps: Option<f64>,
    pub cpu_busy_ms: Option<f64>,
    pub gpu_busy_ms: Option<f64>,
    /// Frame start to on screen.
    pub latency_ms: Option<f64>,
    pub displayed_fps: Option<f64>,
    pub bound: Option<Bound>,
    /// Unix seconds.
    pub started: i64,
    pub frames: u64,
    /// Whole session so far.
    pub session_fps: Option<f64>,
    pub session_low1_fps: Option<f64>,
    /// Frame times over the last 15 s in 100 ms buckets, oldest first, for the graph.
    pub timeline: Vec<Option<FramePoint>>,
}

/// One 100 ms stretch of the frame-time graph.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FramePoint {
    pub avg_ms: f32,
    /// Worst frame in the stretch (a stutter shows here first).
    pub max_ms: f32,
}

/// Summary of one game session, kept in the engine's data folder.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameSession {
    /// In the history store (0: not stored).
    #[serde(default)]
    pub id: i64,
    pub app: String,
    pub runtime: Option<String>,
    /// Unix seconds.
    pub started: i64,
    pub secs: i64,
    pub frames: u64,
    pub avg_fps: Option<f64>,
    pub low1_fps: Option<f64>,
    pub low01_fps: Option<f64>,
    pub max_frame_ms: Option<f64>,
    pub bound: Option<Bound>,
    pub cpu_temp_max: Option<f64>,
    pub gpu_temp_max: Option<f64>,
    pub cpu_usage_avg: Option<f64>,
    pub gpu_usage_avg: Option<f64>,
    /// Profile in force when it ended (logged since 0.3).
    #[serde(default)]
    pub profile: Option<Profile>,
    /// Unix seconds (since 0.3).
    #[serde(default)]
    pub ended: Option<i64>,
    /// Its replay is kept (since 0.3; the oldest go when the history store is full).
    #[serde(default)]
    pub has_timeline: bool,
}

/// Running mean and maximum of a reading over the session.
#[derive(Debug, Clone, Copy, Default)]
struct Track {
    sum: f64,
    n: u32,
    max: Option<f64>,
}

impl Track {
    fn add(&mut self, v: Option<f64>) {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            self.sum += v;
            self.n += 1;
            self.max = Some(self.max.map_or(v, |m| m.max(v)));
        }
    }
    fn avg(&self) -> Option<f64> {
        (self.n > 0).then(|| self.sum / f64::from(self.n))
    }
}

/// One capture from start to end.
struct Session {
    target: CaptureTarget,
    generation: u64,
    capture: Option<Capture>,
    started: i64,
    app: Option<String>,
    runtime: Option<String>,
    present_mode: Option<String>,
    window: Window,
    totals: Totals,
    last_frame: Option<Instant>,
    failed: Option<String>,
    ended: bool,
    cpu_temp: Track,
    gpu_temp: Track,
    cpu_usage: Track,
    gpu_usage: Track,
    profile: Option<Profile>,
    /// In the history store.
    id: Option<i64>,
    begun: Instant,
    /// Time of the last sample, ms from `begun`.
    sampled: Option<i64>,
    detector: Detector,
}

impl Session {
    fn new(
        target: CaptureTarget,
        generation: u64,
        started: i64,
        context: Option<GameContext>,
    ) -> Self {
        Self {
            target,
            generation,
            capture: None,
            started,
            app: None,
            runtime: None,
            present_mode: None,
            window: Window::default(),
            totals: Totals::default(),
            last_frame: None,
            failed: None,
            ended: false,
            cpu_temp: Track::default(),
            gpu_temp: Track::default(),
            cpu_usage: Track::default(),
            gpu_usage: Track::default(),
            profile: context.map(|c| c.profile),
            id: None,
            begun: Instant::now(),
            sampled: None,
            detector: Detector::new(context),
        }
    }

    /// Milliseconds since the session began.
    fn now_ms(&self) -> i64 {
        i64::try_from(self.begun.elapsed().as_millis()).unwrap_or(i64::MAX)
    }

    fn summary(&self) -> GameSession {
        let t = &self.totals;
        GameSession {
            id: self.id.unwrap_or(0),
            app: self.app.clone().unwrap_or_else(|| self.target.name.clone()),
            runtime: self.runtime.clone(),
            started: self.started,
            secs: (t.total_ms / 1000.0).round() as i64,
            frames: t.frames,
            avg_fps: t.fps(),
            low1_fps: t.low(99.0),
            low01_fps: t.low(99.9),
            max_frame_ms: (t.frames > 0).then_some(t.max_frame_ms),
            bound: t.bound(),
            cpu_temp_max: self.cpu_temp.max,
            gpu_temp_max: self.gpu_temp.max,
            cpu_usage_avg: self.cpu_usage.avg(),
            gpu_usage_avg: self.gpu_usage.avg(),
            profile: self.profile,
            ended: None,
            has_timeline: self.id.is_some(),
        }
    }
}

/// What a tick or a context change leaves for the history store, written once the gaming lock
/// is released (the store's lock is always taken last).
struct Record {
    id: i64,
    sample: Option<GameSample>,
    events: Vec<GameEvent>,
    summary: Option<GameSession>,
}

struct Inner {
    session: Option<Session>,
    generation: u64,
    sessions: Vec<GameSession>,
    /// Latest from the app; applies to the running session and the next one.
    context: Option<GameContext>,
}

/// The engine's side of gaming: the capture target, the running capture and the session log.
/// Shared by the engine (commands) and [`GameProvider`] (every tick).
pub struct Gaming {
    inner: Arc<Mutex<Inner>>,
    launch: Launch,
    /// The JSON log, used only without a history store.
    path: Option<PathBuf>,
    store: Option<Arc<Mutex<History>>>,
}

impl Gaming {
    /// `store`: the history store, which keeps sessions with their replays; a `games.json`
    /// in `dir` is moved into it once. Without one, sessions are kept in `dir/games.json`
    /// (`None`: memory only).
    pub fn new(
        launch: Launch,
        dir: Option<&std::path::Path>,
        store: Option<Arc<Mutex<History>>>,
    ) -> Arc<Self> {
        let json = dir.map(|d| d.join(SESSIONS_FILE));
        let path = match (&store, json) {
            (Some(store), Some(json)) => {
                import(store, &json);
                None
            }
            (_, json) => json,
        };
        let sessions = path.as_deref().map(load).unwrap_or_default();
        Arc::new(Self {
            inner: Arc::new(Mutex::new(Inner {
                session: None,
                generation: 0,
                sessions,
                context: None,
            })),
            launch,
            path,
            store,
        })
    }

    /// PresentMon on Windows, simulated frames elsewhere or with `VYRO_SIM=1`.
    pub fn default_launch() -> Launch {
        #[cfg(windows)]
        if std::env::var_os("VYRO_SIM").is_none_or(|v| v != "1") {
            return capture::presentmon();
        }
        capture::simulated("SimGame.exe")
    }

    /// Starts capturing `target` (ending the current session), or stops with `None`. The same
    /// process again is a no-op.
    pub fn set_target(&self, target: Option<CaptureTarget>) {
        let old = {
            let mut inner = lock(&self.inner);
            if inner.session.as_ref().map(|s| s.target.pid) == target.as_ref().map(|t| t.pid) {
                return;
            }
            inner.session.take()
        };
        if let Some(old) = old {
            self.finish(old);
        }
        let Some(target) = target else {
            return;
        };
        log::info!("gaming: capturing {} ({})", target.name, target.pid);
        let context = lock(&self.inner).context;
        let mut session = Session::new(target.clone(), 0, unix_now(), context);
        if let Some(store) = &self.store {
            session.id = lock(store)
                .game_begin(&session.summary())
                .inspect_err(|e| log::warn!("gaming: cannot store the session: {e}"))
                .ok();
        }
        let generation = {
            let mut inner = lock(&self.inner);
            inner.generation += 1;
            session.generation = inner.generation;
            inner.session = Some(session);
            inner.generation
        };
        let frames = Arc::clone(&self.inner);
        let ends = Arc::clone(&self.inner);
        let started = Capture::start(
            &self.launch,
            target.pid,
            move |f: Frame| {
                let mut inner = lock(&frames);
                if let Some(s) = inner
                    .session
                    .as_mut()
                    .filter(|s| s.generation == generation)
                {
                    if s.app.is_none() {
                        s.app = Some(f.app.clone());
                        s.runtime = Some(f.runtime.clone());
                    }
                    if f.present_mode.is_some() {
                        s.present_mode.clone_from(&f.present_mode);
                    }
                    s.window.push(&f);
                    s.totals.push(&f);
                    s.last_frame = Some(Instant::now());
                }
            },
            move |end| {
                let mut inner = lock(&ends);
                if let Some(s) = inner
                    .session
                    .as_mut()
                    .filter(|s| s.generation == generation)
                {
                    match end {
                        End::Done => s.ended = true,
                        End::Failed(why) => {
                            log::warn!("gaming: capture failed: {why}");
                            s.failed = Some(why);
                        }
                    }
                }
            },
        );
        let mut inner = lock(&self.inner);
        if let Some(s) = inner
            .session
            .as_mut()
            .filter(|s| s.generation == generation)
        {
            match started {
                Ok(c) => s.capture = Some(c),
                Err(e) => {
                    log::warn!("gaming: cannot start capture: {e}");
                    s.failed = Some(e.to_string());
                }
            }
        }
    }

    /// The app's view of the game: in front, profile, overlay. Changes during a session are
    /// marked on its replay.
    pub fn set_context(&self, context: Option<GameContext>) {
        let record = {
            let mut inner = lock(&self.inner);
            inner.context = context;
            inner.session.as_mut().and_then(|s| {
                if let Some(c) = context {
                    s.profile = Some(c.profile);
                }
                let t = s.now_ms();
                let events = s.detector.context(t, context);
                s.id.map(|id| Record {
                    id,
                    sample: None,
                    events,
                    summary: None,
                })
            })
        };
        self.store(record);
    }

    /// Hands a tick's sample and markers to the history store.
    fn store(&self, record: Option<Record>) {
        let (Some(store), Some(r)) = (&self.store, record) else {
            return;
        };
        let mut h = lock(store);
        if let Some(sample) = r.sample {
            h.game_sample(r.id, sample);
        }
        for e in r.events {
            h.game_event(r.id, e);
        }
        if let Some(summary) = &r.summary {
            h.game_update(r.id, summary);
        }
    }

    /// The session log, newest first.
    pub fn sessions(&self) -> Vec<GameSession> {
        if let Some(store) = &self.store {
            match lock(store).game_sessions(MAX_SESSIONS) {
                Ok(list) => return list,
                Err(e) => log::warn!("gaming: sessions: {e}"),
            }
        }
        lock(&self.inner).sessions.iter().rev().cloned().collect()
    }

    /// Ends a session: stops its capture and logs it if it ran long enough.
    fn finish(&self, mut s: Session) {
        if let Some(mut c) = s.capture.take() {
            c.stop();
        }
        let summary = s.summary();
        let keep = summary.secs >= MIN_SESSION_SECS;
        if let (Some(store), Some(id)) = (&self.store, s.id) {
            let ended = s.started + s.now_ms() / 1000;
            let mut h = lock(store);
            let done = if keep {
                h.game_finish(id, ended, &summary)
            } else {
                h.game_discard(id)
            };
            if let Err(e) = done {
                log::warn!("gaming: cannot store the session: {e}");
            }
        }
        if !keep {
            return;
        }
        log::info!(
            "gaming: {} for {} s, {:.0} FPS average",
            summary.app,
            summary.secs,
            summary.avg_fps.unwrap_or(0.0)
        );
        if self.store.is_some() {
            return;
        }
        let all = {
            let mut inner = lock(&self.inner);
            inner.sessions.push(summary);
            let excess = inner.sessions.len().saturating_sub(MAX_SESSIONS);
            inner.sessions.drain(..excess);
            inner.sessions.clone()
        };
        if let Some(path) = &self.path {
            if let Err(e) = save(path, &all) {
                log::warn!("gaming: cannot save {}: {e}", path.display());
            }
        }
    }

    /// This tick's view of the capture; also tracks temperatures and load for the summary.
    /// Ends a session whose game has exited.
    fn tick(&self, snap: &SensorSnapshot) -> Option<GameSnapshot> {
        let (game, record) = self.tick_locked(snap)?;
        self.store(record);
        Some(game)
    }

    /// [`Self::tick`] under the gaming lock: the snapshot and what goes to the store.
    fn tick_locked(&self, snap: &SensorSnapshot) -> Option<(GameSnapshot, Option<Record>)> {
        let mut inner = lock(&self.inner);
        let s = inner.session.as_mut()?;
        if s.ended {
            let done = inner.session.take();
            drop(inner);
            if let Some(done) = done {
                self.finish(done);
            }
            return None;
        }
        let fresh = s.last_frame.is_some_and(|t| t.elapsed() < STALE);
        if fresh {
            s.cpu_temp.add(snap.cpu.temp_package);
            s.cpu_usage.add(snap.cpu.usage_total);
            if let Some(g) = snap.gpu.first() {
                s.gpu_temp.add(g.temp_edge);
                s.gpu_usage.add(g.usage);
            }
        }
        let state = if s.failed.is_some() {
            CaptureState::Unavailable
        } else if fresh {
            CaptureState::Capturing
        } else if s.last_frame.is_some() {
            CaptureState::Idle
        } else {
            CaptureState::Starting
        };
        let live = if fresh {
            s.window.live()
        } else {
            stats::Live::default()
        };
        // The replay: a sample and the markers of every tick with frames.
        let record = match s.id {
            Some(id) if fresh => {
                let t = s.now_ms();
                let since = s.sampled.map_or(1000, |at| t - at);
                s.sampled = Some(t);
                let frames = Frames::new(live.fps, &live.timeline, since);
                Some(Record {
                    id,
                    sample: Some(GameSample::new(t, &frames, live.frame_ms, snap)),
                    events: s.detector.step(t, &frames, snap),
                    summary: Some(s.summary()),
                })
            }
            _ => None,
        };
        let game = GameSnapshot {
            state,
            detail: s.failed.clone(),
            app: s.app.clone().unwrap_or_else(|| s.target.name.clone()),
            pid: s.target.pid,
            runtime: s.runtime.clone(),
            present_mode: s.present_mode.clone(),
            fps: live.fps,
            frame_ms: live.frame_ms,
            low1_fps: live.low1_fps,
            low01_fps: live.low01_fps,
            cpu_busy_ms: live.cpu_busy_ms,
            gpu_busy_ms: live.gpu_busy_ms,
            latency_ms: live.latency_ms,
            displayed_fps: live.displayed_fps,
            bound: live.bound,
            started: s.started,
            frames: s.totals.frames,
            session_fps: s.totals.fps(),
            session_low1_fps: s.totals.low(99.0),
            timeline: live.timeline,
        };
        Some((game, record))
    }

    /// Ends the running session (engine shutting down).
    pub fn stop(&self) {
        self.set_target(None);
    }
}

/// Moves the sessions of a `games.json` (0.2) into the history store and renames the file.
fn import(store: &Mutex<History>, json: &std::path::Path) {
    if !json.exists() {
        return;
    }
    let sessions = load(json);
    match lock(store).game_import(&sessions) {
        Ok(()) => {
            log::info!(
                "gaming: {} session(s) moved to the history store",
                sessions.len()
            );
            if let Err(e) = std::fs::rename(json, json.with_file_name(IMPORTED_FILE)) {
                log::warn!("gaming: cannot rename {}: {e}", json.display());
            }
        }
        Err(e) => log::warn!("gaming: cannot import {}: {e}", json.display()),
    }
}

fn load(path: &std::path::Path) -> Vec<GameSession> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!(
                "gaming: {} unreadable ({e}); starting a new log",
                path.display()
            );
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

fn save(path: &std::path::Path, sessions: &[GameSession]) -> std::io::Result<()> {
    let json = serde_json::to_string(sessions).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Fills `snapshot.game` and the `game.*` sensors. Runs last, so the summary can use this
/// tick's temperatures.
pub struct GameProvider {
    gaming: Arc<Gaming>,
    keys: Vec<SensorKey>,
}

impl GameProvider {
    pub fn new(gaming: Arc<Gaming>) -> Self {
        Self {
            gaming,
            keys: Vec::new(),
        }
    }
}

impl Provider for GameProvider {
    fn name(&self) -> &'static str {
        "gaming"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let sensor = |id: &str, name: &str, unit| {
            SensorDesc::new(id, Group::Game, name, unit, Source::PresentMon).with_history()
        };
        self.keys = vec![
            reg.register(sensor("game.fps", "Game FPS", Unit::Fps)),
            reg.register(sensor(
                "game.frametime",
                "Game frame time",
                Unit::Millisecond,
            )),
            reg.register(sensor("game.low1", "Game 1% low FPS", Unit::Fps)),
        ];
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        snap.game = self.gaming.tick(snap);
        let g = snap.game.as_ref();
        let values = [
            g.and_then(|g| g.fps),
            g.and_then(|g| g.frame_ms),
            g.and_then(|g| g.low1_fps),
        ];
        for (&key, value) in self.keys.iter().zip(values) {
            reg.set(key, value);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(pid: u32) -> Option<CaptureTarget> {
        Some(CaptureTarget {
            pid,
            name: "SimGame.exe".into(),
        })
    }

    /// A finished minute of Long.exe at 100 FPS.
    fn long_session(id: Option<i64>) -> Session {
        let context = GameContext {
            in_front: true,
            profile: Profile::Gaming,
            auto: true,
            overlay: false,
        };
        let mut s = Session::new(target(5).unwrap(), 0, 1000, Some(context));
        s.app = Some("Long.exe".into());
        s.runtime = Some("DXGI".into());
        s.id = id;
        let mut at = 0.0;
        while at < 60_000.0 {
            s.totals.push(&Frame {
                app: "Long.exe".into(),
                pid: 5,
                runtime: "DXGI".into(),
                present_mode: None,
                start_ms: at,
                frame_ms: 10.0,
                cpu_busy_ms: Some(3.0),
                gpu_busy_ms: Some(4.0),
                display_latency_ms: None,
                displayed_ms: None,
            });
            at += 10.0;
        }
        s.cpu_temp.add(Some(70.0));
        s.cpu_temp.add(Some(80.0));
        s
    }

    #[test]
    fn with_a_history_store_sessions_keep_their_replay() {
        let dir = tempfile::tempdir().unwrap();
        // A 0.2 log is moved into the store.
        let old = vec![GameSession {
            app: "Old.exe".into(),
            started: 500,
            secs: 3600,
            ..GameSession::default()
        }];
        save(&dir.path().join(SESSIONS_FILE), &old).unwrap();
        let store = Arc::new(Mutex::new(
            History::open_in_memory(crate::HistoryConfig::default()).unwrap(),
        ));
        let gaming = Gaming::new(
            capture::simulated("SimGame.exe"),
            Some(dir.path()),
            Some(Arc::clone(&store)),
        );
        assert!(!dir.path().join(SESSIONS_FILE).exists());
        assert!(dir.path().join(IMPORTED_FILE).exists());
        assert_eq!(gaming.sessions()[0].app, "Old.exe");

        // A live capture records a sample per tick and marks an alt-tab.
        let ctx = |in_front| {
            Some(GameContext {
                in_front,
                profile: Profile::Gaming,
                auto: true,
                overlay: false,
            })
        };
        gaming.set_context(ctx(true));
        gaming.set_target(target(77));
        let mut reg = Registry::new();
        let mut p = GameProvider::new(Arc::clone(&gaming));
        p.init(&mut reg).unwrap();
        let mut snap = SensorSnapshot::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut ticks = 0;
        while ticks < 3 {
            p.poll(&mut reg, &mut snap).unwrap();
            if snap.game.as_ref().unwrap().state == CaptureState::Capturing {
                ticks += 1;
            }
            assert!(Instant::now() < deadline, "no frames");
            std::thread::sleep(Duration::from_millis(50));
        }
        gaming.set_context(ctx(false));
        let id = lock(&gaming.inner).session.as_ref().unwrap().id.unwrap();
        let tl = lock(&store).game_timeline(id, 100).unwrap().unwrap();
        assert_eq!(tl.samples.len(), 3);
        assert!(tl.samples.iter().all(|s| s.fps.is_some()));
        assert_eq!(tl.events.len(), 1);
        assert_eq!(tl.events[0].marker, markers::Marker::Left);
        // Too short: forgotten when it ends.
        gaming.set_target(None);
        assert!(lock(&store).game_timeline(id, 100).unwrap().is_none());

        // A long one is kept with its summary.
        let s = long_session(None);
        let id = lock(&store).game_begin(&s.summary()).unwrap();
        let mut s = s;
        s.id = Some(id);
        gaming.finish(s);
        let list = gaming.sessions();
        assert_eq!(list.len(), 2);
        assert_eq!(
            (list[0].id, list[0].app.as_str(), list[0].secs),
            (id, "Long.exe", 60)
        );
        assert!(list[0].has_timeline && list[0].ended.is_some());
        assert!(!list[1].has_timeline, "imported: summary only");
    }

    #[test]
    fn captures_reports_and_logs_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let gaming = Gaming::new(capture::simulated("SimGame.exe"), Some(dir.path()), None);
        let mut reg = Registry::new();
        let mut p = GameProvider::new(Arc::clone(&gaming));
        p.init(&mut reg).unwrap();

        let mut snap = SensorSnapshot::default();
        p.poll(&mut reg, &mut snap).unwrap();
        assert!(snap.game.is_none(), "nothing to capture yet");

        gaming.set_target(target(77));
        gaming.set_target(target(77)); // same process: keeps the session
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            p.poll(&mut reg, &mut snap).unwrap();
            let g = snap.game.as_ref().unwrap();
            if g.state == CaptureState::Capturing && g.frames > 20 {
                break;
            }
            assert!(Instant::now() < deadline, "no frames: {g:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
        let g = snap.game.clone().unwrap();
        assert_eq!((g.app.as_str(), g.pid), ("SimGame.exe", 77));
        assert_eq!(g.runtime.as_deref(), Some("DXGI"));
        assert!((120.0..160.0).contains(&g.fps.unwrap()), "{:?}", g.fps);
        assert_eq!(g.bound, Some(Bound::Gpu));
        assert!(reg.get("game.fps").unwrap().value.is_some());

        // Too short to keep.
        gaming.set_target(None);
        p.poll(&mut reg, &mut snap).unwrap();
        assert!(snap.game.is_none());
        assert!(reg.get("game.fps").unwrap().value.is_none());
        assert!(gaming.sessions().is_empty());

        // A long session is logged and saved.
        gaming.finish(long_session(None));
        let logged = gaming.sessions();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].secs, 60);
        assert_eq!(logged[0].bound, Some(Bound::Capped));
        assert_eq!(logged[0].cpu_temp_max, Some(80.0));
        assert_eq!(logged[0].profile, Some(Profile::Gaming));
        assert!((logged[0].avg_fps.unwrap() - 100.0).abs() < 1e-6);
        let again = Gaming::new(capture::simulated("x"), Some(dir.path()), None);
        assert_eq!(again.sessions(), logged);
    }

    #[test]
    fn a_failed_start_is_reported() {
        let fail: Launch = Arc::new(|_| Err(std::io::Error::other("PresentMon.exe missing")));
        let gaming = Gaming::new(fail, None, None);
        gaming.set_target(target(3));
        let g = gaming.tick(&SensorSnapshot::default()).unwrap();
        assert_eq!(g.state, CaptureState::Unavailable);
        assert_eq!(g.detail.as_deref(), Some("PresentMon.exe missing"));
    }
}

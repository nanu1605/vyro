//! The fan controller: the poller decides speeds every tick ([`FanControl::on_tick`], pure
//! [`Runtime`]); a worker thread writes them to the chip, so a slow EC request never delays
//! a tick. Everything that can go wrong ends in the BIOS curve:
//!
//! - before the first write the BIOS state is saved to a file; it is written back and the file
//!   deleted when control ends (turned off, all headers on BIOS, shutdown);
//! - a register that does not keep a write, or three I/O errors in a row, restore and turn
//!   control off;
//! - no sensor tick for the watchdog period (the poller stopped) restores until ticks resume;
//! - a pump that stops reporting a speed restores and turns control off;
//! - going to sleep restores (the service tells the controller); after waking, the BIOS state
//!   is captured afresh before the next write;
//! - the chip is re-read every few seconds: a header that lost Vyro's setting (the EC was reset,
//!   or another fan tool wrote it) is written again, and if that keeps happening control ends;
//! - a file left by a crash is written back when the next controller starts, before anything
//!   else (the service restarts itself after a crash).

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{
    validate, Decision, FanBrief, FanCapability, FanConfig, FanEvent, FanEventKind, FanStatus,
    HeaderMode, HeaderStatus, Runtime, PUMP_MIN_PCT,
};
use crate::profiles::Profile;
use crate::sio::control::{self, BiosState, Board, ControlError};

pub const CONFIG_FILE: &str = "fan-control.json";
pub const RESTORE_FILE: &str = "fan-restore.json";
/// The profile whose header settings are in force (set by the app).
pub const PROFILE_FILE: &str = "fan-profile.json";
/// Fan events kept in memory for the UI.
pub const RECENT_EVENTS: usize = 100;
/// No tick for this long while driving fans → BIOS control until ticks resume.
pub const WATCHDOG: Duration = Duration::from_secs(10);
/// Longest tick interval while fans are driven (the poller's tray interval is longer).
pub const MAX_TICK: Duration = Duration::from_secs(2);
const MAX_IO_ERRORS: u32 = 3;
/// Chip state (mode, duties) is re-read every this many worker loops.
const READ_EVERY: u32 = 3;
/// A driven header's output may differ from what was written by this many steps (rounding).
const DRIFT_STEPS: u8 = 2;
/// Reads in a row that disagree with what Vyro wrote before it writes again (one may catch
/// the chip mid-request).
const DRIFT_READS: u32 = 2;
/// This many rewrites within [`DRIFT_WINDOW`] mean another program is driving the same fans.
const MAX_DRIFTS: usize = 3;
const DRIFT_WINDOW: Duration = Duration::from_secs(120);
/// Longest wait for the BIOS state to be written back before the PC sleeps.
const SUSPEND_WAIT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevError {
    /// Another program holds the bus or the EC; try again next tick.
    Busy,
    /// A register did not keep a write.
    Mismatch(String),
    Io(String),
}

impl<E: std::fmt::Display> From<ControlError<E>> for DevError {
    fn from(e: ControlError<E>) -> Self {
        match e {
            ControlError::Busy => Self::Busy,
            ControlError::Mismatch { .. } => Self::Mismatch(e.to_string()),
            e => Self::Io(e.to_string()),
        }
    }
}

impl std::fmt::Display for DevError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("another program is using the fan controller"),
            Self::Mismatch(m) | Self::Io(m) => f.write_str(m),
        }
    }
}

/// Mode register and each header's output duty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChipState {
    pub mode: u8,
    pub outputs: Vec<u8>,
}

/// A fan controller chip on an allow-listed board (the NCT6687 through PawnIO, or the
/// simulator). Each call is one locked transaction.
pub trait FanDevice: Send {
    fn board(&self) -> &'static Board;
    fn capture(&mut self, now: u64) -> Result<BiosState, DevError>;
    fn set(&mut self, header: u16, pct: f64) -> Result<u8, DevError>;
    fn release(&mut self, header: u16, bios: &BiosState) -> Result<(), DevError>;
    fn restore(&mut self, bios: &BiosState) -> Result<(), DevError>;
    fn read(&mut self) -> Result<ChipState, DevError>;
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Inner {
    /// Everything the owner configured (every profile).
    config: FanConfig,
    /// The profile in use and the configuration in force for it.
    profile: Profile,
    effective: FanConfig,
    runtime: Runtime,
    decision: Option<Decision>,
    fresh: bool,
    last_tick: Option<Instant>,
    active: bool,
    override_reason: Option<String>,
    last_error: Option<String>,
    chip: Option<ChipState>,
    /// The PC is going to sleep: the BIOS drives every header until it wakes.
    suspended: bool,
    stop: bool,
    /// Newest last, at most [`RECENT_EVENTS`].
    recent: VecDeque<FanEvent>,
    /// Not yet handed to the event sink (the worker does that, holding no lock).
    pending: Vec<FanEvent>,
}

impl Inner {
    fn record(&mut self, kind: FanEventKind, message: String) {
        log::info!("fans: {message}");
        let ev = FanEvent {
            ts: crate::unix_now(),
            kind,
            message,
        };
        if self.recent.len() >= RECENT_EVENTS {
            self.recent.pop_front();
        }
        self.recent.push_back(ev.clone());
        if self.pending.len() >= RECENT_EVENTS {
            self.pending.remove(0);
        }
        self.pending.push(ev);
    }

    fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
        self.effective = self.config.effective(profile);
    }
}

/// Where fan events go besides memory (the service: the history store).
pub type EventSink = Box<dyn Fn(&FanEvent) + Send + Sync>;

struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
    config_path: Option<PathBuf>,
    profile_path: Option<PathBuf>,
    events: Option<EventSink>,
    started: Instant,
}

/// Writes `value` as JSON through a temporary file, so a crash never leaves half a file.
fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("saving {}: {e}", path.display()))
}

impl Shared {
    fn save(&self, cfg: &FanConfig) -> Result<(), String> {
        self.config_path
            .as_deref()
            .map_or(Ok(()), |p| write_json(p, cfg))
    }

    /// Hands recorded events to the sink. Call without holding `inner`.
    fn flush_events(&self) {
        let pending = std::mem::take(&mut lock(&self.inner).pending);
        if let Some(sink) = &self.events {
            for ev in &pending {
                sink(ev);
            }
        }
    }
}

/// Fan control for one engine; `Unavailable` / `ReadOnly` when there is nothing to drive.
pub struct FanControl {
    capability: FanCapability,
    board: Option<&'static Board>,
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

/// Loads a saved configuration; an unreadable or invalid one falls back to "off".
pub fn load_config(path: &Path, board: Option<&Board>) -> FanConfig {
    let parsed = std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|b| serde_json::from_slice::<FanConfig>(&b).map_err(|e| e.to_string()))
        .and_then(|c| validate(&c, board).map(|()| c));
    match parsed {
        Ok(c) => c,
        Err(e) => {
            if path.exists() {
                log::warn!("fans: {} not used: {e}", path.display());
            }
            FanConfig::default()
        }
    }
}

/// Writes back a BIOS state a previous run left behind (it crashed while driving fans).
/// True when one was restored.
pub fn recover(dev: &mut dyn FanDevice, restore_path: &Path) -> bool {
    match control::load(restore_path) {
        Ok(None) => false,
        Ok(Some(state)) => match dev.restore(&state) {
            Ok(()) => {
                let _ = std::fs::remove_file(restore_path);
                true
            }
            Err(e) => {
                log::error!("fans: restoring the saved BIOS state failed: {e}");
                false
            }
        },
        Err(e) => {
            log::error!("fans: {} unreadable: {e}", restore_path.display());
            false
        }
    }
}

struct Files {
    config: Option<PathBuf>,
    profile: Option<PathBuf>,
}

fn shared(
    config: FanConfig,
    profile: Profile,
    chip: Option<ChipState>,
    files: Files,
    events: Option<EventSink>,
) -> Arc<Shared> {
    let effective = config.effective(profile);
    Arc::new(Shared {
        inner: Mutex::new(Inner {
            config,
            profile,
            effective,
            runtime: Runtime::default(),
            decision: None,
            fresh: false,
            last_tick: None,
            active: false,
            override_reason: None,
            last_error: None,
            chip,
            suspended: false,
            stop: false,
            recent: VecDeque::new(),
            pending: Vec::new(),
        }),
        wake: Condvar::new(),
        config_path: files.config,
        profile_path: files.profile,
        events,
        started: Instant::now(),
    })
}

impl FanControl {
    pub fn unavailable(capability: FanCapability) -> Self {
        Self {
            capability,
            board: None,
            shared: shared(
                FanConfig::default(),
                Profile::default(),
                None,
                Files {
                    config: None,
                    profile: None,
                },
                None,
            ),
            worker: Mutex::new(None),
        }
    }

    /// [`Self::start_with`] without an event sink (events stay in memory).
    pub fn start(dev: Box<dyn FanDevice>, dir: Option<&Path>, watchdog: Duration) -> Self {
        Self::start_with(dev, dir, watchdog, None)
    }

    /// Restores anything a crash left behind, loads the saved configuration and starts the
    /// worker. `dir` holds the configuration, the profile in use and the BIOS state file
    /// (`None`: memory only, for tests and the simulator); `events` also receives every fan
    /// event.
    pub fn start_with(
        mut dev: Box<dyn FanDevice>,
        dir: Option<&Path>,
        watchdog: Duration,
        events: Option<EventSink>,
    ) -> Self {
        let board = dev.board();
        let restore_path = dir.map(|d| d.join(RESTORE_FILE));
        let recovered = restore_path
            .as_deref()
            .is_some_and(|p| recover(dev.as_mut(), p));
        let files = Files {
            config: dir.map(|d| d.join(CONFIG_FILE)),
            profile: dir.map(|d| d.join(PROFILE_FILE)),
        };
        let config = files
            .config
            .as_deref()
            .map_or_else(FanConfig::default, |p| load_config(p, Some(board)));
        let profile = files
            .profile
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Profile>(&b).ok())
            .unwrap_or_default();
        let mut me = Self {
            capability: FanCapability::Writable {
                board: board.name.into(),
            },
            board: Some(board),
            shared: shared(config, profile, dev.read().ok(), files, events),
            worker: Mutex::new(None),
        };
        if recovered {
            lock(&me.shared.inner).record(
                FanEventKind::Recovered,
                "Vyro stopped unexpectedly while driving fans: the BIOS settings were put back"
                    .into(),
            );
        }
        let shared = Arc::clone(&me.shared);
        let worker = Worker {
            dev,
            shared,
            restore_path,
            watchdog,
            bios: None,
            applied: BTreeMap::new(),
            io_errors: 0,
            loops: 0,
            drift_reads: 0,
            drifts: Vec::new(),
        };
        match std::thread::Builder::new()
            .name("vyro-fans".into())
            .spawn(move || worker.run())
        {
            Ok(h) => *lock(&me.worker) = Some(h),
            Err(e) => {
                log::error!("fans: worker thread: {e}");
                me.capability = FanCapability::Unavailable {
                    reason: format!("could not start: {e}"),
                };
                me.board = None;
            }
        }
        me
    }

    pub fn capability(&self) -> &FanCapability {
        &self.capability
    }

    /// Whether ticks must come at least every [`MAX_TICK`].
    pub fn drives(&self) -> bool {
        self.board.is_some() && lock(&self.shared.inner).effective.drives_any()
    }

    /// What each snapshot reports about fan control.
    pub fn brief(&self) -> FanBrief {
        let inner = lock(&self.shared.inner);
        FanBrief {
            active: inner.active,
            full_speed: inner.active && inner.override_reason.is_some(),
        }
    }

    /// Recent fan events, newest last (this run only; the service keeps older ones in the
    /// history store).
    pub fn events(&self) -> Vec<FanEvent> {
        lock(&self.shared.inner).recent.iter().cloned().collect()
    }

    /// Decides this tick's speeds from the registry (called by the poller after the
    /// providers ran).
    pub fn on_tick(&self, read: impl Fn(&str) -> Option<f64>) {
        let Some(board) = self.board else {
            return;
        };
        let now = self.shared.started.elapsed().as_secs_f64();
        let mut g = lock(&self.shared.inner);
        let inner = &mut *g;
        if let Some(chip) = &inner.chip {
            for h in &inner.effective.headers {
                if h.mode != HeaderMode::Bios {
                    if let Some(&raw) = chip.outputs.get(usize::from(h.index)) {
                        inner.runtime.seed(h.index, control::percent(raw));
                    }
                }
            }
        }
        let d = inner.runtime.decide(&inner.effective, board, now, read);
        match (&inner.override_reason, &d.override_reason) {
            (None, Some(why)) => inner.record(FanEventKind::FullSpeed, why.clone()),
            (Some(_), None) => inner.record(
                FanEventKind::Normal,
                "Full speed no longer needed: fans back to their settings".into(),
            ),
            _ => {}
        }
        inner.override_reason.clone_from(&d.override_reason);
        inner.decision = Some(d);
        inner.fresh = true;
        inner.last_tick = Some(Instant::now());
        drop(g);
        self.shared.wake.notify_one();
    }

    /// Validates, saves and applies a configuration.
    pub fn set_config(&self, cfg: FanConfig) -> Result<(), String> {
        validate(&cfg, self.board)?;
        self.shared.save(&cfg)?;
        let mut inner = lock(&self.shared.inner);
        if !cfg.enabled {
            inner.runtime.reset();
        }
        match (inner.config.enabled, cfg.enabled) {
            (false, true) => inner.record(FanEventKind::On, "Fan control turned on".into()),
            (true, false) => inner.record(FanEventKind::Off, "Fan control turned off".into()),
            _ => {}
        }
        inner.config = cfg;
        let profile = inner.profile;
        inner.set_profile(profile);
        inner.last_error = None;
        Ok(())
    }

    /// Puts `profile`'s header settings in force (remembered across restarts).
    pub fn set_profile(&self, profile: Profile) -> Result<(), String> {
        {
            let mut inner = lock(&self.shared.inner);
            if inner.profile == profile {
                return Ok(());
            }
            inner.set_profile(profile);
            let own = inner.config.profiles.contains_key(&profile);
            if inner.config.enabled {
                inner.record(
                    FanEventKind::Profile,
                    format!(
                        "{} profile: {}",
                        profile.spec().name,
                        if own {
                            "its own fan settings"
                        } else {
                            "the fan settings for every profile"
                        }
                    ),
                );
            }
        }
        self.shared
            .profile_path
            .as_deref()
            .map_or(Ok(()), |p| write_json(p, &profile))
    }

    /// Turns control off (saved) and hands every header back to the BIOS.
    pub fn restore(&self) -> Result<(), String> {
        let cfg = {
            let mut inner = lock(&self.shared.inner);
            inner.config.enabled = false;
            inner.effective.enabled = false;
            inner.runtime.reset();
            inner.decision = Some(Decision::default());
            inner.fresh = true;
            inner.record(
                FanEventKind::Bios,
                "Returned to BIOS: fan control turned off".into(),
            );
            inner.config.clone()
        };
        self.shared.wake.notify_one();
        self.shared.save(&cfg)
    }

    /// Before the PC sleeps (`true`): hands every header back to the BIOS and waits, briefly,
    /// until that is done. After it wakes (`false`): control resumes with the next tick, from a
    /// fresh capture of the BIOS state (the firmware may have reset the chip).
    pub fn set_suspended(&self, on: bool) {
        if self.board.is_none() {
            return;
        }
        {
            let mut inner = lock(&self.shared.inner);
            if inner.suspended == on {
                return;
            }
            inner.suspended = on;
            inner.runtime.reset();
            // Nothing is written after waking until a fresh tick (the watchdog would read
            // the time asleep as stopped sensors).
            inner.last_tick = None;
            inner.decision = None;
            inner.fresh = false;
            if inner.effective.drives_any() {
                let (kind, message) = if on {
                    (
                        FanEventKind::Sleep,
                        "PC going to sleep: fans handed to the BIOS",
                    )
                } else {
                    (FanEventKind::Wake, "PC woke up: fan control resumes")
                };
                inner.record(kind, message.into());
            }
        }
        self.shared.wake.notify_one();
        if on {
            let until = Instant::now() + SUSPEND_WAIT;
            while lock(&self.shared.inner).active && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    /// `rpm(header)` reads the monitor's fan speed.
    pub fn status(&self, rpm: impl Fn(u16) -> Option<f64>) -> FanStatus {
        let inner = lock(&self.shared.inner);
        let Some(board) = self.board else {
            return FanStatus {
                capability: self.capability.clone(),
                ..FanStatus::unavailable("")
            };
        };
        let targets = inner.decision.as_ref();
        let headers = board
            .headers
            .iter()
            .map(|h| {
                let i = usize::from(h.index);
                let duty = inner.chip.as_ref().and_then(|c| c.outputs.get(i).copied());
                let software = inner
                    .chip
                    .as_ref()
                    .is_some_and(|c| c.mode & (1 << h.index) != 0);
                let rpm = rpm(h.index);
                HeaderStatus {
                    index: h.index,
                    name: h.name.into(),
                    kind: h.kind,
                    min_pct: h.min_pct,
                    controllable: true,
                    rpm,
                    duty_pct: duty.map(control::percent),
                    software,
                    target_pct: targets
                        .and_then(|d| d.targets.iter().find(|t| t.0 == h.index).map(|t| t.1)),
                    source_c: targets
                        .and_then(|d| d.sources.iter().find(|t| t.0 == h.index).map(|t| t.1)),
                    connected: !(rpm == Some(0.0) && duty.is_some_and(|d| d > 0)),
                }
            })
            .collect();
        FanStatus {
            capability: self.capability.clone(),
            config: inner.config.clone(),
            active: inner.active,
            override_reason: inner.override_reason.clone(),
            last_error: inner.last_error.clone(),
            headers,
            pump_min_pct: PUMP_MIN_PCT,
            profile: inner.profile,
        }
    }

    /// Hands every header back to the BIOS and stops the worker. Safe to call twice.
    pub fn shutdown(&self) {
        lock(&self.shared.inner).stop = true;
        self.shared.wake.notify_one();
        if let Some(h) = lock(&self.worker).take() {
            let _ = h.join();
        }
    }
}

impl Drop for FanControl {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Worker {
    dev: Box<dyn FanDevice>,
    shared: Arc<Shared>,
    restore_path: Option<PathBuf>,
    watchdog: Duration,
    bios: Option<BiosState>,
    /// Duty last written per driven header.
    applied: BTreeMap<u16, u8>,
    io_errors: u32,
    loops: u32,
    /// Chip reads in a row where a driven header lost its setting.
    drift_reads: u32,
    /// When lost settings were last written again.
    drifts: Vec<Instant>,
}

enum Step {
    Stop,
    Suspend,
    Stale,
    Apply(Option<Decision>),
}

impl Worker {
    fn run(mut self) {
        loop {
            match self.wait() {
                Step::Stop => {
                    self.restore_all();
                    self.shared.flush_events();
                    return;
                }
                Step::Suspend => self.restore_all(),
                Step::Stale => {
                    if self.bios.is_some() {
                        self.restore_all();
                        if self.bios.is_none() {
                            let why = "sensor readings stopped: BIOS fan control restored";
                            self.note_error(why.into());
                            lock(&self.shared.inner).record(
                                FanEventKind::Watchdog,
                                "Sensor readings stopped: the BIOS took over until they resume"
                                    .into(),
                            );
                        }
                    }
                }
                Step::Apply(d) => {
                    if let Some(d) = d {
                        self.apply(&d);
                    }
                }
            }
            self.loops = self.loops.wrapping_add(1);
            let chip = self
                .loops
                .is_multiple_of(READ_EVERY)
                .then(|| self.dev.read().ok())
                .flatten();
            if let Some(c) = &chip {
                self.check_drift(c);
            }
            {
                let mut inner = lock(&self.shared.inner);
                if chip.is_some() {
                    inner.chip = chip;
                }
                inner.active = self.bios.is_some() && !self.applied.is_empty();
            }
            self.shared.flush_events();
        }
    }

    /// Compares what the chip drives with what Vyro wrote. A header that lost its setting
    /// (mode bit cleared, or another duty) is written again on the next tick; if that keeps
    /// happening, another program is driving the fans and Vyro gives them back to the BIOS.
    fn check_drift(&mut self, chip: &ChipState) {
        let lost: Vec<u16> = self
            .applied
            .iter()
            .filter(|&(&h, &raw)| {
                let on = chip.mode & (1 << h) != 0;
                let out = chip.outputs.get(usize::from(h)).copied();
                !on || out.is_some_and(|o| o.abs_diff(raw) > DRIFT_STEPS)
            })
            .map(|(&h, _)| h)
            .collect();
        if lost.is_empty() {
            self.drift_reads = 0;
            return;
        }
        self.drift_reads += 1;
        if self.drift_reads < DRIFT_READS {
            return;
        }
        self.drift_reads = 0;
        let now = Instant::now();
        self.drifts
            .retain(|t| now.duration_since(*t) < DRIFT_WINDOW);
        self.drifts.push(now);
        if self.drifts.len() >= MAX_DRIFTS {
            self.drifts.clear();
            return self.fail(
                "another program keeps changing the fan settings (close MSI Center, FanControl \
                 or similar tools)"
                    .into(),
            );
        }
        let names: Vec<&str> = lost
            .iter()
            .filter_map(|h| self.dev.board().headers.iter().find(|x| x.index == *h))
            .map(|h| h.name)
            .collect();
        for h in lost {
            self.applied.remove(&h);
        }
        // Rewrite with the latest decision even if no tick arrives first.
        let mut inner = lock(&self.shared.inner);
        inner.fresh = inner.decision.is_some();
        inner.record(
            FanEventKind::Rewritten,
            format!("{} lost Vyro's setting and was set again", names.join(", ")),
        );
    }

    fn wait(&mut self) -> Step {
        let mut inner = lock(&self.shared.inner);
        if !inner.fresh && !inner.stop {
            inner = self
                .shared
                .wake
                .wait_timeout(inner, Duration::from_secs(1))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        if inner.stop {
            return Step::Stop;
        }
        if inner.suspended {
            return Step::Suspend;
        }
        let stale = inner.effective.drives_any()
            && inner.last_tick.is_none_or(|t| t.elapsed() > self.watchdog);
        if stale && self.bios.is_some() {
            return Step::Stale;
        }
        let fresh = std::mem::take(&mut inner.fresh);
        Step::Apply(fresh.then(|| inner.decision.clone()).flatten())
    }

    fn note_error(&self, e: String) {
        log::warn!("fans: {e}");
        lock(&self.shared.inner).last_error = Some(e);
    }

    /// Turns control off after a fault the controller must not retry. Control is marked off,
    /// with the reason, before the BIOS settings go back (no I/O in between), so anyone who
    /// sees them back also sees control off.
    fn fail(&mut self, e: String) {
        let message = format!("{e}; fan control turned off and BIOS control restored");
        let cfg = {
            let mut inner = lock(&self.shared.inner);
            inner.config.enabled = false;
            inner.effective.enabled = false;
            inner.runtime.reset();
            inner.decision = None;
            let mut first = e.chars();
            let e = first
                .next()
                .map(|c| c.to_uppercase().chain(first).collect::<String>())
                .unwrap_or_default();
            inner.record(
                FanEventKind::Fault,
                format!("{e}. Fan control turned off and the BIOS settings put back"),
            );
            inner.last_error = Some(message.clone());
            inner.config.clone()
        };
        log::warn!("fans: {message}");
        // A failed restore replaces the message with its own.
        self.restore_all();
        if let Err(save) = self.shared.save(&cfg) {
            log::warn!("fans: {save}");
        }
    }

    fn apply(&mut self, d: &Decision) {
        if let Some(f) = &d.fault {
            return self.fail(f.clone());
        }
        if d.targets.is_empty() {
            self.restore_all();
            return;
        }
        let board = self.dev.board();
        if self.bios.is_none() {
            let now = u64::try_from(crate::unix_now()).unwrap_or(0);
            let state = match self.dev.capture(now) {
                Ok(s) => s,
                Err(DevError::Busy) => return,
                Err(e) => return self.fail(format!("reading the BIOS fan settings failed: {e}")),
            };
            // Never write without a way back after a crash.
            if let Some(p) = &self.restore_path {
                if let Err(e) = control::save(p, &state) {
                    return self.fail(format!("saving the BIOS fan settings failed: {e}"));
                }
            }
            self.bios = Some(state);
        }
        let mut wrote = false;
        for &(header, pct) in &d.targets {
            let Some(h) = board.headers.iter().find(|h| h.index == header) else {
                continue;
            };
            if self.applied.get(&header) == Some(&control::duty(h, pct)) {
                continue;
            }
            match self.dev.set(header, pct) {
                Ok(raw) => {
                    self.applied.insert(header, raw);
                    self.io_errors = 0;
                    wrote = true;
                }
                Err(DevError::Busy) => {}
                Err(DevError::Mismatch(m)) => return self.fail(m),
                Err(DevError::Io(m)) => {
                    self.io_errors += 1;
                    if self.io_errors >= MAX_IO_ERRORS {
                        return self.fail(m);
                    }
                }
            }
        }
        let gone: Vec<u16> = self
            .applied
            .keys()
            .copied()
            .filter(|h| !d.targets.iter().any(|t| t.0 == *h))
            .collect();
        for header in gone {
            let Some(bios) = &self.bios else { break };
            match self.dev.release(header, bios) {
                Ok(()) => {
                    self.applied.remove(&header);
                    wrote = true;
                }
                Err(DevError::Busy) => {}
                Err(e) => return self.fail(e.to_string()),
            }
        }
        if wrote {
            self.loops = READ_EVERY - 1; // re-read the chip after this loop
            lock(&self.shared.inner).last_error = None;
        }
    }

    /// Writes the BIOS state back and deletes its file; on failure both stay, and the next
    /// loop (or the next start) tries again.
    fn restore_all(&mut self) {
        let Some(bios) = &self.bios else {
            return;
        };
        match self.dev.restore(bios) {
            Ok(()) => {
                self.bios = None;
                self.applied.clear();
                if let Some(p) = &self.restore_path {
                    let _ = std::fs::remove_file(p);
                }
                self.loops = READ_EVERY - 1;
                log::info!("fans: BIOS fan control restored");
            }
            Err(e) => self.note_error(format!("restoring BIOS fan control failed: {e}")),
        }
    }
}

/// A simulated NCT6687 on the reference board, for the simulator and tests.
#[derive(Debug, Default)]
pub struct SimChip {
    pub mode: u8,
    pub commands: Vec<u8>,
    pub outputs: Vec<u8>,
    /// Errors the next calls return, first to last.
    pub fail: Vec<DevError>,
    /// Another fan tool: every software-driven header reads this duty.
    pub fight: Option<u8>,
    pub log: Vec<String>,
}

impl SimChip {
    pub fn new() -> Self {
        Self {
            mode: 0,
            commands: vec![0x66; 8],
            outputs: vec![153, 255, 153, 153, 153, 255, 255, 128],
            ..Self::default()
        }
    }
}

#[derive(Clone)]
pub struct SimDevice(pub Arc<Mutex<SimChip>>);

impl SimDevice {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(SimChip::new())))
    }

    fn call<R>(&self, what: String, f: impl FnOnce(&mut SimChip) -> R) -> Result<R, DevError> {
        let mut c = lock(&self.0);
        if !c.fail.is_empty() {
            let e = c.fail.remove(0);
            c.log.push(format!("{what} -> {e}"));
            return Err(e);
        }
        c.log.push(what);
        Ok(f(&mut c))
    }
}

impl Default for SimDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl FanDevice for SimDevice {
    fn board(&self) -> &'static Board {
        &control::ALLOW_LIST[0]
    }

    fn capture(&mut self, now: u64) -> Result<BiosState, DevError> {
        self.call("capture".into(), |c| BiosState {
            chip_id: 0xD5,
            chip_rev: 0x92,
            base: 0xA20,
            mode: c.mode,
            commands: c.commands.clone(),
            saved_at: now,
        })
    }

    fn set(&mut self, header: u16, pct: f64) -> Result<u8, DevError> {
        let board = self.board();
        let h = *board
            .headers
            .iter()
            .find(|h| h.index == header)
            .ok_or(DevError::Io(format!(
                "header {header} may not be controlled"
            )))?;
        let raw = control::duty(&h, pct);
        self.call(format!("set {header} {raw}"), |c| {
            c.mode |= 1 << header;
            c.commands[usize::from(header)] = raw;
            c.outputs[usize::from(header)] = raw;
            raw
        })
    }

    fn release(&mut self, header: u16, bios: &BiosState) -> Result<(), DevError> {
        self.call(format!("release {header}"), |c| {
            let bit = 1 << header;
            c.mode = (c.mode & !bit) | (bios.mode & bit);
            c.commands[usize::from(header)] = bios.commands[usize::from(header)];
            c.outputs[usize::from(header)] = 153;
        })
    }

    fn restore(&mut self, bios: &BiosState) -> Result<(), DevError> {
        self.call("restore".into(), |c| {
            c.mode = bios.mode;
            c.commands.clone_from(&bios.commands);
            for (n, o) in c.outputs.iter_mut().enumerate().take(5) {
                *o = if n == 1 { 255 } else { 153 };
            }
        })
    }

    fn read(&mut self) -> Result<ChipState, DevError> {
        let mut c = lock(&self.0);
        if let Some(v) = c.fight {
            for n in 0..c.outputs.len() {
                if c.mode & (1 << n) != 0 {
                    c.outputs[n] = v;
                }
            }
        }
        Ok(ChipState {
            mode: c.mode,
            outputs: c.outputs.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fan::{FanCurve, HeaderConfig, CPU_TEMP};

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vyro-fanctl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fixed(index: u16, pct: f64) -> HeaderConfig {
        HeaderConfig {
            index,
            mode: HeaderMode::Fixed { pct },
        }
    }

    fn on(headers: Vec<HeaderConfig>) -> FanConfig {
        FanConfig {
            enabled: true,
            headers,
            ..FanConfig::default()
        }
    }

    /// Ticks until the worker has handled the tick (it writes asynchronously).
    fn tick(fc: &FanControl, dev: &SimDevice, temp: f64, expect: usize) {
        fc.on_tick(|id| (id == CPU_TEMP).then_some(temp));
        for _ in 0..200 {
            if lock(&dev.0).log.len() >= expect {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("worker did not act: {:?}", lock(&dev.0).log);
    }

    fn settle() {
        std::thread::sleep(Duration::from_millis(50));
    }

    #[test]
    fn saves_bios_state_first_writes_on_change_and_restores_when_turned_off() {
        let dir = tmp_dir("basic");
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), Some(&dir), WATCHDOG);
        assert!(!fc.drives());
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        assert!(fc.drives());
        assert!(dir.join(CONFIG_FILE).exists());
        tick(&fc, &dev, 50.0, 2);
        assert_eq!(lock(&dev.0).log, ["capture", "set 0 102"]);
        assert!(
            dir.join(RESTORE_FILE).exists(),
            "BIOS state saved before writing"
        );
        // Same duty: no write.
        tick(&fc, &dev, 50.0, 2);
        settle();
        assert_eq!(lock(&dev.0).log.len(), 2);
        fc.restore().unwrap();
        for _ in 0..100 {
            if !dir.join(RESTORE_FILE).exists() {
                break;
            }
            settle();
        }
        assert_eq!(lock(&dev.0).log.last().unwrap(), "restore");
        assert!(!dir.join(RESTORE_FILE).exists());
        assert_eq!(lock(&dev.0).mode, 0);
        let saved = load_config(&dir.join(CONFIG_FILE), Some(dev.board()));
        assert!(!saved.enabled, "turning off is saved");
        drop(fc);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn switching_one_header_to_bios_releases_only_it() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0), fixed(2, 50.0)]))
            .unwrap();
        tick(&fc, &dev, 50.0, 3);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 4);
        assert_eq!(lock(&dev.0).log[3], "release 2");
        assert_eq!(lock(&dev.0).mode, 0b01);
    }

    #[test]
    fn shutdown_hands_back_to_the_bios() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        drop(fc);
        assert_eq!(lock(&dev.0).log.last().unwrap(), "restore");
        assert_eq!(lock(&dev.0).mode, 0);
    }

    fn wait_for_restore(dev: &SimDevice) {
        for _ in 0..200 {
            if lock(&dev.0).log.last().is_some_and(|l| l == "restore") {
                return;
            }
            settle();
        }
        panic!("no restore: {:?}", lock(&dev.0).log);
    }

    #[test]
    fn a_readback_mismatch_restores_and_turns_control_off() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        fc.set_config(on(vec![fixed(0, 60.0)])).unwrap();
        lock(&dev.0).fail = vec![DevError::Mismatch("register 0xa28 read back 0x66".into())];
        tick(&fc, &dev, 50.0, 3);
        wait_for_restore(&dev);
        let st = fc.status(|_| None);
        assert!(!st.config.enabled);
        assert!(st.last_error.unwrap().contains("turned off"));
        assert_eq!(lock(&dev.0).mode, 0);
    }

    #[test]
    fn repeated_io_errors_turn_control_off_after_three() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        fc.set_config(on(vec![fixed(0, 60.0)])).unwrap();
        let io = || DevError::Io("port I/O failed".into());
        lock(&dev.0).fail = vec![io(), io(), io()];
        for n in 3..=5 {
            tick(&fc, &dev, 50.0, n);
        }
        wait_for_restore(&dev);
        assert!(!fc.status(|_| None).config.enabled);
    }

    #[test]
    fn missing_ticks_trip_the_watchdog_and_control_resumes_with_them() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, Duration::from_millis(300));
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        // No ticks: within about a second the worker restores.
        wait_for_restore(&dev);
        assert!(fc.status(|_| None).last_error.unwrap().contains("stopped"));
        assert!(
            fc.status(|_| None).config.enabled,
            "still on: resumes with ticks"
        );
        tick(&fc, &dev, 50.0, 5);
        assert_eq!(&lock(&dev.0).log[3..5], ["capture", "set 0 102"]);
    }

    #[test]
    fn a_state_left_by_a_crash_is_restored_before_anything_else() {
        let dir = tmp_dir("crash");
        let left = BiosState {
            chip_id: 0xD5,
            chip_rev: 0x92,
            base: 0xA20,
            mode: 0,
            commands: vec![0x66; 8],
            saved_at: 1,
        };
        control::save(&dir.join(RESTORE_FILE), &left).unwrap();
        let dev = SimDevice::new();
        lock(&dev.0).mode = 0b1;
        let fc = FanControl::start(Box::new(dev.clone()), Some(&dir), WATCHDOG);
        assert_eq!(lock(&dev.0).log, ["restore"]);
        assert_eq!(lock(&dev.0).mode, 0);
        assert!(!dir.join(RESTORE_FILE).exists());
        drop(fc);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_configs_are_refused_and_a_bad_file_means_off() {
        let dir = tmp_dir("config");
        std::fs::write(dir.join(CONFIG_FILE), br#"{"enabled":true,"headers":[{"index":1,"mode":{"mode":"fixed","pct":10}}],"emergencyCpuC":95,"emergencyGpuC":90}"#).unwrap();
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), Some(&dir), WATCHDOG);
        assert!(
            !fc.status(|_| None).config.enabled,
            "pump config not loaded"
        );
        assert!(fc.set_config(on(vec![fixed(1, 80.0)])).is_err());
        drop(fc);
        let ro = FanControl::unavailable(FanCapability::ReadOnly {
            reason: "not validated".into(),
        });
        assert!(ro.set_config(on(vec![fixed(0, 50.0)])).is_err());
        assert!(!ro.drives());
        assert!(ro.status(|_| None).headers.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Ticks with `read` until `done` holds for the simulated chip (the worker acts
    /// asynchronously, and drift checks need several chip reads).
    fn tick_until(
        fc: &FanControl,
        dev: &SimDevice,
        read: impl Fn(&str) -> Option<f64> + Copy,
        done: impl Fn(&SimChip) -> bool,
    ) {
        for _ in 0..1000 {
            if done(&lock(&dev.0)) {
                return;
            }
            fc.on_tick(read);
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("condition not reached: {:?}", lock(&dev.0).log);
    }

    fn cool(id: &str) -> Option<f64> {
        (id == CPU_TEMP).then_some(50.0)
    }

    #[test]
    fn sleep_hands_back_to_the_bios_and_waking_captures_afresh() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        fc.set_suspended(true);
        assert_eq!(
            lock(&dev.0).log.last().unwrap(),
            "restore",
            "done before returning"
        );
        assert_eq!(lock(&dev.0).mode, 0);
        // Ticks while going to sleep write nothing.
        fc.on_tick(cool);
        settle();
        assert_eq!(lock(&dev.0).log.len(), 3);
        fc.set_suspended(false);
        tick(&fc, &dev, 50.0, 5);
        assert_eq!(&lock(&dev.0).log[3..5], ["capture", "set 0 102"]);
        assert!(
            fc.status(|_| None).last_error.is_none(),
            "not a watchdog trip"
        );
    }

    #[test]
    fn a_setting_the_chip_lost_is_written_again() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        // The EC was reset (the header is back on the BIOS curve).
        {
            let mut c = lock(&dev.0);
            c.mode = 0;
            c.outputs[0] = 153;
        }
        tick_until(&fc, &dev, cool, |c| c.log.len() >= 3);
        assert_eq!(lock(&dev.0).log[2], "set 0 102");
        assert_eq!(lock(&dev.0).mode, 1);
        assert!(fc.status(|_| None).config.enabled);
    }

    #[test]
    fn another_program_fighting_over_the_fans_ends_control() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        // Another tool sets its own duty after every write of Vyro's.
        lock(&dev.0).fight = Some(200);
        tick_until(&fc, &dev, cool, |c| {
            c.log.last().is_some_and(|l| l == "restore")
        });
        let rewrites = lock(&dev.0)
            .log
            .iter()
            .filter(|l| *l == "set 0 102")
            .count();
        assert_eq!(rewrites, 3, "written again twice before giving up");
        let st = fc.status(|_| None);
        assert!(!st.config.enabled);
        assert!(st.last_error.unwrap().contains("another program"));
    }

    #[test]
    fn a_stopped_pump_hands_everything_back() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        let mut cfg = on(vec![fixed(0, 40.0), fixed(1, 70.0)]);
        assert!(fc.set_config(cfg.clone()).is_err(), "pump not confirmed");
        cfg.pumps = vec![1];
        fc.set_config(cfg).unwrap();
        let spinning = |id: &str| match id {
            "fan.1" => Some(3900.0),
            _ => cool(id),
        };
        tick_until(&fc, &dev, spinning, |c| c.log.len() >= 3);
        assert_eq!(&lock(&dev.0).log[1..3], ["set 0 102", "set 1 179"]);
        let stopped = |id: &str| match id {
            "fan.1" => Some(0.0),
            _ => cool(id),
        };
        tick_until(&fc, &dev, stopped, |c| {
            c.log.last().is_some_and(|l| l == "restore")
        });
        let st = fc.status(|_| None);
        assert!(!st.config.enabled);
        assert!(st.last_error.unwrap().contains("pump may have stopped"));
        assert_eq!(lock(&dev.0).mode, 0);
    }

    #[test]
    fn profiles_switch_header_settings_and_the_choice_is_remembered() {
        let dir = tmp_dir("profile");
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), Some(&dir), WATCHDOG);
        let mut cfg = on(vec![fixed(0, 40.0)]);
        cfg.profiles.insert(Profile::Gaming, vec![fixed(0, 70.0)]);
        fc.set_config(cfg).unwrap();
        tick(&fc, &dev, 50.0, 2);
        assert_eq!(lock(&dev.0).log[1], "set 0 102");
        fc.set_profile(Profile::Gaming).unwrap();
        tick(&fc, &dev, 50.0, 3);
        assert_eq!(lock(&dev.0).log[2], "set 0 179");
        assert_eq!(fc.status(|_| None).profile, Profile::Gaming);
        drop(fc);
        let fc = FanControl::start(Box::new(SimDevice::new()), Some(&dir), WATCHDOG);
        assert_eq!(fc.status(|_| None).profile, Profile::Gaming, "remembered");
        drop(fc);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn events_are_logged_and_handed_to_the_sink() {
        let got = Arc::new(Mutex::new(Vec::<FanEvent>::new()));
        let sink = Arc::clone(&got);
        let dev = SimDevice::new();
        let fc = FanControl::start_with(
            Box::new(dev.clone()),
            None,
            WATCHDOG,
            Some(Box::new(move |e: &FanEvent| lock(&sink).push(e.clone()))),
        );
        fc.set_config(on(vec![fixed(0, 40.0)])).unwrap();
        tick(&fc, &dev, 50.0, 2);
        assert_eq!(
            fc.brief(),
            FanBrief {
                active: true,
                full_speed: false
            }
        );
        // Hot: full speed, then normal again.
        fc.on_tick(|id| (id == CPU_TEMP).then_some(99.0));
        settle();
        assert!(fc.brief().full_speed);
        tick(&fc, &dev, 50.0, 3);
        fc.restore().unwrap();
        wait_for_restore(&dev);
        let kinds: Vec<FanEventKind> = fc.events().iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                FanEventKind::On,
                FanEventKind::FullSpeed,
                FanEventKind::Normal,
                FanEventKind::Bios
            ]
        );
        assert!(fc.events()[1].message.starts_with("CPU at 99 °C"));
        for _ in 0..100 {
            if lock(&got).len() == 4 {
                break;
            }
            settle();
        }
        assert_eq!(*lock(&got), fc.events(), "the sink gets every event");
        assert!(!fc.brief().active);
    }

    #[test]
    fn status_reports_headers_targets_and_unconnected_fans() {
        let dev = SimDevice::new();
        let fc = FanControl::start(Box::new(dev.clone()), None, WATCHDOG);
        fc.set_config(on(vec![HeaderConfig {
            index: 0,
            mode: HeaderMode::Curve {
                curve: FanCurve::default(),
            },
        }]))
        .unwrap();
        tick(&fc, &dev, 40.0, 2);
        settle();
        let st = fc.status(|h| Some(if h == 0 { 1200.0 } else { 0.0 }));
        assert_eq!(
            st.capability,
            FanCapability::Writable {
                board: "MSI B650M GAMING PLUS WIFI".into()
            }
        );
        assert_eq!(st.headers.len(), 5);
        let cpu = &st.headers[0];
        // Taken over at the 60 % the BIOS was running, ramping down from there.
        assert_eq!(cpu.target_pct, Some(60.0));
        assert_eq!(cpu.source_c, Some(40.0));
        assert!(cpu.connected);
        assert!(st.headers[1].controllable, "the pump, once confirmed");
        assert_eq!(st.pump_min_pct, 60);
        assert!(!st.headers[2].connected, "0 RPM at 60 %");
    }
}

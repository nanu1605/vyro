//! The GPU tuning controller. A worker thread owns the card: it writes the settings in force,
//! reads them back every few seconds, and gives the card its stock values when anything goes
//! wrong. Stock is the driver's own (no offsets, its default power limit), so nothing needs
//! saving before the first write.
//!
//! - While the card runs other than stock, `gpu-tuned.json` says since when and with what. A
//!   start that finds it writes stock first, then reads the Windows logs: a crash or a display
//!   driver reset since then turns "apply at start" off and says so ([`super::crash_verdict`]).
//! - At start the preset in force is applied only with "apply at start" on and only if its
//!   settings passed the stability test; otherwise the card stays at stock until the owner
//!   applies a preset.
//! - Values the driver no longer reports (a driver reset, another tuning program) are written
//!   again; if that keeps happening, or a write fails, tuning holds the card at stock.
//! - A temperature over the limits ([`super::TempGuard`]) holds it at stock until the owner
//!   applies a preset again.
//! - Before sleep, at shutdown and when the service stops: stock. After waking: the preset again.
//! - A stability test puts settings on trial; the trial ends by itself unless the app renews it
//!   within [`TRIAL_LEASE`], so a test that crashed the app never leaves the card tuned.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{
    all_presets, crash_verdict, describe, drifted, find, suggestions, validate, Clocks, CrashKind,
    GpuInfo, Limits, Marker, PresetView, Settings, TempGuard, Trial, TuneBrief, TuneCapability,
    TuneConfig, TuneEvent, TuneEventKind, TuneStatus, Verdict,
};
use crate::lock;
use crate::profiles::Profile;
use crate::snapshot::GpuSnapshot;

pub const CONFIG_FILE: &str = "gpu-tune.json";
pub const MARKER_FILE: &str = "gpu-tuned.json";
/// The profile whose preset is in force (set by the app).
pub const PROFILE_FILE: &str = "gpu-profile.json";
/// Events kept in memory for the window.
pub const RECENT_EVENTS: usize = 100;
/// The driver's values are read back this often.
pub const CHECK_EVERY: Duration = Duration::from_secs(5);
/// A trial ends unless the app renews it within this time.
pub const TRIAL_LEASE: Duration = Duration::from_secs(30);
/// This many rewrites within [`DRIFT_WINDOW`] mean another program is tuning the card.
const MAX_DRIFTS: usize = 3;
const DRIFT_WINDOW: Duration = Duration::from_secs(120);

/// An NVIDIA card through its driver (or the simulator). Each call is one driver request.
pub trait TuneDevice: Send {
    fn info(&self) -> GpuInfo;
    fn limits(&self) -> Limits;
    /// The settings the driver reports now.
    fn read(&mut self) -> Result<Settings, String>;
    fn clocks(&mut self) -> Clocks;
    /// Writes `s`; the caller reads back.
    fn apply(&mut self, s: &Settings) -> Result<(), String>;
}

/// Crash events of the Windows logs newer than a Unix time.
pub type CrashLog = Box<dyn Fn(i64) -> Vec<(i64, CrashKind)> + Send + Sync>;
/// Where tuning events go besides memory (the service: the history store).
pub type EventSink = Box<dyn Fn(&TuneEvent) + Send + Sync>;

/// Where the controller keeps its files and sends its events.
pub struct Options {
    /// `None`: memory only (tests, the simulator).
    pub dir: Option<PathBuf>,
    pub events: Option<EventSink>,
    pub crashes: CrashLog,
    pub check_every: Duration,
}

impl Options {
    pub fn memory() -> Self {
        Self {
            dir: None,
            events: None,
            crashes: Box::new(|_| Vec::new()),
            check_every: CHECK_EVERY,
        }
    }
}

/// What Vyro last wrote.
#[derive(Debug, Clone, PartialEq)]
struct Written {
    settings: Settings,
    /// The preset it came from (`None`: stock, or a trial).
    preset: Option<String>,
    trial: bool,
}

struct Inner {
    config: TuneConfig,
    profile: Profile,
    presets: Vec<PresetView>,
    /// The preset in force is kept on the card. False after a start that did not apply it,
    /// "Return to stock", or a crash, until the owner applies a preset.
    armed: bool,
    /// Why the card is at stock although tuning is on (shown with an Apply button).
    waiting: Option<String>,
    /// A guard holds the card at stock until the owner applies a preset again.
    held: Option<String>,
    trial: Option<(Trial, Instant)>,
    suspended: bool,
    stop: bool,
    guard: TempGuard,
    written: Option<Written>,
    current: Option<Settings>,
    clocks: Clocks,
    last_error: Option<String>,
    recent: VecDeque<TuneEvent>,
    pending: Vec<TuneEvent>,
}

impl Inner {
    fn record(&mut self, kind: TuneEventKind, message: String) {
        log::info!("gpu tuning: {message}");
        let ev = TuneEvent {
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

    /// What the card should run now.
    fn desired(&mut self, uuid: &str) -> Written {
        let stock = Written {
            settings: Settings::STOCK,
            preset: None,
            trial: false,
        };
        if self.suspended || self.held.is_some() || !self.config.enabled || !self.ours(uuid) {
            return stock;
        }
        if let Some((t, until)) = &self.trial {
            if Instant::now() < *until {
                return Written {
                    settings: t.settings,
                    preset: None,
                    trial: true,
                };
            }
            self.trial = None;
            self.record(
                TuneEventKind::Failed,
                "The stability test stopped answering: the card went back to its preset".into(),
            );
        }
        if !self.armed {
            return stock;
        }
        let id = self.config.preset_for(self.profile).to_owned();
        match find(&self.presets, &id) {
            Some(s) => Written {
                settings: s,
                preset: Some(id),
                trial: false,
            },
            None => stock,
        }
    }

    /// Whether the presets were made on this card.
    fn ours(&self, uuid: &str) -> bool {
        self.config.gpu.as_deref().is_none_or(|g| g == uuid)
    }

    fn preset_name(&self, id: &str) -> String {
        self.presets
            .iter()
            .find(|p| p.preset.id == id)
            .map_or_else(|| id.to_owned(), |p| p.preset.name.clone())
    }
}

struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
    config_path: Option<PathBuf>,
    profile_path: Option<PathBuf>,
    marker_path: Option<PathBuf>,
    events: Option<EventSink>,
}

impl Shared {
    fn save(&self, cfg: &TuneConfig) -> Result<(), String> {
        self.config_path.as_deref().map_or(Ok(()), |p| {
            crate::files::write_json(p, cfg).map_err(|e| format!("saving {}: {e}", p.display()))
        })
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

/// Loads a saved configuration. The owner's presets are brought within the driver's ranges
/// (a driver update can narrow them); one that is unreadable or still invalid falls back to
/// "off", and a file that does not parse is set aside.
pub fn load_config(path: &Path, limits: &Limits, suggested: &[super::Preset]) -> TuneConfig {
    let mut cfg = match crate::files::read_json::<TuneConfig>(path, |_| Ok(())) {
        Ok(c) => c.unwrap_or_default(),
        Err(e) => {
            log::warn!("gpu tuning: {} not used: {e}", path.display());
            return TuneConfig::default();
        }
    };
    for p in &mut cfg.presets {
        let within = limits.clamp(p.settings);
        if within != p.settings {
            log::info!(
                "gpu tuning: \"{}\" brought within the driver's ranges: {}",
                p.name,
                describe(&within)
            );
            p.settings = within;
        }
    }
    match validate(&cfg, Some(limits), suggested) {
        Ok(()) => cfg,
        Err(e) => {
            log::warn!("gpu tuning: {} not used: {e}", path.display());
            TuneConfig::default()
        }
    }
}

/// GPU tuning for one engine; `Unavailable` / `ReadOnly` when there is nothing to tune.
pub struct TuneControl {
    capability: TuneCapability,
    gpu: Option<GpuInfo>,
    limits: Limits,
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

fn empty_shared() -> Arc<Shared> {
    Arc::new(Shared {
        inner: Mutex::new(Inner {
            config: TuneConfig::default(),
            profile: Profile::default(),
            presets: Vec::new(),
            armed: false,
            waiting: None,
            held: None,
            trial: None,
            suspended: false,
            stop: false,
            guard: TempGuard::default(),
            written: None,
            current: None,
            clocks: Clocks::default(),
            last_error: None,
            recent: VecDeque::new(),
            pending: Vec::new(),
        }),
        wake: Condvar::new(),
        config_path: None,
        profile_path: None,
        marker_path: None,
        events: None,
    })
}

impl TuneControl {
    pub fn unavailable(capability: TuneCapability) -> Self {
        Self {
            capability,
            gpu: None,
            limits: Limits::default(),
            shared: empty_shared(),
            worker: Mutex::new(None),
        }
    }

    /// Gives the card stock values if a run left it tuned, loads the configuration, applies the
    /// preset in force if it may be applied at start, and starts the worker.
    pub fn start(mut dev: Box<dyn TuneDevice>, opts: Options) -> Self {
        let gpu = dev.info();
        let limits = dev.limits();
        if !limits.any() {
            return Self::unavailable(TuneCapability::ReadOnly {
                reason: format!("the driver allows no changes on the {}", gpu.name),
            });
        }
        let Some(family) = gpu.family else {
            return Self::unavailable(TuneCapability::ReadOnly {
                reason: format!(
                    "Vyro tunes NVIDIA RTX 20 to RTX 50 cards; the {} is not one",
                    gpu.name
                ),
            });
        };
        let dir = opts.dir.as_deref();
        let suggested = suggestions(Some(family), gpu.laptop, &limits);
        let config_path = dir.map(|d| d.join(CONFIG_FILE));
        let profile_path = dir.map(|d| d.join(PROFILE_FILE));
        let marker_path = dir.map(|d| d.join(MARKER_FILE));
        let mut config = config_path
            .as_deref()
            .map_or_else(TuneConfig::default, |p| load_config(p, &limits, &suggested));
        let profile = profile_path
            .as_deref()
            .and_then(|p| {
                crate::files::read_json::<Profile>(p, |_| Ok(()))
                    .ok()
                    .flatten()
            })
            .unwrap_or_default();
        // A marker that cannot be read still means the card may be tuned.
        let left = marker_path.as_deref().map(|p| {
            crate::files::read_json::<Marker>(p, |_| Ok(()))
                .inspect_err(|e| log::warn!("gpu tuning: {} unreadable: {e}", p.display()))
        });
        let tuned_left = matches!(left, Some(Ok(Some(_)) | Err(_)));
        let marker = left.and_then(Result::ok).flatten();

        let mut inner = new_inner(config.clone(), profile, &gpu, &limits);
        let mut changed = false;
        if tuned_left {
            match dev.apply(&Settings::STOCK) {
                Ok(()) => {
                    if let Some(p) = &marker_path {
                        let _ = std::fs::remove_file(p);
                    }
                }
                Err(e) => {
                    let why = format!("Giving the card its stock values failed: {e}");
                    inner.last_error = Some(why.clone());
                    inner.held = Some(why.clone());
                    inner.record(TuneEventKind::Fault, why);
                }
            }
        }
        let since = marker.as_ref().map(|m| m.since);
        let events = since.map(|s| (opts.crashes)(s - 60)).unwrap_or_default();
        let verdict = match crash_verdict(marker, &events) {
            Verdict::Clean if tuned_left => Verdict::Unclean,
            v => v,
        };
        match verdict {
            Verdict::Clean => {}
            Verdict::Unclean => inner.record(
                TuneEventKind::Stock,
                "Vyro stopped while the card was tuned: it was given its stock values".into(),
            ),
            Verdict::Crashed { kind, marker, .. } => {
                let what = if marker.trial {
                    format!("a stability test ({})", describe(&marker.settings))
                } else {
                    format!(
                        "{} ({})",
                        inner.preset_name(&marker.preset),
                        describe(&marker.settings)
                    )
                };
                let mut why = format!(
                    "While the card ran {what}, {}. It is at stock",
                    kind.describe()
                );
                if !marker.trial && config.apply_at_start {
                    config.apply_at_start = false;
                    inner.config.apply_at_start = false;
                    changed = true;
                    why.push_str(" and \"Apply at start\" is off");
                }
                why.push('.');
                inner.record(TuneEventKind::Crash, why.clone());
                inner.waiting = Some(why);
            }
        }

        if inner.config.enabled && inner.waiting.is_none() && inner.held.is_none() {
            let id = inner.config.preset_for(profile).to_owned();
            let settings = find(&inner.presets, &id);
            let tested = settings.and_then(|s| inner.config.tested_at(&s)).is_some();
            if !inner.ours(&gpu.uuid) {
                inner.waiting = Some(
                    "Your presets were made on another card: apply one to use them on this one"
                        .into(),
                );
            } else if settings.is_some_and(|s| s.is_stock()) {
                inner.armed = true;
            } else if inner.config.apply_at_start && tested {
                inner.armed = true;
                let name = inner.preset_name(&id);
                inner.record(TuneEventKind::Applied, format!("{name} applied at start"));
            } else {
                let name = inner.preset_name(&id);
                inner.waiting = Some(if inner.config.apply_at_start {
                    format!(
                        "{name} is applied at start only once it passed the stability test: \
                         apply it, or test it"
                    )
                } else {
                    format!("{name} is chosen: apply it to use it")
                });
            }
        }

        let shared = Arc::new(Shared {
            inner: Mutex::new(inner),
            wake: Condvar::new(),
            config_path,
            profile_path,
            marker_path,
            events: opts.events,
        });
        if changed {
            if let Err(e) = shared.save(&config) {
                log::warn!("gpu tuning: {e}");
            }
        }
        shared.flush_events();
        let mut me = Self {
            capability: TuneCapability::Writable {
                gpu: gpu.name.clone(),
            },
            gpu: Some(gpu.clone()),
            limits,
            shared: Arc::clone(&shared),
            worker: Mutex::new(None),
        };
        let worker = Worker {
            dev,
            shared,
            uuid: gpu.uuid,
            limits,
            crashes: opts.crashes,
            check_every: opts.check_every,
            drifts: Vec::new(),
            since: None,
        };
        match std::thread::Builder::new()
            .name("vyro-gpu-tune".into())
            .spawn(move || worker.run())
        {
            Ok(h) => *lock(&me.worker) = Some(h),
            Err(e) => {
                log::error!("gpu tuning: worker thread: {e}");
                me.capability = TuneCapability::Unavailable {
                    reason: format!("could not start: {e}"),
                };
                me.gpu = None;
            }
        }
        me
    }

    pub fn capability(&self) -> &TuneCapability {
        &self.capability
    }

    fn writable(&self) -> Result<(), String> {
        match &self.capability {
            TuneCapability::Writable { .. } => Ok(()),
            TuneCapability::ReadOnly { reason } | TuneCapability::Unavailable { reason } => {
                Err(format!("GPU tuning is not available: {reason}"))
            }
        }
    }

    /// What each snapshot reports about tuning.
    pub fn brief(&self) -> TuneBrief {
        let inner = lock(&self.shared.inner);
        TuneBrief {
            tuned: inner
                .written
                .as_ref()
                .is_some_and(|w| !w.settings.is_stock()),
            held: inner.held.is_some(),
        }
    }

    pub fn status(&self) -> TuneStatus {
        let Some(gpu) = &self.gpu else {
            return match &self.capability {
                TuneCapability::Writable { .. } => TuneStatus::unavailable("not started"),
                c => TuneStatus {
                    capability: c.clone(),
                    ..TuneStatus::unavailable("")
                },
            };
        };
        let inner = lock(&self.shared.inner);
        let applied = inner
            .written
            .as_ref()
            .filter(|w| !w.trial && !w.settings.is_stock())
            .and_then(|w| w.preset.clone());
        TuneStatus {
            capability: self.capability.clone(),
            gpu: Some(gpu.clone()),
            limits: self.limits,
            config: inner.config.clone(),
            presets: inner.presets.clone(),
            current: inner.current,
            clocks: inner.clocks,
            applied,
            profile: inner.profile,
            held: inner.held.clone(),
            waiting: inner.waiting.clone(),
            last_error: inner.last_error.clone(),
            trial: inner.trial.as_ref().map(|(t, _)| t.clone()),
        }
    }

    /// Recent events, newest last (this run; the service keeps older ones in history).
    pub fn events(&self) -> Vec<TuneEvent> {
        lock(&self.shared.inner).recent.iter().cloned().collect()
    }

    /// Saves what the owner set. With `apply` the preset in force goes on the card (a guard's
    /// hold ends, since the owner chose again); without, only the presets and choices are kept
    /// (the card keeps what it runs, with the new values of a preset it runs).
    pub fn set_config(&self, mut cfg: TuneConfig, apply: bool) -> Result<(), String> {
        self.writable()?;
        let gpu = self.gpu.as_ref().ok_or("not started")?;
        let suggested = suggestions(gpu.family, gpu.laptop, &self.limits);
        validate(&cfg, Some(&self.limits), &suggested)?;
        if cfg.gpu.as_deref() != Some(gpu.uuid.as_str()) {
            if cfg.gpu.is_some() {
                cfg.tested.clear();
            }
            cfg.gpu = Some(gpu.uuid.clone());
        }
        self.shared.save(&cfg)?;
        {
            let mut inner = lock(&self.shared.inner);
            let was = inner.config.enabled;
            inner.presets = all_presets(&cfg, gpu.family, gpu.laptop, &self.limits);
            inner.config = cfg;
            match (was, inner.config.enabled) {
                (false, true) => inner.record(TuneEventKind::On, "GPU tuning turned on".into()),
                (true, false) => inner.record(TuneEventKind::Off, "GPU tuning turned off".into()),
                _ => {}
            }
            if inner.config.enabled && apply {
                inner.armed = true;
                inner.waiting = None;
                inner.held = None;
                inner.last_error = None;
                inner.guard = TempGuard::default();
                let id = inner.config.preset_for(inner.profile).to_owned();
                let name = inner.preset_name(&id);
                let message = match find(&inner.presets, &id) {
                    Some(s) if !s.is_stock() => format!("{name} applied ({})", describe(&s)),
                    _ => format!("{name} applied"),
                };
                inner.record(TuneEventKind::Applied, message);
            }
        }
        self.shared.flush_events();
        self.shared.wake.notify_all();
        Ok(())
    }

    /// "Return GPU to stock": stock until the owner applies a preset again.
    pub fn restore(&self) -> Result<(), String> {
        {
            let mut inner = lock(&self.shared.inner);
            inner.armed = false;
            inner.trial = None;
            if inner.config.enabled {
                inner.waiting =
                    Some("Returned to stock: apply a preset to tune the card again".into());
            }
            inner.record(
                TuneEventKind::Stock,
                "The card was returned to stock".into(),
            );
        }
        self.shared.flush_events();
        self.shared.wake.notify_all();
        Ok(())
    }

    /// The P9c profile in use: its preset applies, if it has one.
    pub fn set_profile(&self, profile: Profile) -> Result<(), String> {
        if let Some(p) = &self.shared.profile_path {
            crate::files::write_json(p, &profile).map_err(|e| e.to_string())?;
        }
        {
            let mut inner = lock(&self.shared.inner);
            if inner.profile == profile {
                return Ok(());
            }
            let before = inner.config.preset_for(inner.profile).to_owned();
            inner.profile = profile;
            let after = inner.config.preset_for(profile).to_owned();
            if before != after && inner.config.enabled && inner.armed {
                let name = inner.preset_name(&after);
                inner.record(
                    TuneEventKind::Profile,
                    format!("{} profile: {name}", profile.spec().name),
                );
            }
        }
        self.shared.flush_events();
        self.shared.wake.notify_all();
        Ok(())
    }

    /// The PC is going to sleep (`true`; stock before it does) or has woken (`false`).
    pub fn set_suspended(&self, on: bool) {
        {
            let mut inner = lock(&self.shared.inner);
            if inner.suspended == on {
                return;
            }
            inner.suspended = on;
            let tuned = inner
                .written
                .as_ref()
                .is_some_and(|w| !w.settings.is_stock());
            if on && tuned {
                inner.record(
                    TuneEventKind::Sleep,
                    "Going to sleep: the card is at stock".into(),
                );
            } else if !on && inner.armed && inner.config.enabled && inner.held.is_none() {
                inner.record(
                    TuneEventKind::Wake,
                    "Awake: the preset is applied again".into(),
                );
            }
        }
        self.shared.flush_events();
        self.shared.wake.notify_all();
        if on {
            // Give the worker a moment to write stock before the PC sleeps.
            let deadline = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < deadline {
                let done = lock(&self.shared.inner)
                    .written
                    .as_ref()
                    .is_none_or(|w| w.settings.is_stock());
                if done {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }

    /// Starts or renews a trial of `settings` for the stability test; `None` ends it.
    pub fn trial(&self, trial: Option<Trial>) -> Result<(), String> {
        self.writable()?;
        {
            let mut inner = lock(&self.shared.inner);
            match trial {
                Some(t) => {
                    if !inner.config.enabled {
                        return Err("Turn GPU tuning on first".into());
                    }
                    if let Some(why) = &inner.held {
                        return Err(format!("The card is held at stock: {why}"));
                    }
                    if !self.limits.allows(&t.settings) {
                        return Err("Those settings are outside what the driver allows".into());
                    }
                    inner.trial = Some((t, Instant::now() + TRIAL_LEASE));
                }
                None => inner.trial = None,
            }
        }
        self.shared.wake.notify_all();
        Ok(())
    }

    /// How a stability test of `settings` went.
    pub fn tested(&self, settings: Settings, passed: bool, note: &str) -> Result<(), String> {
        self.writable()?;
        let cfg = {
            let mut inner = lock(&self.shared.inner);
            let what = describe(&settings);
            if passed {
                inner.config.mark_tested(settings, crate::unix_now());
                let gpu = self.gpu.as_ref().ok_or("not started")?;
                let cfg = inner.config.clone();
                inner.presets = all_presets(&cfg, gpu.family, gpu.laptop, &self.limits);
                inner.record(
                    TuneEventKind::Passed,
                    format!("Passed the stability test: {what}"),
                );
            } else {
                let sep = if note.is_empty() { "" } else { ": " };
                inner.record(
                    TuneEventKind::Failed,
                    format!("Failed the stability test ({what}){sep}{note}"),
                );
            }
            inner.config.clone()
        };
        let saved = if passed {
            self.shared.save(&cfg)
        } else {
            Ok(())
        };
        self.shared.flush_events();
        saved
    }

    /// Checks the tuned card's temperatures (called by the poller every tick).
    pub fn on_tick(&self, gpus: &[GpuSnapshot]) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let tripped = {
            let mut inner = lock(&self.shared.inner);
            let tuned = inner
                .written
                .as_ref()
                .is_some_and(|w| !w.settings.is_stock());
            if !tuned || inner.held.is_some() {
                inner.guard = TempGuard::default();
                return;
            }
            let Some(card) = gpus.iter().find(|g| same_card(&g.name, &gpu.name)) else {
                return;
            };
            match inner.guard.check(card.temp_edge, card.temp_hotspot) {
                Some(reason) => {
                    let why = format!("At stock: {reason}. Apply a preset to try again");
                    inner.trial = None;
                    inner.held = Some(why.clone());
                    inner.record(TuneEventKind::Hot, why);
                    true
                }
                None => false,
            }
        };
        if tripped {
            self.shared.flush_events();
            self.shared.wake.notify_all();
        }
    }

    /// Stops the worker; the card gets its stock values.
    pub fn shutdown(&self) {
        lock(&self.shared.inner).stop = true;
        self.shared.wake.notify_all();
        if let Some(h) = lock(&self.worker).take() {
            let _ = h.join();
        }
        self.shared.flush_events();
    }
}

impl Drop for TuneControl {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn new_inner(config: TuneConfig, profile: Profile, gpu: &GpuInfo, limits: &Limits) -> Inner {
    let presets = all_presets(&config, gpu.family, gpu.laptop, limits);
    Inner {
        config,
        profile,
        presets,
        armed: false,
        waiting: None,
        held: None,
        trial: None,
        suspended: false,
        stop: false,
        guard: TempGuard::default(),
        written: None,
        current: None,
        clocks: Clocks::default(),
        last_error: None,
        recent: VecDeque::new(),
        pending: Vec::new(),
    }
}

/// Whether a snapshot's GPU name is the tuned card ("NVIDIA GeForce RTX 4070" in either).
fn same_card(snapshot: &str, nvml: &str) -> bool {
    let norm = |s: &str| {
        s.to_ascii_lowercase()
            .replace("nvidia", "")
            .trim()
            .to_owned()
    };
    let (a, b) = (norm(snapshot), norm(nvml));
    !a.is_empty() && (a == b || a.contains(&b) || b.contains(&a))
}

struct Worker {
    dev: Box<dyn TuneDevice>,
    shared: Arc<Shared>,
    uuid: String,
    limits: Limits,
    crashes: CrashLog,
    check_every: Duration,
    drifts: Vec<Instant>,
    /// When the settings now on the card went on (for the crash check).
    since: Option<i64>,
}

impl Worker {
    fn run(mut self) {
        let mut next_check = Instant::now();
        loop {
            let (want, stop) = {
                let mut inner = lock(&self.shared.inner);
                (inner.desired(&self.uuid), inner.stop)
            };
            if stop {
                break;
            }
            let written = lock(&self.shared.inner).written.clone();
            if written.as_ref() != Some(&want) {
                self.write(&want);
                next_check = Instant::now();
            }
            if Instant::now() >= next_check {
                self.check();
                next_check = Instant::now() + self.check_every;
            }
            self.shared.flush_events();
            let inner = lock(&self.shared.inner);
            let wait = next_check.saturating_duration_since(Instant::now());
            let _ = self.shared.wake.wait_timeout(inner, wait);
        }
        let stock = Written {
            settings: Settings::STOCK,
            preset: None,
            trial: false,
        };
        if lock(&self.shared.inner).written.as_ref() != Some(&stock) {
            self.write(&stock);
        }
        self.shared.flush_events();
    }

    /// Puts `want` on the card and keeps the marker in step: it goes down before tuned values
    /// reach the card, so a crash in between is still seen at the next start, and it is gone
    /// once the card is at stock.
    fn write(&mut self, want: &Written) {
        let marker = (!want.settings.is_stock()).then(|| Marker {
            since: crate::unix_now(),
            preset: want.preset.clone().unwrap_or_default(),
            settings: want.settings,
            trial: want.trial,
        });
        if let Some(m) = &marker {
            self.mark(Some(m));
        }
        let result = self.dev.apply(&want.settings);
        let failed = result.is_err();
        let read = self.dev.read().ok();
        let clocks = self.dev.clocks();
        let mut inner = lock(&self.shared.inner);
        inner.current = read;
        inner.clocks = clocks;
        match result {
            Ok(()) => match read {
                Some(r)
                    if !want.settings.is_stock() && drifted(&want.settings, &r, &self.limits) =>
                {
                    let why = format!(
                        "The driver did not keep {} (it reports {}). The card is at stock",
                        describe(&want.settings),
                        describe(&r)
                    );
                    inner.last_error = Some(why.clone());
                    inner.held = Some(why.clone());
                    inner.trial = None;
                    inner.record(TuneEventKind::Fault, why);
                    // Marked as written: the next loop writes stock, since tuning now holds.
                    inner.written = Some(want.clone());
                }
                _ => inner.written = Some(want.clone()),
            },
            Err(e) if want.settings.is_stock() => {
                let why = format!("Giving the card its stock values failed: {e}");
                inner.last_error = Some(why.clone());
                inner.record(TuneEventKind::Fault, why);
                // Recorded as written so the worker does not retry in a tight loop; the next
                // check reads what the card runs.
                inner.written = Some(want.clone());
            }
            Err(e) => {
                let why = format!(
                    "Writing {} failed: {e}. The card is at stock",
                    describe(&want.settings)
                );
                inner.last_error = Some(why.clone());
                inner.held = Some(why.clone());
                inner.trial = None;
                inner.record(TuneEventKind::Fault, why);
                // The driver takes the settings one at a time: those before the one it refused
                // are on the card. Not known what it runs, so the next loop writes stock.
                inner.written = None;
            }
        }
        // A write that failed may have left tuned values on the card: the marker stays until
        // stock is on.
        let tuned = failed
            || inner
                .written
                .as_ref()
                .is_some_and(|w| !w.settings.is_stock());
        drop(inner);
        let marker = marker.filter(|_| tuned);
        self.since = marker.as_ref().map(|m| m.since);
        if !tuned {
            self.mark(None);
        }
    }

    /// Writes the marker, or removes it.
    fn mark(&self, marker: Option<&Marker>) {
        let Some(path) = &self.shared.marker_path else {
            return;
        };
        match marker {
            Some(m) => {
                if let Err(e) = crate::files::write_json(path, m) {
                    log::warn!("gpu tuning: {}: {e}", path.display());
                }
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Reads the card back; values that changed are written again, or tuning holds at stock.
    fn check(&mut self) {
        let read = self.dev.read();
        let clocks = self.dev.clocks();
        let mut inner = lock(&self.shared.inner);
        inner.clocks = clocks;
        let Ok(read) = read else {
            return;
        };
        inner.current = Some(read);
        let Some(w) = inner.written.clone() else {
            return;
        };
        if w.settings.is_stock() || !drifted(&w.settings, &read, &self.limits) {
            return;
        }
        drop(inner);
        let crash = self
            .since
            .and_then(|since| (self.crashes)(since - 5).into_iter().next());
        let mut inner = lock(&self.shared.inner);
        if let Some((_, kind)) = crash {
            let what = if w.trial {
                "a stability test".to_owned()
            } else {
                w.preset
                    .as_deref()
                    .map(|p| inner.preset_name(p))
                    .unwrap_or_default()
            };
            let mut why = format!(
                "While the card ran {what} ({}), {}. It is at stock",
                describe(&w.settings),
                kind.describe()
            );
            if !w.trial && inner.config.apply_at_start {
                inner.config.apply_at_start = false;
                why.push_str(" and \"Apply at start\" is off");
                let cfg = inner.config.clone();
                if let Err(e) = self.shared.save(&cfg) {
                    log::warn!("gpu tuning: {e}");
                }
            }
            why.push('.');
            inner.trial = None;
            if !w.trial {
                inner.armed = false;
                inner.waiting = Some(why.clone());
            }
            inner.record(TuneEventKind::Crash, why);
            // What the card runs after a reset is stock; the next write makes sure.
            inner.written = None;
            return;
        }
        let now = Instant::now();
        self.drifts
            .retain(|t| now.duration_since(*t) < DRIFT_WINDOW);
        self.drifts.push(now);
        if self.drifts.len() >= MAX_DRIFTS {
            let why = "Another program keeps changing the card's clocks: Vyro stopped tuning it. \
                       Close the other tuning program, then apply a preset again"
                .to_owned();
            inner.held = Some(why.clone());
            inner.trial = None;
            inner.record(TuneEventKind::Fault, why);
            self.drifts.clear();
        } else {
            inner.record(
                TuneEventKind::Rewritten,
                format!(
                    "The card no longer ran {}: written again",
                    describe(&w.settings)
                ),
            );
            inner.written = None;
        }
    }
}

/// A simulated card for tests, the simulator and the browser preview's engine.
pub struct SimDevice {
    info: GpuInfo,
    limits: Limits,
    now: Settings,
    /// Writes fail while set (tests).
    pub fail: Arc<Mutex<bool>>,
    /// A memory offset is refused while set, after the power limit and core offset went on, as
    /// a driver that refuses one setting does (tests).
    pub refuse_mem: Arc<Mutex<bool>>,
    /// Another program's values, reported instead of Vyro's while set (tests).
    pub other: Arc<Mutex<Option<Settings>>>,
}

impl SimDevice {
    pub fn new() -> Self {
        Self {
            info: GpuInfo {
                name: "NVIDIA GeForce RTX 4070 (simulated)".into(),
                uuid: "GPU-sim".into(),
                family: Some(super::Family::Ada),
                laptop: false,
                driver: "sim".into(),
            },
            limits: Limits {
                core: Some(super::Range {
                    min: -500,
                    max: 1000,
                }),
                mem: Some(super::Range {
                    min: -1000,
                    max: 1500,
                }),
                power: Some(super::Range { min: 50, max: 115 }),
                power_default_w: Some(200.0),
            },
            now: Settings::STOCK,
            fail: Arc::new(Mutex::new(false)),
            refuse_mem: Arc::new(Mutex::new(false)),
            other: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_info(mut self, info: GpuInfo, limits: Limits) -> Self {
        self.info = info;
        self.limits = limits;
        self
    }
}

impl Default for SimDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl TuneDevice for SimDevice {
    fn info(&self) -> GpuInfo {
        self.info.clone()
    }

    fn limits(&self) -> Limits {
        self.limits
    }

    fn read(&mut self) -> Result<Settings, String> {
        Ok((*lock(&self.other)).unwrap_or(self.now))
    }

    fn clocks(&mut self) -> Clocks {
        let base = 2475.0;
        Clocks {
            core_max_mhz: Some(base + f64::from(self.now.core_mhz)),
            mem_max_mhz: Some(10_501.0 + f64::from(self.now.mem_mhz)),
            power_limit_w: self
                .limits
                .power_default_w
                .map(|w| w * f64::from(self.now.power_pct) / 100.0),
        }
    }

    fn apply(&mut self, s: &Settings) -> Result<(), String> {
        if *lock(&self.fail) {
            return Err("the driver refused".into());
        }
        *lock(&self.other) = None;
        if *lock(&self.refuse_mem) && s.mem_mhz != self.now.mem_mhz {
            self.now.power_pct = s.power_pct;
            self.now.core_mhz = s.core_mhz;
            return Err("the driver refused the memory offset".into());
        }
        self.now = *s;
        Ok(())
    }
}

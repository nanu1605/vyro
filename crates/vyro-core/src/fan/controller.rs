//! The fan controller: the poller decides speeds every tick ([`FanControl::on_tick`], pure
//! [`Runtime`]); a worker thread writes them to the chip, so a slow EC request never delays
//! a tick. Everything that can go wrong ends in the BIOS curve:
//!
//! - before the first write the BIOS state is saved to a file; it is written back and the file
//!   deleted when control ends (turned off, all headers on BIOS, shutdown);
//! - a register that does not keep a write, or three I/O errors in a row, restore and turn
//!   control off;
//! - no sensor tick for the watchdog period (the poller stopped) restores until ticks resume;
//! - a file left by a crash is written back when the next controller starts, before anything
//!   else (the service restarts itself after a crash).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{
    validate, Decision, FanCapability, FanConfig, FanStatus, HeaderMode, HeaderStatus, Runtime,
};
use crate::sio::control::{self, BiosState, Board, ControlError};
use crate::snapshot::FanKind;

pub const CONFIG_FILE: &str = "fan-control.json";
pub const RESTORE_FILE: &str = "fan-restore.json";
/// No tick for this long while driving fans → BIOS control until ticks resume.
pub const WATCHDOG: Duration = Duration::from_secs(10);
/// Longest tick interval while fans are driven (the poller's tray interval is longer).
pub const MAX_TICK: Duration = Duration::from_secs(2);
const MAX_IO_ERRORS: u32 = 3;
/// Chip state (mode, duties) is re-read every this many worker loops.
const READ_EVERY: u32 = 3;

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
    config: FanConfig,
    runtime: Runtime,
    decision: Option<Decision>,
    fresh: bool,
    last_tick: Option<Instant>,
    active: bool,
    override_reason: Option<String>,
    last_error: Option<String>,
    chip: Option<ChipState>,
    stop: bool,
}

struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
    config_path: Option<PathBuf>,
    started: Instant,
}

impl Shared {
    fn save(&self, cfg: &FanConfig) -> Result<(), String> {
        let Some(path) = &self.config_path else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| format!("saving {}: {e}", path.display()))
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
pub fn recover(dev: &mut dyn FanDevice, restore_path: &Path) {
    match control::load(restore_path) {
        Ok(None) => {}
        Ok(Some(state)) => match dev.restore(&state) {
            Ok(()) => {
                let _ = std::fs::remove_file(restore_path);
                log::warn!("fans: BIOS fan control restored after an unclean stop");
            }
            Err(e) => log::error!("fans: restoring the saved BIOS state failed: {e}"),
        },
        Err(e) => log::error!("fans: {} unreadable: {e}", restore_path.display()),
    }
}

fn shared(config: FanConfig, chip: Option<ChipState>, config_path: Option<PathBuf>) -> Arc<Shared> {
    Arc::new(Shared {
        inner: Mutex::new(Inner {
            config,
            runtime: Runtime::default(),
            decision: None,
            fresh: false,
            last_tick: None,
            active: false,
            override_reason: None,
            last_error: None,
            chip,
            stop: false,
        }),
        wake: Condvar::new(),
        config_path,
        started: Instant::now(),
    })
}

impl FanControl {
    pub fn unavailable(capability: FanCapability) -> Self {
        Self {
            capability,
            board: None,
            shared: shared(FanConfig::default(), None, None),
            worker: Mutex::new(None),
        }
    }

    /// Restores anything a crash left behind, loads the saved configuration and starts the
    /// worker. `dir` holds the configuration and the BIOS state file (`None`: memory only,
    /// for tests and the simulator).
    pub fn start(mut dev: Box<dyn FanDevice>, dir: Option<&Path>, watchdog: Duration) -> Self {
        let board = dev.board();
        let restore_path = dir.map(|d| d.join(RESTORE_FILE));
        if let Some(p) = &restore_path {
            recover(dev.as_mut(), p);
        }
        let config_path = dir.map(|d| d.join(CONFIG_FILE));
        let config = config_path
            .as_deref()
            .map_or_else(FanConfig::default, |p| load_config(p, Some(board)));
        let mut me = Self {
            capability: FanCapability::Writable {
                board: board.name.into(),
            },
            board: Some(board),
            shared: shared(config, dev.read().ok(), config_path),
            worker: Mutex::new(None),
        };
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
        self.board.is_some() && lock(&self.shared.inner).config.drives_any()
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
            for h in &inner.config.headers {
                if h.mode != HeaderMode::Bios {
                    if let Some(&raw) = chip.outputs.get(usize::from(h.index)) {
                        inner.runtime.seed(h.index, control::percent(raw));
                    }
                }
            }
        }
        let d = inner.runtime.decide(&inner.config, board, now, read);
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
        inner.config = cfg;
        inner.last_error = None;
        Ok(())
    }

    /// Turns control off (saved) and hands every header back to the BIOS.
    pub fn restore(&self) -> Result<(), String> {
        let cfg = {
            let mut inner = lock(&self.shared.inner);
            inner.config.enabled = false;
            inner.runtime.reset();
            inner.decision = Some(Decision::default());
            inner.fresh = true;
            inner.config.clone()
        };
        self.shared.wake.notify_one();
        self.shared.save(&cfg)
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
                    controllable: h.kind != FanKind::Pump,
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
}

enum Step {
    Stop,
    Stale,
    Apply(Option<Decision>),
}

impl Worker {
    fn run(mut self) {
        loop {
            match self.wait() {
                Step::Stop => {
                    self.restore_all();
                    return;
                }
                Step::Stale => {
                    if self.bios.is_some() {
                        self.restore_all();
                        if self.bios.is_none() {
                            self.note_error(
                                "sensor readings stopped: BIOS fan control restored".into(),
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
            let mut inner = lock(&self.shared.inner);
            if chip.is_some() {
                inner.chip = chip;
            }
            inner.active = self.bios.is_some() && !self.applied.is_empty();
        }
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
        let stale = inner.config.drives_any()
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

    /// Turns control off after a fault the controller must not retry.
    fn fail(&mut self, e: String) {
        self.restore_all();
        let cfg = {
            let mut inner = lock(&self.shared.inner);
            inner.config.enabled = false;
            inner.runtime.reset();
            inner.decision = None;
            inner.config.clone()
        };
        if let Err(save) = self.shared.save(&cfg) {
            log::warn!("fans: {save}");
        }
        self.note_error(format!(
            "{e}; fan control turned off and BIOS control restored"
        ));
    }

    fn apply(&mut self, d: &Decision) {
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
        let c = lock(&self.0);
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
        assert!(!st.headers[1].controllable, "pump");
        assert!(!st.headers[2].connected, "0 RPM at 60 %");
    }
}

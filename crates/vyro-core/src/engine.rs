//! The sensor engine as one object: registry, poller with its providers, history store,
//! System Health and alert rules. The sensor service runs it and serves it over IPC
//! ([`crate::ipc`]); the desktop app talks to it through [`EngineApi`], either in-process
//! ([`LocalEngine`], the fallback without the service) or remotely.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::alerts::{self, AlertEngine, AlertFired, AlertRule};
use crate::bench::{BenchRun, RunSummary};
use crate::changes::{HardwareChange, PcInventory, MAX_CHANGES};
use crate::diag::{self, DiagAppInfo, DiagReport, DiagSummary};
use crate::fan::controller::{self, FanControl, SimDevice};
use crate::fan::{FanCapability, FanConfig, FanEvent, FanStatus};
use crate::gaming::replay::{self, GameTimeline};
use crate::gaming::{CaptureTarget, GameContext, GameProvider, GameSession, Gaming};
use crate::health::{self, HealthEvent};
use crate::history::WindowSeries;
use crate::info::{DriveInfo, PawnIoStatus};
use crate::intel::{Insights, Intel};
use crate::poller::{EVENT_ALERT, EVENT_HEALTH};
use crate::profiles::Profile;
use crate::sio::control;
use crate::{
    lock, unix_now, History, HistoryConfig, HistoryRange, HistorySeries, Poller, PollerConfig,
    PollerHandle, Provider, Registry, SensorSnapshot, SensorTree, SnapshotSink,
};

/// Process ids of the connected UIs, counted in the app footprint with the engine's own.
pub type ClientPids = Arc<Mutex<Vec<u32>>>;

/// Poll interval bounds a client may ask for.
pub const MIN_POLL_MS: u32 = 250;
pub const MAX_POLL_MS: u32 = 10_000;

/// History database and alert rules, inside the engine's data folder.
pub const HISTORY_FILE: &str = "history.sqlite";
pub const ALERTS_FILE: &str = "alerts.json";

/// Where the app's sensor data comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BackendMode {
    /// The Vyro sensor service (full sensor set, history kept while the app is closed).
    Service,
    /// This app reads the sensors itself: without administrator rights, PawnIO and SMART
    /// sensors are missing.
    #[default]
    Local,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackendStatus {
    pub mode: BackendMode,
    /// Service mode: whether the connection is up (it reconnects by itself).
    pub connected: bool,
    pub service_version: Option<String>,
    /// Local mode: why the service is not used. Service mode: why the connection dropped.
    pub detail: Option<String>,
}

/// What the desktop app needs from the engine, wherever it runs. Remote calls can fail (the
/// service restarting), hence `Result`s even where the local engine cannot fail.
pub trait EngineApi: Send + Sync {
    /// Most recent snapshot, so a freshly loaded UI need not wait for the next tick.
    fn latest(&self) -> Option<SensorSnapshot>;
    /// Visible → the poll interval; hidden → the slow tray interval.
    fn set_visible(&self, visible: bool);
    /// Poll interval while visible (profiles), clamped to [`MIN_POLL_MS`, `MAX_POLL_MS`].
    fn set_poll_interval(&self, millis: u32);
    fn query_history(&self, metric: &str, range: HistoryRange) -> Result<HistorySeries, String>;
    fn sensor_tree(&self) -> Result<SensorTree, String>;
    /// Restarts min / max / avg for every sensor.
    fn reset_stats(&self) -> Result<(), String>;
    /// Recent System Health changes, oldest first.
    fn health_events(&self) -> Result<Vec<HealthEvent>, String>;
    fn alert_rules(&self) -> Result<Vec<AlertRule>, String>;
    /// Validates, applies and saves a new rule set.
    fn set_alert_rules(&self, rules: Vec<AlertRule>) -> Result<(), String>;
    /// Fired alerts, oldest first.
    fn alert_log(&self) -> Result<Vec<AlertFired>, String>;
    /// `sensor`'s usual reading now, for "above usual" conditions (P18): `None` until there
    /// is about a day of history.
    fn alert_usual(&self, sensor: &str) -> Result<Option<f64>, String>;
    /// Drives with full health data, read fresh (SMART needs administrator rights).
    fn drives(&self) -> Result<Vec<DriveInfo>, String>;
    /// Whether the engine can use PawnIO (CPU temperature and power, board and RAM sensors).
    fn pawnio_status(&self) -> Result<PawnIoStatus, String>;
    /// Changes the ping target (IPv4 address or host name).
    fn set_ping_host(&self, host: &str) -> Result<(), String>;
    /// BitLocker protection of the system drive (needs administrator rights).
    fn bitlocker_system_drive(&self) -> Result<Option<bool>, String>;
    /// The intelligence engine's findings (P16), with their evidence and causes, and the
    /// vital signs. Worked out from the history store; kept a minute.
    fn insights(&self) -> Result<Insights, String>;
    /// The Performance Score (P17) now, with every input, and the daily scores of the past
    /// year.
    fn performance_score(&self) -> Result<crate::score::ScoreReport, String>;
    /// Captures frames of this process (the fullscreen app in the foreground), or stops.
    fn set_capture_target(&self, target: Option<CaptureTarget>) -> Result<(), String>;
    /// The app's view of the captured game: in front, profile, overlay (`None`: no game).
    fn set_game_context(&self, context: Option<GameContext>) -> Result<(), String>;
    /// Logged game sessions, newest first.
    fn game_sessions(&self) -> Result<Vec<GameSession>, String>;
    /// A logged session with its samples (at most [`replay::MAX_POINTS`]) and markers; `None`
    /// once it is gone.
    fn game_timeline(&self, id: i64) -> Result<Option<GameTimeline>, String>;
    /// Sensor history from `from` to `to` (unix seconds): the context of a moment in a replay.
    /// At most [`MAX_WINDOW_METRICS`] metrics over at most [`MAX_WINDOW_SECS`].
    fn history_window(
        &self,
        metrics: &[String],
        from: i64,
        to: i64,
    ) -> Result<Vec<WindowSeries>, String>;
    /// Compares the PC as the app sees it with the last inventory and keeps the changes (P19).
    /// Returns the new ones.
    fn record_inventory(&self, inventory: PcInventory) -> Result<Vec<HardwareChange>, String>;
    /// The PC change history, newest first (at most `limit`).
    fn hardware_changes(&self, limit: u32) -> Result<Vec<HardwareChange>, String>;
    /// Keeps a benchmark run (P20; the app runs it); returns its id.
    fn add_benchmark(&self, run: BenchRun) -> Result<i64, String>;
    /// Kept benchmark runs, newest first, with their metrics.
    fn benchmark_runs(&self) -> Result<Vec<RunSummary>, String>;
    /// One kept benchmark run in full; `None` once it is gone.
    fn benchmark_run(&self, id: i64) -> Result<Option<BenchRun>, String>;
    /// Forgets a benchmark run; false when there was none.
    fn delete_benchmark(&self, id: i64) -> Result<bool, String>;
    /// Runs the diagnostics (P21) with what the app knows, keeps the report and returns it.
    fn run_diagnostics(&self, app: DiagAppInfo) -> Result<DiagReport, String>;
    /// Kept diagnostics reports, newest first.
    fn diagnostic_runs(&self) -> Result<Vec<DiagSummary>, String>;
    /// One kept report in full; `None` once it is gone.
    fn diagnostic_run(&self, id: i64) -> Result<Option<DiagReport>, String>;
    /// Forgets a report; false when there was none.
    fn delete_diagnostic(&self, id: i64) -> Result<bool, String>;
    /// Fan control: capability, configuration and each header's state.
    fn fan_status(&self) -> Result<FanStatus, String>;
    /// Validates, saves and applies a fan configuration.
    fn set_fan_config(&self, config: FanConfig) -> Result<(), String>;
    /// Turns fan control off and hands every header back to the BIOS.
    fn restore_fans(&self) -> Result<(), String>;
    /// Fan control log, newest last (kept across restarts where there is a history store).
    fn fan_events(&self) -> Result<Vec<FanEvent>, String>;
    /// The P9c profile in use: its own fan settings apply, if it has any.
    fn set_fan_profile(&self, profile: Profile) -> Result<(), String>;
    /// Stops polling and flushes history. Safe to call more than once.
    fn shutdown(&self);
}

/// Where the engine keeps its files and how often it polls.
#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    /// Folder for the history database and alert rules; `None` keeps both in memory only.
    pub data_dir: Option<PathBuf>,
    pub poller: PollerConfig,
    /// Drive fans (the service only; the app's fallback engine never does).
    pub fan_control: bool,
}

/// The engine running in this process.
pub struct LocalEngine {
    registry: Arc<Mutex<Registry>>,
    history: Option<Arc<Mutex<History>>>,
    alerts: Arc<Mutex<AlertEngine>>,
    alerts_path: Option<PathBuf>,
    poller: Mutex<Option<PollerHandle>>,
    clients: ClientPids,
    gaming: Arc<Gaming>,
    fans: Arc<FanControl>,
    intel: Arc<Intel>,
}

impl LocalEngine {
    /// Opens the data folder, loads the alert rules and starts polling `providers`. Every
    /// snapshot goes to `sink`. A data folder that cannot be used only disables history and
    /// saving rules.
    pub fn start(
        cfg: EngineConfig,
        providers: Vec<Box<dyn Provider>>,
        clients: ClientPids,
        sink: impl SnapshotSink,
    ) -> std::io::Result<Self> {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let dir = cfg.data_dir.as_deref().and_then(usable_dir);
        let history = dir.and_then(open_history).map(|h| Arc::new(Mutex::new(h)));
        let alerts_path = dir.map(|d| d.join(ALERTS_FILE));
        let rules = alerts_path
            .as_deref()
            .map_or_else(alerts::default_rules, alerts::load);
        log::info!("alerts: {} rule(s)", rules.len());
        let alerts = Arc::new(Mutex::new(AlertEngine::new(rules)));
        let gaming = Gaming::new(Gaming::default_launch(), dir, history.clone());
        let fans = Arc::new(Self::start_fans(cfg.fan_control, dir, history.clone()));
        let intel = Intel::new(Arc::clone(&registry), history.clone(), unix_now());
        intel.set_alert_sensors(lock(&alerts).usual_sensors(), unix_now());
        intel.set_score_sources(Self::score_sources());

        // The game provider runs last: its session summary reads this tick's temperatures.
        let game: Box<dyn Provider> = Box::new(GameProvider::new(Arc::clone(&gaming)));
        let mut poller = providers
            .into_iter()
            .chain(std::iter::once(game))
            .fold(
                Poller::new(Arc::clone(&registry), cfg.poller),
                Poller::with_boxed,
            )
            .with_alerts(Arc::clone(&alerts))
            .with_fans(Arc::clone(&fans))
            .with_intel(Arc::clone(&intel));
        if let Some(history) = &history {
            poller = poller.with_history(Arc::clone(history));
        }
        let handle = poller.spawn(sink)?;
        Ok(Self {
            registry,
            history,
            alerts,
            alerts_path,
            poller: Mutex::new(Some(handle)),
            clients,
            gaming,
            fans,
            intel,
        })
    }

    /// The fan controller: the simulated chip in simulation, the real one only in the service
    /// and only where `fan::open_device` allows it.
    fn start_fans(
        wanted: bool,
        dir: Option<&std::path::Path>,
        history: Option<Arc<Mutex<History>>>,
    ) -> FanControl {
        if Self::simulated() {
            return FanControl::start(Box::new(SimDevice::new()), None, controller::WATCHDOG);
        }
        if !wanted {
            return FanControl::unavailable(FanCapability::Unavailable {
                reason: "fan control runs in the Vyro service".into(),
            });
        }
        // Fan events are kept with the health and alert events.
        let events = history.map(|h| -> controller::EventSink {
            Box::new(move |ev: &FanEvent| {
                if let Err(e) = lock(&h).log_events(crate::fan::EVENT_KIND, ev.ts, &[ev]) {
                    log::warn!("fan events: {e}");
                }
            })
        });
        #[cfg(windows)]
        match crate::fan::open_device() {
            Ok(dev) => FanControl::start_with(dev, dir, controller::WATCHDOG, events),
            Err(cap) => {
                log::info!("fans: control not possible: {cap:?}");
                FanControl::unavailable(cap)
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (dir, events);
            FanControl::unavailable(FanCapability::Unavailable {
                reason: "fan control needs Windows".into(),
            })
        }
    }

    /// The real providers on Windows (the simulator with `VYRO_SIM=1`); the simulator
    /// elsewhere.
    pub fn default_providers(clients: &ClientPids) -> Vec<Box<dyn Provider>> {
        #[cfg(windows)]
        if std::env::var_os("VYRO_SIM").is_none_or(|v| v != "1") {
            log::info!("sensors: live providers");
            return crate::live::providers(Arc::clone(clients));
        }
        let _ = clients;
        log::info!("sensors: simulated");
        vec![Box::new(crate::SimProvider::new())]
    }

    /// The connected UI processes (for the app footprint).
    pub fn set_clients(&self, pids: Vec<u32>) {
        *lock(&self.clients) = pids;
    }

    /// The PC is about to sleep (`true`) or has woken (`false`): fans go back to the BIOS
    /// for the duration (the service forwards the power events).
    pub fn set_suspended(&self, on: bool) {
        self.fans.set_suspended(on);
    }

    /// The score's inputs outside the history: the CPU's brand, the drives and the event log
    /// (sample drives and a clean log in simulation).
    fn score_sources() -> crate::intel::runner::ScoreSources {
        #[cfg(target_arch = "x86_64")]
        let cpu_brand = crate::hwid::cpuid::read_live().brand;
        #[cfg(not(target_arch = "x86_64"))]
        let cpu_brand = None;
        let sim = Self::simulated();
        crate::intel::runner::ScoreSources {
            cpu_brand,
            drives: Box::new(move || {
                #[cfg(windows)]
                if !sim {
                    return crate::storage::collect();
                }
                crate::storage::sample()
            }),
            stability: Box::new(move |since| {
                if sim {
                    Some(crate::score::StabilityCounts::default())
                } else {
                    crate::sysinfo::stability_counts(since)
                }
            }),
        }
    }

    fn simulated() -> bool {
        !cfg!(windows) || std::env::var_os("VYRO_SIM").is_some_and(|v| v == "1")
    }
}

/// Limits of [`EngineApi::history_window`].
pub const MAX_WINDOW_METRICS: usize = 24;
pub const MAX_WINDOW_SECS: i64 = 600;

impl EngineApi for LocalEngine {
    fn latest(&self) -> Option<SensorSnapshot> {
        lock(&self.poller).as_ref().and_then(PollerHandle::latest)
    }

    fn set_visible(&self, visible: bool) {
        if let Some(poller) = lock(&self.poller).as_ref() {
            poller.set_visible(visible);
        }
    }

    fn set_poll_interval(&self, millis: u32) {
        let ms = millis.clamp(MIN_POLL_MS, MAX_POLL_MS);
        if let Some(poller) = lock(&self.poller).as_ref() {
            poller.set_interval(std::time::Duration::from_millis(u64::from(ms)));
        }
    }

    fn query_history(&self, metric: &str, range: HistoryRange) -> Result<HistorySeries, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history)
            .query(metric, range, unix_now())
            .map_err(|e| e.to_string())
    }

    fn sensor_tree(&self) -> Result<SensorTree, String> {
        Ok(lock(&self.registry).tree())
    }

    fn reset_stats(&self) -> Result<(), String> {
        lock(&self.registry).reset_stats();
        Ok(())
    }

    /// From the history store (kept across restarts), else from memory.
    fn health_events(&self) -> Result<Vec<HealthEvent>, String> {
        let stored = self.history.as_ref().and_then(|h| {
            lock(h)
                .events(EVENT_HEALTH, health::MAX_EVENTS as i64)
                .inspect_err(|e| log::warn!("health events: {e}"))
                .ok()
        });
        Ok(stored.unwrap_or_else(|| {
            lock(&self.poller)
                .as_ref()
                .map(PollerHandle::health_events)
                .unwrap_or_default()
        }))
    }

    fn alert_rules(&self) -> Result<Vec<AlertRule>, String> {
        Ok(lock(&self.alerts).rules().to_vec())
    }

    fn set_alert_rules(&self, rules: Vec<AlertRule>) -> Result<(), String> {
        let usual = {
            let mut alerts = lock(&self.alerts);
            alerts.set_rules(rules.clone())?;
            alerts.usual_sensors()
        };
        self.intel.set_alert_sensors(usual, unix_now());
        match &self.alerts_path {
            Some(path) => alerts::save(path, &rules)
                .map_err(|e| format!("rules applied but not saved ({}): {e}", path.display())),
            None => Err("rules applied but not saved: no data folder".into()),
        }
    }

    /// From the history store (kept across restarts), else from memory.
    fn alert_log(&self) -> Result<Vec<AlertFired>, String> {
        Ok(self
            .history
            .as_ref()
            .and_then(|h| {
                lock(h)
                    .events(EVENT_ALERT, alerts::MAX_LOG as i64)
                    .inspect_err(|e| log::warn!("alert log: {e}"))
                    .ok()
            })
            .unwrap_or_else(|| lock(&self.alerts).log()))
    }

    fn alert_usual(&self, sensor: &str) -> Result<Option<f64>, String> {
        self.intel.usual_for(sensor, unix_now())
    }

    fn drives(&self) -> Result<Vec<DriveInfo>, String> {
        #[cfg(windows)]
        if !Self::simulated() {
            return Ok(crate::storage::collect());
        }
        Ok(crate::storage::sample())
    }

    fn pawnio_status(&self) -> Result<PawnIoStatus, String> {
        Ok(crate::pawnio::status())
    }

    fn set_ping_host(&self, host: &str) -> Result<(), String> {
        #[cfg(windows)]
        return crate::network::set_ping_host(host);
        #[cfg(not(windows))]
        {
            if crate::network::valid_host(host) {
                Ok(())
            } else {
                Err("Enter an IPv4 address or a host name".into())
            }
        }
    }

    fn bitlocker_system_drive(&self) -> Result<Option<bool>, String> {
        Ok(if Self::simulated() {
            None
        } else {
            crate::sysinfo::bitlocker_system_drive()
        })
    }

    fn insights(&self) -> Result<Insights, String> {
        self.intel.insights(unix_now())
    }

    fn performance_score(&self) -> Result<crate::score::ScoreReport, String> {
        self.intel.score_report(unix_now())
    }

    fn set_capture_target(&self, target: Option<CaptureTarget>) -> Result<(), String> {
        self.gaming.set_target(target);
        Ok(())
    }

    fn set_game_context(&self, context: Option<GameContext>) -> Result<(), String> {
        self.gaming.set_context(context);
        Ok(())
    }

    fn game_sessions(&self) -> Result<Vec<GameSession>, String> {
        Ok(self.gaming.sessions())
    }

    fn game_timeline(&self, id: i64) -> Result<Option<GameTimeline>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history)
            .game_timeline(id, replay::MAX_POINTS)
            .map_err(|e| e.to_string())
    }

    fn history_window(
        &self,
        metrics: &[String],
        from: i64,
        to: i64,
    ) -> Result<Vec<WindowSeries>, String> {
        if metrics.len() > MAX_WINDOW_METRICS || to < from || to - from > MAX_WINDOW_SECS {
            return Err(format!(
                "at most {MAX_WINDOW_METRICS} readings over {MAX_WINDOW_SECS} s"
            ));
        }
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        let now = unix_now();
        let mut h = lock(history);
        metrics
            .iter()
            .map(|m| h.query_window(m, from, to, now).map_err(|e| e.to_string()))
            .collect()
    }

    fn record_inventory(&self, inventory: PcInventory) -> Result<Vec<HardwareChange>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history)
            .record_inventory(inventory, unix_now())
            .map_err(|e| e.to_string())
    }

    fn hardware_changes(&self, limit: u32) -> Result<Vec<HardwareChange>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history)
            .hardware_changes(i64::from(limit).min(MAX_CHANGES))
            .map_err(|e| e.to_string())
    }

    fn add_benchmark(&self, run: BenchRun) -> Result<i64, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history).add_benchmark(&run).map_err(|e| e.to_string())
    }

    fn benchmark_runs(&self) -> Result<Vec<RunSummary>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history).benchmark_runs().map_err(|e| e.to_string())
    }

    fn benchmark_run(&self, id: i64) -> Result<Option<BenchRun>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history).benchmark_run(id).map_err(|e| e.to_string())
    }

    fn delete_benchmark(&self, id: i64) -> Result<bool, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history)
            .delete_benchmark(id)
            .map_err(|e| e.to_string())
    }

    fn run_diagnostics(&self, app: DiagAppInfo) -> Result<DiagReport, String> {
        let now = unix_now();
        let (measures, insights, mut drives) = match self.intel.diagnostics_inputs(now) {
            Ok((m, i, d)) => (Some(m), i, d),
            Err(e) => {
                log::debug!("diagnostics: {e}");
                (None, None, Vec::new())
            }
        };
        if drives.is_empty() {
            drives = self.drives().unwrap_or_default();
        }
        let snapshot = self.latest();
        let events = self.health_events().unwrap_or_default();
        let pawnio = self.pawnio_status().ok();
        let fans = self.fan_status().ok();
        let checks = diag::checks::evaluate(&diag::checks::DiagInputs {
            now,
            measures: measures.as_ref(),
            insights: insights.as_ref(),
            health_events: &events,
            snapshot: snapshot.as_ref(),
            drives: &drives,
            pawnio: pawnio.as_ref(),
            fans: fans.as_ref(),
            app: &app,
        });
        let mut report = DiagReport {
            id: 0,
            ts: now,
            version: diag::CHECKS_VERSION,
            app_version: app.app_version.clone(),
            engine: diag::checks::engine_label(&app),
            pc: app.pc.clone(),
            checks,
        };
        if let Some(h) = &self.history {
            match lock(h).add_diagnostic(&report) {
                Ok(id) => report.id = id,
                Err(e) => log::warn!("diagnostics: not kept: {e}"),
            }
        }
        Ok(report)
    }

    fn diagnostic_runs(&self) -> Result<Vec<DiagSummary>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history).diagnostic_runs().map_err(|e| e.to_string())
    }

    fn diagnostic_run(&self, id: i64) -> Result<Option<DiagReport>, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history).diagnostic_run(id).map_err(|e| e.to_string())
    }

    fn delete_diagnostic(&self, id: i64) -> Result<bool, String> {
        let history = self.history.as_ref().ok_or("history store unavailable")?;
        lock(history)
            .delete_diagnostic(id)
            .map_err(|e| e.to_string())
    }

    fn fan_status(&self) -> Result<FanStatus, String> {
        // Registry first and released before the controller's lock (the poller takes them in
        // that order).
        let rpms: Vec<Option<f64>> = {
            let reg = lock(&self.registry);
            (0..control::HEADERS)
                .map(|n| reg.get(&crate::fan::rpm_id(n)).and_then(|s| s.value))
                .collect()
        };
        Ok(self
            .fans
            .status(|h| rpms.get(usize::from(h)).copied().flatten()))
    }

    fn set_fan_config(&self, config: FanConfig) -> Result<(), String> {
        self.fans.set_config(config)
    }

    fn restore_fans(&self) -> Result<(), String> {
        self.fans.restore()
    }

    /// From the history store, else this run's.
    fn fan_events(&self) -> Result<Vec<FanEvent>, String> {
        let stored = self.history.as_ref().and_then(|h| {
            lock(h)
                .events::<FanEvent>(crate::fan::EVENT_KIND, controller::RECENT_EVENTS as i64)
                .inspect_err(|e| log::warn!("fan events: {e}"))
                .ok()
        });
        Ok(stored.unwrap_or_else(|| self.fans.events()))
    }

    fn set_fan_profile(&self, profile: Profile) -> Result<(), String> {
        self.intel.set_profile(profile);
        self.fans.set_profile(profile)
    }

    fn shutdown(&self) {
        // Fans first: they go back to the BIOS while the sensors still run.
        self.fans.shutdown();
        self.gaming.stop();
        if let Some(poller) = lock(&self.poller).take() {
            poller.stop();
        }
    }
}

impl Drop for LocalEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn usable_dir(dir: &Path) -> Option<&Path> {
    match std::fs::create_dir_all(dir) {
        Ok(()) => Some(dir),
        Err(e) => {
            log::warn!("data folder {} unusable: {e}", dir.display());
            None
        }
    }
}

fn open_history(dir: &Path) -> Option<History> {
    let path = dir.join(HISTORY_FILE);
    match History::open(&path, HistoryConfig::default()) {
        Ok(history) => {
            log::info!("history store: {}", path.display());
            Some(history)
        }
        Err(e) => {
            log::warn!("history disabled: {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimProvider;
    use std::sync::mpsc;
    use std::time::Duration;

    fn fast() -> PollerConfig {
        PollerConfig {
            interval: Duration::from_millis(10),
            hidden_interval: Duration::from_secs(60),
        }
    }

    #[test]
    fn runs_polls_and_keeps_rules_and_history_in_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let cfg = EngineConfig {
            data_dir: Some(dir.path().join("data")),
            poller: fast(),
            fan_control: false,
        };
        let engine = LocalEngine::start(
            cfg,
            vec![Box::new(SimProvider::with_seed(4))],
            ClientPids::default(),
            move |s: &SensorSnapshot| {
                let _ = tx.send(s.clone());
            },
        )
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(engine.latest().is_some());
        assert!(!engine.sensor_tree().unwrap().groups.is_empty());

        let mut rules = engine.alert_rules().unwrap();
        rules.retain(|r| r.id != "gpu-temp");
        engine.set_alert_rules(rules.clone()).unwrap();
        assert_eq!(engine.alert_rules().unwrap(), rules);
        assert!(engine.set_ping_host("not a host!").is_err());
        // A few seconds of history: no baselines yet, so nothing to report.
        let insights = engine.insights().unwrap();
        assert!(insights.findings.is_empty(), "{insights:?}");
        assert_eq!(insights.vitals.len(), 6);
        // The Performance Score: drives and the (simulated) event log, but no score before
        // about a day of history; nothing kept.
        let report = engine.performance_score().unwrap();
        assert_eq!(report.latest.score, None);
        assert!(report.history.is_empty());
        let has = |id: crate::score::ScoreInputId| {
            report
                .latest
                .inputs
                .iter()
                .any(|i| i.id == id && i.score.is_some())
        };
        assert!(has(crate::score::ScoreInputId::Storage));
        assert!(has(crate::score::ScoreInputId::Stability));
        assert!(!has(crate::score::ScoreInputId::Thermals));
        // The PC change history starts with the first inventory.
        let first = engine.record_inventory(PcInventory::default()).unwrap();
        assert_eq!(engine.hardware_changes(10).unwrap(), first);
        engine.shutdown();
        engine.shutdown();

        assert!(dir.path().join("data").join(HISTORY_FILE).exists());
        assert_eq!(
            alerts::load(&dir.path().join("data").join(ALERTS_FILE)),
            rules
        );
    }
}

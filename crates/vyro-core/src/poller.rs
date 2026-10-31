//! Background polling loop (spec §4.2).
//!
//! One thread owns the providers. Each tick builds a fresh [`SensorSnapshot`], lets every
//! provider fill its part and update the registry, records history, then hands the snapshot to
//! the sink (the Tauri shell emits it as the `sensors` event). Control messages double as the
//! inter-tick sleep, so visibility changes and shutdown take effect immediately.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::alerts::{AlertEngine, Reading, TickInputs};
use crate::fan::{self, controller::FanControl};
use crate::gpu_tune::controller::TuneControl;
use crate::health::{HealthEvent, HealthMonitor, Reach};
use crate::history::History;
use crate::intel::Intel;
use crate::provider::Provider;
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{EngineStatus, SensorSnapshot};
use crate::{lock, process_start, unix_now};

/// History-store event kinds: System Health changes and fired alerts.
pub const EVENT_HEALTH: &str = "health";
pub const EVENT_ALERT: &str = "alert";

/// Fastest poll rate accepted from [`PollerHandle::set_interval`].
pub const MIN_INTERVAL: Duration = Duration::from_millis(10);

pub trait SnapshotSink: Send + 'static {
    fn emit(&self, snap: &SensorSnapshot);
}

impl<F> SnapshotSink for F
where
    F: Fn(&SensorSnapshot) + Send + 'static,
{
    fn emit(&self, snap: &SensorSnapshot) {
        self(snap);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollerConfig {
    /// Tick interval while the UI is visible.
    pub interval: Duration,
    /// Tick interval while the UI is hidden / in the tray.
    pub hidden_interval: Duration,
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            hidden_interval: Duration::from_secs(5),
        }
    }
}

enum Control {
    Visible(bool),
    Interval(Duration),
    Stop,
}

struct Slot {
    provider: Box<dyn Provider>,
    enabled: bool,
    last_error: Option<String>,
    /// When a slow poll was last logged.
    slow_logged: Option<Instant>,
}

/// A provider's poll longer than this is logged: the snapshot, and every reading in it, waits
/// for it (1.5 field check: the gauges lagged Task Manager).
const SLOW_POLL: Duration = Duration::from_millis(250);
/// At most one such line per provider this often.
const SLOW_LOG_EVERY: Duration = Duration::from_secs(300);

pub struct Poller {
    slots: Vec<Slot>,
    registry: Arc<Mutex<Registry>>,
    history: Option<Arc<Mutex<History>>>,
    history_error: Option<String>,
    health: Arc<Mutex<HealthMonitor>>,
    alerts: Option<Arc<Mutex<AlertEngine>>>,
    fans: Option<Arc<FanControl>>,
    tune: Option<Arc<TuneControl>>,
    intel: Option<Arc<Intel>>,
    cfg: PollerConfig,
    /// `app.cpu` / `app.ram`: the footprint as sensors, for averages, peaks and history.
    app_keys: Option<(SensorKey, SensorKey)>,
    startup_ms: Option<f64>,
}

impl Poller {
    pub fn new(registry: Arc<Mutex<Registry>>, cfg: PollerConfig) -> Self {
        Self {
            slots: Vec::new(),
            registry,
            history: None,
            history_error: None,
            health: Arc::new(Mutex::new(HealthMonitor::new())),
            alerts: None,
            fans: None,
            tune: None,
            intel: None,
            cfg,
            app_keys: None,
            startup_ms: None,
        }
    }

    pub fn with_provider(self, provider: impl Provider + 'static) -> Self {
        self.with_boxed(Box::new(provider))
    }

    pub fn with_boxed(mut self, provider: Box<dyn Provider>) -> Self {
        self.slots.push(Slot {
            provider,
            enabled: true,
            last_error: None,
            slow_logged: None,
        });
        self
    }

    pub fn with_history(mut self, history: Arc<Mutex<History>>) -> Self {
        self.history = Some(history);
        self
    }

    /// Evaluates these alert rules every tick; fired alerts go out in `SensorSnapshot::alerts`.
    pub fn with_alerts(mut self, alerts: Arc<Mutex<AlertEngine>>) -> Self {
        self.alerts = Some(alerts);
        self
    }

    /// Decides fan speeds every tick, right after the providers ran; while fans are driven the
    /// tray interval is capped so their sources stay fresh.
    pub fn with_fans(mut self, fans: Arc<FanControl>) -> Self {
        self.fans = Some(fans);
        self
    }

    /// Checks the tuned GPU's temperatures every tick (1.6); while it runs tuned the tray
    /// interval is capped as for fans.
    pub fn with_tune(mut self, tune: Arc<TuneControl>) -> Self {
        self.tune = Some(tune);
        self
    }

    /// Feeds the intelligence engine's minute digest every tick and puts its brief in the
    /// snapshot.
    pub fn with_intel(mut self, intel: Arc<Intel>) -> Self {
        self.intel = Some(intel);
        self
    }

    /// What the engine reads, so Health's notes say why a reading is missing.
    pub fn with_reach(self, reach: Reach) -> Self {
        lock(&self.health).set_reach(reach);
        self
    }

    /// Starts the poller thread. The first tick runs immediately.
    pub fn spawn(self, sink: impl SnapshotSink) -> std::io::Result<PollerHandle> {
        let (tx, rx) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let latest_thread = Arc::clone(&latest);
        let health = Arc::clone(&self.health);
        let join = thread::Builder::new()
            .name("vyro-poller".into())
            .spawn(move || self.run(&rx, &sink, &latest_thread))?;
        Ok(PollerHandle {
            tx,
            join: Some(join),
            latest,
            health,
        })
    }

    fn run(
        mut self,
        rx: &mpsc::Receiver<Control>,
        sink: &impl SnapshotSink,
        latest: &Mutex<Option<SensorSnapshot>>,
    ) {
        self.init_providers();
        let mut visible = true;
        let mut last_tick: Option<Instant> = None;
        loop {
            let started = Instant::now();
            let poll_hz =
                last_tick.map(|t| 1.0 / started.duration_since(t).as_secs_f64().max(1e-3));
            last_tick = Some(started);

            let snap = self.tick(visible, poll_hz);
            // Stored first, so whoever the sink wakes sees it as the latest.
            *lock(latest) = Some(snap.clone());
            sink.emit(&snap);

            loop {
                let interval = if visible {
                    self.cfg.interval
                } else if self.fans.as_ref().is_some_and(|f| f.drives())
                    || self.tune.as_ref().is_some_and(|t| t.brief().tuned)
                {
                    self.cfg.hidden_interval.min(fan::controller::MAX_TICK)
                } else {
                    self.cfg.hidden_interval
                };
                let Some(wait) = (started + interval).checked_duration_since(Instant::now()) else {
                    break;
                };
                match rx.recv_timeout(wait) {
                    Ok(Control::Visible(v)) => {
                        let shown = v && !visible;
                        visible = v;
                        if shown {
                            break; // fresh data as soon as the window comes back
                        }
                    }
                    Ok(Control::Interval(d)) => self.cfg.interval = d.max(MIN_INTERVAL),
                    Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => {
                        self.finish(latest);
                        return;
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
        }
    }

    fn init_providers(&mut self) {
        let mut reg = lock(&self.registry);
        for slot in &mut self.slots {
            let name = slot.provider.name();
            match catch_unwind(AssertUnwindSafe(|| slot.provider.init(&mut reg))) {
                Ok(Ok(())) => log::info!("provider {name}: ready"),
                Ok(Err(e)) => {
                    log::warn!("provider {name}: init failed, disabled: {e}");
                    slot.enabled = false;
                }
                Err(_) => {
                    log::error!("provider {name}: panicked during init, disabled");
                    slot.enabled = false;
                }
            }
        }
    }

    fn tick(&mut self, visible: bool, poll_hz: Option<f64>) -> SensorSnapshot {
        let mut snap = SensorSnapshot {
            ts: unix_now(),
            ..Default::default()
        };
        let mut reg = lock(&self.registry);
        for slot in self.slots.iter_mut().filter(|s| s.enabled) {
            let name = slot.provider.name();
            let started = Instant::now();
            let polled = catch_unwind(AssertUnwindSafe(|| slot.provider.poll(&mut reg, &mut snap)));
            let took = started.elapsed();
            if took > SLOW_POLL
                && slot
                    .slow_logged
                    .is_none_or(|t| t.elapsed() >= SLOW_LOG_EVERY)
            {
                log::info!("provider {name}: a poll took {} ms", took.as_millis());
                slot.slow_logged = Some(Instant::now());
            }
            match polled {
                Ok(Ok(())) => {
                    if slot.last_error.take().is_some() {
                        log::info!("provider {name}: recovered");
                    }
                }
                Ok(Err(e)) => {
                    // Log once per distinct error, not every tick.
                    let msg = e.to_string();
                    if slot.last_error.as_deref() != Some(msg.as_str()) {
                        log::warn!("provider {name}: {msg}");
                    }
                    slot.last_error = Some(msg);
                }
                Err(_) => {
                    log::error!("provider {name}: panicked during poll, disabled");
                    slot.enabled = false;
                }
            }
        }

        if let Some(fans) = &self.fans {
            fans.on_tick(|id| reg.get(id).and_then(|s| s.value));
            snap.fan_control = fans.brief();
        }
        if let Some(tune) = &self.tune {
            tune.on_tick(&snap.gpu);
            snap.gpu_tune = tune.brief();
        }

        let (cpu_key, ram_key) = *self.app_keys.get_or_insert_with(|| {
            let mut app = |id: &str, name: &str, unit| {
                reg.register(
                    SensorDesc::new(id, Group::App, name, unit, Source::App).with_history(),
                )
            };
            (
                app("app.cpu", "Vyro CPU", Unit::Percent),
                app("app.ram", "Vyro Memory", Unit::Megabyte),
            )
        });
        reg.set(cpu_key, snap.app.cpu_pct);
        reg.set(ram_key, snap.app.ram_mb);
        snap.app.cpu_avg_pct = reg.get("app.cpu").and_then(|s| s.avg);
        snap.app.ram_peak_mb = reg.get("app.ram").and_then(|s| s.max);
        snap.app.startup_ms = Some(
            *self
                .startup_ms
                .get_or_insert_with(|| process_start().elapsed().as_secs_f64() * 1000.0),
        );
        snap.app.poll_hz = poll_hz;
        snap.app.active_sensors = u32::try_from(reg.available_count()).unwrap_or(u32::MAX);
        snap.app.engine = if visible {
            EngineStatus::Running
        } else {
            EngineStatus::Throttled
        };

        snap.health = lock(&self.health).update(&snap);
        if let Some(alerts) = &self.alerts {
            let reading = |id: &str| {
                reg.get(id).and_then(|s| {
                    s.value.map(|value| Reading {
                        value,
                        unit: s.unit,
                        name: s.name.clone(),
                    })
                })
            };
            let usual = |id: &str| {
                self.intel
                    .as_ref()
                    .and_then(|i| i.usual_now(id, |load| reg.get(load).and_then(|s| s.value)))
            };
            let health = lock(&self.health);
            let mut engine = lock(alerts);
            snap.alerts = engine.evaluate_with(
                snap.ts,
                reading,
                &TickInputs {
                    health: health.fresh(),
                    usual: Some(&usual),
                    fans_on: snap.fan_control.active,
                    processes: &snap.processes,
                },
            );
            snap.alert_state = engine.brief();
            drop(engine);
            drop(health);
            if let Some(fans) = &self.fans {
                fans.set_boost(snap.alert_state.fan_boost());
            }
        }

        let digest = self.intel.as_ref().and_then(|intel| {
            snap.insights = intel.brief();
            intel.tick(&snap)
        });

        if let Some(history) = &self.history {
            let mut h = lock(history);
            if let Some(d) = digest {
                h.add_digest(d);
            }
            let fresh = lock(&self.health).fresh().to_vec();
            let result = h
                .log_events(EVENT_HEALTH, snap.ts, &fresh)
                .and_then(|()| h.log_events(EVENT_ALERT, snap.ts, &snap.alerts))
                .and_then(|()| h.record(snap.ts, reg.history_values()));
            drop(h);
            match result {
                Ok(()) => self.history_error = None,
                Err(e) => {
                    let msg = e.to_string();
                    if self.history_error.as_deref() != Some(msg.as_str()) {
                        log::warn!("history: {msg}");
                    }
                    self.history_error = Some(msg);
                }
            }
        }
        snap
    }

    fn finish(&mut self, latest: &Mutex<Option<SensorSnapshot>>) {
        if let Some(history) = &self.history {
            if let Err(e) = lock(history).flush_all() {
                log::warn!("history: final flush failed: {e}");
            }
        }
        if let Some(snap) = lock(latest).as_mut() {
            snap.app.engine = EngineStatus::Stopped;
        }
    }
}

/// Owner handle for the poller thread. Dropping it stops the thread.
pub struct PollerHandle {
    tx: Sender<Control>,
    join: Option<JoinHandle<()>>,
    latest: Arc<Mutex<Option<SensorSnapshot>>>,
    health: Arc<Mutex<HealthMonitor>>,
}

impl PollerHandle {
    /// Recent health changes (thermal limits, throttling, stopped fans…), oldest first.
    pub fn health_events(&self) -> Vec<HealthEvent> {
        lock(&self.health).events()
    }

    /// Visible → normal interval; hidden → `hidden_interval`. Becoming visible ticks at once.
    pub fn set_visible(&self, visible: bool) {
        let _ = self.tx.send(Control::Visible(visible));
    }

    pub fn set_interval(&self, interval: Duration) {
        let _ = self.tx.send(Control::Interval(interval));
    }

    /// Most recent snapshot, if a tick has completed.
    pub fn latest(&self) -> Option<SensorSnapshot> {
        lock(&self.latest).clone()
    }

    /// Stops the thread and waits for the final history flush.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.tx.send(Control::Stop);
        if let Some(join) = self.join.take() {
            if join.join().is_err() {
                log::error!("poller thread panicked");
            }
        }
    }
}

impl Drop for PollerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{HistoryConfig, HistoryRange};
    use crate::provider::BoxError;
    use crate::sim::SimProvider;
    use crate::snapshot::Status;
    use std::sync::atomic::{AtomicU32, Ordering};

    const WAIT: Duration = Duration::from_secs(5);

    fn fast() -> PollerConfig {
        PollerConfig {
            interval: Duration::from_millis(10),
            hidden_interval: Duration::from_secs(60),
        }
    }

    fn spawn(poller: Poller) -> (PollerHandle, mpsc::Receiver<SensorSnapshot>) {
        let (tx, rx) = mpsc::channel();
        let handle = poller
            .spawn(move |s: &SensorSnapshot| {
                let _ = tx.send(s.clone());
            })
            .unwrap();
        (handle, rx)
    }

    #[test]
    fn emits_snapshots_through_registry() {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let poller =
            Poller::new(Arc::clone(&registry), fast()).with_provider(SimProvider::with_seed(1));
        let (handle, rx) = spawn(poller);

        let snaps: Vec<_> = (0..3).map(|_| rx.recv_timeout(WAIT).unwrap()).collect();
        let last = snaps.last().unwrap();
        assert!(last.ts > 0);
        assert!(last.cpu.usage_total.is_some());
        assert!(last.app.poll_hz.is_some());
        assert!(last.app.active_sensors > 0);
        assert_eq!(last.app.engine, EngineStatus::Running);
        // Startup is measured once and repeated; the footprint sensors exist for history.
        assert!(last.app.startup_ms.is_some_and(|ms| ms >= 0.0));
        assert_eq!(snaps[0].app.startup_ms, last.app.startup_ms);
        assert!(lock(&registry).get("app.cpu").is_some_and(|s| s.history));
        assert!(lock(&registry)
            .get("cpu.usage.total")
            .unwrap()
            .avg
            .is_some());
        assert!(handle.latest().is_some());
        handle.stop();
    }

    #[test]
    fn hidden_throttles_and_showing_ticks_immediately() {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let poller = Poller::new(registry, fast()).with_provider(SimProvider::with_seed(2));
        let (handle, rx) = spawn(poller);
        rx.recv_timeout(WAIT).unwrap();

        handle.set_visible(false);
        // Drain ticks already in flight, then expect silence at the 60 s hidden interval.
        while rx.recv_timeout(Duration::from_millis(100)).is_ok() {}
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());

        handle.set_visible(true);
        let snap = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(snap.app.engine, EngineStatus::Running);
        handle.stop();
    }

    #[test]
    fn evaluates_alert_rules_and_grades_health_each_tick() {
        use crate::alerts::{AlertKind, AlertRule};
        let registry = Arc::new(Mutex::new(Registry::new()));
        let rule = AlertRule {
            id: "busy".into(),
            name: "Busy CPU".into(),
            enabled: true,
            kind: AlertKind::Threshold {
                sensor: "cpu.usage.total".into(),
                above: true,
                value: -1.0,
            },
            for_secs: 0,
            cooldown_secs: 3600,
            sound: false,
            actions: crate::alerts::AlertActions {
                record: true,
                profile: Some(crate::profiles::Profile::Silent),
                ..crate::alerts::AlertActions::default()
            },
        };
        let alerts = Arc::new(Mutex::new(AlertEngine::new(vec![rule])));
        let history = Arc::new(Mutex::new(
            History::open_in_memory(HistoryConfig::default()).unwrap(),
        ));
        let poller = Poller::new(registry, fast())
            .with_provider(SimProvider::with_seed(3))
            .with_alerts(Arc::clone(&alerts))
            .with_history(Arc::clone(&history));
        let (handle, rx) = spawn(poller);
        let snaps: Vec<_> = (0..3).map(|_| rx.recv_timeout(WAIT).unwrap()).collect();
        handle.stop();
        // Fires on the first tick, then stays quiet (latched).
        assert_eq!(snaps[0].alerts.len(), 1);
        assert!(snaps[0].alerts[0].message.starts_with("CPU Total at"));
        assert!(snaps[1..].iter().all(|s| s.alerts.is_empty()));
        assert_eq!(lock(&alerts).log().len(), 1);
        // It holds on every tick, so its profile stays asked for; the record has readings.
        assert!(snaps.iter().all(
            |s| s.alert_state.profile() == Some((crate::profiles::Profile::Silent, "Busy CPU"))
        ));
        let d = snaps[0].alerts[0].diagnostic.as_ref().unwrap();
        assert_eq!(d.readings[0].id, "cpu.usage.total");
        // Persisted for the next start.
        let stored: Vec<crate::alerts::AlertFired> =
            lock(&history).events(EVENT_ALERT, 10).unwrap();
        assert_eq!(stored, snaps[0].alerts);
        assert_ne!(snaps[0].health.overall, Status::Unknown);
    }

    struct Failing;
    impl Provider for Failing {
        fn name(&self) -> &'static str {
            "failing"
        }
        fn init(&mut self, _: &mut Registry) -> Result<(), BoxError> {
            Ok(())
        }
        fn poll(&mut self, _: &mut Registry, _: &mut SensorSnapshot) -> Result<(), BoxError> {
            Err("device went away".into())
        }
    }

    struct Panicking(Arc<AtomicU32>);
    impl Provider for Panicking {
        fn name(&self) -> &'static str {
            "panicking"
        }
        fn init(&mut self, _: &mut Registry) -> Result<(), BoxError> {
            Ok(())
        }
        fn poll(&mut self, _: &mut Registry, _: &mut SensorSnapshot) -> Result<(), BoxError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("bug in provider");
        }
    }

    #[test]
    fn failing_and_panicking_providers_do_not_break_the_tick() {
        let calls = Arc::new(AtomicU32::new(0));
        let registry = Arc::new(Mutex::new(Registry::new()));
        let poller = Poller::new(registry, fast())
            .with_provider(Failing)
            .with_provider(Panicking(Arc::clone(&calls)))
            .with_provider(SimProvider::with_seed(3));
        let (handle, rx) = spawn(poller);

        for _ in 0..3 {
            let snap = rx.recv_timeout(WAIT).unwrap();
            assert!(
                snap.cpu.usage_total.is_some(),
                "healthy provider still reports"
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "panicking provider disabled after first panic"
        );
        handle.stop();
    }

    #[test]
    fn records_history_and_flushes_on_stop() {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let history = Arc::new(Mutex::new(
            History::open_in_memory(HistoryConfig::default()).unwrap(),
        ));
        let poller = Poller::new(registry, fast())
            .with_provider(SimProvider::with_seed(4))
            .with_history(Arc::clone(&history));
        let (handle, rx) = spawn(poller);
        for _ in 0..3 {
            rx.recv_timeout(WAIT).unwrap();
        }
        handle.stop();

        let series = lock(&history)
            .query("cpu.usage.total", HistoryRange::M5, unix_now())
            .unwrap();
        assert!(!series.points.is_empty());
    }

    #[test]
    fn stop_returns_promptly_during_long_interval() {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let cfg = PollerConfig {
            interval: Duration::from_secs(60),
            ..fast()
        };
        let (handle, rx) =
            spawn(Poller::new(registry, cfg).with_provider(SimProvider::with_seed(5)));
        rx.recv_timeout(WAIT).unwrap();
        let t = Instant::now();
        handle.stop();
        assert!(t.elapsed() < Duration::from_secs(2));
    }
}

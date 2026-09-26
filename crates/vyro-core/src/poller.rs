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

use crate::health::{HealthEvent, HealthMonitor};
use crate::history::History;
use crate::provider::Provider;
use crate::registry::Registry;
use crate::snapshot::{EngineStatus, SensorSnapshot};
use crate::{lock, unix_now};

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
}

pub struct Poller {
    slots: Vec<Slot>,
    registry: Arc<Mutex<Registry>>,
    history: Option<Arc<Mutex<History>>>,
    history_error: Option<String>,
    health: Arc<Mutex<HealthMonitor>>,
    cfg: PollerConfig,
}

impl Poller {
    pub fn new(registry: Arc<Mutex<Registry>>, cfg: PollerConfig) -> Self {
        Self {
            slots: Vec::new(),
            registry,
            history: None,
            history_error: None,
            health: Arc::new(Mutex::new(HealthMonitor::new())),
            cfg,
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
        });
        self
    }

    pub fn with_history(mut self, history: Arc<Mutex<History>>) -> Self {
        self.history = Some(history);
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
            sink.emit(&snap);
            *lock(latest) = Some(snap);

            loop {
                let interval = if visible {
                    self.cfg.interval
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
            match catch_unwind(AssertUnwindSafe(|| slot.provider.poll(&mut reg, &mut snap))) {
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

        snap.app.poll_hz = poll_hz;
        snap.app.active_sensors = u32::try_from(reg.available_count()).unwrap_or(u32::MAX);
        snap.app.engine = if visible {
            EngineStatus::Running
        } else {
            EngineStatus::Throttled
        };

        snap.health = lock(&self.health).update(&snap);

        if let Some(history) = &self.history {
            let result = lock(history).record(snap.ts, reg.history_values());
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

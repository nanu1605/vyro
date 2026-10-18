//! The service side: accepts connections, answers commands and streams snapshots.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

use super::transport::{Conn, Listener};
use super::{
    read_frame, send, write_frame, Hello, Reply, Request, Role, Welcome, MAX_REQUEST,
    PROTOCOL_VERSION,
};
use crate::engine::EngineApi;
use crate::gaming::CaptureTarget;
use crate::{lock, SensorSnapshot};

/// Connections served at once; more are closed straight away (each holds a thread).
pub const MAX_CONNECTIONS: usize = 32;

/// Hands each snapshot, encoded once, to every subscribed connection. Slow subscribers skip
/// snapshots rather than queue them.
pub struct Hub {
    /// (sequence number, encoded snapshot).
    frame: Mutex<(u64, Option<Arc<Vec<u8>>>)>,
    cv: Condvar,
    closed: AtomicBool,
}

impl Hub {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            frame: Mutex::new((0, None)),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
        })
    }

    /// The engine's snapshot sink.
    pub fn publish(&self, snap: &SensorSnapshot) {
        match serde_json::to_vec(snap) {
            Ok(bytes) => {
                let mut f = lock(&self.frame);
                f.0 += 1;
                f.1 = Some(Arc::new(bytes));
                drop(f);
                self.cv.notify_all();
            }
            Err(e) => log::warn!("ipc: snapshot not encodable: {e}"),
        }
    }

    /// Ends every snapshot stream (service stopping).
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    /// Blocks until a snapshot newer than `seen` exists; `None` once closed.
    fn next(&self, seen: u64) -> Option<(u64, Arc<Vec<u8>>)> {
        let mut f = lock(&self.frame);
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return None;
            }
            if f.0 > seen {
                if let Some(bytes) = &f.1 {
                    return Some((f.0, Arc::clone(bytes)));
                }
            }
            f = self
                .cv
                .wait_timeout(f, Duration::from_secs(10))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

#[derive(Debug, Clone)]
struct Session {
    pid: u32,
    visible: bool,
    /// Capture target this client asked for, with the order it asked in (the latest wins).
    capture: Option<(u64, CaptureTarget)>,
}

/// Called with (any client visible, client process ids) whenever the set of connected
/// clients or their visibility changes.
pub type SessionsChanged = dyn Fn(bool, Vec<u32>) + Send + Sync;

pub struct Server {
    engine: Arc<dyn EngineApi>,
    hub: Arc<Hub>,
    version: String,
    sessions: Mutex<HashMap<u64, Session>>,
    on_sessions: Box<SessionsChanged>,
    active: AtomicUsize,
    capture_seq: AtomicU64,
    /// The capture target last handed to the engine.
    capture: Mutex<Option<CaptureTarget>>,
}

impl Server {
    /// `on_sessions` runs once now (no clients yet) and on every change.
    pub fn new(
        engine: Arc<dyn EngineApi>,
        hub: Arc<Hub>,
        version: impl Into<String>,
        on_sessions: impl Fn(bool, Vec<u32>) + Send + Sync + 'static,
    ) -> Arc<Self> {
        on_sessions(false, Vec::new());
        Arc::new(Self {
            engine,
            hub,
            version: version.into(),
            sessions: Mutex::new(HashMap::new()),
            on_sessions: Box::new(on_sessions),
            active: AtomicUsize::new(0),
            capture_seq: AtomicU64::new(0),
            capture: Mutex::new(None),
        })
    }

    /// Accepts clients forever, one thread each. Returns only if accepting fails repeatedly.
    pub fn serve(self: &Arc<Self>, listener: &Listener) -> io::Result<()> {
        let mut failures = 0;
        loop {
            match listener.accept() {
                Ok((conn, pid)) => {
                    failures = 0;
                    if self.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        self.active.fetch_sub(1, Ordering::SeqCst);
                        log::warn!("ipc: too many connections, refusing one");
                        continue;
                    }
                    let server = Arc::clone(self);
                    let spawned =
                        std::thread::Builder::new()
                            .name("vyro-ipc".into())
                            .spawn(move || {
                                server.handle(conn, pid);
                                server.active.fetch_sub(1, Ordering::SeqCst);
                            });
                    if let Err(e) = spawned {
                        self.active.fetch_sub(1, Ordering::SeqCst);
                        log::warn!("ipc: cannot start a connection thread: {e}");
                    }
                }
                Err(e) => {
                    failures += 1;
                    log::warn!("ipc: accept failed: {e}");
                    if failures >= 10 {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }

    /// Serves one connection until the client leaves.
    pub fn handle(&self, mut conn: Box<dyn Conn>, peer_pid: Option<u32>) {
        let Ok(hello) = super::recv::<Hello>(&mut conn, MAX_REQUEST) else {
            return;
        };
        if hello.protocol != PROTOCOL_VERSION {
            let refused: Result<Welcome, String> = Err(format!(
                "the sensor service speaks protocol {PROTOCOL_VERSION}, this app {}; \
                 reinstall Vyro so both match",
                hello.protocol
            ));
            let _ = send(&mut conn, &refused);
            return;
        }
        let welcome: Result<Welcome, String> = Ok(Welcome {
            protocol: PROTOCOL_VERSION,
            version: self.version.clone(),
        });
        if send(&mut conn, &welcome).is_err() {
            return;
        }
        match hello.role {
            Role::Commands => self.commands(&mut conn, hello.session),
            Role::Snapshots => {
                let pid = peer_pid.unwrap_or(hello.pid);
                self.update_sessions(|s| {
                    s.insert(
                        hello.session,
                        Session {
                            pid,
                            visible: hello.visible,
                            capture: None,
                        },
                    );
                });
                self.stream(&mut conn);
                self.update_sessions(|s| {
                    s.remove(&hello.session);
                });
            }
        }
    }

    fn commands(&self, conn: &mut Box<dyn Conn>, session: u64) {
        loop {
            let body = match read_frame(conn, MAX_REQUEST) {
                Ok(body) => body,
                Err(e) => {
                    if e.kind() == io::ErrorKind::InvalidData {
                        log::warn!("ipc: {e}");
                    }
                    return;
                }
            };
            let reply = match serde_json::from_slice::<Request>(&body) {
                Ok(req) => self.dispatch(session, req),
                Err(e) => Err(format!("unsupported request: {e}")),
            };
            if send(conn, &reply).is_err() {
                return;
            }
        }
    }

    fn stream(&self, conn: &mut Box<dyn Conn>) {
        let mut seen = 0;
        while let Some((seq, bytes)) = self.hub.next(seen) {
            seen = seq;
            if write_frame(conn, &bytes).is_err() {
                return;
            }
        }
    }

    fn update_sessions(&self, f: impl FnOnce(&mut HashMap<u64, Session>)) {
        let (visible, pids, capture) = {
            let mut s = lock(&self.sessions);
            f(&mut s);
            let visible = s.values().any(|s| s.visible);
            let mut pids: Vec<u32> = s.values().map(|s| s.pid).filter(|&p| p != 0).collect();
            pids.sort_unstable();
            pids.dedup();
            let capture = s
                .values()
                .filter_map(|s| s.capture.as_ref())
                .max_by_key(|(seq, _)| *seq)
                .map(|(_, t)| t.clone());
            (visible, pids, capture)
        };
        (self.on_sessions)(visible, pids);
        // A client that leaves takes its capture with it.
        let mut applied = lock(&self.capture);
        if *applied != capture {
            applied.clone_from(&capture);
            if let Err(e) = self.engine.set_capture_target(capture) {
                log::warn!("ipc: capture target: {e}");
            }
        }
    }

    fn dispatch(&self, session: u64, req: Request) -> Reply {
        fn json<T: Serialize>(r: Result<T, String>) -> Reply {
            r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        let e = &self.engine;
        match req {
            Request::Latest => json(Ok(e.latest())),
            Request::SetVisible { visible } => {
                // Only sessions with a live snapshot stream count; a commands-only client
                // cannot keep the service polling fast.
                self.update_sessions(|s| {
                    if let Some(s) = s.get_mut(&session) {
                        s.visible = visible;
                    }
                });
                json(Ok(()))
            }
            Request::SetPollInterval { millis } => {
                e.set_poll_interval(millis);
                json(Ok(()))
            }
            Request::QueryHistory { metric, range } => json(e.query_history(&metric, range)),
            Request::SensorTree => json(e.sensor_tree()),
            Request::ResetStats => json(e.reset_stats()),
            Request::HealthEvents => json(e.health_events()),
            Request::AlertRules => json(e.alert_rules()),
            Request::SetAlertRules { rules } => json(e.set_alert_rules(rules)),
            Request::AlertLog => json(e.alert_log()),
            Request::AlertUsual { sensor } => json(e.alert_usual(&sensor)),
            Request::Drives => json(e.drives()),
            Request::PawnIoStatus => json(e.pawnio_status()),
            Request::SetPingHost { host } => json(e.set_ping_host(&host)),
            Request::BitlockerSystemDrive => json(e.bitlocker_system_drive()),
            Request::Insights => json(e.insights()),
            Request::PerformanceScore => json(e.performance_score()),
            Request::SetCaptureTarget { target } => {
                let seq = self.capture_seq.fetch_add(1, Ordering::SeqCst);
                let mut known = false;
                self.update_sessions(|s| {
                    if let Some(s) = s.get_mut(&session) {
                        known = true;
                        s.capture = target.map(|t| (seq, t));
                    }
                });
                if known {
                    json(Ok(()))
                } else {
                    Err("capture needs a snapshot stream open".into())
                }
            }
            Request::SetGameContext { context } => json(e.set_game_context(context)),
            Request::GameSessions => json(e.game_sessions()),
            Request::GameTimeline { id } => json(e.game_timeline(id)),
            Request::HistoryWindow { metrics, from, to } => {
                json(e.history_window(&metrics, from, to))
            }
            Request::RecordInventory { inventory } => json(e.record_inventory(inventory)),
            Request::HardwareChanges { limit } => json(e.hardware_changes(limit)),
            Request::AddBenchmark { run } => json(e.add_benchmark(*run)),
            Request::BenchmarkRuns => json(e.benchmark_runs()),
            Request::BenchmarkRun { id } => json(e.benchmark_run(id)),
            Request::DeleteBenchmark { id } => json(e.delete_benchmark(id)),
            Request::RunDiagnostics { app } => json(e.run_diagnostics(app)),
            Request::DiagnosticRuns => json(e.diagnostic_runs()),
            Request::DiagnosticRun { id } => json(e.diagnostic_run(id)),
            Request::DeleteDiagnostic { id } => json(e.delete_diagnostic(id)),
            Request::FanStatus => json(e.fan_status()),
            Request::SetFanConfig { config } => json(e.set_fan_config(config)),
            Request::RestoreFans => json(e.restore_fans()),
            Request::FanEvents => json(e.fan_events()),
            Request::SetFanProfile { profile } => json(e.set_fan_profile(profile)),
            Request::BluetoothDisconnect { id } => json(e.bluetooth_disconnect(&id)),
            Request::GpuTuneStatus => json(e.gpu_tune_status()),
            Request::SetGpuTune { config } => json(e.set_gpu_tune(config)),
            Request::RestoreGpu => json(e.restore_gpu()),
            Request::GpuTuneEvents => json(e.gpu_tune_events()),
            Request::SetGpuTuneProfile { profile } => json(e.set_gpu_tune_profile(profile)),
            Request::GpuTrial { trial } => json(e.gpu_trial(trial)),
            Request::GpuTested {
                settings,
                passed,
                note,
            } => json(e.gpu_tested(settings, passed, note)),
        }
    }
}

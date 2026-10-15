//! The app side: [`RemoteEngine`] implements [`EngineApi`] by talking to the sensor service,
//! and reconnects by itself when the service restarts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::de::DeserializeOwned;

use super::transport::{connect, Conn};
use super::PROTOCOL_VERSION;
use super::{read_frame, recv, send, Hello, Reply, Request, Role, Welcome, MAX_MESSAGE};
use crate::alerts::{AlertFired, AlertRule};
use crate::changes::{HardwareChange, PcInventory};
use crate::engine::EngineApi;
use crate::fan::{FanConfig, FanEvent, FanStatus};
use crate::gaming::replay::GameTimeline;
use crate::gaming::{CaptureTarget, GameContext, GameSession};
use crate::health::HealthEvent;
use crate::history::WindowSeries;
use crate::info::{DriveInfo, PawnIoStatus};
use crate::profiles::Profile;
use crate::{lock, HistoryRange, HistorySeries, SensorSnapshot, SensorTree, SnapshotSink};

/// Longest pause between reconnection attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(5);

/// Connection to the service, as the app shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteStatus {
    pub connected: bool,
    /// Service version, once connected.
    pub version: Option<String>,
    /// Why the last attempt failed, while disconnected.
    pub error: Option<String>,
}

struct Shared {
    endpoint: String,
    session: u64,
    visible: AtomicBool,
    /// Poll interval asked for (0: the service's default); sent again after a reconnect.
    poll_ms: std::sync::atomic::AtomicU32,
    /// Capture target asked for; sent again after a reconnect.
    capture: Mutex<Option<CaptureTarget>>,
    game: Mutex<Option<GameContext>>,
    stop: AtomicBool,
    latest: Mutex<Option<SensorSnapshot>>,
    status: Mutex<RemoteStatus>,
    /// The commands connection. Dropped as soon as the snapshot stream breaks: a handle left
    /// open to a stopped service would keep its pipe alive and stop it from starting again.
    cmd: Mutex<Option<Box<dyn Conn>>>,
}

impl Shared {
    fn open(&self, role: Role) -> Result<(Box<dyn Conn>, Welcome), String> {
        let mut conn = connect(&self.endpoint).map_err(|e| e.to_string())?;
        let hello = Hello {
            protocol: PROTOCOL_VERSION,
            role,
            session: self.session,
            pid: std::process::id(),
            visible: self.visible.load(Ordering::SeqCst),
        };
        send(&mut conn, &hello).map_err(|e| e.to_string())?;
        let welcome: Result<Welcome, String> =
            recv(&mut conn, MAX_MESSAGE).map_err(|e| e.to_string())?;
        Ok((conn, welcome?))
    }

    /// Sends visibility and the poll interval on a short-lived commands connection (after a
    /// change, or to a service that restarted).
    fn push_state(self: &Arc<Self>) {
        let shared = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("vyro-state".into())
            .spawn(move || {
                if let Ok((mut conn, _)) = shared.open(Role::Commands) {
                    let mut reqs = vec![Request::SetVisible {
                        visible: shared.visible.load(Ordering::SeqCst),
                    }];
                    let ms = shared.poll_ms.load(Ordering::SeqCst);
                    if ms > 0 {
                        reqs.push(Request::SetPollInterval { millis: ms });
                    }
                    if let Some(target) = lock(&shared.capture).clone() {
                        reqs.push(Request::SetCaptureTarget {
                            target: Some(target),
                        });
                    }
                    if let Some(context) = *lock(&shared.game) {
                        reqs.push(Request::SetGameContext {
                            context: Some(context),
                        });
                    }
                    for req in reqs {
                        let sent =
                            send(&mut conn, &req).and_then(|()| read_frame(&mut conn, MAX_MESSAGE));
                        if sent.is_err() {
                            break;
                        }
                    }
                }
            });
    }

    /// Records a status change; true if it changed.
    fn set_status(&self, status: RemoteStatus) -> bool {
        let mut s = lock(&self.status);
        if *s == status {
            return false;
        }
        *s = status;
        true
    }
}

/// The engine in the sensor service.
pub struct RemoteEngine {
    shared: Arc<Shared>,
}

impl RemoteEngine {
    /// Connects to the service at `endpoint`. Fails if it does not answer or speaks another
    /// protocol version, so the caller can fall back to an in-process engine. Afterwards,
    /// every snapshot goes to `sink` and connection changes to `on_status`.
    pub fn connect(
        endpoint: &str,
        visible: bool,
        sink: impl SnapshotSink,
        on_status: impl Fn(&RemoteStatus) + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            endpoint: endpoint.to_owned(),
            session: fastrand::u64(..),
            visible: AtomicBool::new(visible),
            poll_ms: std::sync::atomic::AtomicU32::new(0),
            capture: Mutex::new(None),
            game: Mutex::new(None),
            stop: AtomicBool::new(false),
            latest: Mutex::new(None),
            status: Mutex::new(RemoteStatus::default()),
            cmd: Mutex::new(None),
        });
        let (cmd, welcome) = shared.open(Role::Commands)?;
        *lock(&shared.cmd) = Some(cmd);
        let (snaps, _) = shared.open(Role::Snapshots)?;
        shared.set_status(RemoteStatus {
            connected: true,
            version: Some(welcome.version),
            error: None,
        });
        let thread_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vyro-remote".into())
            .spawn(move || stream(&thread_shared, Some(snaps), &sink, &on_status))
            .map_err(|e| e.to_string())?;
        Ok(Self { shared })
    }

    pub fn status(&self) -> RemoteStatus {
        lock(&self.shared.status).clone()
    }

    /// One request/reply on the commands connection, reopening it once if it broke (the
    /// service restarted).
    fn call<T: DeserializeOwned>(&self, req: &Request) -> Result<T, String> {
        let mut guard = lock(&self.shared.cmd);
        let mut last_err = String::new();
        for _ in 0..2 {
            if guard.is_none() {
                *guard = Some(self.shared.open(Role::Commands)?.0);
            }
            let Some(conn) = guard.as_mut() else {
                continue;
            };
            let exchange = send(conn, req).and_then(|()| {
                let body = read_frame(conn, MAX_MESSAGE)?;
                serde_json::from_slice::<Reply>(&body)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            });
            match exchange {
                Ok(reply) => {
                    return reply.and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
                }
                Err(e) => {
                    *guard = None;
                    last_err = e.to_string();
                }
            }
        }
        Err(format!("sensor service unavailable: {last_err}"))
    }
}

/// Reads snapshots until told to stop, reconnecting with backoff whenever the stream breaks.
fn stream(
    shared: &Arc<Shared>,
    mut conn: Option<Box<dyn Conn>>,
    sink: &impl SnapshotSink,
    on_status: &impl Fn(&RemoteStatus),
) {
    let mut backoff = Duration::from_millis(250);
    let mut bad_snapshot_logged = false;
    while !shared.stop.load(Ordering::SeqCst) {
        let mut c = match conn.take() {
            Some(c) => c,
            None => match shared.open(Role::Snapshots) {
                Ok((c, welcome)) => {
                    log::info!("sensor service: reconnected ({})", welcome.version);
                    backoff = Duration::from_millis(250);
                    let status = RemoteStatus {
                        connected: true,
                        version: Some(welcome.version),
                        error: None,
                    };
                    if shared.set_status(status.clone()) {
                        on_status(&status);
                    }
                    // A restarted service starts at its defaults.
                    shared.push_state();
                    c
                }
                Err(e) => {
                    let status = RemoteStatus {
                        connected: false,
                        version: None,
                        error: Some(e),
                    };
                    if shared.set_status(status.clone()) {
                        log::warn!("sensor service: {}", status.error.as_deref().unwrap_or(""));
                        on_status(&status);
                    }
                    std::thread::sleep(backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                    continue;
                }
            },
        };
        loop {
            // Shut down: closing the stream ends this client's session on the service.
            if shared.stop.load(Ordering::SeqCst) {
                return;
            }
            match read_frame(&mut c, MAX_MESSAGE) {
                Ok(body) => match serde_json::from_slice::<SensorSnapshot>(&body) {
                    Ok(snap) => {
                        *lock(&shared.latest) = Some(snap.clone());
                        sink.emit(&snap);
                    }
                    Err(e) if !bad_snapshot_logged => {
                        bad_snapshot_logged = true;
                        log::warn!("sensor service: unreadable snapshot: {e}");
                    }
                    Err(_) => {}
                },
                Err(e) => {
                    drop(c);
                    lock(&shared.cmd).take();
                    if shared.stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let status = RemoteStatus {
                        connected: false,
                        version: None,
                        error: Some(format!("connection lost: {e}")),
                    };
                    if shared.set_status(status.clone()) {
                        log::warn!("sensor service: connection lost: {e}");
                        on_status(&status);
                    }
                    break;
                }
            }
        }
    }
}

impl EngineApi for RemoteEngine {
    fn latest(&self) -> Option<SensorSnapshot> {
        lock(&self.shared.latest).clone()
    }

    /// Remembered for reconnects and sent in the background (never blocks the caller).
    fn set_visible(&self, visible: bool) {
        self.shared.visible.store(visible, Ordering::SeqCst);
        // In the background: a slow service must not hold up the window.
        self.shared.push_state();
    }

    /// Remembered for reconnects and sent in the background.
    fn set_poll_interval(&self, millis: u32) {
        self.shared.poll_ms.store(millis, Ordering::SeqCst);
        self.shared.push_state();
    }

    fn query_history(&self, metric: &str, range: HistoryRange) -> Result<HistorySeries, String> {
        self.call(&Request::QueryHistory {
            metric: metric.to_owned(),
            range,
        })
    }

    fn sensor_tree(&self) -> Result<SensorTree, String> {
        self.call(&Request::SensorTree)
    }

    fn reset_stats(&self) -> Result<(), String> {
        self.call(&Request::ResetStats)
    }

    fn health_events(&self) -> Result<Vec<HealthEvent>, String> {
        self.call(&Request::HealthEvents)
    }

    fn alert_rules(&self) -> Result<Vec<AlertRule>, String> {
        self.call(&Request::AlertRules)
    }

    fn set_alert_rules(&self, rules: Vec<AlertRule>) -> Result<(), String> {
        self.call(&Request::SetAlertRules { rules })
    }

    fn alert_log(&self) -> Result<Vec<AlertFired>, String> {
        self.call(&Request::AlertLog)
    }

    fn drives(&self) -> Result<Vec<DriveInfo>, String> {
        self.call(&Request::Drives)
    }

    fn pawnio_status(&self) -> Result<PawnIoStatus, String> {
        self.call(&Request::PawnIoStatus)
    }

    fn set_ping_host(&self, host: &str) -> Result<(), String> {
        self.call(&Request::SetPingHost {
            host: host.to_owned(),
        })
    }

    fn bitlocker_system_drive(&self) -> Result<Option<bool>, String> {
        self.call(&Request::BitlockerSystemDrive)
    }

    fn insights(&self) -> Result<Vec<crate::analysis::Insight>, String> {
        self.call(&Request::Insights)
    }

    /// Remembered for reconnects.
    fn set_capture_target(&self, target: Option<CaptureTarget>) -> Result<(), String> {
        lock(&self.shared.capture).clone_from(&target);
        self.call(&Request::SetCaptureTarget { target })
    }

    /// Remembered for reconnects.
    fn set_game_context(&self, context: Option<GameContext>) -> Result<(), String> {
        *lock(&self.shared.game) = context;
        self.call(&Request::SetGameContext { context })
    }

    fn game_sessions(&self) -> Result<Vec<GameSession>, String> {
        self.call(&Request::GameSessions)
    }

    fn game_timeline(&self, id: i64) -> Result<Option<GameTimeline>, String> {
        self.call(&Request::GameTimeline { id })
    }

    fn history_window(
        &self,
        metrics: &[String],
        from: i64,
        to: i64,
    ) -> Result<Vec<WindowSeries>, String> {
        self.call(&Request::HistoryWindow {
            metrics: metrics.to_vec(),
            from,
            to,
        })
    }

    fn record_inventory(&self, inventory: PcInventory) -> Result<Vec<HardwareChange>, String> {
        self.call(&Request::RecordInventory { inventory })
    }

    fn hardware_changes(&self, limit: u32) -> Result<Vec<HardwareChange>, String> {
        self.call(&Request::HardwareChanges { limit })
    }

    fn fan_status(&self) -> Result<FanStatus, String> {
        self.call(&Request::FanStatus)
    }

    fn set_fan_config(&self, config: FanConfig) -> Result<(), String> {
        self.call(&Request::SetFanConfig { config })
    }

    fn restore_fans(&self) -> Result<(), String> {
        self.call(&Request::RestoreFans)
    }

    fn fan_events(&self) -> Result<Vec<FanEvent>, String> {
        self.call(&Request::FanEvents)
    }

    fn set_fan_profile(&self, profile: Profile) -> Result<(), String> {
        self.call(&Request::SetFanProfile { profile })
    }

    /// Stops reconnecting; the service keeps running.
    fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        lock(&self.shared.cmd).take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ClientPids, EngineConfig, LocalEngine};
    use crate::ipc::{Hub, Listener, Server};
    use crate::{PollerConfig, SimProvider};
    use std::sync::mpsc;

    const WAIT: Duration = Duration::from_secs(5);

    /// A private endpoint: a socket in `dir`, or a uniquely named pipe on Windows.
    fn endpoint(dir: &tempfile::TempDir) -> String {
        #[cfg(windows)]
        {
            let _ = dir;
            format!(r"\\.\pipe\vyro-test-{:016x}", fastrand::u64(..))
        }
        #[cfg(unix)]
        {
            dir.path()
                .join("engine.sock")
                .to_string_lossy()
                .into_owned()
        }
    }

    /// A service on a temporary socket: simulated sensors, 10 ms ticks.
    fn service(endpoint: &str) -> (Arc<LocalEngine>, mpsc::Receiver<(bool, Vec<u32>)>) {
        let hub = Hub::new();
        let publish = Arc::clone(&hub);
        let engine = Arc::new(
            LocalEngine::start(
                EngineConfig {
                    data_dir: None,
                    poller: PollerConfig {
                        interval: Duration::from_millis(10),
                        hidden_interval: Duration::from_millis(10),
                    },
                    fan_control: false,
                },
                vec![Box::new(SimProvider::with_seed(9))],
                ClientPids::default(),
                move |s: &SensorSnapshot| publish.publish(s),
            )
            .expect("engine starts"),
        );
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let server = Server::new(
            Arc::clone(&engine) as Arc<dyn EngineApi>,
            hub,
            "test",
            move |visible, pids| {
                let _ = lock(&tx).send((visible, pids));
            },
        );
        let listener = Listener::bind(endpoint).expect("socket binds");
        std::thread::spawn(move || server.serve(&listener));
        (engine, rx)
    }

    fn client(
        endpoint: &str,
    ) -> (
        RemoteEngine,
        mpsc::Receiver<SensorSnapshot>,
        mpsc::Receiver<RemoteStatus>,
    ) {
        let (snaps_tx, snaps) = mpsc::channel();
        let (status_tx, statuses) = mpsc::channel();
        let snaps_tx = Mutex::new(snaps_tx);
        let status_tx = Mutex::new(status_tx);
        let remote = RemoteEngine::connect(
            endpoint,
            true,
            move |s: &SensorSnapshot| {
                let _ = lock(&snaps_tx).send(s.clone());
            },
            move |s: &RemoteStatus| {
                let _ = lock(&status_tx).send(s.clone());
            },
        )
        .expect("service answers");
        (remote, snaps, statuses)
    }

    #[test]
    fn serves_snapshots_and_commands_and_tracks_client_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = endpoint(&dir);
        let (_engine, sessions) = service(&endpoint);
        assert_eq!(
            sessions.recv_timeout(WAIT).unwrap(),
            (false, vec![]),
            "no clients yet"
        );

        let (remote, snaps, _) = client(&endpoint);
        let status = remote.status();
        assert!(status.connected);
        assert_eq!(status.version.as_deref(), Some("test"));
        assert!(snaps.recv_timeout(WAIT).unwrap().cpu.usage_total.is_some());
        assert!(remote.latest().is_some());
        let (visible, pids) = sessions.recv_timeout(WAIT).unwrap();
        assert!(visible);
        assert_eq!(pids, vec![std::process::id()]);

        assert!(!remote.sensor_tree().unwrap().groups.is_empty());
        remote.reset_stats().unwrap();
        let mut rules = remote.alert_rules().unwrap();
        rules.truncate(1);
        remote.set_alert_rules(rules.clone()).unwrap_err(); // applied, not saved: no data dir
        assert_eq!(remote.alert_rules().unwrap(), rules);
        assert_eq!(
            remote
                .query_history("cpu.usage.total", HistoryRange::M5)
                .unwrap_err(),
            "history store unavailable"
        );
        assert!(remote.set_ping_host("bad host!").is_err());
        assert_eq!(remote.insights().unwrap_err(), "history store unavailable");
        assert_eq!(
            remote.game_timeline(1).unwrap_err(),
            "history store unavailable"
        );
        assert!(remote
            .history_window(&["cpu.usage.total".into()], 100, 10_000)
            .unwrap_err()
            .starts_with("at most"));
        assert_eq!(
            remote
                .record_inventory(crate::changes::PcInventory::default())
                .unwrap_err(),
            "history store unavailable"
        );
        assert_eq!(
            remote.hardware_changes(10).unwrap_err(),
            "history store unavailable"
        );
        assert!(!remote.drives().unwrap().is_empty());
        // A real answer on an elevated Windows runner, `None` elsewhere: either way it arrives.
        assert!(remote.bitlocker_system_drive().is_ok());

        // Capture: the game shows in the snapshots (simulated frames here, PresentMon or its
        // absence on Windows), and ends when the client leaves.
        let target = crate::gaming::CaptureTarget {
            pid: 4242,
            name: "SimGame.exe".into(),
        };
        remote.set_capture_target(Some(target)).unwrap();
        let game = loop {
            if let Some(g) = snaps.recv_timeout(WAIT).unwrap().game {
                break g;
            }
        };
        assert_eq!(game.pid, 4242);
        assert!(remote.game_sessions().unwrap().is_empty());

        remote.set_visible(false);
        let hidden = loop {
            let (visible, _) = sessions.recv_timeout(WAIT).unwrap();
            if !visible {
                break true;
            }
        };
        assert!(hidden);
        remote.shutdown();
        let deadline = std::time::Instant::now() + WAIT;
        while _engine.latest().and_then(|s| s.game).is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "capture outlived its client"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn refuses_other_protocol_versions() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = endpoint(&dir);
        let _service = service(&endpoint);
        let mut conn = connect(&endpoint).unwrap();
        let hello = Hello {
            protocol: PROTOCOL_VERSION + 1,
            role: Role::Commands,
            session: 1,
            pid: 1,
            visible: false,
        };
        send(&mut conn, &hello).unwrap();
        let answer: Result<Welcome, String> = recv(&mut conn, MAX_MESSAGE).unwrap();
        assert!(answer.unwrap_err().contains("reinstall"));
    }

    #[test]
    fn reconnects_when_the_service_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = endpoint(&dir);
        // First "service": a bare listener that accepts one session and goes away on cue.
        let listener = Listener::bind(&endpoint).unwrap();
        let (release, released) = mpsc::channel::<()>();
        let first = std::thread::spawn(move || {
            let mut conns = Vec::new();
            for _ in 0..2 {
                let (mut c, _) = listener.accept().unwrap();
                let _: Hello = recv(&mut c, MAX_MESSAGE).unwrap();
                let ok: Result<Welcome, String> = Ok(Welcome {
                    protocol: PROTOCOL_VERSION,
                    version: "old".into(),
                });
                send(&mut c, &ok).unwrap();
                conns.push(c);
            }
            let _ = released.recv();
            // Dropping the listener and both connections: the service stopped.
        });
        let (remote, snaps, statuses) = client(&endpoint);
        assert_eq!(remote.status().version.as_deref(), Some("old"));
        release.send(()).unwrap();
        first.join().unwrap();
        let lost = statuses.recv_timeout(WAIT).unwrap();
        assert!(!lost.connected);

        // The real service starts on the same endpoint.
        let (_engine, _sessions) = service(&endpoint);
        // Failed attempts may report other errors first ("connection refused").
        let back = loop {
            let s = statuses.recv_timeout(Duration::from_secs(15)).unwrap();
            if s.connected {
                break s;
            }
            assert!(s.error.is_some());
        };
        assert_eq!(back.version.as_deref(), Some("test"));
        assert!(snaps.recv_timeout(WAIT).is_ok());
        // The commands connection broke with the old service and reopens by itself.
        assert!(remote.sensor_tree().is_ok());
        remote.shutdown();
    }
}

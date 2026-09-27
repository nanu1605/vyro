//! Connects the app to the sensor engine: the Vyro service when it runs (the app itself stays
//! unelevated), else an in-process engine without the privileged sensors.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vyro_core::alerts::AlertFired;
use vyro_core::engine::{
    BackendMode, BackendStatus, ClientPids, EngineApi, EngineConfig, LocalEngine,
};
use vyro_core::ipc::{RemoteEngine, RemoteStatus};
use vyro_core::{lock, Inventory, PollerConfig, SensorSnapshot};

/// Event carrying each [`SensorSnapshot`] (spec §4.1).
pub const SENSORS_EVENT: &str = "sensors";
/// Emitted (no payload) when the display configuration changes.
#[cfg_attr(not(windows), allow(dead_code))]
pub const DISPLAYS_EVENT: &str = "displays-changed";
/// Emitted with a [`BackendStatus`] when the service connection changes.
pub const BACKEND_EVENT: &str = "backend-status";

/// How long to wait for a service that is starting (sign-in right after boot).
const SERVICE_WAIT: Duration = Duration::from_secs(5);

pub struct Engine {
    pub api: Arc<dyn EngineApi>,
    /// Static hardware / system inventory, collected once in the background at startup. It
    /// needs no privileges, and parts of it (startup apps) belong to the signed-in user.
    pub inventory: Arc<Inventory>,
    status: Arc<Mutex<BackendStatus>>,
}

impl Engine {
    pub fn start(app: &AppHandle, visible: bool) -> Result<Self, Box<dyn std::error::Error>> {
        // Inventory is slow (SMBIOS, event log, Task Scheduler, WMI) and never needed for the
        // first frame, so it is warmed off the startup path.
        let inventory = Arc::new(Inventory::new());
        let warm = Arc::clone(&inventory);
        std::thread::Builder::new()
            .name("vyro-inventory".into())
            .spawn(move || warm.warm())?;

        #[cfg(windows)]
        if let Err(e) = vyro_core::alerts::register_toast_app(&app.config().identifier, "Vyro") {
            log::warn!("alerts: could not register for notifications: {e}");
        }

        let status = Arc::new(Mutex::new(BackendStatus::default()));
        let api: Arc<dyn EngineApi> = match connect_service(app, visible, &status) {
            Ok(remote) => {
                log::info!("sensors: from the Vyro service");
                let remote: Arc<dyn EngineApi> = Arc::new(remote);
                migrate_alert_rules(app, &remote);
                remote
            }
            Err(reason) => {
                log::warn!("sensors: in-process ({reason})");
                *lock(&status) = BackendStatus {
                    mode: BackendMode::Local,
                    connected: true,
                    service_version: None,
                    detail: Some(reason),
                };
                let local = start_local(app)?;
                local.set_visible(visible);
                Arc::new(local)
            }
        };

        // Hot-plug / mode / HDR changes: the UI re-queries `get_displays`.
        #[cfg(windows)]
        {
            let app = app.clone();
            vyro_core::display::watch(move || {
                if let Err(e) = app.emit(DISPLAYS_EVENT, ()) {
                    log::debug!("emit {DISPLAYS_EVENT}: {e}");
                }
            });
        }

        Ok(Self {
            api,
            inventory,
            status,
        })
    }

    pub fn status(&self) -> BackendStatus {
        lock(&self.status).clone()
    }

    pub fn set_visible(&self, visible: bool) {
        self.api.set_visible(visible);
    }

    pub fn shutdown(&self) {
        self.api.shutdown();
    }
}

/// Emits each snapshot to the UI and shows the alerts it fired.
fn sink(app: &AppHandle) -> impl Fn(&SensorSnapshot) + Send + 'static {
    let app = app.clone();
    move |snap: &SensorSnapshot| {
        if let Err(e) = app.emit(SENSORS_EVENT, snap) {
            log::debug!("emit {SENSORS_EVENT}: {e}");
        }
        for alert in &snap.alerts {
            notify(&app, alert);
        }
    }
}

fn env_is(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == "1")
}

/// The service, unless `VYRO_LOCAL=1`. On Windows waits a little for a service that is still
/// starting; elsewhere only tries when `VYRO_SOCKET` names one (development).
fn connect_service(
    app: &AppHandle,
    visible: bool,
    status: &Arc<Mutex<BackendStatus>>,
) -> Result<RemoteEngine, String> {
    use vyro_core::ipc::transport::{service_state, ServiceState};

    if env_is("VYRO_LOCAL") {
        return Err("VYRO_LOCAL=1".into());
    }
    let dev_socket = std::env::var_os("VYRO_SOCKET").is_some();
    if !cfg!(windows) && !dev_socket {
        return Err("no sensor service on this platform".into());
    }
    let endpoint = vyro_core::ipc::default_endpoint();
    let deadline = Instant::now() + SERVICE_WAIT;
    loop {
        if cfg!(windows) && !env_is("VYRO_DEV_PIPE") {
            match service_state() {
                ServiceState::NotInstalled => {
                    return Err("the Vyro service is not installed".into())
                }
                ServiceState::Stopped => return Err("the Vyro service is stopped".into()),
                ServiceState::Starting | ServiceState::Running | ServiceState::Other => {}
            }
        }
        let on_status = {
            let app = app.clone();
            let status = Arc::clone(status);
            move |s: &RemoteStatus| {
                let next = service_status(s);
                *lock(&status) = next.clone();
                if let Err(e) = app.emit(BACKEND_EVENT, next) {
                    log::debug!("emit {BACKEND_EVENT}: {e}");
                }
            }
        };
        match RemoteEngine::connect(&endpoint, visible, sink(app), on_status) {
            Ok(remote) => {
                *lock(status) = service_status(&remote.status());
                return Ok(remote);
            }
            // A different protocol version or an impostor will not change by waiting.
            Err(e)
                if e.contains("protocol")
                    || e.contains("not served by")
                    || Instant::now() >= deadline =>
            {
                return Err(e)
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

fn service_status(s: &RemoteStatus) -> BackendStatus {
    BackendStatus {
        mode: BackendMode::Service,
        connected: s.connected,
        service_version: s.version.clone(),
        detail: s.error.clone(),
    }
}

/// The fallback: this process reads what it can, and keeps its own history and rules.
fn start_local(app: &AppHandle) -> std::io::Result<LocalEngine> {
    let data_dir = app
        .path()
        .app_data_dir()
        .inspect_err(|e| log::warn!("history and alert rules not saved: {e}"))
        .ok();
    let clients = ClientPids::default();
    let providers = LocalEngine::default_providers(&clients);
    LocalEngine::start(
        EngineConfig {
            data_dir,
            poller: PollerConfig::default(),
        },
        providers,
        clients,
        sink(app),
    )
}

/// Alert rules customised before the service existed live in this user's `alerts.json`. The
/// first time the app meets a service still on the default rules, it hands them over (once;
/// a marker file remembers).
fn migrate_alert_rules(app: &AppHandle, api: &Arc<dyn EngineApi>) {
    use vyro_core::alerts;
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let path = dir.join(vyro_core::engine::ALERTS_FILE);
    let marker = dir.join("alerts.migrated");
    if !path.exists() || marker.exists() {
        return;
    }
    let api = Arc::clone(api);
    std::thread::spawn(move || {
        let mine = alerts::load(&path);
        match api.alert_rules() {
            Ok(theirs) if theirs == alerts::default_rules() && mine != theirs => {
                match api.set_alert_rules(mine) {
                    Ok(()) => log::info!("alerts: moved this user's rules to the service"),
                    Err(e) => {
                        log::warn!("alerts: could not move rules to the service: {e}");
                        return;
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!("alerts: service unreachable for migration: {e}");
                return;
            }
        }
        if let Err(e) = std::fs::write(&marker, b"") {
            log::warn!("alerts: {}: {e}", marker.display());
        }
    });
}

/// Shows a fired alert as a Windows notification (silent unless the rule asks for sound).
pub fn notify(app: &AppHandle, alert: &AlertFired) {
    log::info!("alert: {}: {}", alert.title, alert.message);
    let mut n = app
        .notification()
        .builder()
        .title(&alert.title)
        .body(&alert.message);
    if alert.sound {
        n = n.sound("Default");
    }
    if let Err(e) = n.show() {
        log::warn!("alert notification failed: {e}");
    }
}

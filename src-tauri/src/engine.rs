//! Connects the app to the sensor engine: the Vyro service when it runs (the app itself stays
//! unelevated), else an in-process engine without the privileged sensors.

use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Whether the window shows. While it does not, snapshots are not sent to it (alerts still
    /// notify) and the engine polls at the tray rate.
    visible: Arc<AtomicBool>,
    /// The game overlay shows: the engine keeps the window's poll rate.
    overlay: AtomicBool,
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
        let shown = Arc::new(AtomicBool::new(visible));
        let api: Arc<dyn EngineApi> = match connect_service(app, visible, &status, &shown) {
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
                let local = start_local(app, &shown)?;
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
            visible: shown,
            overlay: AtomicBool::new(false),
        })
    }

    pub fn status(&self) -> BackendStatus {
        lock(&self.status).clone()
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
        self.api
            .set_visible(visible || self.overlay.load(Ordering::SeqCst));
    }

    pub fn set_overlay(&self, on: bool) {
        self.overlay.store(on, Ordering::SeqCst);
        self.api
            .set_visible(on || self.visible.load(Ordering::SeqCst));
    }

    pub fn set_poll_interval(&self, millis: u32) {
        self.api.set_poll_interval(millis);
    }

    pub fn visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub fn shutdown(&self) {
        self.api.shutdown();
    }
}

/// Emits each snapshot to the UI while it shows, and shows the alerts it fired.
fn sink(app: &AppHandle, shown: &Arc<AtomicBool>) -> impl Fn(&SensorSnapshot) + Send + 'static {
    let app = app.clone();
    let shown = Arc::clone(shown);
    move |snap: &SensorSnapshot| {
        if shown.load(Ordering::SeqCst) {
            if let Err(e) = app.emit(SENSORS_EVENT, snap) {
                log::debug!("emit {SENSORS_EVENT}: {e}");
            }
        }
        crate::overlay::update(&app, snap);
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
    shown: &Arc<AtomicBool>,
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
        match RemoteEngine::connect(&endpoint, visible, sink(app, shown), on_status) {
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
fn start_local(app: &AppHandle, shown: &Arc<AtomicBool>) -> std::io::Result<LocalEngine> {
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
            fan_control: false,
        },
        providers,
        clients,
        sink(app, shown),
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

/// Shows a fired alert as a Windows notification, if the profile lets it through (silent
/// unless the rule and the profile ask for sound).
pub fn notify(app: &AppHandle, alert: &AlertFired) {
    show_notification(app, alert, false);
}

/// `always`: the Test button, which shows whatever the profile says.
pub fn show_notification(app: &AppHandle, alert: &AlertFired, always: bool) {
    log::info!("alert: {}: {}", alert.title, alert.message);
    let settings = app
        .try_state::<crate::tray::SettingsStore>()
        .map(|s| s.get())
        .unwrap_or_default();
    // The profile decides whether this alert notifies (it is logged either way).
    let policy = settings.profile.spec().notifications;
    let Some(sound) = (if always {
        Some(alert.sound)
    } else {
        policy.notify(alert)
    }) else {
        return;
    };
    let unit = settings.temperature_unit;
    let title = vyro_core::units::temps_in(&alert.title, unit);
    let body = vyro_core::units::temps_in(&alert.message, unit);
    let mut n = app.notification().builder().title(&title).body(&body);
    if sound {
        n = n.sound("Default");
    }
    if let Err(e) = n.show() {
        log::warn!("alert notification failed: {e}");
    }
}

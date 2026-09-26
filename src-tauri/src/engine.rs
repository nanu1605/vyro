//! Wires the platform-neutral sensor engine (`vyro-core`) into the Tauri app.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vyro_core::alerts::{self, AlertEngine, AlertFired, AlertRule};
use vyro_core::health::HealthEvent;
use vyro_core::poller::{EVENT_ALERT, EVENT_HEALTH};
use vyro_core::{
    lock, History, HistoryConfig, Inventory, Poller, PollerConfig, PollerHandle, Registry,
    SensorSnapshot, SimProvider,
};

/// Event carrying each [`SensorSnapshot`] (spec §4.1).
pub const SENSORS_EVENT: &str = "sensors";
/// Emitted (no payload) when the display configuration changes.
#[cfg_attr(not(windows), allow(dead_code))]
pub const DISPLAYS_EVENT: &str = "displays-changed";

pub struct Engine {
    pub registry: Arc<Mutex<Registry>>,
    /// `None` when the history database could not be opened; live data still flows.
    pub history: Option<Arc<Mutex<History>>>,
    /// Static hardware / system inventory, collected once in the background at startup.
    pub inventory: Arc<Inventory>,
    alerts: Arc<Mutex<AlertEngine>>,
    /// `alerts.json` in the app config folder; `None` if that folder is unavailable.
    alerts_path: Option<PathBuf>,
    poller: Mutex<Option<PollerHandle>>,
}

impl Engine {
    pub fn start(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let registry = Arc::new(Mutex::new(Registry::new()));
        let history = open_history(app).map(|h| Arc::new(Mutex::new(h)));

        // Inventory is slow (SMBIOS, event log, Task Scheduler, WMI) and never needed for the
        // first frame, so it is warmed off the startup path.
        let inventory = Arc::new(Inventory::new());
        let warm = Arc::clone(&inventory);
        std::thread::Builder::new()
            .name("vyro-inventory".into())
            .spawn(move || warm.warm())?;

        let alerts_path = app
            .path()
            .app_config_dir()
            .map(|d| d.join("alerts.json"))
            .inspect_err(|e| log::warn!("alert rules will not be saved: {e}"))
            .ok();
        let rules = alerts_path
            .as_deref()
            .map_or_else(alerts::default_rules, alerts::load);
        log::info!("alerts: {} rule(s)", rules.len());
        let alerts = Arc::new(Mutex::new(AlertEngine::new(rules)));
        #[cfg(windows)]
        if let Err(e) = alerts::register_toast_app(&app.config().identifier, "Vyro") {
            log::warn!("alerts: could not register for notifications: {e}");
        }

        let mut poller =
            with_providers(Poller::new(Arc::clone(&registry), PollerConfig::default()))
                .with_alerts(Arc::clone(&alerts));
        if let Some(history) = &history {
            poller = poller.with_history(Arc::clone(history));
        }

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

        let emitter = app.clone();
        let handle = poller.spawn(move |snap: &SensorSnapshot| {
            if let Err(e) = emitter.emit(SENSORS_EVENT, snap) {
                log::debug!("emit {SENSORS_EVENT}: {e}");
            }
            for alert in &snap.alerts {
                notify(&emitter, alert);
            }
        })?;

        Ok(Self {
            registry,
            history,
            inventory,
            alerts,
            alerts_path,
            poller: Mutex::new(Some(handle)),
        })
    }

    pub fn latest(&self) -> Option<SensorSnapshot> {
        lock(&self.poller).as_ref().and_then(PollerHandle::latest)
    }

    /// Health changes from the history store (kept across restarts), else from memory.
    pub fn health_events(&self) -> Vec<HealthEvent> {
        let stored = self.history.as_ref().and_then(|h| {
            lock(h)
                .events(EVENT_HEALTH, vyro_core::health::MAX_EVENTS as i64)
                .inspect_err(|e| log::warn!("health events: {e}"))
                .ok()
        });
        stored.unwrap_or_else(|| {
            lock(&self.poller)
                .as_ref()
                .map(PollerHandle::health_events)
                .unwrap_or_default()
        })
    }

    pub fn alert_rules(&self) -> Vec<AlertRule> {
        lock(&self.alerts).rules().to_vec()
    }

    /// Applies and saves a new rule set.
    pub fn set_alert_rules(&self, rules: Vec<AlertRule>) -> Result<(), String> {
        lock(&self.alerts).set_rules(rules.clone())?;
        match &self.alerts_path {
            Some(path) => alerts::save(path, &rules)
                .map_err(|e| format!("rules applied but not saved ({}): {e}", path.display())),
            None => Err("rules applied but not saved: no config folder".into()),
        }
    }

    /// Fired alerts from the history store (kept across restarts), else from memory.
    pub fn alert_log(&self) -> Vec<AlertFired> {
        self.history
            .as_ref()
            .and_then(|h| {
                lock(h)
                    .events(EVENT_ALERT, alerts::MAX_LOG as i64)
                    .inspect_err(|e| log::warn!("alert log: {e}"))
                    .ok()
            })
            .unwrap_or_else(|| lock(&self.alerts).log())
    }

    pub fn set_visible(&self, visible: bool) {
        if let Some(poller) = lock(&self.poller).as_ref() {
            poller.set_visible(visible);
        }
    }

    /// Stops polling and flushes history. Safe to call more than once.
    pub fn shutdown(&self) {
        if let Some(poller) = lock(&self.poller).take() {
            poller.stop();
        }
    }
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

/// Real providers on Windows; the simulator elsewhere and with `VYRO_SIM=1`.
fn with_providers(poller: Poller) -> Poller {
    #[cfg(windows)]
    if std::env::var_os("VYRO_SIM").is_none_or(|v| v != "1") {
        log::info!("sensors: live providers");
        return vyro_core::live::providers()
            .into_iter()
            .fold(poller, Poller::with_boxed);
    }
    log::info!("sensors: simulated");
    poller.with_provider(SimProvider::new())
}

fn open_history(app: &AppHandle) -> Option<History> {
    let dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("history disabled: no app data dir: {e}");
            return None;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("history disabled: cannot create {}: {e}", dir.display());
        return None;
    }
    let path = dir.join("history.sqlite");
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

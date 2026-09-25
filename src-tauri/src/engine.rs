//! Wires the platform-neutral sensor engine (`vyro-core`) into the Tauri app.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use vyro_core::{
    lock, History, HistoryConfig, Inventory, Poller, PollerConfig, PollerHandle, Registry,
    SensorSnapshot, SimProvider,
};

/// Event carrying each [`SensorSnapshot`] (spec §4.1).
pub const SENSORS_EVENT: &str = "sensors";

pub struct Engine {
    pub registry: Arc<Mutex<Registry>>,
    /// `None` when the history database could not be opened; live data still flows.
    pub history: Option<Arc<Mutex<History>>>,
    /// Static hardware / system inventory, collected once in the background at startup.
    pub inventory: Arc<Inventory>,
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

        // Live readings are simulated until the Phase 2 providers land; the simulator then
        // remains for non-Windows hosts and `VYRO_SIM=1`.
        let mut poller = Poller::new(Arc::clone(&registry), PollerConfig::default())
            .with_provider(SimProvider::new());
        if let Some(history) = &history {
            poller = poller.with_history(Arc::clone(history));
        }

        let emitter = app.clone();
        let handle = poller.spawn(move |snap: &SensorSnapshot| {
            if let Err(e) = emitter.emit(SENSORS_EVENT, snap) {
                log::debug!("emit {SENSORS_EVENT}: {e}");
            }
        })?;

        Ok(Self {
            registry,
            history,
            inventory,
            poller: Mutex::new(Some(handle)),
        })
    }

    pub fn latest(&self) -> Option<SensorSnapshot> {
        lock(&self.poller).as_ref().and_then(PollerHandle::latest)
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

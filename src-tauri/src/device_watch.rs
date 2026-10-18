//! Devices read in the background (1.4.1), so a low battery shows on the Dashboard and as a dot
//! on Devices without the tab open. The list is read 20 s after launch, after each hot-plug
//! change and every 5 minutes, on its own thread; the Devices tab's own reads count too.

use std::sync::{mpsc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use vyro_core::devices::{DeviceInfo, Devices};
use vyro_core::lock;

/// Emitted with the devices whose battery is low (empty when none) whenever that list changes.
pub const LOW_BATTERY_EVENT: &str = "low-battery";
/// The first read waits for the app to settle; later ones come with changes or this often.
const FIRST: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct DeviceWatch {
    /// Devices whose battery is low, as last read.
    low: Mutex<Vec<DeviceInfo>>,
    /// Wakes the reader after a hot-plug change.
    poke: Mutex<Option<mpsc::Sender<()>>>,
}

/// Starts the hot-plug watch and the background reader. Once, at launch.
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    *lock(&app.state::<DeviceWatch>().poke) = Some(tx);
    crate::engine::watch_devices(app);
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vyro-devices".into())
        .spawn(move || {
            let mut wait = FIRST;
            loop {
                if let Err(mpsc::RecvTimeoutError::Disconnected) = rx.recv_timeout(wait) {
                    return;
                }
                // Changes that came while reading are covered by the next read.
                while rx.try_recv().is_ok() {}
                store(&app, &crate::commands::collect_devices());
                wait = EVERY;
            }
        });
    if let Err(e) = spawned {
        log::warn!("devices: background reader failed to start: {e}");
    }
}

/// A device arrived, left, connected or disconnected: read again.
pub fn poke(app: &AppHandle) {
    if let Some(w) = app.try_state::<DeviceWatch>() {
        if let Some(tx) = lock(&w.poke).as_ref() {
            let _ = tx.send(());
        }
    }
}

/// Keeps a fresh list's low batteries, and tells the window when they changed.
pub fn store(app: &AppHandle, devices: &Devices) {
    let Some(w) = app.try_state::<DeviceWatch>() else {
        return;
    };
    let low: Vec<DeviceInfo> = devices
        .devices
        .iter()
        .filter(|d| d.low.is_some())
        .cloned()
        .collect();
    {
        let mut kept = lock(&w.low);
        if *kept == low {
            return;
        }
        kept.clone_from(&low);
    }
    if let Err(e) = app.emit(LOW_BATTERY_EVENT, &low) {
        log::debug!("emit {LOW_BATTERY_EVENT}: {e}");
    }
}

/// Devices whose battery is low, as last read (empty before the first read).
#[tauri::command]
pub fn get_low_batteries(watch: State<'_, DeviceWatch>) -> Vec<DeviceInfo> {
    lock(&watch.low).clone()
}

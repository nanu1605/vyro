//! Devices read in the background (1.4.1), so a low battery shows on the Dashboard and as a dot
//! on Devices without the tab open. The list is read 20 s after launch, after each hot-plug
//! change and every 5 minutes, on its own thread, and the window gets it (the Devices tab shows
//! it); the Devices tab's own reads count too.

use std::sync::{mpsc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use vyro_core::devices::{DeviceInfo, Devices};
use vyro_core::lock;

/// Emitted with the devices whose battery is low (empty when none) whenever that list changes.
pub const LOW_BATTERY_EVENT: &str = "low-battery";
/// Emitted with each list read here.
pub const READ_EVENT: &str = "devices-read";
/// The first read waits for the app to settle; later ones come with changes or this often.
const FIRST: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(5 * 60);
/// After a read, a change waits this long for the next: a read can itself stir Bluetooth (a
/// battery read wakes a device), and changes that keep coming then cost a read every few
/// seconds rather than one after another.
const GAP: Duration = Duration::from_secs(5);
/// Back-to-back reads after changes for this long are logged, once.
const BUSY_LOGGED: u32 = 12;

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
            let mut changed = false;
            let mut busy = 0;
            loop {
                if !changed {
                    if let Err(mpsc::RecvTimeoutError::Disconnected) = rx.recv_timeout(wait) {
                        return;
                    }
                }
                // Changes that came before this read are covered by it.
                while rx.try_recv().is_ok() {}
                let devices = crate::commands::collect_devices();
                store(&app, &devices);
                if let Err(e) = app.emit(READ_EVENT, &devices) {
                    log::debug!("emit {READ_EVENT}: {e}");
                }
                wait = EVERY;
                std::thread::sleep(GAP);
                changed = rx.try_recv().is_ok();
                busy = if changed { busy + 1 } else { 0 };
                if busy == BUSY_LOGGED {
                    log::info!(
                        "devices: changes kept coming for a minute, each read {} s after the last",
                        GAP.as_secs()
                    );
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("devices: background reader failed to start: {e}");
    }
}

/// A device arrived, left, connected or disconnected: read again.
#[cfg(windows)]
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

//! PC change history (P19), app side: the app sees the PC's hardware, Windows, monitors and
//! network adapters, so it builds the fingerprint (`vyro_core::changes::fingerprint`) and hands
//! it to the engine, which compares it with the last one and keeps the changes. Checked once
//! after start, and again when monitors or drives come or go. Each check runs on a short-lived
//! thread; checks asked for meanwhile are folded into one more.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use vyro_core::changes::{self, HardwareChange};
use vyro_core::engine::BackendMode;
use vyro_core::info::DisplayInfo;
use vyro_core::lock;

use crate::engine::Engine;

/// Emitted with the new [`HardwareChange`]s when a check finds any.
pub const CHANGES_EVENT: &str = "hardware-changed";

/// Why a check runs: a display change only matters when the set of monitors changed (not a
/// resolution, HDR or refresh rate switch).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Start,
    Displays,
    Drives,
}

static BUSY: AtomicBool = AtomicBool::new(false);
static AGAIN: AtomicBool = AtomicBool::new(false);
/// Monitors at the last check, by EDID identity (serials stay in memory, never stored).
static MONITORS: Mutex<Option<Vec<String>>> = Mutex::new(None);
/// The reason folded into the next run: anything but `Displays` forces a full check.
static PENDING_FULL: AtomicBool = AtomicBool::new(false);

pub fn check(app: &AppHandle, reason: Reason) {
    if reason != Reason::Displays {
        PENDING_FULL.store(true, Ordering::SeqCst);
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        AGAIN.store(true, Ordering::SeqCst);
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vyro-changes".into())
        .spawn(move || {
            loop {
                AGAIN.store(false, Ordering::SeqCst);
                let full = PENDING_FULL.swap(false, Ordering::SeqCst);
                run(&app, full);
                if !AGAIN.load(Ordering::SeqCst) {
                    break;
                }
            }
            BUSY.store(false, Ordering::SeqCst);
            // Asked for between the last run and now: run once more.
            if AGAIN.swap(false, Ordering::SeqCst) {
                check(&app, Reason::Displays);
            }
        });
    if let Err(e) = spawned {
        BUSY.store(false, Ordering::SeqCst);
        log::warn!("hardware changes: cannot start a check: {e}");
    }
}

fn monitors(displays: &[DisplayInfo]) -> Vec<String> {
    let mut ids: Vec<String> = displays
        .iter()
        .filter_map(|d| {
            let serial = d.serial.as_ref().map_or("", |s| s.0.as_str());
            Some(format!(
                "{}:{}:{serial}",
                d.manufacturer_id.as_deref()?,
                d.product_code.as_deref()?
            ))
        })
        .collect();
    ids.sort();
    ids
}

fn run(app: &AppHandle, full: bool) {
    let Some(engine) = app.try_state::<Engine>() else {
        return;
    };
    let displays = crate::commands::collect_displays();
    let now = monitors(&displays);
    {
        let mut last = lock(&MONITORS);
        if !full && last.as_ref() == Some(&now) {
            return;
        }
        *last = Some(now);
    }
    let inventory = Arc::clone(&engine.inventory);
    let api = Arc::clone(&engine.api);
    // Waits for the startup collection if it is still running.
    let hardware = inventory.hardware();
    let system = inventory.system();
    // Drives only from the service: without it the app reads serials and firmware another way,
    // which would read as other drives. Left out, they stay as they were (not removed).
    let drives = if engine.status().mode == BackendMode::Service {
        api.drives().unwrap_or_else(|e| {
            log::debug!("hardware changes: drives: {e}");
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let adapters = crate::commands::collect_network();
    let fingerprint = changes::fingerprint(hardware, &system, &displays, &drives, &adapters);
    match api.record_inventory(fingerprint) {
        Ok(found) if !found.is_empty() => {
            log::info!("hardware changes: {} new", found.len());
            if let Err(e) = app.emit(CHANGES_EVENT, &found) {
                log::debug!("emit {CHANGES_EVENT}: {e}");
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!("hardware changes: {e}"),
    }
}

/// The PC change history, newest first.
#[tauri::command]
pub async fn get_hardware_changes(
    engine: State<'_, Engine>,
    limit: u32,
) -> Result<Vec<HardwareChange>, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.hardware_changes(limit))
        .await
        .map_err(|e| e.to_string())?
}

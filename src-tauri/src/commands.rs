//! On-demand commands (spec §4.1). Anything touching SQLite or cloning the sensor tree is
//! `async` so it runs off the main thread.

use std::sync::Arc;

use tauri::State;
use vyro_core::alerts::{AlertFired, AlertRule};
use vyro_core::health::HealthEvent;
use vyro_core::info::{
    DisplayInfo, DriveInfo, HardwareId, HealthReport, NetAdapterInfo, PawnIoStatus, SystemInfo,
};
use vyro_core::snapshot::AppSnapshot;
use vyro_core::{lock, unix_now, HistoryRange, HistorySeries, SensorSnapshot, SensorTree};

use crate::engine::Engine;

/// Latest snapshot, so a freshly loaded UI need not wait for the next tick.
#[tauri::command]
pub fn get_latest(engine: State<'_, Engine>) -> Option<SensorSnapshot> {
    engine.latest()
}

/// The UI reports visibility so the engine can poll slower while hidden.
#[tauri::command]
pub fn set_ui_visible(engine: State<'_, Engine>, visible: bool) {
    engine.set_visible(visible);
}

#[tauri::command]
pub async fn query_history(
    engine: State<'_, Engine>,
    metric: String,
    range: HistoryRange,
) -> Result<HistorySeries, String> {
    let history = engine.history.as_ref().ok_or("history store unavailable")?;
    lock(history)
        .query(&metric, range, unix_now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_sensor_tree(engine: State<'_, Engine>) -> Result<SensorTree, String> {
    Ok(lock(&engine.registry).tree())
}

/// Restarts min / max / avg for every sensor (Sensor Explorer "Reset").
#[tauri::command]
pub fn reset_sensor_stats(engine: State<'_, Engine>) {
    lock(&engine.registry).reset_stats();
}

#[tauri::command]
pub fn get_app_metrics(engine: State<'_, Engine>) -> AppSnapshot {
    engine.latest().map(|s| s.app).unwrap_or_default()
}

/// Static inventory; waits for the startup collection if it is still running.
#[tauri::command]
pub async fn get_system_info(engine: State<'_, Engine>) -> Result<SystemInfo, String> {
    let inventory = Arc::clone(&engine.inventory);
    tauri::async_runtime::spawn_blocking(move || inventory.system())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_hardware_id(engine: State<'_, Engine>) -> Result<HardwareId, String> {
    let inventory = Arc::clone(&engine.inventory);
    tauri::async_runtime::spawn_blocking(move || inventory.hardware().clone())
        .await
        .map_err(|e| e.to_string())
}

/// Collected fresh on every call (cheap apart from DDC/CI), so the panel is current when
/// opened and after each `displays-changed` event.
#[tauri::command]
pub async fn get_displays() -> Result<Vec<DisplayInfo>, String> {
    tauri::async_runtime::spawn_blocking(collect_displays)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn collect_displays() -> Vec<DisplayInfo> {
    if std::env::var_os("VYRO_SIM").is_some_and(|v| v == "1") {
        return vyro_core::display::sample();
    }
    vyro_core::display::collect()
}

#[cfg(not(windows))]
fn collect_displays() -> Vec<DisplayInfo> {
    vyro_core::display::sample()
}

/// Drives with full health data, read fresh (SMART needs the app elevated). Live rates and
/// temperature arrive in the `sensors` event.
#[tauri::command]
pub async fn get_drives() -> Result<Vec<DriveInfo>, String> {
    tauri::async_runtime::spawn_blocking(collect_drives)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn collect_drives() -> Vec<DriveInfo> {
    if std::env::var_os("VYRO_SIM").is_some_and(|v| v == "1") {
        return vyro_core::storage::sample();
    }
    vyro_core::storage::collect()
}

#[cfg(not(windows))]
fn collect_drives() -> Vec<DriveInfo> {
    vyro_core::storage::sample()
}

/// Adapters with addresses and Wi-Fi details, read fresh. Rates and ping arrive in `sensors`.
#[tauri::command]
pub async fn get_network() -> Result<Vec<NetAdapterInfo>, String> {
    tauri::async_runtime::spawn_blocking(collect_network)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn collect_network() -> Vec<NetAdapterInfo> {
    if std::env::var_os("VYRO_SIM").is_some_and(|v| v == "1") {
        return vyro_core::network::sample();
    }
    vyro_core::network::collect()
}

#[cfg(not(windows))]
fn collect_network() -> Vec<NetAdapterInfo> {
    vyro_core::network::sample()
}

/// Changes the ping target (IPv4 address or host name).
#[tauri::command]
pub fn set_ping_host(host: String) -> Result<(), String> {
    #[cfg(windows)]
    return vyro_core::network::set_ping_host(&host);
    #[cfg(not(windows))]
    {
        if vyro_core::network::valid_host(&host) {
            Ok(())
        } else {
            Err("Enter an IPv4 address or a host name".into())
        }
    }
}

/// System Health as of the latest tick.
#[tauri::command]
pub fn get_health(engine: State<'_, Engine>) -> HealthReport {
    engine.latest().map(|s| s.health).unwrap_or_default()
}

#[tauri::command]
pub fn get_alert_rules(engine: State<'_, Engine>) -> Vec<AlertRule> {
    engine.alert_rules()
}

/// Replaces the alert rules (validated, then saved to `alerts.json`).
#[tauri::command]
pub fn set_alert_rules(engine: State<'_, Engine>, rules: Vec<AlertRule>) -> Result<(), String> {
    engine.set_alert_rules(rules)
}

/// Alerts fired since start, oldest first.
#[tauri::command]
pub fn get_alert_log(engine: State<'_, Engine>) -> Vec<AlertFired> {
    engine.alert_log()
}

/// Sends a sample notification so the user can check Windows lets Vyro notify.
#[tauri::command]
pub fn test_alert(app: tauri::AppHandle, sound: bool) {
    crate::engine::notify(
        &app,
        &AlertFired {
            ts: unix_now(),
            rule_id: "test".into(),
            title: "Vyro test alert".into(),
            message: "Alerts will look like this.".into(),
            sound,
        },
    );
}

/// Recent health changes (thermal limits, throttling, stopped fans…), oldest first.
#[tauri::command]
pub fn get_health_events(engine: State<'_, Engine>) -> Vec<HealthEvent> {
    engine.health_events()
}

/// Whether PawnIO (low-level sensors: CPU temperature and power, board fans) is usable.
#[tauri::command]
pub async fn get_pawnio_status() -> Result<PawnIoStatus, String> {
    tauri::async_runtime::spawn_blocking(vyro_core::pawnio::status)
        .await
        .map_err(|e| e.to_string())
}

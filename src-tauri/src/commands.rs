//! On-demand commands (spec §4.1). Anything touching SQLite or cloning the sensor tree is
//! `async` so it runs off the main thread.

use tauri::State;
use vyro_core::info::{DisplayInfo, HardwareId, HealthReport, SystemInfo};
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

#[tauri::command]
pub fn get_app_metrics(engine: State<'_, Engine>) -> AppSnapshot {
    engine.latest().map(|s| s.app).unwrap_or_default()
}

// Filled in by later phases: system/hardware (P1), displays (P3), health (P7).

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    SystemInfo::default()
}

#[tauri::command]
pub fn get_hardware_id() -> HardwareId {
    HardwareId::default()
}

#[tauri::command]
pub fn get_displays() -> Vec<DisplayInfo> {
    Vec::new()
}

#[tauri::command]
pub fn get_health() -> HealthReport {
    HealthReport::default()
}

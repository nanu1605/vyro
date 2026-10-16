//! On-demand commands (spec §4.1). Anything that may wait on the sensor service, SQLite or
//! the inventory is `async` and runs on a blocking thread, never on the main thread.

use std::sync::Arc;

use tauri::State;
use vyro_core::alerts::{AlertFired, AlertRule};
use vyro_core::engine::{BackendStatus, EngineApi};
use vyro_core::health::HealthEvent;
use vyro_core::info::{
    DisplayInfo, DriveInfo, HardwareId, HealthReport, NetAdapterInfo, PawnIoStatus, SystemInfo,
};
use vyro_core::snapshot::AppSnapshot;
use vyro_core::{unix_now, HistoryRange, HistorySeries, SensorSnapshot, SensorTree};

use crate::engine::Engine;

/// Runs `f` against the engine on a blocking thread.
async fn with_engine<T: Send + 'static>(
    engine: &Engine,
    f: impl FnOnce(&dyn EngineApi) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || f(api.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Latest snapshot, so a freshly loaded UI need not wait for the next tick.
#[tauri::command]
pub fn get_latest(engine: State<'_, Engine>) -> Option<SensorSnapshot> {
    engine.api.latest()
}

/// The UI reports visibility so the engine can poll slower while hidden.
#[tauri::command]
pub fn set_ui_visible(engine: State<'_, Engine>, visible: bool) {
    engine.set_visible(visible);
}

/// Where sensor data comes from (the service, or this app without it).
#[tauri::command]
pub fn get_backend_status(engine: State<'_, Engine>) -> BackendStatus {
    engine.status()
}

#[tauri::command]
pub async fn query_history(
    engine: State<'_, Engine>,
    metric: String,
    range: HistoryRange,
) -> Result<HistorySeries, String> {
    with_engine(&engine, move |e| e.query_history(&metric, range)).await
}

#[tauri::command]
pub async fn get_sensor_tree(engine: State<'_, Engine>) -> Result<SensorTree, String> {
    with_engine(&engine, |e| e.sensor_tree()).await
}

/// Restarts min / max / avg for every sensor (Sensor Explorer "Reset").
#[tauri::command]
pub async fn reset_sensor_stats(engine: State<'_, Engine>) -> Result<(), String> {
    with_engine(&engine, |e| e.reset_stats()).await
}

#[tauri::command]
pub fn get_app_metrics(engine: State<'_, Engine>) -> AppSnapshot {
    engine.api.latest().map(|s| s.app).unwrap_or_default()
}

/// Static inventory; waits for the startup collection if it is still running. BitLocker
/// needs administrator rights, so the engine (the service) supplies it.
#[tauri::command]
pub async fn get_system_info(engine: State<'_, Engine>) -> Result<SystemInfo, String> {
    let inventory = Arc::clone(&engine.inventory);
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || {
        let mut info = inventory.system();
        if info.security.bitlocker_system_drive.is_none() {
            info.security.bitlocker_system_drive = api.bitlocker_system_drive().ok().flatten();
        }
        info
    })
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
pub(crate) fn collect_displays() -> Vec<DisplayInfo> {
    if std::env::var_os("VYRO_SIM").is_some_and(|v| v == "1") {
        return vyro_core::display::sample();
    }
    vyro_core::display::collect()
}

#[cfg(not(windows))]
pub(crate) fn collect_displays() -> Vec<DisplayInfo> {
    vyro_core::display::sample()
}

/// Drives with full health data, read fresh by the engine (SMART needs the service or
/// administrator rights). Live rates and temperature arrive in the `sensors` event.
#[tauri::command]
pub async fn get_drives(engine: State<'_, Engine>) -> Result<Vec<DriveInfo>, String> {
    with_engine(&engine, |e| e.drives()).await
}

/// Adapters with addresses and Wi-Fi details, read fresh. Rates and ping arrive in `sensors`.
#[tauri::command]
pub async fn get_network() -> Result<Vec<NetAdapterInfo>, String> {
    tauri::async_runtime::spawn_blocking(collect_network)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
pub(crate) fn collect_network() -> Vec<NetAdapterInfo> {
    if std::env::var_os("VYRO_SIM").is_some_and(|v| v == "1") {
        return vyro_core::network::sample();
    }
    vyro_core::network::collect()
}

#[cfg(not(windows))]
pub(crate) fn collect_network() -> Vec<NetAdapterInfo> {
    vyro_core::network::sample()
}

/// Changes the ping target (IPv4 address or host name).
#[tauri::command]
pub async fn set_ping_host(engine: State<'_, Engine>, host: String) -> Result<(), String> {
    with_engine(&engine, move |e| e.set_ping_host(&host)).await
}

/// System Health as of the latest tick.
#[tauri::command]
pub fn get_health(engine: State<'_, Engine>) -> HealthReport {
    engine.api.latest().map(|s| s.health).unwrap_or_default()
}

#[tauri::command]
pub async fn get_alert_rules(engine: State<'_, Engine>) -> Result<Vec<AlertRule>, String> {
    with_engine(&engine, |e| e.alert_rules()).await
}

/// Replaces the alert rules (validated, then saved to `alerts.json`).
#[tauri::command]
pub async fn set_alert_rules(
    engine: State<'_, Engine>,
    rules: Vec<AlertRule>,
) -> Result<(), String> {
    with_engine(&engine, move |e| e.set_alert_rules(rules)).await
}

/// Fired alerts, oldest first.
#[tauri::command]
pub async fn get_alert_log(engine: State<'_, Engine>) -> Result<Vec<AlertFired>, String> {
    with_engine(&engine, |e| e.alert_log()).await
}

/// A sensor's usual reading now, for "above usual" alert conditions (`None` until there is
/// about a day of history).
#[tauri::command]
pub async fn get_alert_usual(
    engine: State<'_, Engine>,
    sensor: String,
) -> Result<Option<f64>, String> {
    with_engine(&engine, move |e| e.alert_usual(&sensor)).await
}

/// Sends a sample notification so the user can check Windows lets Vyro notify.
#[tauri::command]
pub fn test_alert(app: tauri::AppHandle, sound: bool) {
    crate::engine::show_notification(
        &app,
        &AlertFired::new(
            unix_now(),
            "test",
            "Vyro test alert".into(),
            "Alerts will look like this.".into(),
            sound,
        ),
        true,
    );
}

/// Recent health changes (thermal limits, throttling, stopped fans…), oldest first.
#[tauri::command]
pub async fn get_health_events(engine: State<'_, Engine>) -> Result<Vec<HealthEvent>, String> {
    with_engine(&engine, |e| e.health_events()).await
}

/// The intelligence engine's findings and vital signs (P16).
#[tauri::command]
pub async fn get_insights(engine: State<'_, Engine>) -> Result<vyro_core::intel::Insights, String> {
    with_engine(&engine, |e| e.insights()).await
}

/// Whether the engine can use PawnIO (low-level sensors: CPU temperature and power, board
/// fans).
#[tauri::command]
pub async fn get_pawnio_status(engine: State<'_, Engine>) -> Result<PawnIoStatus, String> {
    with_engine(&engine, |e| e.pawnio_status()).await
}

#[tauri::command]
pub fn get_settings(store: State<'_, crate::tray::SettingsStore>) -> crate::tray::Settings {
    store.get()
}

#[tauri::command]
pub fn set_settings(app: tauri::AppHandle, settings: crate::tray::Settings) -> Result<(), String> {
    crate::tray::apply(&app, settings)
}

/// The profile presets (the chosen one is in the settings).
#[tauri::command]
pub fn get_profiles() -> Vec<vyro_core::profiles::ProfileSpec> {
    vyro_core::profiles::Profile::ALL
        .iter()
        .map(|p| p.spec())
        .collect()
}

/// Whether Vyro starts (in the tray) at sign-in.
#[tauri::command]
pub async fn get_autostart() -> bool {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(vyro_core::autostart::is_enabled)
        .await
        .unwrap_or(false);
    #[cfg(not(windows))]
    false
}

#[tauri::command]
pub async fn set_autostart(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            vyro_core::autostart::enable()
        } else {
            vyro_core::autostart::disable()
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("Start with Windows is only available on Windows".into())
    }
}

/// Fan control (P12): capability, configuration and each header's state.
#[tauri::command]
pub async fn get_fan_status(
    engine: State<'_, Engine>,
) -> Result<vyro_core::fan::FanStatus, String> {
    with_engine(&engine, |e| e.fan_status()).await
}

#[tauri::command]
pub async fn set_fan_config(
    engine: State<'_, Engine>,
    config: vyro_core::fan::FanConfig,
) -> Result<(), String> {
    with_engine(&engine, move |e| e.set_fan_config(config)).await
}

/// Turns fan control off and hands every header back to the BIOS.
#[tauri::command]
pub async fn restore_fans(engine: State<'_, Engine>) -> Result<(), String> {
    with_engine(&engine, |e| e.restore_fans()).await
}

/// The fan control log, newest last.
#[tauri::command]
pub async fn get_fan_events(
    engine: State<'_, Engine>,
) -> Result<Vec<vyro_core::fan::FanEvent>, String> {
    with_engine(&engine, |e| e.fan_events()).await
}

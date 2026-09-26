// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod engine;

use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use engine::Engine;

fn main() {
    let log = tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::LogDir { file_name: None }),
        ])
        .level(log::LevelFilter::Info)
        .max_file_size(2_000_000)
        .rotation_strategy(RotationStrategy::KeepSome(3))
        .build();

    let app = tauri::Builder::default()
        .plugin(log)
        .setup(|app| {
            let engine = Engine::start(app.handle())?;
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_latest,
            commands::set_ui_visible,
            commands::query_history,
            commands::get_sensor_tree,
            commands::reset_sensor_stats,
            commands::get_app_metrics,
            commands::get_system_info,
            commands::get_hardware_id,
            commands::get_displays,
            commands::get_drives,
            commands::get_network,
            commands::get_pawnio_status,
            commands::set_ping_host,
            commands::get_health,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri application");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(engine) = handle.try_state::<Engine>() {
                engine.shutdown();
            }
        }
    });
}

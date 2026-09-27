// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod engine;
mod tray;

use tauri::Manager;
use tauri_plugin_log::{RotationStrategy, Target, TargetKind};

use engine::Engine;

fn main() {
    vyro_core::process_start();
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
        // First: a second launch (Start menu while Vyro sits in the tray) shows the running
        // copy instead of starting another engine.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(log)
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let engine = Engine::start(app.handle())?;
            app.manage(engine);
            app.manage(tray::SettingsStore::load(app.handle()));
            tray::build(app.handle())?;
            // The window starts hidden (tauri.conf.json); autostart passes --minimized to stay
            // in the tray.
            let minimized = std::env::args().any(|a| a == vyro_core::autostart::MINIMIZED_ARG);
            if minimized {
                app.state::<Engine>().set_visible(false);
            } else {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(tray::on_window_event)
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
            commands::get_health_events,
            commands::get_alert_rules,
            commands::set_alert_rules,
            commands::get_alert_log,
            commands::test_alert,
            commands::get_settings,
            commands::set_settings,
            commands::get_autostart,
            commands::set_autostart,
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

// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod engine;
mod gaming;
mod overlay;
mod tray;
mod updates;

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

    let context = tauri::generate_context!();
    // Release builds only: the release workflow adds the updater's public key.
    let can_update = updates::configured(context.config());
    let current = context.package_info().version.to_string();

    let mut builder = tauri::Builder::default()
        // First: a second launch (Start menu while Vyro sits in the tray) shows the running
        // copy instead of starting another engine.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(log)
        .plugin(tauri_plugin_notification::init());
    if can_update {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
    let app = builder
        .setup(move |app| {
            // The window starts hidden (tauri.conf.json); autostart passes --minimized to stay
            // in the tray.
            let minimized = std::env::args().any(|a| a == vyro_core::autostart::MINIMIZED_ARG);
            let engine = Engine::start(app.handle(), !minimized)?;
            app.manage(overlay::Overlay::default());
            app.manage(updates::Updates::new(can_update, current));
            app.manage(engine);
            app.manage(tray::SettingsStore::load(app.handle()));
            tray::build(app.handle())?;
            let settings = app.state::<tray::SettingsStore>().get();
            if let Err(e) = tray::apply(app.handle(), settings) {
                log::warn!("settings: {e}");
            }
            gaming::watch(app.handle());
            updates::watch(app.handle());
            if minimized {
                tray::park_after_start(app.handle());
            } else {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(tray::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_latest,
            commands::set_ui_visible,
            commands::get_backend_status,
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
            commands::get_insights,
            commands::get_alert_rules,
            commands::set_alert_rules,
            commands::get_alert_log,
            commands::test_alert,
            commands::get_settings,
            commands::set_settings,
            commands::get_profiles,
            commands::get_autostart,
            commands::set_autostart,
            gaming::get_game_sessions,
            commands::get_fan_status,
            commands::set_fan_config,
            commands::restore_fans,
            updates::get_update_status,
            updates::check_for_updates,
            updates::install_update,
        ])
        .build(context)
        .expect("failed to build the Tauri application");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(engine) = handle.try_state::<Engine>() {
                engine.shutdown();
            }
        }
    });
}

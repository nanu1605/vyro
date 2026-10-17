// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bench;
mod changes;
mod commands;
mod dashboard;
mod diag;
mod engine;
mod gaming;
mod look;
mod overlay;
mod pin;
mod remote;
mod share;
mod smoke;
mod snap;
mod tray;
mod updates;
mod widgets;

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
            app.manage(gaming::GameState::default());
            app.manage(bench::BenchState::default());
            app.manage(updates::Updates::new(can_update, current));
            app.manage(engine);
            app.manage(tray::SettingsStore::load(app.handle()));
            app.manage(dashboard::DashboardStore::load(app.handle()));
            app.manage(remote::Remote::load(app.handle()));
            app.manage(widgets::WidgetStore::load(app.handle()));
            tray::build(app.handle())?;
            let settings = app.state::<tray::SettingsStore>().get();
            if let Err(e) = tray::apply(app.handle(), settings) {
                log::warn!("settings: {e}");
            }
            gaming::watch(app.handle());
            app.state::<remote::Remote>().resume(app.handle());
            changes::check(app.handle(), changes::Reason::Start);
            // Desktop widgets come back with Vyro, also when it starts in the tray.
            widgets::sync(app.handle());
            updates::watch(app.handle());
            // CI's smoke test of the installed app (only with VYRO_SMOKE set).
            smoke::start();
            if minimized {
                tray::park_after_start(app.handle());
            } else {
                tray::show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            tray::on_window_event(window, event);
            widgets::on_window_event(window, event);
        })
        .on_page_load(smoke::on_page_load)
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
            commands::get_devices,
            commands::set_device_connected,
            dashboard::get_dashboard,
            dashboard::set_dashboard,
            widgets::get_widgets,
            widgets::set_widgets,
            widgets::widget_action,
            smoke::smoke_report,
            commands::get_drives,
            commands::get_network,
            commands::get_pawnio_status,
            commands::set_ping_host,
            commands::get_health,
            commands::get_health_events,
            commands::get_insights,
            commands::get_performance_score,
            commands::get_alert_rules,
            commands::set_alert_rules,
            commands::get_alert_log,
            commands::get_alert_usual,
            commands::test_alert,
            commands::get_settings,
            commands::set_settings,
            commands::get_profiles,
            commands::get_autostart,
            commands::set_autostart,
            gaming::get_game_sessions,
            gaming::get_game_timeline,
            changes::get_hardware_changes,
            gaming::get_history_window,
            gaming::get_game_state,
            gaming::pick_profile,
            gaming::keep_profile,
            commands::get_fan_status,
            commands::set_fan_config,
            commands::restore_fans,
            commands::get_fan_events,
            bench::benchmark_preflight,
            bench::start_benchmark,
            bench::stop_benchmark,
            bench::get_benchmark_progress,
            bench::get_benchmark_runs,
            bench::get_benchmark_run,
            bench::delete_benchmark,
            diag::run_diagnostics,
            diag::get_diagnostic_runs,
            diag::get_diagnostic_run,
            diag::delete_diagnostic,
            diag::export_diagnostics,
            diag::reveal_report,
            remote::get_remote_status,
            remote::get_remote_brief,
            remote::set_remote_enabled,
            remote::set_remote_options,
            remote::start_remote_pairing,
            remote::cancel_remote_pairing,
            remote::sign_out_remote,
            share::save_share_card,
            share::reveal_card,
            updates::get_update_status,
            updates::check_for_updates,
            updates::install_update,
        ])
        .build(context)
        .expect("failed to build the Tauri application");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            // Ends a benchmark's load and keeps the stopped run before the engine goes.
            bench::stop_and_wait(handle, "Vyro quit.", std::time::Duration::from_secs(3));
            if let Some(remote) = handle.try_state::<remote::Remote>() {
                remote.stop();
            }
            if let Some(engine) = handle.try_state::<Engine>() {
                engine.shutdown();
            }
        }
    });
}

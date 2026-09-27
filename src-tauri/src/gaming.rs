//! Gaming capture (P10): once a second, looks for a fullscreen game in the foreground and
//! tells the engine to capture it. The engine (the service) runs PresentMon; this app only
//! picks the process, since only it sees the signed-in user's desktop.

use tauri::AppHandle;

use crate::engine::Engine;

/// Starts the watcher thread (Windows only; elsewhere there is nothing to capture).
pub fn watch(app: &AppHandle) {
    #[cfg(windows)]
    {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("vyro-game-watch".into())
            .spawn(move || run(&app));
        if let Err(e) = spawned {
            log::warn!("gaming: watcher not started: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn run(app: &AppHandle) {
    use tauri::Manager;
    use vyro_core::gaming::foreground::{alive, foreground, Watcher};

    use crate::tray::SettingsStore;

    let own = std::process::id();
    let mut watcher = Watcher::default();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let (Some(engine), Some(store)) =
            (app.try_state::<Engine>(), app.try_state::<SettingsStore>())
        else {
            continue;
        };
        let fg = foreground();
        let change = if store.get().game_capture {
            watcher.step(fg.as_ref(), own, alive)
        } else if watcher.current().is_some() {
            watcher.reset();
            Some(None)
        } else {
            None
        };
        // The overlay shows only while the captured game is in front, on its monitor.
        let front = fg
            .as_ref()
            .filter(|f| watcher.current().is_some_and(|t| t.pid == f.pid));
        crate::overlay::set_front(app, front.is_some(), front.map(|f| f.monitor));
        if let Some(target) = change {
            match &target {
                Some(t) => log::info!("gaming: {} ({}) in fullscreen", t.name, t.pid),
                None => log::info!("gaming: capture ended"),
            }
            if let Err(e) = engine.api.set_capture_target(target) {
                log::warn!("gaming: {e}");
            }
        }
    }
}

/// Logged game sessions, newest first.
#[tauri::command]
pub async fn get_game_sessions(
    engine: tauri::State<'_, Engine>,
) -> Result<Vec<vyro_core::gaming::GameSession>, String> {
    let api = std::sync::Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.game_sessions())
        .await
        .map_err(|e| e.to_string())?
}

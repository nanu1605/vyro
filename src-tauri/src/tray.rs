//! Tray icon and "keep running in the tray": closing the window hides it, so monitoring and
//! alerts continue until Quit. The choice is saved in `settings.json` (app config folder).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, WindowEvent};
use vyro_core::alerts::AlertFired;
use vyro_core::{lock, unix_now};

use crate::engine::{notify, Engine};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Closing the window keeps Vyro running in the tray (alerts keep working).
    pub close_to_tray: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
        }
    }
}

pub struct SettingsStore {
    path: Option<PathBuf>,
    current: Mutex<Settings>,
    /// The "still running in the tray" hint is shown once per session.
    hinted: AtomicBool,
}

impl SettingsStore {
    pub fn load(app: &AppHandle) -> Self {
        let path = app
            .path()
            .app_config_dir()
            .ok()
            .map(|d| d.join("settings.json"));
        let current = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
            hinted: AtomicBool::new(false),
        }
    }

    pub fn get(&self) -> Settings {
        lock(&self.current).clone()
    }

    pub fn set(&self, s: Settings) -> Result<(), String> {
        *lock(&self.current) = s.clone();
        let path = self.path.as_ref().ok_or("no config folder")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("{}: {e}", path.display()))
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_visible(true);
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Vyro", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Vyro", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Vyro")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Close button: hide to the tray instead of exiting, when the setting is on.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let app = window.app_handle();
    let Some(store) = app.try_state::<SettingsStore>() else {
        return;
    };
    if !store.get().close_to_tray {
        return;
    }
    api.prevent_close();
    let _ = window.hide();
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_visible(false);
    }
    if !store.hinted.swap(true, Ordering::Relaxed) {
        notify(
            app,
            &AlertFired {
                ts: unix_now(),
                rule_id: "tray".into(),
                title: "Vyro is still running".into(),
                message:
                    "Monitoring and alerts continue in the tray. Right-click the icon to quit."
                        .into(),
                sound: false,
            },
        );
    }
}

//! Tray icon, settings and "keep running in the tray": closing the window hides it, so
//! monitoring and alerts continue until Quit. Settings are saved in `settings.json` (app config
//! folder); the tray menu also switches profiles.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent, Wry};
use vyro_core::alerts::AlertFired;
use vyro_core::profiles::Profile;
use vyro_core::units::TempUnit;
use vyro_core::{lock, unix_now};

use crate::engine::{notify, Engine};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Closing the window keeps Vyro running in the tray (alerts keep working).
    pub close_to_tray: bool,
    /// How temperatures are shown (the engine always works in °C).
    pub temperature_unit: TempUnit,
    /// Sensor ids left out of the Sensors and History views.
    pub hidden_sensors: Vec<String>,
    /// Poll rate, notifications, animation and dashboard density.
    pub profile: Profile,
}

/// Emitted with the new [`Settings`] after any change (window or tray menu).
pub const SETTINGS_EVENT: &str = "settings-changed";

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            temperature_unit: TempUnit::C,
            hidden_sensors: Vec::new(),
            profile: Profile::Desktop,
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
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_visible(true);
    }
    if let Some(w) = app.get_webview_window("main") {
        set_webview_active(&w, true);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Hides the window to the tray. Monitoring and alerts go on in the engine; the page gets no
/// more snapshots, and WebView2 is told to trim its memory and suspend until shown again.
pub fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
        set_webview_active(&w, false);
    }
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_visible(false);
    }
}

/// `--minimized` start: the window never showed, so park its WebView2 once the page has had
/// time to load (suspending a page mid-load fails).
pub fn park_after_start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let still_hidden = app.try_state::<Engine>().is_some_and(|e| !e.visible());
        if still_hidden {
            if let Some(w) = app.get_webview_window("main") {
                set_webview_active(&w, false);
            }
        }
    });
}

/// WebView2 while the window is hidden: invisible to the compositor, low memory target, and
/// suspended (no timers or rendering). Undone before the window shows. Best effort: older
/// WebView2 runtimes lack some of these calls.
#[cfg(windows)]
fn set_webview_active(window: &tauri::WebviewWindow, active: bool) {
    let result = window.with_webview(move |webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, ICoreWebView2_3, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;

        let controller = webview.controller();
        // SAFETY: COM calls on the thread that owns the WebView2 controller (with_webview runs
        // the closure there); every interface stays alive for the duration of its call.
        unsafe {
            let Ok(core) = controller.CoreWebView2() else {
                return;
            };
            let level = if active {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
            } else {
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
            };
            if active {
                if let Ok(w3) = core.cast::<ICoreWebView2_3>() {
                    let _ = w3.Resume();
                }
                let _ = controller.SetIsVisible(true);
            } else {
                // Suspending requires an invisible WebView.
                let _ = controller.SetIsVisible(false);
            }
            if let Ok(w19) = core.cast::<ICoreWebView2_19>() {
                let _ = w19.SetMemoryUsageTargetLevel(level);
            }
            if !active {
                if let Ok(w3) = core.cast::<ICoreWebView2_3>() {
                    let done =
                        webview2_com::TrySuspendCompletedHandler::create(Box::new(|_, _| Ok(())));
                    if let Err(e) = w3.TrySuspend(&done) {
                        log::debug!("webview: suspend: {e}");
                    }
                }
            }
        }
    });
    if let Err(e) = result {
        log::debug!("webview: {e}");
    }
}

#[cfg(not(windows))]
fn set_webview_active(_window: &tauri::WebviewWindow, _active: bool) {}

/// The tray's profile items, kept to move the check mark.
pub struct ProfileItems(Vec<(Profile, CheckMenuItem<Wry>)>);

/// Saves `settings` and applies them: the profile's poll rate, the tray check mark, and the
/// window's view (via [`SETTINGS_EVENT`]).
pub fn apply(app: &AppHandle, settings: Settings) -> Result<(), String> {
    let saved = match app.try_state::<SettingsStore>() {
        Some(store) => store.set(settings.clone()),
        None => Ok(()),
    };
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_poll_interval(settings.profile.spec().poll_ms);
    }
    if let Some(items) = app.try_state::<ProfileItems>() {
        for (p, item) in &items.0 {
            let _ = item.set_checked(*p == settings.profile);
        }
    }
    if let Err(e) = app.emit(SETTINGS_EVENT, &settings) {
        log::debug!("emit {SETTINGS_EVENT}: {e}");
    }
    saved
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let current = app
        .try_state::<SettingsStore>()
        .map(|s| s.get().profile)
        .unwrap_or_default();
    let mut items = Vec::new();
    for p in Profile::ALL {
        let spec = p.spec();
        let id = format!(
            "profile:{}",
            serde_json::to_string(&p)
                .unwrap_or_default()
                .trim_matches('"')
        );
        items.push((
            p,
            CheckMenuItem::with_id(app, id, spec.name, true, p == current, None::<&str>)?,
        ));
    }
    let refs: Vec<&dyn IsMenuItem<Wry>> = items
        .iter()
        .map(|(_, i)| i as &dyn IsMenuItem<Wry>)
        .collect();
    let profiles = Submenu::with_items(app, "Profile", true, &refs)?;
    app.manage(ProfileItems(items));

    let show = MenuItem::with_id(app, "show", "Show Vyro", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Vyro", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &profiles, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Vyro")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            id => {
                let Some(name) = id.strip_prefix("profile:") else {
                    return;
                };
                let Ok(profile) = serde_json::from_value::<Profile>(name.into()) else {
                    return;
                };
                if let Some(store) = app.try_state::<SettingsStore>() {
                    let settings = Settings {
                        profile,
                        ..store.get()
                    };
                    if let Err(e) = apply(app, settings) {
                        log::warn!("settings: {e}");
                    }
                }
            }
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
    hide_main(app);
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

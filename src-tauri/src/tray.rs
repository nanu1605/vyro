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
use vyro_core::fan::FanBrief;
use vyro_core::gaming::auto::GameAutomation;
use vyro_core::profiles::Profile;
use vyro_core::units::TempUnit;
use vyro_core::{lock, unix_now};

use crate::engine::{notify, Engine};
use crate::overlay::OverlaySettings;

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
    /// Measure frame rates of fullscreen games (PresentMon).
    pub game_capture: bool,
    /// The companion overlay over the captured game.
    pub overlay: OverlaySettings,
    /// Look for a new release daily (release builds only).
    pub check_updates: bool,
    /// Automatic game profiles and each saved game's own settings.
    pub games: GameAutomation,
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
            game_capture: true,
            overlay: OverlaySettings::default(),
            check_updates: true,
            games: GameAutomation::default(),
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
/// The tray's "Game overlay" item.
pub struct OverlayItem(CheckMenuItem<Wry>);

/// "Return fans to BIOS", enabled while Vyro drives fans.
pub struct FanItem(MenuItem<Wry>);

/// Shows fan control and the alerts that ask to be named (P18) in the tray: the tooltip says so
/// and the menu offers the way back to the BIOS. False until the tray exists (the caller tries
/// again with the next snapshot).
pub fn set_tray_state(app: &AppHandle, fans: FanBrief, alerts: &[String]) -> bool {
    let Some(tray) = app.tray_by_id("main") else {
        return false;
    };
    let _ = tray.set_tooltip(Some(tooltip(fans, alerts)));
    if let Some(item) = app.try_state::<FanItem>() {
        let _ = item.0.set_enabled(fans.active);
    }
    true
}

/// "Vyro · fan control on · alert: Hot under load" (Windows keeps 127 characters).
fn tooltip(fans: FanBrief, alerts: &[String]) -> String {
    let mut tip = String::from(match (fans.active, fans.full_speed) {
        (true, true) => "Vyro · fans at full speed",
        (true, false) => "Vyro · fan control on",
        (false, _) => "Vyro",
    });
    if !alerts.is_empty() {
        tip.push_str(if alerts.len() == 1 {
            " · alert: "
        } else {
            " · alerts: "
        });
        tip.push_str(&alerts.join(", "));
    }
    if tip.chars().count() > 127 {
        tip = tip.chars().take(126).collect::<String>() + "…";
    }
    tip
}

/// Saves `settings` and applies them: the profile in force (the owner's, or a game's), the
/// tray, and the window's view (via [`SETTINGS_EVENT`]). A new profile here is the owner's
/// pick and wins over a running game's.
pub fn apply(app: &AppHandle, mut settings: Settings) -> Result<(), String> {
    for rule in &mut settings.games.rules {
        rule.exe = rule.exe.trim().to_ascii_lowercase();
    }
    settings.games.validate()?;
    let saved = match app.try_state::<SettingsStore>() {
        Some(store) => {
            if store.get().profile != settings.profile {
                crate::gaming::manual_pick(app);
            }
            store.set(settings.clone())
        }
        None => Ok(()),
    };
    crate::gaming::apply_effective(app);
    if let Some(item) = app.try_state::<OverlayItem>() {
        let _ = item.0.set_checked(settings.overlay.enabled);
    }
    if let Err(e) = app.emit(SETTINGS_EVENT, &settings) {
        log::debug!("emit {SETTINGS_EVENT}: {e}");
    }
    saved
}

/// A profile picked in the tray or the window: saved as the owner's profile and put in force,
/// also when it already was the owner's while a game's was in force.
pub fn pick_profile(app: &AppHandle, profile: Profile) -> Result<(), String> {
    let Some(store) = app.try_state::<SettingsStore>() else {
        return Ok(());
    };
    crate::gaming::manual_pick(app);
    apply(
        app,
        Settings {
            profile,
            ..store.get()
        },
    )
}

/// Moves the tray's check mark to the profile in force.
pub fn check_profile(app: &AppHandle, profile: Profile) {
    if let Some(items) = app.try_state::<ProfileItems>() {
        for (p, item) in &items.0 {
            let _ = item.set_checked(*p == profile);
        }
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let settings = app
        .try_state::<SettingsStore>()
        .map(|s| s.get())
        .unwrap_or_default();
    let current = settings.profile;
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

    let overlay = CheckMenuItem::with_id(
        app,
        "overlay",
        "Game overlay",
        true,
        settings.overlay.enabled,
        None::<&str>,
    )?;
    app.manage(OverlayItem(overlay.clone()));

    let fans = MenuItem::with_id(app, "fans-bios", "Return fans to BIOS", false, None::<&str>)?;
    app.manage(FanItem(fans.clone()));

    let show = MenuItem::with_id(app, "show", "Show Vyro", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Vyro", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &profiles, &overlay, &fans, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Vyro")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            "fans-bios" => {
                if let Some(engine) = app.try_state::<Engine>() {
                    if let Err(e) = engine.api.restore_fans() {
                        log::warn!("fans: return to BIOS: {e}");
                    }
                }
            }
            "overlay" => {
                if let Some(store) = app.try_state::<SettingsStore>() {
                    let mut settings = store.get();
                    settings.overlay.enabled = !settings.overlay.enabled;
                    if let Err(e) = apply(app, settings) {
                        log::warn!("settings: {e}");
                    }
                }
            }
            id => {
                let Some(name) = id.strip_prefix("profile:") else {
                    return;
                };
                let Ok(profile) = serde_json::from_value::<Profile>(name.into()) else {
                    return;
                };
                if let Err(e) = pick_profile(app, profile) {
                    log::warn!("settings: {e}");
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
            &AlertFired::new(
                unix_now(),
                "tray",
                "Vyro is still running".into(),
                "Monitoring and alerts continue in the tray. Right-click the icon to quit.".into(),
                false,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_names_fan_control_and_alerts() {
        let on = FanBrief {
            active: true,
            full_speed: false,
        };
        assert_eq!(tooltip(FanBrief::default(), &[]), "Vyro");
        assert_eq!(
            tooltip(on, &["Hot under load".into()]),
            "Vyro · fan control on · alert: Hot under load"
        );
        let many: Vec<String> = (0..20).map(|i| format!("Alert number {i}")).collect();
        let tip = tooltip(FanBrief::default(), &many);
        assert!(tip.starts_with("Vyro · alerts: Alert number 0, "));
        assert_eq!(tip.chars().count(), 127);
    }

    #[test]
    fn settings_from_0_2_get_automatic_game_profiles() {
        let s: Settings =
            serde_json::from_str(r#"{"profile":"work","gameCapture":true,"checkUpdates":false}"#)
                .unwrap();
        assert_eq!(s.profile, Profile::Work);
        assert!(!s.check_updates);
        assert!(s.games.enabled);
        assert_eq!(s.games.default_profile, Some(Profile::Gaming));
        assert!(s.games.rules.is_empty());
    }
}

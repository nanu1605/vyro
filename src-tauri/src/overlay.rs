//! Companion overlay (P10b): a small always-on-top, click-through window in a corner of the
//! game's monitor with the frame rate and temperatures. It is an ordinary desktop window over
//! the game (nothing is injected), so it shows over borderless and windowed-fullscreen games,
//! not over exclusive fullscreen ones. It never takes focus, exists only while a captured
//! game is in front and the overlay is switched on, and is destroyed afterwards.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use vyro_core::gaming::foreground::Rect;
use vyro_core::gaming::{CaptureState, GameSnapshot};
use vyro_core::{lock, SensorSnapshot};

use crate::engine::Engine;
use crate::tray::SettingsStore;

pub const LABEL: &str = "overlay";
/// Carries an [`OverlayData`] to the overlay page every tick.
pub const EVENT: &str = "overlay";
/// WebView2 arguments; windows sharing the browser process must use the same ones, so this
/// matches the main window's `additionalBrowserArgs` (tauri.conf.json). Tauri's defaults plus
/// `--in-process-gpu`: no separate GPU process (tens of MB for a page this light).
pub const WEBVIEW_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --in-process-gpu";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OverlaySettings {
    pub enabled: bool,
    pub corner: Corner,
    /// Adds the frame-time graph and CPU / GPU readings to the FPS figures.
    pub detailed: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            corner: Corner::TopLeft,
            detailed: true,
        }
    }
}

/// What the overlay page shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayData<'a> {
    pub game: &'a GameSnapshot,
    pub detailed: bool,
    pub cpu_temp: Option<f64>,
    pub cpu_usage: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub gpu_usage: Option<f64>,
    pub gpu_power: Option<f64>,
}

/// Overlay state, kept by the app.
#[derive(Default)]
pub struct Overlay {
    /// The captured game is the foreground app (set by the game watcher).
    in_front: AtomicBool,
    /// Its monitor, physical pixels.
    monitor: Mutex<Option<Rect>>,
    /// The window exists and shows.
    shown: AtomicBool,
    /// Size and corner the window was placed with, to move it when they change.
    placed: Mutex<Option<(Corner, bool, Rect)>>,
}

/// From the game watcher, once a second.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn set_front(app: &AppHandle, in_front: bool, monitor: Option<Rect>) {
    if let Some(o) = app.try_state::<Overlay>() {
        o.in_front.store(in_front, Ordering::SeqCst);
        if monitor.is_some() {
            *lock(&o.monitor) = monitor;
        }
    }
}

/// Logical size of the overlay window.
fn size(detailed: bool) -> (f64, f64) {
    if detailed {
        (236.0, 168.0)
    } else {
        (172.0, 64.0)
    }
}

/// Top-left corner of a `w`×`h` window in `corner` of `monitor` (physical pixels), with a
/// margin.
pub fn place(monitor: Rect, corner: Corner, w: i32, h: i32, margin: i32) -> (i32, i32) {
    let left = monitor.left + margin;
    let right = monitor.right - margin - w;
    let top = monitor.top + margin;
    let bottom = monitor.bottom - margin - h;
    match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    }
}

/// Every snapshot: shows, updates or removes the overlay.
pub fn update(app: &AppHandle, snap: &SensorSnapshot) {
    let (Some(o), Some(store)) = (app.try_state::<Overlay>(), app.try_state::<SettingsStore>())
    else {
        return;
    };
    let settings = store.get().overlay;
    let game = snap
        .game
        .as_ref()
        .filter(|g| g.state != CaptureState::Unavailable);
    let monitor = *lock(&o.monitor);
    let want = settings.enabled && o.in_front.load(Ordering::SeqCst) && game.is_some();
    match (want, monitor, game) {
        (true, Some(monitor), Some(game)) => {
            let placed = Some((settings.corner, settings.detailed, monitor));
            let moved = *lock(&o.placed) != placed;
            if !o.shown.swap(true, Ordering::SeqCst) || moved {
                *lock(&o.placed) = placed;
                show(app, monitor, &settings);
            } else {
                raise(app);
            }
            let gpu = snap.gpu.first();
            let data = OverlayData {
                game,
                detailed: settings.detailed,
                cpu_temp: snap.cpu.temp_package,
                cpu_usage: snap.cpu.usage_total,
                gpu_temp: gpu.and_then(|g| g.temp_edge),
                gpu_usage: gpu.and_then(|g| g.usage),
                gpu_power: gpu.and_then(|g| g.power_w),
            };
            if let Err(e) = app.emit_to(LABEL, EVENT, data) {
                log::debug!("emit {EVENT}: {e}");
            }
        }
        _ => {
            if o.shown.swap(false, Ordering::SeqCst) {
                *lock(&o.placed) = None;
                close(app);
            }
        }
    }
}

/// Creates (or moves) the window on the main thread, without activating it.
fn show(app: &AppHandle, monitor: Rect, settings: &OverlaySettings) {
    let handle = app.clone();
    let settings = settings.clone();
    let run = app.run_on_main_thread(move || {
        let scale = handle
            .monitor_from_point(f64::from(monitor.left + 1), f64::from(monitor.top + 1))
            .ok()
            .flatten()
            .map_or(1.0, |m| m.scale_factor());
        let (lw, lh) = size(settings.detailed);
        let (w, h) = ((lw * scale).round() as i32, (lh * scale).round() as i32);
        let (x, y) = place(
            monitor,
            settings.corner,
            w,
            h,
            (16.0 * scale).round() as i32,
        );
        let window = match handle.get_webview_window(LABEL) {
            Some(w) => w,
            None => {
                let built = WebviewWindowBuilder::new(
                    &handle,
                    LABEL,
                    WebviewUrl::App("index.html#overlay".into()),
                )
                .title("Vyro overlay")
                .additional_browser_args(WEBVIEW_ARGS)
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focusable(false)
                .focused(false)
                .visible(false)
                .build();
                match built {
                    Ok(w) => w,
                    Err(e) => {
                        log::warn!("overlay: {e}");
                        return;
                    }
                }
            }
        };
        let _ = window.set_ignore_cursor_events(true);
        let _ = window.set_size(tauri::PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        show_without_focus(&window);
    });
    if let Err(e) = run {
        log::warn!("overlay: {e}");
    }
    set_engine_overlay(app, true);
}

/// Puts the overlay back on top. A fullscreen game is often a topmost window itself and, once
/// it is activated, sits above windows made topmost before it; so this runs every tick.
fn raise(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            show_without_focus(&w);
        }
    });
}

fn close(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(LABEL) {
            let _ = w.destroy();
        }
    });
    set_engine_overlay(app, false);
}

/// While the overlay shows, the engine polls at the profile's rate, not the tray rate.
fn set_engine_overlay(app: &AppHandle, on: bool) {
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_overlay(on);
    }
}

/// Shows the window topmost without taking focus from the game (Tauri's `show` activates).
#[cfg(windows)]
fn show_without_focus(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SW_SHOWNOACTIVATE,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // SAFETY: the handle belongs to a live window of this process, used on its UI thread.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
fn show_without_focus(window: &tauri::WebviewWindow) {
    let _ = window.show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_keep_the_margin_on_any_monitor() {
        let m = Rect {
            left: 2560,
            top: 0,
            right: 5120,
            bottom: 1440,
        };
        assert_eq!(place(m, Corner::TopLeft, 200, 100, 16), (2576, 16));
        assert_eq!(place(m, Corner::TopRight, 200, 100, 16), (4904, 16));
        assert_eq!(place(m, Corner::BottomLeft, 200, 100, 16), (2576, 1324));
        assert_eq!(place(m, Corner::BottomRight, 200, 100, 16), (4904, 1324));
    }

    #[test]
    fn overlay_shares_the_main_windows_webview_arguments() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(
            conf["app"]["windows"][0]["additionalBrowserArgs"].as_str(),
            Some(WEBVIEW_ARGS)
        );
    }

    #[test]
    fn settings_default_off_and_read_partial_json() {
        let s: OverlaySettings = serde_json::from_str(r#"{"corner":"bottomRight"}"#).unwrap();
        assert!(!s.enabled);
        assert!(s.detailed);
        assert_eq!(s.corner, Corner::BottomRight);
    }
}

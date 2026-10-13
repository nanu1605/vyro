//! Companion overlay (P10b, P14): an always-on-top, click-through window in a corner of the
//! game's monitor with the readings the owner picked (Minimal, Performance, Full or Graph mode).
//! It is an ordinary desktop window over the game (nothing is injected), so it shows over
//! borderless and windowed-fullscreen games, not over exclusive fullscreen ones. It never takes
//! focus, exists only while a captured game is in front and the overlay is switched on, and is
//! destroyed afterwards.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use vyro_core::gaming::foreground::Rect;
use vyro_core::gaming::overlay::{self as layout, OverlayMetric, OverlayMode, OverlayRow};
use vyro_core::gaming::{CaptureState, FramePoint, GameSnapshot};
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
/// How often the overlay may refresh, ms.
pub const UPDATE_CHOICES: [u32; 3] = [500, 1000, 2000];
/// Offsets from the corner, at most (logical px).
pub const MAX_OFFSET: u16 = 200;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Colour of the frame rate and graph (fixed choices until themes arrive).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Green,
    Cyan,
    Amber,
    Pink,
    White,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", from = "StoredOverlay")]
pub struct OverlaySettings {
    pub enabled: bool,
    pub mode: OverlayMode,
    pub corner: Corner,
    /// From the corner inward, logical px.
    pub offset_x: u16,
    pub offset_y: u16,
    /// Percent, 75–200.
    pub scale: u16,
    /// Background opacity, percent.
    pub opacity: u8,
    /// Readings shown; empty: the mode's.
    pub metrics: Vec<OverlayMetric>,
    /// Frame-time graph in Performance and Full mode (Graph mode always has it).
    pub graph: bool,
    /// Refreshes at most this often, ms.
    pub update_ms: u32,
    pub accent: Accent,
    /// Tighter spacing.
    pub compact: bool,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: OverlayMode::Performance,
            corner: Corner::TopLeft,
            offset_x: 0,
            offset_y: 0,
            scale: 100,
            opacity: 72,
            metrics: Vec::new(),
            graph: true,
            update_ms: 1000,
            accent: Accent::Green,
            compact: false,
        }
    }
}

/// Settings as saved, also by 0.2 (`detailed` instead of a mode).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredOverlay {
    enabled: Option<bool>,
    mode: Option<OverlayMode>,
    corner: Option<Corner>,
    detailed: Option<bool>,
    offset_x: Option<u16>,
    offset_y: Option<u16>,
    scale: Option<u16>,
    opacity: Option<u8>,
    metrics: Option<Vec<OverlayMetric>>,
    graph: Option<bool>,
    update_ms: Option<u32>,
    accent: Option<Accent>,
    compact: Option<bool>,
}

impl From<StoredOverlay> for OverlaySettings {
    fn from(s: StoredOverlay) -> Self {
        let d = Self::default();
        let mode = s.mode.unwrap_or(match s.detailed {
            Some(false) => OverlayMode::Minimal,
            _ => OverlayMode::Performance,
        });
        Self {
            enabled: s.enabled.unwrap_or(d.enabled),
            mode,
            corner: s.corner.unwrap_or(d.corner),
            offset_x: s.offset_x.unwrap_or(0),
            offset_y: s.offset_y.unwrap_or(0),
            scale: s.scale.unwrap_or(d.scale),
            opacity: s.opacity.unwrap_or(d.opacity),
            metrics: s.metrics.unwrap_or_default(),
            graph: s.graph.unwrap_or(d.graph),
            update_ms: s.update_ms.unwrap_or(d.update_ms),
            accent: s.accent.unwrap_or(d.accent),
            compact: s.compact.unwrap_or(d.compact),
        }
        .clamped()
    }
}

impl OverlaySettings {
    /// Every value within what the window offers.
    pub fn clamped(mut self) -> Self {
        self.offset_x = self.offset_x.min(MAX_OFFSET);
        self.offset_y = self.offset_y.min(MAX_OFFSET);
        self.scale = self.scale.clamp(layout::SCALE_MIN, layout::SCALE_MAX);
        self.opacity = self.opacity.min(100);
        if !UPDATE_CHOICES.contains(&self.update_ms) {
            self.update_ms = 1000;
        }
        self
    }

    /// The readings shown.
    pub fn shown(&self) -> Vec<OverlayMetric> {
        layout::metrics(self.mode, &self.metrics)
    }

    /// The graph shows (always in Graph mode).
    pub fn graph_shown(&self) -> bool {
        self.mode == OverlayMode::Graph || (self.graph && self.mode != OverlayMode::Minimal)
    }

    /// Room the window needs, logical px.
    pub fn size(&self) -> (f64, f64) {
        layout::size(
            self.mode,
            &self.shown(),
            self.graph_shown(),
            self.compact,
            self.scale,
        )
    }
}

/// What the overlay page shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayData<'a> {
    pub app: &'a str,
    pub paused: bool,
    pub mode: OverlayMode,
    pub rows: Vec<OverlayRow>,
    /// Frame-time graph points (empty when the graph is off).
    pub timeline: &'a [Option<FramePoint>],
    pub corner: Corner,
    pub scale: u16,
    pub opacity: u8,
    pub accent: Accent,
    pub compact: bool,
}

impl<'a> OverlayData<'a> {
    pub fn new(settings: &OverlaySettings, snap: &SensorSnapshot, game: &'a GameSnapshot) -> Self {
        Self {
            app: &game.app,
            paused: game.state != CaptureState::Capturing,
            mode: settings.mode,
            rows: layout::rows(&settings.shown(), snap, game),
            timeline: if settings.graph_shown() {
                &game.timeline
            } else {
                &[]
            },
            corner: settings.corner,
            scale: settings.scale,
            opacity: settings.opacity,
            accent: settings.accent,
            compact: settings.compact,
        }
    }
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
    /// Settings and monitor the window was placed with, to move it when they change.
    placed: Mutex<Option<(OverlaySettings, Rect)>>,
    /// Last readings sent (the update rate).
    sent: Mutex<Option<Instant>>,
}

/// The overlay window shows.
pub fn shown(app: &AppHandle) -> bool {
    app.try_state::<Overlay>()
        .is_some_and(|o| o.shown.load(Ordering::SeqCst))
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

/// Top-left corner of a `w`×`h` window in `corner` of `monitor` (physical pixels), `dx` and
/// `dy` in from the corner.
pub fn place(monitor: Rect, corner: Corner, w: i32, h: i32, dx: i32, dy: i32) -> (i32, i32) {
    let left = monitor.left + dx;
    let right = monitor.right - dx - w;
    let top = monitor.top + dy;
    let bottom = monitor.bottom - dy - h;
    match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    }
}

/// The overlay settings in force: the game's own mode, if it has one.
fn in_force(app: &AppHandle, settings: OverlaySettings) -> OverlaySettings {
    match crate::gaming::overlay_mode(app) {
        Some(mode) => OverlaySettings { mode, ..settings },
        None => settings,
    }
}

/// Every snapshot: shows, updates or removes the overlay.
pub fn update(app: &AppHandle, snap: &SensorSnapshot) {
    let (Some(o), Some(store)) = (app.try_state::<Overlay>(), app.try_state::<SettingsStore>())
    else {
        return;
    };
    let settings = in_force(app, store.get().overlay);
    let game = snap
        .game
        .as_ref()
        .filter(|g| g.state != CaptureState::Unavailable);
    let monitor = *lock(&o.monitor);
    let enabled = crate::gaming::overlay_wanted(app, settings.enabled);
    let want = enabled && o.in_front.load(Ordering::SeqCst) && game.is_some();
    match (want, monitor, game) {
        (true, Some(monitor), Some(game)) => {
            let placed = Some((settings.clone(), monitor));
            let changed = *lock(&o.placed) != placed;
            if !o.shown.swap(true, Ordering::SeqCst) || changed {
                *lock(&o.placed) = placed;
                *lock(&o.sent) = None;
                show(app, monitor, &settings);
            } else {
                raise(app);
            }
            // At most once per update_ms (a little early is fine: ticks are not exact).
            let due = Duration::from_millis(u64::from(settings.update_ms).saturating_sub(150));
            let mut sent = lock(&o.sent);
            if sent.is_some_and(|t| t.elapsed() < due) {
                return;
            }
            *sent = Some(Instant::now());
            drop(sent);
            if let Err(e) = app.emit_to(LABEL, EVENT, OverlayData::new(&settings, snap, game)) {
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
        let (lw, lh) = settings.size();
        let (w, h) = ((lw * scale).round() as i32, (lh * scale).round() as i32);
        let margin = 16.0;
        let px = |logical: u16| ((margin + f64::from(logical)) * scale).round() as i32;
        let (x, y) = place(
            monitor,
            settings.corner,
            w,
            h,
            px(settings.offset_x),
            px(settings.offset_y),
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
        assert_eq!(place(m, Corner::TopLeft, 200, 100, 16, 16), (2576, 16));
        assert_eq!(place(m, Corner::TopRight, 200, 100, 16, 16), (4904, 16));
        assert_eq!(place(m, Corner::BottomLeft, 200, 100, 16, 16), (2576, 1324));
        assert_eq!(
            place(m, Corner::BottomRight, 200, 100, 16, 16),
            (4904, 1324)
        );
        // Offsets move inward from the corner, on each axis.
        assert_eq!(place(m, Corner::TopLeft, 200, 100, 66, 116), (2626, 116));
        assert_eq!(
            place(m, Corner::BottomRight, 200, 100, 66, 116),
            (4854, 1224)
        );
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
        assert_eq!(s.mode, OverlayMode::Performance);
        assert_eq!(s.corner, Corner::BottomRight);
        assert_eq!((s.scale, s.opacity, s.update_ms), (100, 72, 1000));
    }

    #[test]
    fn settings_from_0_2_keep_their_look() {
        let detailed: OverlaySettings =
            serde_json::from_str(r#"{"enabled":true,"corner":"topRight","detailed":true}"#)
                .unwrap();
        assert_eq!(
            (detailed.enabled, detailed.mode, detailed.graph),
            (true, OverlayMode::Performance, true)
        );
        let plain: OverlaySettings = serde_json::from_str(r#"{"detailed":false}"#).unwrap();
        assert_eq!(plain.mode, OverlayMode::Minimal);
        // Saved again, without the old field.
        let json = serde_json::to_string(&plain).unwrap();
        assert!(
            !json.contains("detailed") && json.contains(r#""mode":"minimal""#),
            "{json}"
        );
    }

    #[test]
    fn values_out_of_range_are_brought_back() {
        let s: OverlaySettings = serde_json::from_str(
            r#"{"scale":500,"opacity":150,"offsetX":900,"updateMs":250,"mode":"graph"}"#,
        )
        .unwrap();
        assert_eq!(
            (s.scale, s.opacity, s.offset_x, s.update_ms),
            (200, 100, 200, 1000)
        );
        assert!(s.graph_shown(), "graph mode always has the graph");
        let minimal = OverlaySettings {
            mode: OverlayMode::Minimal,
            ..OverlaySettings::default()
        };
        assert!(!minimal.graph_shown());
        assert_eq!(minimal.shown(), vec![OverlayMetric::Fps]);
    }
}

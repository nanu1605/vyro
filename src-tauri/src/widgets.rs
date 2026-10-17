//! Desktop widgets (P30): small windows on the desktop with the readings the owner picked, kept
//! behind other windows (or on top, if asked), in `widgets.json` next to `settings.json`. Each
//! widget is its own frameless, transparent window (`widget-<id>`) loading the app's page with
//! `#widget=<id>`; they share the app's WebView2 browser process. Readings reach them as
//! `widget-sensors` events, also while the main window sits in the tray, and the engine keeps
//! the window's poll rate for them. Dragging a widget moves its window; where it ends up is
//! saved a moment after it stops; while dragged it snaps to the screen's edges and to other
//! widgets (`snap.rs`, 1.2.0). Locked, a widget can let clicks through to what is under it.
//! Widgets behind windows stay in view through Windows' Show desktop (Win+D): while the desktop
//! shows they are kept on top (`pin.rs`, 1.1.1).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};
use vyro_core::{files, lock, SensorSnapshot};

use crate::engine::Engine;

/// Window labels start with this, followed by the widget's id.
pub const PREFIX: &str = "widget-";
/// Carries each snapshot to the widget windows.
pub const DATA_EVENT: &str = "widget-sensors";
/// Carries the stored [`Widgets`] after any change (Widgets page, a widget's menu, the tray).
pub const CHANGED_EVENT: &str = "widgets-changed";
pub const MAX_WIDGETS: usize = 8;
pub const MAX_SOURCES: usize = 6;
pub const KINDS: [&str; 8] = [
    "gauge", "reading", "graph", "stack", "clock", "network", "game", "top",
];
/// What a "top" widget ranks processes by (its one source).
pub const PROC_SORTS: [&str; 3] = ["proc.cpu", "proc.ram", "proc.gpu"];
/// Every widget window's title (`pin.rs` tells widgets from Vyro's other windows by it).
pub const TITLE: &str = "Vyro widget";

/// How a widget's card looks: the app's theme, see-through dark, or solid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum Style {
    #[default]
    App,
    Glass,
    Solid,
}

impl From<String> for Style {
    fn from(s: String) -> Self {
        match s.as_str() {
            "glass" => Self::Glass,
            "solid" => Self::Solid,
            _ => Self::App,
        }
    }
}

/// Behind every window (on the desktop) or in front of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "String")]
pub enum Layer {
    #[default]
    Desktop,
    Top,
}

impl From<String> for Layer {
    fn from(s: String) -> Self {
        match s.as_str() {
            "top" => Self::Top,
            _ => Self::Desktop,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopWidget {
    /// `[a-z0-9-]`, 1–24 characters.
    pub id: String,
    /// One of [`KINDS`].
    pub kind: String,
    /// Reading ids (the Dashboard's): one, several for "stack", none or two for "clock", none
    /// for "network" and "game". A "top" widget keeps what it ranks by ([`PROC_SORTS`]).
    pub sources: Vec<String>,
    /// Top left on the desktop, logical px; set once the window has been placed.
    pub x: i32,
    pub y: i32,
    pub placed: bool,
    /// Size, 50–200 %.
    pub scale: u16,
    /// The card's background, 0–100 %.
    pub opacity: u8,
    pub style: Style,
    pub layer: Layer,
    /// Not moved by dragging.
    pub locked: bool,
    /// While locked, the mouse goes through to what is under the widget ([`passes_clicks`]).
    pub click_through: bool,
}

impl Default for DesktopWidget {
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: "reading".into(),
            sources: Vec::new(),
            x: 0,
            y: 0,
            placed: false,
            scale: 100,
            opacity: 85,
            style: Style::App,
            layer: Layer::Desktop,
            locked: false,
            click_through: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Widgets {
    /// File format version.
    pub version: u32,
    /// Shown on the desktop (the Widgets page's switch and the tray's).
    pub shown: bool,
    pub widgets: Vec<DesktopWidget>,
}

impl Default for Widgets {
    fn default() -> Self {
        Self {
            version: 1,
            shown: true,
            widgets: Vec::new(),
        }
    }
}

fn valid_id(id: &str) -> bool {
    (1..=24).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_source(s: &str) -> bool {
    (1..=40).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// Checks and tidies widgets from the page or the file: known kinds, unique ids, at most
/// [`MAX_WIDGETS`], sizes and backgrounds in range, only as many readings as the kind shows.
pub fn sanitize(mut w: Widgets) -> Widgets {
    w.version = 1;
    let mut seen = HashSet::new();
    w.widgets.retain(|x| {
        valid_id(&x.id) && KINDS.contains(&x.kind.as_str()) && seen.insert(x.id.clone())
    });
    w.widgets.truncate(MAX_WIDGETS);
    for x in &mut w.widgets {
        let mut kept = HashSet::new();
        let top = x.kind == "top";
        x.sources.retain(|s| {
            valid_source(s) && (!top || PROC_SORTS.contains(&s.as_str())) && kept.insert(s.clone())
        });
        let max = match x.kind.as_str() {
            "stack" => MAX_SOURCES,
            "clock" => 2,
            "network" | "game" => 0,
            _ => 1,
        };
        x.sources.truncate(max);
        x.scale = x.scale.clamp(50, 200);
        x.opacity = x.opacity.min(100);
    }
    w
}

/// A widget's window size at its scale, logical px.
pub fn size(w: &DesktopWidget) -> (f64, f64) {
    let (bw, bh) = match w.kind.as_str() {
        "gauge" => (200.0, 200.0),
        "graph" => (300.0, 150.0),
        "stack" => (250.0, 52.0 + 36.0 * w.sources.len().max(1) as f64),
        "clock" => (260.0, 120.0),
        "network" => (280.0, 150.0),
        "game" => (260.0, 150.0),
        "top" => (260.0, 200.0),
        _ => (220.0, 104.0),
    };
    let k = f64::from(w.scale) / 100.0;
    (bw * k, bh * k)
}

/// Whether the mouse goes through the widget: only while it is locked, so unlocking it (on the
/// Widgets page, as it can't be right-clicked then) always makes it grabbable again.
pub fn passes_clicks(w: &DesktopWidget) -> bool {
    w.locked && w.click_through
}

/// The saved widgets with the positions the store keeps: a page's copy may be older than the
/// last drag, so its x, y and "placed" never win for a widget that already exists.
pub fn merge_positions(stored: &Widgets, mut next: Widgets) -> Widgets {
    for w in &mut next.widgets {
        match stored.widgets.iter().find(|s| s.id == w.id) {
            Some(s) => {
                w.x = s.x;
                w.y = s.y;
                w.placed = s.placed;
            }
            None => w.placed = false,
        }
    }
    next
}

/// A monitor's work area, logical px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A widget's place on the desktop, logical px: left, top, width, height.
pub type Spot = (f64, f64, f64, f64);

/// The space kept between widgets Vyro places, as when they snap (`snap.rs`).
const SPACING: f64 = 8.0;

/// Where a widget goes: its saved place while that is on a connected monitor (its middle at
/// least). Else the first free spot down the right edge of the first monitor, one under the
/// other with a small space, then the next column to the left; `taken` holds the other
/// widgets' places. When the monitor is full, the top of its right edge.
pub fn place(w: &DesktopWidget, size: (f64, f64), areas: &[Area], taken: &[Spot]) -> (f64, f64) {
    let (sw, sh) = size;
    if w.placed {
        let (cx, cy) = (f64::from(w.x) + sw / 2.0, f64::from(w.y) + sh / 2.0);
        if areas
            .iter()
            .any(|a| cx >= a.x && cx < a.x + a.w && cy >= a.y && cy < a.y + a.h)
        {
            return (f64::from(w.x), f64::from(w.y));
        }
    }
    let a = areas.first().copied().unwrap_or(Area {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    });
    let margin = 24.0;
    let right = a.x + a.w - sw - margin;
    let mut x = right;
    while x >= a.x + margin - 0.5 {
        let mut y = a.y + margin;
        while y + sh <= a.y + a.h - margin + 0.5 {
            // The widgets in the way of this spot; below the lowest of them is the next try.
            let below = taken
                .iter()
                .filter(|&&(tx, ty, tw, th)| {
                    x < tx + tw + SPACING
                        && tx < x + sw + SPACING
                        && y < ty + th + SPACING
                        && ty < y + sh + SPACING
                })
                .map(|&(_, ty, _, th)| ty + th + SPACING)
                .fold(None, |m: Option<f64>, b| Some(m.map_or(b, |m| m.max(b))));
            match below {
                None => return (x, y),
                Some(b) => y = b,
            }
        }
        x -= sw + SPACING;
    }
    (right, a.y + margin)
}

// ---------------------------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------------------------

pub struct WidgetStore {
    path: Option<PathBuf>,
    current: Mutex<Widgets>,
    /// Positions dragged to, waiting to be saved: id → (x, y, when).
    moved: Mutex<HashMap<String, (i32, i32, Instant)>>,
    flushing: AtomicBool,
}

fn read(path: &Path) -> Widgets {
    match files::read_json::<Widgets>(path, |_| Ok(())) {
        Ok(w) => sanitize(w.unwrap_or_default()),
        Err(e) => {
            log::warn!(
                "widgets: {} does not load ({e}); starting without widgets",
                path.display()
            );
            Widgets::default()
        }
    }
}

impl WidgetStore {
    pub fn load(app: &AppHandle) -> Self {
        let path = app
            .path()
            .app_config_dir()
            .ok()
            .map(|d| d.join("widgets.json"));
        let current = path.as_deref().map(read).unwrap_or_default();
        Self {
            path,
            current: Mutex::new(current),
            moved: Mutex::new(HashMap::new()),
            flushing: AtomicBool::new(false),
        }
    }

    pub fn get(&self) -> Widgets {
        lock(&self.current).clone()
    }

    fn set(&self, w: Widgets) -> Result<(), String> {
        if let Some(path) = &self.path {
            files::write_json(path, &w).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        *lock(&self.current) = w;
        Ok(())
    }
}

/// Whether any widget shows (the engine then keeps the window's poll rate).
pub fn active(app: &AppHandle) -> bool {
    app.try_state::<WidgetStore>().is_some_and(|s| {
        let w = lock(&s.current);
        w.shown && !w.widgets.is_empty()
    })
}

/// Each snapshot to the widget windows (only they listen to this event).
pub fn update(app: &AppHandle, snap: &SensorSnapshot) {
    if active(app) {
        if let Err(e) = app.emit(DATA_EVENT, snap) {
            log::debug!("emit {DATA_EVENT}: {e}");
        }
    }
}

fn areas(app: &AppHandle) -> Vec<Area> {
    let mut out: Vec<Area> = Vec::new();
    let primary = app.primary_monitor().ok().flatten();
    let monitors = app.available_monitors().unwrap_or_default();
    for m in primary.iter().chain(monitors.iter()) {
        let k = m.scale_factor();
        let wa = m.work_area();
        let a = Area {
            x: f64::from(wa.position.x) / k,
            y: f64::from(wa.position.y) / k,
            w: f64::from(wa.size.width) / k,
            h: f64::from(wa.size.height) / k,
        };
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// Opens, updates or closes the widget windows to match the store.
///
/// The window work is always queued to the event loop from a thread of its own. Called on the
/// main thread (a synchronous command, the tray, setup), `run_on_main_thread` runs it at once,
/// and building or destroying a WebView2 window inside WebView2's own message callback hangs
/// the main thread on Windows: no widget, no answer to any later command, and Quit does
/// nothing (1.1's first field check). The game overlay is shown the same way from its own
/// thread.
pub fn sync(app: &AppHandle) {
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_widgets(active(app));
    }
    let handle = app.clone();
    let queued = std::thread::Builder::new()
        .name("vyro-widgets".into())
        .spawn(move || {
            let app = handle.clone();
            if let Err(e) = handle.run_on_main_thread(move || apply(&app)) {
                log::warn!("widgets: {e}");
            }
        });
    if let Err(e) = queued {
        log::warn!("widgets: {e}");
    }
}

/// The windows as the store has them when the event loop gets to it (the latest of several
/// quick saves wins). Runs on the main thread, from the event loop.
fn apply(handle: &AppHandle) {
    let Some(store) = handle.try_state::<WidgetStore>() else {
        return;
    };
    let cfg = store.get();
    let want: Vec<DesktopWidget> = if cfg.shown { cfg.widgets } else { Vec::new() };
    for (label, w) in handle.webview_windows() {
        if let Some(id) = label.strip_prefix(PREFIX) {
            if !want.iter().any(|x| x.id == id) {
                let _ = w.destroy();
            }
        }
    }
    let monitors = areas(handle);
    // Where the widgets are, so that a new one goes where none is.
    let mut taken: Vec<(&str, Spot)> = want
        .iter()
        .filter(|w| w.placed)
        .map(|w| {
            let (sw, sh) = size(w);
            (w.id.as_str(), (f64::from(w.x), f64::from(w.y), sw, sh))
        })
        .collect();
    let mut placed = Vec::new();
    for x in &want {
        let others: Vec<Spot> = taken
            .iter()
            .filter(|(id, _)| *id != x.id)
            .map(|&(_, s)| s)
            .collect();
        if let Some(pos) = open_or_update(handle, x, &monitors, &others) {
            let (sw, sh) = size(x);
            taken.retain(|(id, _)| *id != x.id);
            taken.push((x.id.as_str(), (pos.0, pos.1, sw, sh)));
            placed.push((x.id.clone(), pos));
        }
    }
    // Show desktop is watched for while a widget sits behind windows.
    crate::pin::watch(handle, want.iter().any(|w| w.layer == Layer::Desktop));
    // New widgets: keep where they were put, so they come back there.
    if !placed.is_empty() {
        let mut cfg = store.get();
        for (id, (px, py)) in placed {
            if let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) {
                (w.x, w.y, w.placed) = (px.round() as i32, py.round() as i32, true);
            }
        }
        if let Err(e) = store.set(cfg) {
            log::warn!("widgets: {e}");
        }
    }
}

/// Returns the position a new (or moved-back) widget was given.
fn open_or_update(
    app: &AppHandle,
    w: &DesktopWidget,
    monitors: &[Area],
    taken: &[Spot],
) -> Option<(f64, f64)> {
    let label = format!("{PREFIX}{}", w.id);
    let (sw, sh) = size(w);
    let (x, y) = place(w, (sw, sh), monitors, taken);
    let moved = !w.placed || (x, y) != (f64::from(w.x), f64::from(w.y));
    let (top, bottom) = stacking(w.layer, crate::pin::desktop_shown());
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.set_size(LogicalSize::new(sw, sh));
        stack(&win, (top, bottom));
        // tao changes nothing when the flag is as it was.
        let _ = win.set_ignore_cursor_events(passes_clicks(w));
        if moved {
            let _ = win.set_position(LogicalPosition::new(x, y));
        }
        return moved.then_some((x, y));
    }
    let built = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(format!("index.html#widget={}", w.id).into()),
    )
    .title(TITLE)
    .additional_browser_args(crate::overlay::WEBVIEW_ARGS)
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .skip_taskbar(true)
    .always_on_top(top)
    .always_on_bottom(bottom)
    .focused(false)
    .inner_size(sw, sh)
    .position(x, y)
    .visible(false)
    .build();
    match built {
        Ok(win) => {
            // Shown through tao, not with a bare ShowWindow: tao hides a window it believes
            // hidden on its next change of flags (a layer switch, Show desktop). Built unfocused,
            // tao shows it without activating it (SW_SHOWNOACTIVATE), so it takes no focus.
            let _ = win.show();
            let _ = win.set_ignore_cursor_events(passes_clicks(w));
            #[cfg(windows)]
            crate::snap::attach(&win);
            moved.then_some((x, y))
        }
        Err(e) => {
            log::warn!("widgets: {e}");
            None
        }
    }
}

/// Whether a widget's window is kept (on top, at the bottom). "On top" always is on top;
/// "Behind windows" sits at the bottom, and on top while Windows shows the desktop (Win+D), which
/// would otherwise cover it (1.1.1).
pub fn stacking(layer: Layer, desktop_shown: bool) -> (bool, bool) {
    match layer {
        Layer::Top => (true, false),
        Layer::Desktop if desktop_shown => (true, false),
        Layer::Desktop => (false, true),
    }
}

fn stack(win: &WebviewWindow, (top, bottom): (bool, bool)) {
    // One off before the other on: a window kept at the bottom is put back there on every move.
    if top {
        let _ = win.set_always_on_bottom(false);
        let _ = win.set_always_on_top(true);
    } else {
        let _ = win.set_always_on_top(false);
        let _ = win.set_always_on_bottom(bottom);
    }
}

/// Show desktop began or ended (`pin.rs`): every widget window to its place in the stack. Main
/// thread.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn restack(app: &AppHandle) {
    let Some(store) = app.try_state::<WidgetStore>() else {
        return;
    };
    let shown = crate::pin::desktop_shown();
    for w in store.get().widgets {
        if let Some(win) = app.get_webview_window(&format!("{PREFIX}{}", w.id)) {
            stack(&win, stacking(w.layer, shown));
        }
    }
}

/// A widget window moved (dragged, or put back by Vyro): its place is saved once it has been
/// still for a moment.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let Some(id) = window.label().strip_prefix(PREFIX) else {
        return;
    };
    // Alt+F4 on a widget: it stays (widgets go from their menu or the Widgets page).
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        return;
    }
    let WindowEvent::Moved(pos) = event else {
        return;
    };
    let app = window.app_handle();
    let Some(store) = app.try_state::<WidgetStore>() else {
        return;
    };
    let k = window.scale_factor().unwrap_or(1.0);
    let at = (
        (f64::from(pos.x) / k).round() as i32,
        (f64::from(pos.y) / k).round() as i32,
    );
    lock(&store.moved).insert(id.to_owned(), (at.0, at.1, Instant::now()));
    if store.flushing.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(700));
        let Some(store) = app.try_state::<WidgetStore>() else {
            return;
        };
        let due: Vec<(String, (i32, i32))> = {
            let mut moved = lock(&store.moved);
            let ready: Vec<String> = moved
                .iter()
                .filter(|(_, (_, _, t))| t.elapsed() >= Duration::from_millis(600))
                .map(|(id, _)| id.clone())
                .collect();
            ready
                .into_iter()
                .filter_map(|id| moved.remove(&id).map(|(x, y, _)| (id, (x, y))))
                .collect()
        };
        if !due.is_empty() {
            let mut cfg = store.get();
            let mut changed = false;
            for (id, (x, y)) in due {
                if let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) {
                    if (w.x, w.y, w.placed) != (x, y, true) {
                        (w.x, w.y, w.placed) = (x, y, true);
                        changed = true;
                    }
                }
            }
            if changed {
                if let Err(e) = store.set(cfg) {
                    log::warn!("widgets: {e}");
                }
            }
        }
        if lock(&store.moved).is_empty() {
            store.flushing.store(false, Ordering::SeqCst);
            return;
        }
    });
}

/// Saves, then puts the windows and every view in step.
pub(crate) fn store_and_sync(app: &AppHandle, next: Widgets) -> Result<Widgets, String> {
    let store = app.try_state::<WidgetStore>().ok_or("widgets not loaded")?;
    let next = sanitize(merge_positions(&store.get(), next));
    store.set(next.clone())?;
    sync(app);
    crate::tray::set_widgets_checked(app, next.shown);
    if let Err(e) = app.emit(CHANGED_EVENT, &next) {
        log::debug!("emit {CHANGED_EVENT}: {e}");
    }
    Ok(next)
}

/// Shows or hides every widget (the tray's item).
pub fn toggle_shown(app: &AppHandle) {
    let Some(store) = app.try_state::<WidgetStore>() else {
        return;
    };
    let mut w = store.get();
    w.shown = !w.shown;
    if let Err(e) = store_and_sync(app, w) {
        log::warn!("widgets: {e}");
    }
}

#[tauri::command]
pub fn get_widgets(store: State<'_, WidgetStore>) -> Widgets {
    store.get()
}

/// Saves the widgets (from the Widgets page) and returns them as stored. Async, so it runs off
/// the main thread (see [`sync`]).
#[tauri::command]
pub async fn set_widgets(app: AppHandle, widgets: Widgets) -> Result<Widgets, String> {
    store_and_sync(&app, widgets)
}

/// A widget's own menu: lock, unlock, remove, or edit it on the Widgets page. Async, so a
/// widget removing itself is not destroyed inside its own message callback (see [`sync`]).
#[tauri::command]
pub async fn widget_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    let store = app.try_state::<WidgetStore>().ok_or("widgets not loaded")?;
    let mut cfg = store.get();
    match action.as_str() {
        "lock" | "unlock" => {
            if let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) {
                w.locked = action == "lock";
            }
        }
        "remove" => cfg.widgets.retain(|w| w.id != id),
        "edit" => {
            crate::tray::show_main(&app);
            if let Err(e) = app.emit_to("main", crate::look::OPEN_VIEW_EVENT, "widgets") {
                log::debug!("emit open-view: {e}");
            }
            return Ok(());
        }
        _ => return Err(format!("unknown action {action}")),
    }
    store_and_sync(&app, cfg).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(id: &str, kind: &str, sources: &[&str]) -> DesktopWidget {
        DesktopWidget {
            id: id.into(),
            kind: kind.into(),
            sources: sources.iter().map(|s| (*s).into()).collect(),
            ..DesktopWidget::default()
        }
    }

    #[test]
    fn sanitize_keeps_known_kinds_unique_ids_and_ranges() {
        let mut w = Widgets {
            widgets: vec![
                widget("cpu", "gauge", &["cpu.usage", "cpu.temp"]),
                widget("cpu", "reading", &["mem.used"]),
                widget("Bad Id", "reading", &["mem.used"]),
                widget("map", "map", &[]),
                widget(
                    "s",
                    "stack",
                    &["a", "b", "b", "c", "d", "e", "f", "g", "../etc"],
                ),
            ],
            ..Widgets::default()
        };
        w.widgets[0].scale = 900;
        w.widgets[0].opacity = 255;
        let s = sanitize(w);
        assert_eq!(
            s.widgets.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
            ["cpu", "s"]
        );
        assert_eq!(
            s.widgets[0].sources,
            ["cpu.usage"],
            "a gauge shows one reading"
        );
        assert_eq!((s.widgets[0].scale, s.widgets[0].opacity), (200, 100));
        assert_eq!(s.widgets[1].sources, ["a", "b", "c", "d", "e", "f"]);
        let many = Widgets {
            widgets: (0..20)
                .map(|i| widget(&format!("w{i}"), "clock", &[]))
                .collect(),
            ..Widgets::default()
        };
        assert_eq!(sanitize(many).widgets.len(), MAX_WIDGETS);
    }

    #[test]
    fn the_new_kinds_keep_only_what_they_show() {
        let s = sanitize(Widgets {
            widgets: vec![
                widget("n", "network", &["net.down"]),
                widget("g", "game", &["game.fps"]),
                widget("t", "top", &["cpu.usage", "proc.ram", "proc.gpu"]),
                widget("u", "top", &["../etc"]),
            ],
            ..Widgets::default()
        });
        let sources: Vec<&[String]> = s.widgets.iter().map(|w| w.sources.as_slice()).collect();
        assert!(sources[0].is_empty() && sources[1].is_empty());
        assert_eq!(sources[2], ["proc.ram"], "a known ranking, one of them");
        assert!(sources[3].is_empty(), "an unknown ranking reads as CPU");
    }

    #[test]
    fn clicks_go_through_only_while_locked() {
        let mut w = widget("a", "reading", &[]);
        w.click_through = true;
        assert!(!passes_clicks(&w), "unlocked: it can be dragged");
        w.locked = true;
        assert!(passes_clicks(&w));
        w.click_through = false;
        assert!(!passes_clicks(&w));
        // Files from 1.1 have no such field.
        let old: DesktopWidget =
            serde_json::from_str(r#"{"id":"a","kind":"reading","locked":true}"#).unwrap();
        assert!(!old.click_through);
        assert_eq!(
            serde_json::to_value(&w).unwrap()["clickThrough"],
            serde_json::Value::Bool(false)
        );
    }

    #[test]
    fn unknown_styles_and_layers_read_as_the_defaults() {
        let w: DesktopWidget =
            serde_json::from_str(r#"{"id":"a","kind":"reading","style":"neon","layer":"middle"}"#)
                .unwrap();
        assert_eq!((w.style, w.layer), (Style::App, Layer::Desktop));
        let w: DesktopWidget =
            serde_json::from_str(r#"{"id":"a","kind":"reading","style":"glass","layer":"top"}"#)
                .unwrap();
        assert_eq!((w.style, w.layer), (Style::Glass, Layer::Top));
        assert_eq!(serde_json::to_value(Layer::Top).unwrap(), "top");
    }

    #[test]
    fn behind_windows_goes_on_top_only_while_the_desktop_shows() {
        assert_eq!(stacking(Layer::Desktop, false), (false, true));
        assert_eq!(stacking(Layer::Desktop, true), (true, false));
        assert_eq!(stacking(Layer::Top, false), (true, false));
        assert_eq!(stacking(Layer::Top, true), (true, false));
    }

    #[test]
    fn sizes_follow_the_kind_and_scale() {
        let mut g = widget("g", "gauge", &["cpu.usage"]);
        assert_eq!(size(&g), (200.0, 200.0));
        g.scale = 150;
        assert_eq!(size(&g), (300.0, 300.0));
        let s = widget("s", "stack", &["a", "b", "c"]);
        assert_eq!(size(&s), (250.0, 52.0 + 108.0));
        assert_eq!(size(&widget("n", "network", &[])), (280.0, 150.0));
        assert_eq!(size(&widget("g", "game", &[])), (260.0, 150.0));
        assert_eq!(size(&widget("t", "top", &["proc.cpu"])), (260.0, 200.0));
    }

    #[test]
    fn a_widget_stays_where_it_was_unless_its_monitor_is_gone() {
        let one = [Area {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1040.0,
        }];
        let mut w = widget("a", "reading", &[]);
        // New: at the top of the right edge.
        assert_eq!(place(&w, (220.0, 104.0), &one, &[]), (1676.0, 24.0));
        w.placed = true;
        (w.x, w.y) = (100, 200);
        assert_eq!(place(&w, (220.0, 104.0), &one, &[]), (100.0, 200.0));
        // Saved places are kept even where another widget is.
        let there = [(100.0, 200.0, 220.0, 104.0)];
        assert_eq!(place(&w, (220.0, 104.0), &one, &there), (100.0, 200.0));
        // Its monitor was unplugged: back on the first one.
        (w.x, w.y) = (2500, 200);
        assert_eq!(place(&w, (220.0, 104.0), &one, &[]), (1676.0, 24.0));
        // A second monitor on the left, at negative x.
        let two = [
            one[0],
            Area {
                x: -1920.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
            },
        ];
        (w.x, w.y) = (-1000, 300);
        assert_eq!(place(&w, (220.0, 104.0), &two, &[]), (-1000.0, 300.0));
    }

    #[test]
    fn new_widgets_go_where_no_widget_is() {
        let one = [Area {
            x: 0.0,
            y: 0.0,
            w: 1920.0,
            h: 1040.0,
        }];
        let overlap = |a: Spot, b: Spot| {
            a.0 < b.0 + b.2 && b.0 < a.0 + a.2 && a.1 < b.1 + b.3 && b.1 < a.1 + a.3
        };
        // Three added at once (1.2's first field check: they were put 40 px apart, on top of
        // each other): one under the other, 8 px apart.
        let mut taken: Vec<Spot> = Vec::new();
        for (kind, sources) in [
            ("gauge", &["cpu.usage"][..]),
            ("network", &[]),
            ("top", &[]),
        ] {
            let w = widget("n", kind, sources);
            let (sw, sh) = size(&w);
            let (x, y) = place(&w, (sw, sh), &one, &taken);
            let spot = (x, y, sw, sh);
            assert!(
                taken.iter().all(|&t| !overlap(spot, t)),
                "{kind} at {spot:?}"
            );
            taken.push(spot);
        }
        assert_eq!(taken[0].1, 24.0);
        assert_eq!(taken[1].1, 24.0 + taken[0].3 + 8.0);
        // One the owner put at the top right is gone round, not covered.
        let mine = (1650.0, 10.0, 260.0, 200.0);
        let w = widget("n", "reading", &["cpu.temp"]);
        let (x, y) = place(&w, (220.0, 104.0), &one, &[mine]);
        assert!(!overlap((x, y, 220.0, 104.0), mine));
        assert_eq!((x, y), (1676.0, 218.0));
        // A full right edge: the next column to the left.
        let column: Vec<Spot> = (0..9)
            .map(|i| (1676.0, 24.0 + 112.0 * f64::from(i), 220.0, 104.0))
            .collect();
        let (x, y) = place(&w, (220.0, 104.0), &one, &column);
        assert_eq!((x, y), (1676.0 - 228.0, 24.0));
    }

    #[test]
    fn the_page_never_moves_a_widget_back() {
        let mut stored = Widgets::default();
        let mut a = widget("a", "reading", &[]);
        (a.x, a.y, a.placed) = (500, 600, true);
        stored.widgets.push(a.clone());
        let mut stale = a.clone();
        (stale.x, stale.y) = (10, 10);
        stale.opacity = 40;
        let mut fresh = widget("b", "clock", &[]);
        fresh.placed = true;
        let next = merge_positions(
            &stored,
            Widgets {
                widgets: vec![stale, fresh],
                ..Widgets::default()
            },
        );
        assert_eq!(
            (
                next.widgets[0].x,
                next.widgets[0].y,
                next.widgets[0].opacity
            ),
            (500, 600, 40)
        );
        assert!(!next.widgets[1].placed, "a new widget is placed by Vyro");
    }

    #[test]
    fn saves_and_sets_a_damaged_file_aside() {
        let dir = std::env::temp_dir().join(format!("vyro-widgets-{}", std::process::id()));
        let path = dir.join("widgets.json");
        let w = Widgets {
            widgets: vec![widget("a", "graph", &["cpu.usage"])],
            ..Widgets::default()
        };
        files::write_json(&path, &w).unwrap();
        assert_eq!(read(&path), w);
        std::fs::write(&path, "{ nope").unwrap();
        assert_eq!(read(&path), Widgets::default());
        assert!(dir.join("widgets.bad.json").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! A smoke test of the installed app, for CI's Windows runner (after 1.1's first field check,
//! where adding a widget hung the app). Armed only by `VYRO_SMOKE=<result file>`; otherwise
//! nothing here runs and [`smoke_report`] refuses.
//!
//! Once the main page has loaded, `smoke.js` calls the commands the pages call (hardware,
//! system, sensors, devices, widgets) through the real IPC, asks to disconnect a device that does
//! not exist (it must be refused at once), then saves a widget as the Widgets page does and reads
//! the remote status, which must not announce a change (Settings → Remote reads it again on each
//! one: after 1.7's field check). [`smoke_report`] then checks that the widget's window opened, shows without having
//! taken focus and loaded its page. On Windows it also checks that the widget stays over the
//! desktop through Show desktop (`pin.rs`; skipped where the runner has no desktop to show),
//! snaps when dragged with the mouse near the screen's corner and follows the pointer out again
//! (`snap.rs`), and lets clicks through when locked so.
//! Then it writes the result and quits the way the tray's "Quit Vyro" does. A watchdog writes a
//! failure and ends the process if any of that hangs, so CI fails rather than waits.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{AppHandle, Manager, Webview};

use crate::widgets::PREFIX;

const ENV: &str = "VYRO_SMOKE";
/// The widget `smoke.js` saves.
pub const WIDGET: &str = "smoke1";
/// The commands `smoke.js` must report, in order. `set_device_connected` passes when it refuses
/// an unknown device at once (through the service, on the runner); `get_remote_status` when it
/// answers without announcing a change (Settings → Remote reads again on each one).
const STEPS: [&str; 8] = [
    "get_hardware_id",
    "get_system_info",
    "get_sensor_tree",
    "get_devices",
    "set_device_connected",
    "get_widgets",
    "set_widgets",
    "get_remote_status",
];
/// From start to the result, whatever happens.
const DEADLINE: Duration = Duration::from_secs(120);
/// How long the widget's window may take to load once the report is in.
const WIDGET_WAIT: Duration = Duration::from_secs(30);
const SCRIPT: &str = include_str!("smoke.js");

static INJECTED: AtomicBool = AtomicBool::new(false);
static WIDGET_LOADED: AtomicBool = AtomicBool::new(false);
static REPORTED: AtomicBool = AtomicBool::new(false);

fn armed() -> Option<&'static PathBuf> {
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    FILE.get_or_init(|| {
        std::env::var_os(ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    })
    .as_ref()
}

/// What the app saw, besides the page's steps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    pub widget_window: bool,
    pub widget_visible: bool,
    pub widget_loaded: bool,
    /// The widget took focus when it opened (it must not).
    pub widget_focused: bool,
}

/// The checks of Windows' own behaviour: each "passed", "skipped: …" or "failed: …".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checks {
    /// Show desktop with the widget behind windows (`pin.rs`).
    pub show_desktop: String,
    /// Dragged with the mouse near the work area's corner, the widget snaps to it, and moved on
    /// it follows the pointer out of the snap (`snap.rs`).
    pub snap: String,
    /// Locked with "Clicks pass through", the window lets the mouse through; unlocked, not.
    pub click_through: String,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub ok: bool,
    pub failures: Vec<String>,
    pub steps: Value,
    pub widget_window: bool,
    pub widget_visible: bool,
    pub widget_loaded: bool,
    /// As [`Checks`].
    pub show_desktop: String,
    pub snap: String,
    pub click_through: String,
}

/// Passes when every expected step answered without an error, the widget's window opened, shows
/// and loaded its page, and none of the [`Checks`] failed (a skip is not a failure).
pub fn verdict(steps: &Value, facts: Facts, checks: &Checks) -> Outcome {
    let mut failures = Vec::new();
    let list = steps.as_array().map(Vec::as_slice).unwrap_or_default();
    for name in STEPS {
        match list.iter().find(|s| s["name"] == name) {
            None => failures.push(format!("{name}: not run")),
            Some(s) if s["ok"] != true => failures.push(format!(
                "{name}: {}",
                s["error"].as_str().unwrap_or("failed")
            )),
            Some(_) => {}
        }
    }
    if !facts.widget_window {
        failures.push("the widget's window did not open".into());
    } else {
        if !facts.widget_visible {
            failures.push("the widget's window is not shown".into());
        }
        if !facts.widget_loaded {
            failures.push("the widget's page did not load".into());
        }
        if facts.widget_focused {
            failures.push("the widget took focus when it opened".into());
        }
    }
    for (name, result) in [
        ("Show desktop", &checks.show_desktop),
        ("Snapping", &checks.snap),
        ("Click-through", &checks.click_through),
    ] {
        if let Some(why) = result.strip_prefix("failed: ") {
            failures.push(format!("{name}: {why}"));
        }
    }
    Outcome {
        ok: failures.is_empty(),
        failures,
        steps: steps.clone(),
        widget_window: facts.widget_window,
        widget_visible: facts.widget_visible,
        widget_loaded: facts.widget_loaded,
        show_desktop: checks.show_desktop.clone(),
        snap: checks.snap.clone(),
        click_through: checks.click_through.clone(),
    }
}

fn write(outcome: &Outcome) {
    let Some(file) = armed() else {
        return;
    };
    match serde_json::to_vec_pretty(outcome) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(file, bytes) {
                log::error!("smoke: {}: {e}", file.display());
            }
        }
        Err(e) => log::error!("smoke: {e}"),
    }
}

fn failed(why: &str) -> Outcome {
    Outcome {
        ok: false,
        failures: vec![why.into()],
        steps: Value::Null,
        widget_window: false,
        widget_visible: false,
        widget_loaded: false,
        show_desktop: "not run".into(),
        snap: "not run".into(),
        click_through: "not run".into(),
    }
}

/// Starts the watchdog when armed (from setup).
pub fn start() {
    if armed().is_none() {
        return;
    }
    log::info!("smoke: armed");
    let spawned = std::thread::Builder::new()
        .name("vyro-smoke".into())
        .spawn(|| {
            std::thread::sleep(DEADLINE);
            write(&failed(if REPORTED.load(Ordering::SeqCst) {
                "quitting did not end the app"
            } else {
                "timed out: a command or the widget never answered"
            }));
            std::process::exit(3);
        });
    if let Err(e) = spawned {
        log::error!("smoke: {e}");
    }
}

/// Every webview's page load (the builder's hook): runs the script once the main page is up,
/// and notes when the smoke widget's page has loaded.
pub fn on_page_load(webview: &Webview, payload: &PageLoadPayload<'_>) {
    if armed().is_none() || payload.event() != PageLoadEvent::Finished {
        return;
    }
    let label = webview.label();
    if label == "main" && !INJECTED.swap(true, Ordering::SeqCst) {
        // A moment for the page's own start (settings, first snapshot).
        let webview = webview.clone();
        let _ = std::thread::Builder::new()
            .name("vyro-smoke-run".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                if let Err(e) = webview.eval(SCRIPT) {
                    log::error!("smoke: {e}");
                }
            });
    } else if label.strip_prefix(PREFIX) == Some(WIDGET) {
        WIDGET_LOADED.store(true, Ordering::SeqCst);
    }
}

/// The page's steps: checks the widget, writes the result, then quits as the tray does.
#[tauri::command]
pub async fn smoke_report(app: AppHandle, steps: Value) -> Result<(), String> {
    if armed().is_none() {
        return Err("not a smoke run".into());
    }
    REPORTED.store(true, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("vyro-smoke-report".into())
        .spawn(move || {
            let label = format!("{PREFIX}{WIDGET}");
            let since = Instant::now();
            while !WIDGET_LOADED.load(Ordering::SeqCst) && since.elapsed() < WIDGET_WAIT {
                std::thread::sleep(Duration::from_millis(250));
            }
            let window = app.get_webview_window(&label);
            let facts = Facts {
                widget_window: window.is_some(),
                widget_focused: window
                    .as_ref()
                    .is_some_and(|w| w.is_focused().unwrap_or(false)),
                widget_visible: window.is_some_and(|w| w.is_visible().unwrap_or(false)),
                widget_loaded: WIDGET_LOADED.load(Ordering::SeqCst),
            };
            let checks = if !(facts.widget_loaded && facts.widget_visible) {
                let skip = || "skipped: the widget did not show".to_owned();
                Checks {
                    show_desktop: skip(),
                    snap: skip(),
                    click_through: skip(),
                }
            } else {
                windows_checks(&app, &label)
            };
            let outcome = verdict(&steps, facts, &checks);
            log::info!(
                "smoke: ok {} {:?}; Show desktop {}; snap {}; click-through {}",
                outcome.ok,
                outcome.failures,
                outcome.show_desktop,
                outcome.snap,
                outcome.click_through
            );
            write(&outcome);
            app.exit(0);
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(windows)]
fn windows_checks(app: &AppHandle, label: &str) -> Checks {
    Checks {
        show_desktop: crate::pin::smoke_show_desktop(app, label),
        snap: snap_drag(app, label),
        click_through: click_through(app, label),
    }
}

#[cfg(not(windows))]
fn windows_checks(_app: &AppHandle, _label: &str) -> Checks {
    let skip = || "skipped: not Windows".to_owned();
    Checks {
        show_desktop: skip(),
        snap: skip(),
        click_through: skip(),
    }
}

/// Changes the smoke widget through the store, as the Widgets page would.
#[cfg(windows)]
fn change(
    app: &AppHandle,
    edit: impl FnOnce(&mut crate::widgets::DesktopWidget),
) -> Result<(), String> {
    let store = app
        .try_state::<crate::widgets::WidgetStore>()
        .ok_or("widgets not loaded")?;
    let mut cfg = store.get();
    let w = cfg
        .widgets
        .iter_mut()
        .find(|w| w.id == WIDGET)
        .ok_or("the widget is gone")?;
    edit(w);
    crate::widgets::store_and_sync(app, cfg).map(|_| ())
}

#[cfg(windows)]
fn wait_for(limit: Duration, done: impl Fn() -> bool) -> bool {
    let since = Instant::now();
    while !done() {
        if since.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// Drags the widget with the mouse (`snap.rs`), on top of every window so that nothing covers
/// it, as a widget on the "Always on top" layer.
#[cfg(windows)]
fn snap_drag(app: &AppHandle, label: &str) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOPMOST};

    let Some(hwnd) = app.get_webview_window(label).and_then(|w| w.hwnd().ok()) else {
        return "failed: no widget window".into();
    };
    if let Err(e) = change(app, |w| w.layer = crate::widgets::Layer::Top) {
        return format!("failed: {e}");
    }
    // SAFETY: reads a window's extended style.
    let on_top = || unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & WS_EX_TOPMOST.0 as isize != 0;
    if !wait_for(Duration::from_secs(3), on_top) {
        return "failed: the widget did not go on top of other windows".into();
    }
    crate::snap::smoke_drag(hwnd)
}

/// Locks the widget with "Clicks pass through" as the Widgets page would, and checks that its
/// window lets the mouse through (tao's `WS_EX_TRANSPARENT`); then unlocks it and checks that
/// it catches the mouse again.
#[cfg(windows)]
fn click_through(app: &AppHandle, label: &str) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TRANSPARENT,
    };

    let Some(hwnd) = app.get_webview_window(label).and_then(|w| w.hwnd().ok()) else {
        return "failed: no widget window".into();
    };
    let set = |locked: bool| {
        change(app, |w| {
            (w.locked, w.click_through) = (locked, true);
        })
    };
    // SAFETY: reads a window's extended style.
    let through =
        || unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } & WS_EX_TRANSPARENT.0 as isize != 0;
    if let Err(e) = set(true) {
        return format!("failed: {e}");
    }
    if !wait_for(Duration::from_secs(3), through) {
        return "failed: locked with clicks passing through, the widget still catches the mouse"
            .into();
    }
    if let Err(e) = set(false) {
        return format!("failed: {e}");
    }
    if !wait_for(Duration::from_secs(3), || !through()) {
        return "failed: unlocked, the widget still lets clicks through".into();
    }
    "passed".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all_ok() -> Value {
        Value::Array(
            STEPS
                .iter()
                .map(|n| json!({ "name": n, "ok": true, "ms": 12 }))
                .collect(),
        )
    }
    fn passed() -> Checks {
        Checks {
            show_desktop: "passed".into(),
            snap: "passed".into(),
            click_through: "passed".into(),
        }
    }
    const SHOWN: Facts = Facts {
        widget_window: true,
        widget_visible: true,
        widget_loaded: true,
        widget_focused: false,
    };

    #[test]
    fn passes_when_every_step_answers_and_the_widget_shows() {
        let o = verdict(&all_ok(), SHOWN, &passed());
        assert!(o.ok, "{:?}", o.failures);
    }

    #[test]
    fn a_windows_check_fails_the_run_only_when_it_failed() {
        let skipped = Checks {
            show_desktop: "skipped: no desktop shell".into(),
            ..passed()
        };
        let o = verdict(&all_ok(), SHOWN, &skipped);
        assert!(o.ok, "{:?}", o.failures);
        assert_eq!(o.show_desktop, "skipped: no desktop shell");
        let failed = Checks {
            show_desktop: "failed: the widget did not stay over the shown desktop".into(),
            snap: "failed: a widget 4 px from the corner went to 4,4".into(),
            click_through: "failed: unlocked, the widget still lets clicks through".into(),
        };
        assert_eq!(
            verdict(&all_ok(), SHOWN, &failed).failures,
            [
                "Show desktop: the widget did not stay over the shown desktop",
                "Snapping: a widget 4 px from the corner went to 4,4",
                "Click-through: unlocked, the widget still lets clicks through",
            ]
        );
    }

    #[test]
    fn a_step_that_failed_or_never_ran_fails() {
        let mut steps = all_ok();
        steps[2] =
            json!({ "name": "get_sensor_tree", "ok": false, "error": "timed out after 20 s" });
        let o = verdict(&steps, SHOWN, &passed());
        assert!(!o.ok);
        assert_eq!(o.failures, ["get_sensor_tree: timed out after 20 s"]);

        let o = verdict(&json!([]), SHOWN, &passed());
        assert_eq!(o.failures.len(), STEPS.len());
        assert!(!verdict(&Value::Null, SHOWN, &passed()).ok);
    }

    #[test]
    fn the_widget_must_open_show_and_load() {
        let o = verdict(&all_ok(), Facts::default(), &passed());
        assert_eq!(o.failures, ["the widget's window did not open"]);
        let o = verdict(
            &all_ok(),
            Facts {
                widget_loaded: false,
                ..SHOWN
            },
            &passed(),
        );
        assert_eq!(o.failures, ["the widget's page did not load"]);
        let o = verdict(
            &all_ok(),
            Facts {
                widget_visible: false,
                ..SHOWN
            },
            &passed(),
        );
        assert_eq!(o.failures, ["the widget's window is not shown"]);
        let o = verdict(
            &all_ok(),
            Facts {
                widget_focused: true,
                ..SHOWN
            },
            &passed(),
        );
        assert_eq!(o.failures, ["the widget took focus when it opened"]);
    }
}

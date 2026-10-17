//! A smoke test of the installed app, for CI's Windows runner (after 1.1's first field check,
//! where adding a widget hung the app). Armed only by `VYRO_SMOKE=<result file>`; otherwise
//! nothing here runs and [`smoke_report`] refuses.
//!
//! Once the main page has loaded, `smoke.js` calls the commands the pages call (hardware,
//! system, sensors, widgets) through the real IPC, then saves a widget as the Widgets page
//! does. [`smoke_report`] then checks that the widget's window opened, shows without having
//! taken focus and loaded its page, and that it stays over the desktop through Show desktop (`pin.rs`; skipped where the
//! runner has no desktop to show), writes the result and quits the way the tray's "Quit Vyro"
//! does. A watchdog writes a failure and ends the process if any of that hangs, so CI fails
//! rather than waits.

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
/// The commands `smoke.js` must report, in order.
const STEPS: [&str; 5] = [
    "get_hardware_id",
    "get_system_info",
    "get_sensor_tree",
    "get_widgets",
    "set_widgets",
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

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub ok: bool,
    pub failures: Vec<String>,
    pub steps: Value,
    pub widget_window: bool,
    pub widget_visible: bool,
    pub widget_loaded: bool,
    /// Show desktop with the widget behind windows: "passed", "skipped: …" or "failed: …".
    pub show_desktop: String,
}

/// Passes when every expected step answered without an error, the widget's window opened, shows
/// and loaded its page, and Show desktop did not fail (a skip is not a failure).
pub fn verdict(steps: &Value, facts: Facts, show_desktop: &str) -> Outcome {
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
    if let Some(why) = show_desktop.strip_prefix("failed: ") {
        failures.push(format!("Show desktop: {why}"));
    }
    Outcome {
        ok: failures.is_empty(),
        failures,
        steps: steps.clone(),
        widget_window: facts.widget_window,
        widget_visible: facts.widget_visible,
        widget_loaded: facts.widget_loaded,
        show_desktop: show_desktop.to_owned(),
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
            #[cfg(windows)]
            let show_desktop = if facts.widget_loaded && facts.widget_visible {
                crate::pin::smoke_show_desktop(&app, &label)
            } else {
                "skipped: the widget did not show".to_owned()
            };
            #[cfg(not(windows))]
            let show_desktop = "skipped: not Windows".to_owned();
            let outcome = verdict(&steps, facts, &show_desktop);
            log::info!(
                "smoke: ok {} {:?}; Show desktop {}",
                outcome.ok,
                outcome.failures,
                outcome.show_desktop
            );
            write(&outcome);
            app.exit(0);
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
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
    const SHOWN: Facts = Facts {
        widget_window: true,
        widget_visible: true,
        widget_loaded: true,
        widget_focused: false,
    };

    #[test]
    fn passes_when_every_step_answers_and_the_widget_shows() {
        let o = verdict(&all_ok(), SHOWN, "passed");
        assert!(o.ok, "{:?}", o.failures);
    }

    #[test]
    fn show_desktop_fails_the_run_only_when_it_failed() {
        let skipped = verdict(&all_ok(), SHOWN, "skipped: no desktop shell");
        assert!(skipped.ok, "{:?}", skipped.failures);
        assert_eq!(skipped.show_desktop, "skipped: no desktop shell");
        let failed = verdict(
            &all_ok(),
            SHOWN,
            "failed: the widget did not stay over the shown desktop",
        );
        assert_eq!(
            failed.failures,
            ["Show desktop: the widget did not stay over the shown desktop"]
        );
    }

    #[test]
    fn a_step_that_failed_or_never_ran_fails() {
        let mut steps = all_ok();
        steps[2] =
            json!({ "name": "get_sensor_tree", "ok": false, "error": "timed out after 20 s" });
        let o = verdict(&steps, SHOWN, "passed");
        assert!(!o.ok);
        assert_eq!(o.failures, ["get_sensor_tree: timed out after 20 s"]);

        let o = verdict(&json!([]), SHOWN, "passed");
        assert_eq!(o.failures.len(), STEPS.len());
        assert!(!verdict(&Value::Null, SHOWN, "passed").ok);
    }

    #[test]
    fn the_widget_must_open_show_and_load() {
        let o = verdict(&all_ok(), Facts::default(), "passed");
        assert_eq!(o.failures, ["the widget's window did not open"]);
        let o = verdict(
            &all_ok(),
            Facts {
                widget_loaded: false,
                ..SHOWN
            },
            "passed",
        );
        assert_eq!(o.failures, ["the widget's page did not load"]);
        let o = verdict(
            &all_ok(),
            Facts {
                widget_visible: false,
                ..SHOWN
            },
            "passed",
        );
        assert_eq!(o.failures, ["the widget's window is not shown"]);
        let o = verdict(
            &all_ok(),
            Facts {
                widget_focused: true,
                ..SHOWN
            },
            "passed",
        );
        assert_eq!(o.failures, ["the widget took focus when it opened"]);
    }
}

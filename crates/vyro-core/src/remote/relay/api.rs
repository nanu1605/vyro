//! What a phone may ask of the PC through the relay (1.5): each request by name, and whether it
//! only reads (any paired phone) or changes the PC (only a phone the owner allowed to control
//! it, in Settings → Remote on the PC). Anything not listed is refused. The app answers them
//! (`src-tauri/src/remote_api.rs`); the LAN page stays read-only.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Who may make a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Any paired phone.
    View,
    /// A phone allowed to control the PC.
    Control,
}

/// Every request a phone may make.
pub const METHODS: &[(&str, Access)] = &[
    // Reading.
    ("processes", Access::View),
    ("devices", Access::View),
    ("health", Access::View),
    ("insights", Access::View),
    ("score", Access::View),
    ("alerts", Access::View),
    ("fans", Access::View),
    ("profiles", Access::View),
    ("bench.runs", Access::View),
    ("bench.run", Access::View),
    ("bench.progress", Access::View),
    ("diag.runs", Access::View),
    ("diag.run", Access::View),
    ("changes", Access::View),
    ("updates", Access::View),
    ("power.state", Access::View),
    ("gpu.tune", Access::View),
    // Changing.
    ("profile.pick", Access::Control),
    ("profile.keep", Access::Control),
    ("fans.bios", Access::Control),
    ("alerts.enable", Access::Control),
    ("bench.start", Access::Control),
    ("bench.stop", Access::Control),
    ("diag.start", Access::Control),
    ("device.connect", Access::Control),
    ("process.end", Access::Control),
    ("updates.check", Access::Control),
    ("updates.install", Access::Control),
    ("power", Access::Control),
    ("power.cancel", Access::Control),
    ("wake", Access::Control),
    // GPU tuning (1.6): only Stock and presets that passed the stability test.
    ("gpu.preset", Access::Control),
    ("gpu.stock", Access::Control),
];

/// Who may make `method`; `None`: no such request.
pub fn access(method: &str) -> Option<Access> {
    METHODS.iter().find(|(m, _)| *m == method).map(|(_, a)| *a)
}

/// The requests a phone may make, for its `info`.
pub fn allowed(control: bool) -> Vec<&'static str> {
    METHODS
        .iter()
        .filter(|(_, a)| control || *a == Access::View)
        .map(|(m, _)| *m)
        .collect()
}

/// Why a request was refused before the app saw it.
pub const UNKNOWN: &str = "This PC's Vyro does not know that request. Update Vyro on the PC.";
pub const VIEW_ONLY: &str =
    "This phone may only look. To let it control the PC: Settings → Remote on the PC.";

/// Something a phone changed on the PC, as Settings → Remote lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PhoneAction {
    /// Unix seconds.
    pub at: i64,
    /// The phone's name.
    pub phone: String,
    /// What it asked, in words ("Ended chrome.exe").
    pub what: String,
    /// Why it did not happen, if it did not.
    pub error: Option<String>,
}

/// The most actions kept.
pub const MAX_ACTIONS: usize = 50;

/// A change a phone asked for, in words.
pub fn describe(method: &str, p: &serde_json::Value) -> String {
    let s = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
    let on = p.get("on").and_then(serde_json::Value::as_bool) == Some(true);
    match method {
        "profile.pick" => format!("Switched to the {} profile", s("profile")),
        "profile.keep" => "Kept the profile in force".into(),
        "fans.bios" => "Gave the fans back to the BIOS".into(),
        "alerts.enable" => format!(
            "Turned the alert \"{}\" {}",
            s("id"),
            if on { "on" } else { "off" }
        ),
        "bench.start" => format!("Started a {} benchmark", s("mode")),
        "bench.stop" => "Stopped the benchmark".into(),
        "diag.start" => "Ran diagnostics".into(),
        "device.connect" => {
            if p.get("connect").and_then(serde_json::Value::as_bool) == Some(true) {
                "Connected a Bluetooth device".into()
            } else {
                "Disconnected a Bluetooth device".into()
            }
        }
        "process.end" => format!("Ended {}", s("name")),
        "updates.check" => "Checked for updates".into(),
        "updates.install" => "Installed the update".into(),
        "power" => {
            let wait = p.get("wait").and_then(serde_json::Value::as_u64);
            let what = match s("action").as_str() {
                "shutDown" => "Shut down",
                "restart" => "Restart",
                "sleep" => "Sleep",
                "hibernate" => "Hibernate",
                "lock" => "Lock",
                "signOut" => "Sign out",
                _ => "A power action",
            };
            match wait {
                Some(0) => format!("{what}, now"),
                Some(w) if matches!(what, "Shut down" | "Restart") => format!("{what}, in {w} s"),
                _ => what.into(),
            }
        }
        "power.cancel" => "Cancelled the shutdown".into(),
        "wake" => format!("Woke {}", s("pc")),
        "gpu.preset" => format!("Applied the GPU preset \"{}\"", s("name")),
        "gpu.stock" => "Returned the GPU to stock".into(),
        other => other.into(),
    }
}

//! Writes the files a Vyro 0.1.4 install keeps, with 0.1.4's own code, for the migration tests
//! (`tests/migration.rs`, P27). Not built here: it compiles only against 0.1.4.
//!
//! ```text
//! git worktree add --detach ../old-0.1.4 v0.1.4
//! cp crates/vyro-core/tests/fixtures/gen/v0_1_4.rs ../old-0.1.4/crates/vyro-core/examples/fixtures.rs
//! (cd ../old-0.1.4 && cargo run -p vyro-core --example fixtures -- <repo>/crates/vyro-core/tests/fixtures/v0.1.4)
//! ```
//!
//! The service's data folder gets `history.sqlite`, `alerts.json` and `games.json`; the app's
//! config folder gets `settings.json`. `expect.json` holds what 0.1.4 itself read back (History
//! view series, Health and alert events), which today's code must still return.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;
use vyro_core::alerts::{self, AlertFired, AlertKind, AlertRule};
use vyro_core::gaming::{Bound, GameSession};
use vyro_core::health::HealthEvent;
use vyro_core::poller::{EVENT_ALERT, EVENT_HEALTH};
use vyro_core::profiles::Profile;
use vyro_core::snapshot::Status;
use vyro_core::units::TempUnit;
use vyro_core::{History, HistoryConfig, HistoryRange};

/// Fixed start (2025-10-09 08:53:20 UTC), so the files never change between runs.
const T0: i64 = 1_760_000_000;
/// 40 minutes at 1 Hz: the raw tier keeps its last 30 minutes, the others all of it.
const TICKS: i64 = 2400;
const END: i64 = T0 + TICKS - 1;
const METRICS: [&str; 2] = ["cpu.temp.package", "gpu.0.temp.edge"];

fn value(metric: usize, t: i64) -> f64 {
    let x = (t - T0) as f64;
    match metric {
        0 => 55.0 + 15.0 * (x / 97.0).sin(),
        _ => 48.0 + 0.004 * x,
    }
}

// --- The app's settings types, copied from 0.1.4's src-tauri (tray.rs, overlay.rs). ---

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub close_to_tray: bool,
    pub temperature_unit: TempUnit,
    pub hidden_sensors: Vec<String>,
    pub profile: Profile,
    pub game_capture: bool,
    pub overlay: OverlaySettings,
    pub check_updates: bool,
}

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
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: fixtures <out dir>")?;
    let out = Path::new(&out);
    let data = out.join("data");
    let config = out.join("config");
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(&config)?;
    let db = data.join("history.sqlite");
    let _ = std::fs::remove_file(&db);

    // Settings, as the app saves them (pretty JSON), with every field changed from its default.
    let settings = Settings {
        close_to_tray: false,
        temperature_unit: TempUnit::F,
        hidden_sensors: vec!["fan.4".into(), "cpu.voltage.soc".into()],
        profile: Profile::Gaming,
        game_capture: false,
        overlay: OverlaySettings {
            enabled: true,
            corner: Corner::BottomRight,
            detailed: false,
        },
        check_updates: false,
    };
    std::fs::write(
        config.join("settings.json"),
        serde_json::to_string_pretty(&settings)?,
    )?;

    // Alert rules: the defaults, one edited, one added.
    let mut rules = alerts::default_rules();
    rules[0].kind = AlertKind::Threshold {
        sensor: "cpu.temp.package".into(),
        above: true,
        value: 90.0,
    };
    rules[3].enabled = true;
    rules.push(AlertRule {
        id: "custom-fan".into(),
        name: "CPU fan stopped".into(),
        enabled: true,
        kind: AlertKind::Threshold {
            sensor: "fan.0".into(),
            above: false,
            value: 200.0,
        },
        for_secs: 10,
        cooldown_secs: 300,
        sound: false,
    });
    alerts::save(&data.join("alerts.json"), &rules)?;

    // The game log (0.1 kept sessions in games.json).
    let session = |app: &str, started: i64, secs: i64, fps: f64, bound: Option<Bound>| GameSession {
        app: app.into(),
        runtime: Some("DXGI".into()),
        started,
        secs,
        frames: (fps * secs as f64) as u64,
        avg_fps: Some(fps),
        low1_fps: Some(fps * 0.71),
        low01_fps: Some(fps * 0.52),
        max_frame_ms: Some(41.5),
        bound,
        cpu_temp_max: Some(81.0),
        gpu_temp_max: Some(74.0),
        cpu_usage_avg: Some(38.5),
        gpu_usage_avg: Some(96.0),
    };
    let games = vec![
        session("Cyberpunk2077.exe", T0 - 7200, 3600, 88.4, Some(Bound::Gpu)),
        session("cs2.exe", T0 - 3000, 1500, 301.2, Some(Bound::Cpu)),
    ];
    std::fs::write(data.join("games.json"), serde_json::to_string(&games)?)?;

    // History: two sensors for 40 minutes, then Health and alert events.
    let mut h = History::open(&db, HistoryConfig::default())?;
    for t in T0..=END {
        h.record(
            t,
            METRICS.iter().enumerate().map(|(i, m)| (*m, value(i, t))),
        )?;
    }
    h.flush_all()?;
    let health = vec![
        HealthEvent {
            ts: T0 + 600,
            subsystem: "CPU".into(),
            status: Status::Warn,
            message: "CPU temperature 88 °C".into(),
        },
        HealthEvent {
            ts: T0 + 900,
            subsystem: "CPU".into(),
            status: Status::Ok,
            message: "CPU temperature back to normal".into(),
        },
    ];
    for e in &health {
        h.log_events(EVENT_HEALTH, e.ts, std::slice::from_ref(e))?;
    }
    let fired = vec![
        AlertFired {
            ts: T0 + 620,
            rule_id: "cpu-temp".into(),
            title: "CPU temperature".into(),
            message: "CPU package at 91.0 °C (above 90.0 °C)".into(),
            sound: true,
        },
        AlertFired {
            ts: T0 + 1500,
            rule_id: "health-warnings".into(),
            title: "Health warnings".into(),
            message: "Storage: drive at 61 °C".into(),
            sound: false,
        },
    ];
    for f in &fired {
        h.log_events(EVENT_ALERT, f.ts, std::slice::from_ref(f))?;
    }

    // What 0.1.4 reads back from it.
    // One series per tier: raw (5 min), 10 s (1 h) and 1 min (7 days).
    let mut series = Vec::new();
    for range in [HistoryRange::M5, HistoryRange::H1, HistoryRange::D7] {
        series.push(h.query(METRICS[0], range, END)?);
    }
    series.push(h.query(METRICS[1], HistoryRange::D7, END)?);
    let expect = json!({
        "version": "0.1.4",
        "now": END,
        "series": series,
        "health": h.events::<HealthEvent>(EVENT_HEALTH, 100)?,
        "alerts": h.events::<AlertFired>(EVENT_ALERT, 100)?,
        "rules": alerts::load(&data.join("alerts.json")),
        "games": games,
        "settings": settings,
    });
    drop(h);
    std::fs::write(out.join("expect.json"), serde_json::to_string(&expect)?)?;
    for f in ["history.sqlite-wal", "history.sqlite-shm"] {
        if data.join(f).exists() {
            return Err(format!("{f} left behind").into());
        }
    }
    println!("wrote {}", out.display());
    Ok(())
}

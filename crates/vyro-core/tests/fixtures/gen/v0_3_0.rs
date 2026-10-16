//! Writes the files a Vyro 0.3.0 install keeps, with 0.3.0's own code, for the migration tests
//! (`tests/migration.rs`, P27). Not built here: it compiles only against 0.3.0.
//!
//! ```text
//! git worktree add --detach ../old-0.3.0 v0.3.0
//! cp crates/vyro-core/tests/fixtures/gen/v0_3_0.rs ../old-0.3.0/crates/vyro-core/examples/fixtures.rs
//! (cd ../old-0.3.0 && cargo run -p vyro-core --example fixtures -- <repo>/crates/vyro-core/tests/fixtures/v0.3.0)
//! ```
//!
//! The service's data folder gets `history.sqlite` (schema v2, with game sessions),
//! `alerts.json`, `fan-control.json` and `fan-profile.json`; the app's config folder gets
//! `settings.json` (with game profiles and the 0.3 overlay). `expect.json` holds what 0.3.0
//! itself read back (History view series, Health, alert and fan events, game sessions and a
//! replay timeline), which today's code must still return. Alerts and fans are as in 0.2.0;
//! 0.3.0 had moved the 0.2 game log into the history, so there is no `games.json`.

use std::path::Path;

use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

use vyro_core::alerts::{self, AlertFired, AlertKind, AlertRule};
use vyro_core::fan::{
    self, CurvePoint, FanConfig, FanCurve, FanEvent, FanEventKind, HeaderConfig, HeaderMode,
};
use vyro_core::gaming::auto::{GameAutomation, GameRule};
use vyro_core::gaming::markers::{GameEvent, Marker, Part};
use vyro_core::gaming::overlay::{OverlayMetric, OverlayMode};
use vyro_core::gaming::replay::GameSample;
use vyro_core::gaming::{Bound, GameSession};
use vyro_core::health::HealthEvent;
use vyro_core::poller::{EVENT_ALERT, EVENT_HEALTH};
use vyro_core::profiles::Profile;
use vyro_core::sio::control::ALLOW_LIST;
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

// --- The app's settings types, copied from 0.3.0's src-tauri (tray.rs, overlay.rs); only
// what serializing needs. ---

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    Green,
    Cyan,
    Amber,
    Pink,
    White,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySettings {
    pub enabled: bool,
    pub mode: OverlayMode,
    pub corner: Corner,
    pub offset_x: u16,
    pub offset_y: u16,
    pub scale: u16,
    pub opacity: u8,
    pub metrics: Vec<OverlayMetric>,
    pub graph: bool,
    pub update_ms: u32,
    pub accent: Accent,
    pub compact: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub close_to_tray: bool,
    pub temperature_unit: TempUnit,
    pub hidden_sensors: Vec<String>,
    pub profile: Profile,
    pub game_capture: bool,
    pub overlay: OverlaySettings,
    pub check_updates: bool,
    pub games: GameAutomation,
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
            mode: OverlayMode::Full,
            corner: Corner::TopRight,
            offset_x: 24,
            offset_y: 40,
            scale: 125,
            opacity: 60,
            metrics: vec![OverlayMetric::Fps, OverlayMetric::Low1, OverlayMetric::CpuTemp],
            graph: false,
            update_ms: 500,
            accent: Accent::Cyan,
            compact: true,
        },
        check_updates: false,
        games: GameAutomation {
            enabled: true,
            default_profile: Some(Profile::Benchmark),
            load_trigger: true,
            rules: vec![
                GameRule {
                    name: "Cyberpunk2077.exe".into(),
                    profile: Some(Profile::Gaming),
                    fans: Some(Profile::Silent),
                    overlay: Some(true),
                    overlay_mode: Some(OverlayMode::Graph),
                    poll_ms: Some(500),
                    ..GameRule::new("Cyberpunk2077.exe")
                },
                GameRule {
                    ignore: true,
                    ..GameRule::new("Discord.exe")
                },
            ],
        },
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

    // Game sessions: one moved from a 0.2 log (no timeline) and one recorded by 0.3.0.
    let session = |app: &str, started: i64, secs: i64, fps: f64, bound: Option<Bound>| GameSession {
        id: 0,
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
        profile: None,
        ended: None,
        has_timeline: false,
    };

    // Fan control on the reference board (MS-7E24): a CPU fan curve, a fixed case fan, the
    // pump confirmed and fixed, a header named, and Silent with its own CPU fan curve.
    let p = |temp_c, pct| CurvePoint { temp_c, pct };
    let cpu_curve = FanCurve {
        source: "cpu.temp.package".into(),
        points: vec![p(35.0, 25.0), p(55.0, 40.0), p(70.0, 65.0), p(82.0, 100.0)],
        hysteresis_c: 4.0,
        ramp_up_pct_s: 15.0,
        ramp_down_pct_s: 4.0,
    };
    let fans = FanConfig {
        enabled: true,
        headers: vec![
            HeaderConfig {
                index: 0,
                mode: HeaderMode::Curve {
                    curve: cpu_curve.clone(),
                },
            },
            HeaderConfig {
                index: 1,
                mode: HeaderMode::Fixed { pct: 80.0 },
            },
            HeaderConfig {
                index: 2,
                mode: HeaderMode::Fixed { pct: 45.0 },
            },
        ],
        emergency_cpu_c: 92.0,
        emergency_gpu_c: 88.0,
        pumps: vec![1],
        labels: BTreeMap::from([(2, "Front intake".to_string())]),
        profiles: BTreeMap::from([(
            Profile::Silent,
            vec![HeaderConfig {
                index: 0,
                mode: HeaderMode::Curve {
                    curve: FanCurve {
                        points: vec![p(40.0, 20.0), p(65.0, 35.0), p(80.0, 70.0), p(88.0, 100.0)],
                        ..cpu_curve
                    },
                },
            }],
        )]),
    };
    fan::validate(&fans, Some(&ALLOW_LIST[0]))?;
    // As the controller saves them (pretty JSON).
    std::fs::write(
        data.join("fan-control.json"),
        serde_json::to_vec_pretty(&fans)?,
    )?;
    std::fs::write(
        data.join("fan-profile.json"),
        serde_json::to_vec_pretty(&Profile::Silent)?,
    )?;

    // History: two sensors for 40 minutes, then Health and alert events.
    let mut h = History::open(&db, HistoryConfig::default())?;
    for t in T0..=END {
        h.record(
            t,
            METRICS.iter().enumerate().map(|(i, m)| (*m, value(i, t))),
        )?;
    }
    h.flush_all()?;
    h.game_import(&[session(
        "Cyberpunk2077.exe",
        T0 - 7200,
        3600,
        88.4,
        Some(Bound::Gpu),
    )])?;
    let started = T0 + 1800;
    let mut live = GameSession {
        profile: Some(Profile::Gaming),
        has_timeline: true,
        ..session("cs2.exe", started, 0, 0.0, None)
    };
    let id = h.game_begin(&live)?;
    for t in 0..120 {
        let fps = if (60..64).contains(&t) { 95.0 } else { 290.0 + (t % 7) as f64 };
        h.game_sample(
            id,
            GameSample {
                t,
                fps: Some(fps),
                fps_min: Some(fps - 30.0),
                frame_ms: Some(1000.0 / fps),
                frame_max_ms: Some(1000.0 / fps * 1.8),
                cpu_usage: Some(41.0 + (t % 5) as f64),
                gpu_usage: Some(78.0),
                cpu_temp: Some(70.0 + t as f64 / 20.0),
                gpu_temp: Some(64.0),
                cpu_power: Some(88.5),
                gpu_power: Some(171.25),
            },
        );
    }
    for (t, marker) in [
        (0, Marker::Profile { profile: Profile::Gaming, auto: true }),
        (60, Marker::FpsDrop { fps: 95.0, usual: 292.0 }),
        (75, Marker::Left),
        (80, Marker::Back),
        (100, Marker::Thermal { part: Part::Cpu }),
    ] {
        h.game_event(id, GameEvent { t, marker });
    }
    live.secs = 120;
    live.frames = 34_000;
    live.avg_fps = Some(283.3);
    live.bound = Some(Bound::Cpu);
    h.game_finish(id, started + 120, &live)?;
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

    let fan_events = vec![
        FanEvent {
            ts: T0 + 30,
            kind: FanEventKind::On,
            message: "Fan control on".into(),
        },
        FanEvent {
            ts: T0 + 640,
            kind: FanEventKind::FullSpeed,
            message: "CPU at 92 °C: every fan at full speed".into(),
        },
        FanEvent {
            ts: T0 + 700,
            kind: FanEventKind::Normal,
            message: "Back to the curves".into(),
        },
        FanEvent {
            ts: T0 + 1200,
            kind: FanEventKind::Profile,
            message: "Silent profile".into(),
        },
    ];
    for e in &fan_events {
        h.log_events(fan::EVENT_KIND, e.ts, std::slice::from_ref(e))?;
    }

    // What 0.3.0 reads back from it.
    // One series per tier: raw (5 min), 10 s (1 h) and 1 min (7 days).
    let mut series = Vec::new();
    for range in [HistoryRange::M5, HistoryRange::H1, HistoryRange::D7] {
        series.push(h.query(METRICS[0], range, END)?);
    }
    series.push(h.query(METRICS[1], HistoryRange::D7, END)?);
    let expect = json!({
        "version": "0.3.0",
        "now": END,
        "series": series,
        "health": h.events::<HealthEvent>(EVENT_HEALTH, 100)?,
        "alerts": h.events::<AlertFired>(EVENT_ALERT, 100)?,
        "fanEvents": h.events::<FanEvent>(fan::EVENT_KIND, 100)?,
        "rules": alerts::load(&data.join("alerts.json")),
        "fans": fan::controller::load_config(&data.join("fan-control.json"), Some(&ALLOW_LIST[0])),
        "sessions": h.game_sessions(50)?,
        "timeline": h.game_timeline(id, 600)?,
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

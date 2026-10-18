//! Upgrading keeps the user's data (P27). Each released version's files, written by that
//! version's own code (`fixtures/gen/`), are opened with today's code:
//!
//! - the history database migrates in place, and the History view shows the same series it
//!   showed before; Health, alert and fan events and game sessions are all still there;
//! - the engine starts on the old data folder: alert rules load unchanged, and the 0.1/0.2 game
//!   log moves into the history;
//! - the fan controller (on the simulated reference chip) loads the old fan settings and the
//!   profile in force.
//!
//! The app's `settings.json` from each version is checked by the app's own tests (`tray.rs`).

// A test file: its helpers may unwrap like the tests (clippy.toml allows it only in `#[test]`).
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use vyro_core::alerts::AlertFired;
use vyro_core::engine::{EngineApi, EngineConfig, LocalEngine};
use vyro_core::fan::controller::{self, FanControl, SimDevice};
use vyro_core::fan::FanEvent;
use vyro_core::health::HealthEvent;
use vyro_core::profiles::Profile;
use vyro_core::sio::control::ALLOW_LIST;
use vyro_core::{History, HistoryConfig, HistoryRange, PollerConfig, SimProvider};

const VERSIONS: [&str; 3] = ["v0.1.4", "v0.2.0", "v0.3.0"];

fn fixture(version: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(version)
}

fn expect(version: &str) -> Value {
    let text = std::fs::read_to_string(fixture(version).join("expect.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A copy of the version's data folder (opening the history migrates it in place).
fn data_copy(version: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(fixture(version).join("data")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
    }
    dir
}

/// `actual` holds everything `expected` does (objects may have gained fields since).
fn contains(actual: &Value, expected: &Value, at: &str) -> Result<(), String> {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => e.iter().try_for_each(|(k, ev)| {
            let av = a.get(k).ok_or_else(|| format!("{at}.{k}: missing"))?;
            contains(av, ev, &format!("{at}.{k}"))
        }),
        (Value::Array(a), Value::Array(e)) => {
            if a.len() != e.len() {
                return Err(format!("{at}: {} items, expected {}", a.len(), e.len()));
            }
            a.iter()
                .zip(e)
                .enumerate()
                .try_for_each(|(i, (av, ev))| contains(av, ev, &format!("{at}[{i}]")))
        }
        // Averages are summed in a different order than in SQL before 0.4: the last digit
        // may differ.
        (Value::Number(a), Value::Number(e))
            if a.as_f64()
                .zip(e.as_f64())
                .is_some_and(|(a, e)| a == e || (a - e).abs() <= 1e-9 * e.abs().max(1.0)) =>
        {
            Ok(())
        }
        _ if actual == expected => Ok(()),
        _ => Err(format!("{at}: {actual} != expected {expected}")),
    }
}

fn check(version: &str, what: &str, actual: &impl Serialize, expected: &Value) {
    let actual = serde_json::to_value(actual).unwrap();
    if let Err(e) = contains(&actual, expected, what) {
        panic!("{version}: {e}");
    }
}

fn range(text: &str) -> HistoryRange {
    serde_json::from_value(Value::String(text.into())).unwrap()
}

#[test]
fn the_history_view_shows_what_each_release_showed() {
    for version in VERSIONS {
        let expect = expect(version);
        let dir = data_copy(version);
        let now = expect["now"].as_i64().unwrap();
        let mut h = History::open(&dir.path().join("history.sqlite"), HistoryConfig::default())
            .unwrap_or_else(|e| panic!("{version}: {e}"));
        let series = expect["series"].as_array().unwrap();
        assert!(!series.is_empty());
        for s in series {
            let metric = s["metric"].as_str().unwrap();
            let range = range(s["range"].as_str().unwrap());
            let got = h.query(metric, range, now).unwrap();
            assert!(
                !got.points.is_empty(),
                "{version}: {metric} {range:?} is empty"
            );
            check(version, &format!("{metric} {range:?}"), &got, s);
        }
        check(
            version,
            "health",
            &h.events::<HealthEvent>("health", 100).unwrap(),
            &expect["health"],
        );
        check(
            version,
            "alerts",
            &h.events::<AlertFired>("alert", 100).unwrap(),
            &expect["alerts"],
        );
        if let Some(fan) = expect.get("fanEvents") {
            check(
                version,
                "fan events",
                &h.events::<FanEvent>(vyro_core::fan::EVENT_KIND, 100)
                    .unwrap(),
                fan,
            );
        }
        if let Some(sessions) = expect.get("sessions") {
            check(version, "sessions", &h.game_sessions(50).unwrap(), sessions);
            let timeline = &expect["timeline"];
            let id = timeline["session"]["id"].as_i64().unwrap();
            check(
                version,
                "timeline",
                &h.game_timeline(id, 600).unwrap(),
                timeline,
            );
        }
        drop(h);
        // Migrated: opening again changes nothing and keeps everything.
        let mut again =
            History::open(&dir.path().join("history.sqlite"), HistoryConfig::default()).unwrap();
        let first = &series[0];
        let got = again
            .query(
                first["metric"].as_str().unwrap(),
                range(first["range"].as_str().unwrap()),
                now,
            )
            .unwrap();
        check(version, "reopened", &got, first);
    }
}

#[test]
fn the_engine_starts_on_each_releases_data_folder() {
    for version in VERSIONS {
        let expect = expect(version);
        let dir = data_copy(version);
        let had_log = dir.path().join("games.json").exists();
        let cfg = EngineConfig {
            data_dir: Some(dir.path().to_path_buf()),
            // No tick of its own during the test: only the first, at start.
            poller: PollerConfig {
                interval: Duration::from_secs(3600),
                hidden_interval: Duration::from_secs(3600),
            },
            fan_control: false,
            gpu_tune: false,
        };
        let engine = LocalEngine::start(
            cfg,
            vec![Box::new(SimProvider::new())],
            Arc::new(Mutex::new(Vec::new())),
            |_: &vyro_core::SensorSnapshot| {},
        )
        .unwrap();
        check(
            version,
            "rules",
            &engine.alert_rules().unwrap(),
            &expect["rules"],
        );
        assert!(
            !dir.path().join("alerts.bad.json").exists(),
            "{version}: alert rules set aside"
        );
        check(
            version,
            "alert log",
            &engine.alert_log().unwrap(),
            &expect["alerts"],
        );
        check(
            version,
            "health events",
            &engine.health_events().unwrap(),
            &expect["health"],
        );
        if let Some(fan) = expect.get("fanEvents") {
            check(version, "fan events", &engine.fan_events().unwrap(), fan);
        }
        let sessions = engine.game_sessions().unwrap();
        if let Some(games) = expect.get("games") {
            // The 0.1/0.2 log, newest first now, each with an id and no timeline.
            let mut log: Vec<Value> = games.as_array().unwrap().clone();
            log.reverse();
            check(version, "imported games", &sessions, &Value::Array(log));
            assert!(sessions.iter().all(|s| s.id > 0 && !s.has_timeline));
            assert!(had_log && !dir.path().join("games.json").exists());
            assert!(dir.path().join("games.json.imported").exists());
        } else {
            check(version, "games", &sessions, &expect["sessions"]);
        }
        drop(engine);
    }
}

#[test]
fn fan_settings_and_the_profile_in_force_carry_over() {
    for version in ["v0.2.0", "v0.3.0"] {
        let expect = expect(version);
        let dir = data_copy(version);
        let board = &ALLOW_LIST[0];
        let loaded = controller::load_config(&dir.path().join("fan-control.json"), Some(board));
        check(version, "fan settings", &loaded, &expect["fans"]);
        assert!(loaded.enabled, "{version}: control stays on");

        let fans = FanControl::start(
            Box::new(SimDevice::new()),
            Some(dir.path()),
            controller::WATCHDOG,
        );
        let status = fans.status(|_| None);
        check(
            version,
            "controller settings",
            &status.config,
            &expect["fans"],
        );
        assert_eq!(status.profile, Profile::Silent, "{version}");
        fans.shutdown();
    }
}

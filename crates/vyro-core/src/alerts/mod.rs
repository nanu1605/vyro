//! Alerts (Phase 7): user rules evaluated every tick. A rule either watches one sensor against
//! a threshold (held for a while, then quiet for a cooldown) or forwards System Health changes.
//! Fired alerts ride in the snapshot; the shell turns them into Windows notifications.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::health::HealthEvent;
use crate::registry::Unit;
use crate::snapshot::Status;

/// Fired alerts kept for the Health view.
pub const MAX_LOG: usize = 100;
pub const MAX_RULES: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlertRule {
    /// Stable id chosen by the UI.
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub kind: AlertKind,
    /// How long a threshold must hold before the alert fires.
    pub for_secs: u32,
    /// Quiet time after firing (per rule; per subsystem for health rules).
    pub cooldown_secs: u32,
    /// Play the notification sound.
    pub sound: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "lowercase")]
#[ts(export)]
pub enum AlertKind {
    /// Sensor (registry id) above or below `value`.
    Threshold {
        sensor: String,
        above: bool,
        value: f64,
    },
    /// A System Health change to a problem (`problems`) or also to a warning.
    Health { problems: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlertFired {
    /// Unix seconds.
    pub ts: i64,
    pub rule_id: String,
    pub title: String,
    pub message: String,
    pub sound: bool,
}

/// First-run rules: limits worth being interrupted for, and hardware problems.
pub fn default_rules() -> Vec<AlertRule> {
    let threshold = |id: &str, name: &str, sensor: &str, value: f64| AlertRule {
        id: id.into(),
        name: name.into(),
        enabled: true,
        kind: AlertKind::Threshold {
            sensor: sensor.into(),
            above: true,
            value,
        },
        // Sustained, not a boost spike (Ryzen 7000/9000 sit at 95 °C under full load).
        for_secs: 30,
        cooldown_secs: 600,
        sound: true,
    };
    vec![
        threshold("cpu-temp", "CPU temperature", "cpu.temp.package", 95.0),
        threshold("gpu-temp", "GPU temperature", "gpu.0.temp.edge", 90.0),
        AlertRule {
            id: "health-problems".into(),
            name: "Hardware problems".into(),
            enabled: true,
            kind: AlertKind::Health { problems: true },
            for_secs: 0,
            cooldown_secs: 600,
            sound: true,
        },
        AlertRule {
            id: "health-warnings".into(),
            name: "Health warnings".into(),
            enabled: false,
            kind: AlertKind::Health { problems: false },
            for_secs: 0,
            cooldown_secs: 1800,
            sound: false,
        },
    ]
}

/// Rejects rule sets the engine should not run (the UI validates too).
pub fn validate(rules: &[AlertRule]) -> Result<(), String> {
    if rules.len() > MAX_RULES {
        return Err(format!("at most {MAX_RULES} rules"));
    }
    let mut ids = std::collections::HashSet::new();
    for r in rules {
        if r.id.trim().is_empty() || !ids.insert(r.id.as_str()) {
            return Err(format!(
                "rule ids must be unique and non-empty ({:?})",
                r.id
            ));
        }
        if r.name.trim().is_empty() || r.name.len() > 80 {
            return Err(format!("rule {:?}: name must be 1–80 characters", r.id));
        }
        if r.for_secs > 3600 || r.cooldown_secs > 86_400 {
            return Err(format!("rule {:?}: hold ≤ 1 h and cooldown ≤ 24 h", r.name));
        }
        if let AlertKind::Threshold { sensor, value, .. } = &r.kind {
            if sensor.trim().is_empty() || !value.is_finite() {
                return Err(format!("rule {:?}: pick a sensor and a number", r.name));
            }
        }
    }
    Ok(())
}

/// Reads the saved rules; first run (no file) gets [`default_rules`]. An unreadable or invalid
/// file is moved aside to `alerts.json.bad` and the defaults are used, so a hand edit gone
/// wrong never leaves the user without alerts.
pub fn load(path: &std::path::Path) -> Vec<AlertRule> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return default_rules(),
        Err(e) => {
            log::warn!(
                "alerts: cannot read {}: {e}; using defaults",
                path.display()
            );
            return default_rules();
        }
    };
    let parsed = serde_json::from_str::<Vec<AlertRule>>(&text)
        .map_err(|e| e.to_string())
        .and_then(|rules| validate(&rules).map(|()| rules));
    match parsed {
        Ok(rules) => rules,
        Err(e) => {
            let bad = path.with_extension("json.bad");
            log::warn!(
                "alerts: {} is invalid ({e}); moved to {} and using defaults",
                path.display(),
                bad.display()
            );
            let _ = std::fs::rename(path, bad);
            default_rules()
        }
    }
}

/// Writes the rules (to a temporary file first, so a crash never leaves half a file).
pub fn save(path: &std::path::Path, rules: &[AlertRule]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(rules).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Lets Windows show toasts for `aumid` without an installer-made Start-menu shortcut: the
/// per-user `AppUserModelId` registration unpackaged desktop apps use. The MSI removes the key
/// on uninstall (`src-tauri/wix/vyro.wxs`).
#[cfg(windows)]
pub fn register_toast_app(aumid: &str, display_name: &str) -> Result<(), String> {
    let path = format!(r"Software\Classes\AppUserModelId\{aumid}");
    crate::winutil::set_string(
        crate::winutil::HKEY_CURRENT_USER,
        &path,
        "DisplayName",
        display_name,
    )
    .map_err(|e| e.to_string())
}

/// A sensor's current reading as the rules see it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub value: f64,
    pub unit: Unit,
    pub name: String,
}

#[derive(Debug, Default, Clone)]
struct RuleState {
    since: Option<i64>,
    latched: bool,
    last: Option<i64>,
}

#[derive(Debug)]
pub struct AlertEngine {
    rules: Vec<AlertRule>,
    state: HashMap<String, RuleState>,
    log: VecDeque<AlertFired>,
}

fn unit_str(unit: Unit) -> String {
    serde_json::to_value(unit)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// "96.1 °C", "12.4%", "480 RPM".
pub fn show(value: f64, unit: Unit) -> String {
    let digits = match unit {
        Unit::Mhz | Unit::Rpm | Unit::Megabyte | Unit::PerSecond | Unit::Fps | Unit::Count => 0,
        Unit::Volt => 2,
        _ => 1,
    };
    let u = unit_str(unit);
    match u.as_str() {
        "" => format!("{value:.digits$}"),
        "%" => format!("{value:.digits$}%"),
        _ => format!("{value:.digits$} {u}"),
    }
}

impl AlertEngine {
    pub fn new(rules: Vec<AlertRule>) -> Self {
        Self {
            rules,
            state: HashMap::new(),
            log: VecDeque::new(),
        }
    }

    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// Replaces the rule set. State carries over for rules whose id and condition are unchanged.
    pub fn set_rules(&mut self, rules: Vec<AlertRule>) -> Result<(), String> {
        validate(&rules)?;
        let old: HashMap<&str, &AlertRule> =
            self.rules.iter().map(|r| (r.id.as_str(), r)).collect();
        self.state.retain(|key, _| {
            let id = key.split('\u{1f}').next().unwrap_or(key);
            rules
                .iter()
                .any(|r| r.id == id && old.get(id).is_some_and(|o| o.kind == r.kind))
        });
        self.rules = rules;
        Ok(())
    }

    /// Alerts fired recently, oldest first.
    pub fn log(&self) -> Vec<AlertFired> {
        self.log.iter().cloned().collect()
    }

    fn cooled(state: &RuleState, ts: i64, cooldown: u32) -> bool {
        state
            .last
            .is_none_or(|last| ts - last >= i64::from(cooldown))
    }

    /// Checks every enabled rule against this tick's readings and new health events.
    pub fn evaluate(
        &mut self,
        ts: i64,
        reading: impl Fn(&str) -> Option<Reading>,
        health: &[HealthEvent],
    ) -> Vec<AlertFired> {
        let mut fired = Vec::new();
        for rule in self.rules.iter().filter(|r| r.enabled) {
            match &rule.kind {
                AlertKind::Threshold {
                    sensor,
                    above,
                    value,
                } => {
                    // A missing reading neither trips nor re-arms the rule.
                    let Some(r) = reading(sensor) else { continue };
                    let st = self.state.entry(rule.id.clone()).or_default();
                    let hit = if *above {
                        r.value > *value
                    } else {
                        r.value < *value
                    };
                    if !hit {
                        st.since = None;
                        st.latched = false;
                        continue;
                    }
                    let since = *st.since.get_or_insert(ts);
                    if !st.latched
                        && ts - since >= i64::from(rule.for_secs)
                        && Self::cooled(st, ts, rule.cooldown_secs)
                    {
                        st.latched = true;
                        st.last = Some(ts);
                        let held = if rule.for_secs > 0 {
                            format!(" for {} s", rule.for_secs)
                        } else {
                            String::new()
                        };
                        fired.push(AlertFired {
                            ts,
                            rule_id: rule.id.clone(),
                            title: rule.name.clone(),
                            message: format!(
                                "{} at {} ({} {}{held})",
                                r.name,
                                show(r.value, r.unit),
                                if *above { "above" } else { "below" },
                                show(*value, r.unit),
                            ),
                            sound: rule.sound,
                        });
                    }
                }
                AlertKind::Health { problems } => {
                    for e in health {
                        let wanted = match e.status {
                            Status::Fail => true,
                            Status::Warn => !problems,
                            _ => false,
                        };
                        if !wanted {
                            continue;
                        }
                        let key = format!("{}\u{1f}{}", rule.id, e.subsystem);
                        let st = self.state.entry(key).or_default();
                        if !Self::cooled(st, ts, rule.cooldown_secs) {
                            continue;
                        }
                        st.last = Some(ts);
                        fired.push(AlertFired {
                            ts,
                            rule_id: rule.id.clone(),
                            title: format!(
                                "{} {}",
                                e.subsystem,
                                if e.status == Status::Fail {
                                    "problem"
                                } else {
                                    "warning"
                                }
                            ),
                            message: e.message.clone(),
                            sound: rule.sound,
                        });
                    }
                }
            }
        }
        for f in &fired {
            if self.log.len() == MAX_LOG {
                self.log.pop_front();
            }
            self.log.push_back(f.clone());
        }
        fired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(v: f64) -> impl Fn(&str) -> Option<Reading> {
        move |id| {
            (id == "cpu.temp.package").then(|| Reading {
                value: v,
                unit: Unit::Celsius,
                name: "CPU (Tdie)".into(),
            })
        }
    }

    /// Defaults with a 10 s hold on the CPU rule, to keep the timelines short.
    fn engine() -> AlertEngine {
        let mut rules = default_rules();
        rules[0].for_secs = 10;
        AlertEngine::new(rules)
    }

    #[test]
    fn defaults_are_valid() {
        assert_eq!(validate(&default_rules()), Ok(()));
    }

    #[test]
    fn threshold_must_hold_then_fires_once_until_it_clears() {
        let mut a = engine();
        for ts in 0..10 {
            assert!(a.evaluate(ts, cpu(96.0), &[]).is_empty(), "held {ts} s");
        }
        let f = a.evaluate(10, cpu(96.4), &[]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "CPU temperature");
        assert_eq!(
            f[0].message,
            "CPU (Tdie) at 96.4 °C (above 95.0 °C for 10 s)"
        );
        // Latched while still hot.
        assert!(a.evaluate(30, cpu(97.0), &[]).is_empty());
        // Cools, heats again: re-armed but inside the 10 min cooldown.
        a.evaluate(40, cpu(60.0), &[]);
        for ts in 41..=60 {
            assert!(a.evaluate(ts, cpu(99.0), &[]).is_empty());
        }
        a.evaluate(700, cpu(60.0), &[]);
        let fired: usize = (701..=712)
            .map(|ts| a.evaluate(ts, cpu(99.0), &[]).len())
            .sum();
        assert_eq!(fired, 1);
        assert_eq!(a.log().len(), 2);
    }

    #[test]
    fn a_blip_does_not_fire() {
        let mut a = engine();
        a.evaluate(0, cpu(99.0), &[]);
        a.evaluate(5, cpu(50.0), &[]);
        assert!(a.evaluate(11, cpu(99.0), &[]).is_empty());
    }

    #[test]
    fn below_rules_and_missing_sensors() {
        let rule = AlertRule {
            id: "pump".into(),
            name: "Pump".into(),
            enabled: true,
            kind: AlertKind::Threshold {
                sensor: "fan.1".into(),
                above: false,
                value: 500.0,
            },
            for_secs: 0,
            cooldown_secs: 60,
            sound: true,
        };
        let mut a = AlertEngine::new(vec![rule]);
        let pump = |rpm: f64| {
            move |id: &str| {
                (id == "fan.1").then(|| Reading {
                    value: rpm,
                    unit: Unit::Rpm,
                    name: "Pump Fan".into(),
                })
            }
        };
        assert!(a.evaluate(0, pump(4400.0), &[]).is_empty());
        assert!(a.evaluate(1, |_| None, &[]).is_empty());
        let f = a.evaluate(2, pump(0.0), &[]);
        assert_eq!(f[0].message, "Pump Fan at 0 RPM (below 500 RPM)");
    }

    #[test]
    fn health_rules_forward_problems_per_subsystem() {
        let mut a = engine();
        let ev = |sub: &str, status, msg: &str| HealthEvent {
            ts: 0,
            subsystem: sub.into(),
            status,
            message: msg.into(),
        };
        let f = a.evaluate(
            0,
            |_| None,
            &[
                ev("Fans", Status::Fail, "Pump Fan stopped"),
                ev("Memory", Status::Warn, "RAM 96% in use"),
                ev("CPU", Status::Ok, "Cleared: CPU running hot"),
            ],
        );
        // Only the problem: the warnings rule is off by default.
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "Fans problem");
        // Same subsystem inside the cooldown stays quiet; another subsystem does not.
        let f = a.evaluate(
            5,
            |_| None,
            &[
                ev("Fans", Status::Fail, "CPU Fan stopped"),
                ev("Storage", Status::Fail, "disk failing"),
            ],
        );
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].title, "Storage problem");
    }

    #[test]
    fn rejects_bad_rule_sets_and_keeps_state_for_unchanged_rules() {
        let mut rules = default_rules();
        rules[1].id = rules[0].id.clone();
        assert!(validate(&rules).is_err());
        let mut a = engine();
        for ts in 0..=10 {
            a.evaluate(ts, cpu(99.0), &[]);
        }
        // Renaming keeps the latch; the alert does not fire again.
        let mut rules = default_rules();
        rules[0].for_secs = 10;
        rules[0].name = "Hot CPU".into();
        a.set_rules(rules).unwrap();
        assert!(a.evaluate(11, cpu(99.0), &[]).is_empty());
        // Changing the threshold resets it.
        let mut rules = default_rules();
        rules[0].for_secs = 10;
        rules[0].kind = AlertKind::Threshold {
            sensor: "cpu.temp.package".into(),
            above: true,
            value: 90.0,
        };
        a.set_rules(rules).unwrap();
        let fired: usize = (12..=30)
            .map(|ts| a.evaluate(ts, cpu(99.0), &[]).len())
            .sum();
        assert_eq!(fired, 1);
    }

    #[test]
    fn saves_loads_and_recovers_from_a_bad_file() {
        let dir = std::env::temp_dir().join(format!("vyro-alerts-{}", std::process::id()));
        let path = dir.join("alerts.json");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&path), default_rules(), "first run");
        let mut rules = default_rules();
        rules.truncate(1);
        save(&path, &rules).unwrap();
        assert_eq!(load(&path), rules);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), default_rules());
        assert!(dir.join("alerts.json.bad").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serialises_rules_for_the_ui() {
        let json = serde_json::to_value(&default_rules()[0]).unwrap();
        assert_eq!(json["kind"]["type"], "threshold");
        assert_eq!(json["forSecs"], 30);
        let back: AlertRule = serde_json::from_value(json).unwrap();
        assert_eq!(back, default_rules()[0]);
    }
}

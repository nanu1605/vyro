//! Alerts (Phase 7, P18): user rules evaluated every tick. A rule watches one sensor against a
//! threshold, several sensor conditions at once (all or any of them: above or below a limit,
//! rising or falling fast, or away from their usual reading), or forwards System Health
//! changes. Each must hold for a while before it fires, then stays quiet for a cooldown.
//!
//! When a rule fires it notifies (unless it repeats an alert about the same part from the last
//! minute), and while its conditions hold it can put a profile in force, run the fans Vyro
//! drives faster (only with fan control on) and name itself in the tray; those end once the
//! conditions have been clear for [`CLEAR_SECS`]. It can also keep a diagnostic record of the
//! moment. Fired alerts and what holds ride in the snapshot; the shell turns them into Windows
//! notifications and puts the profile in force.

#[cfg(test)]
mod compound_tests;
mod eval;
pub mod usual;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::health::HealthEvent;
use crate::profiles::Profile;
use crate::registry::Unit;
use crate::snapshot::ProcessSnapshot;

pub use eval::AlertEngine;

/// Fired alerts kept for the Health view.
pub const MAX_LOG: usize = 100;
pub const MAX_RULES: usize = 100;
/// Conditions in one rule, at most.
pub const MAX_CONDITIONS: usize = 5;
/// The window of a rate condition: from this…
pub const MIN_RATE_SECS: u32 = 5;
/// …to this many seconds.
pub const MAX_RATE_SECS: u32 = 600;
/// A rule's profile, fan and tray actions end once its conditions have been clear this long.
pub const CLEAR_SECS: i64 = 30;
/// Alerts about the same part within this long notify once.
pub const DUPLICATE_SECS: i64 = 60;
/// A fan boost raises speeds by this many percentage points, from…
pub const MIN_FAN_BOOST: u8 = 5;
/// …to this many.
pub const MAX_FAN_BOOST: u8 = 50;
/// Programs kept in a diagnostic record.
pub const DIAGNOSTIC_PROGRAMS: usize = 5;
/// Readings kept in a diagnostic record, at most.
pub const DIAGNOSTIC_READINGS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlertRule {
    /// Stable id chosen by the UI.
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub kind: AlertKind,
    /// How long the conditions must hold before the alert fires.
    pub for_secs: u32,
    /// Quiet time after firing (per rule; per subsystem for health rules).
    pub cooldown_secs: u32,
    /// Play the notification sound.
    pub sound: bool,
    /// What else firing does (P18); rules saved before only notify.
    #[serde(default)]
    pub actions: AlertActions,
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
    /// Several sensor conditions (P18): all of them, or any one (`any`).
    Conditions {
        any: bool,
        conditions: Vec<Condition>,
    },
}

/// One sensor condition of a rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Condition {
    /// Registry id.
    pub sensor: String,
    pub test: ConditionTest,
    /// Once it holds, it clears only this far back past its limit (the sensor's unit), so a
    /// reading that hovers at the limit does not fire again and again.
    #[serde(default)]
    pub hysteresis: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum ConditionTest {
    Above {
        value: f64,
    },
    Below {
        value: f64,
    },
    /// Rose by at least `by` within the last `secs` seconds.
    Rises {
        by: f64,
        secs: u32,
    },
    /// Fell by at least `by` within the last `secs` seconds.
    Falls {
        by: f64,
        secs: u32,
    },
    /// At least `by` above its usual reading ([`usual`]); never holds without one.
    AboveUsual {
        by: f64,
    },
    /// At least `by` below its usual reading.
    BelowUsual {
        by: f64,
    },
}

impl ConditionTest {
    /// Seconds of readings it looks back over.
    pub fn window(&self) -> Option<u32> {
        match self {
            Self::Rises { secs, .. } | Self::Falls { secs, .. } => Some(*secs),
            _ => None,
        }
    }

    pub fn needs_usual(&self) -> bool {
        matches!(self, Self::AboveUsual { .. } | Self::BelowUsual { .. })
    }
}

/// What a rule does besides being logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AlertActions {
    /// A Windows notification (the profile in force still decides whether it shows).
    pub notify: bool,
    /// While it holds, the tray icon's tooltip names it.
    pub tray: bool,
    /// While it holds, this profile is in force (with its own fan settings, if it has any).
    pub profile: Option<Profile>,
    /// While it holds, the fans Vyro drives run this many percentage points faster. Never a
    /// header left to the BIOS, and nothing at all while fan control is off.
    pub fan_boost: Option<u8>,
    /// Keeps the readings and the busiest programs of that moment with the fired alert.
    pub record: bool,
}

impl Default for AlertActions {
    fn default() -> Self {
        Self {
            notify: true,
            tray: false,
            profile: None,
            fan_boost: None,
            record: false,
        }
    }
}

impl AlertActions {
    /// Whether anything lasts while the conditions hold.
    pub fn lasting(&self) -> bool {
        self.tray || self.profile.is_some() || self.fan_boost.is_some()
    }
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
    /// Shown as a Windows notification (`false`: the rule only logs, or it repeats [`Self::also`]).
    #[serde(default = "yes")]
    pub notify: bool,
    /// The alert this one repeats (same part, within a minute): logged, not shown again.
    #[serde(default)]
    pub also: Option<String>,
    /// What it did besides notifying ("Gaming profile while it holds").
    #[serde(default)]
    pub actions: Vec<String>,
    /// Readings and busiest programs at that moment, when the rule keeps them.
    #[serde(default)]
    pub diagnostic: Option<Diagnostic>,
}

fn yes() -> bool {
    true
}

impl AlertFired {
    /// An alert that only notifies.
    pub fn new(ts: i64, rule_id: &str, title: String, message: String, sound: bool) -> Self {
        Self {
            ts,
            rule_id: rule_id.into(),
            title,
            message,
            sound,
            notify: true,
            also: None,
            actions: Vec::new(),
            diagnostic: None,
        }
    }
}

/// The moment an alert fired: readings and the busiest programs (names only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Diagnostic {
    pub readings: Vec<DiagnosticReading>,
    pub programs: Vec<DiagnosticProgram>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosticReading {
    pub id: String,
    pub name: String,
    pub value: f64,
    pub unit: Unit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagnosticProgram {
    pub name: String,
    pub cpu_pct: Option<f64>,
    pub gpu_pct: Option<f64>,
}

/// What holds now, for the app (profile, tray) and the fan controller (boost).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlertBrief {
    /// Rules that fired and whose conditions have not been clear for [`CLEAR_SECS`] yet, in
    /// rule order.
    pub active: Vec<ActiveAlert>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActiveAlert {
    pub rule_id: String,
    pub name: String,
    /// When it fired (unix seconds).
    pub since: i64,
    /// Named in the tray tooltip.
    pub tray: bool,
    pub profile: Option<Profile>,
    pub fan_boost: Option<u8>,
}

impl AlertBrief {
    /// The profile in force because of an alert: the first active rule's that has one.
    pub fn profile(&self) -> Option<(Profile, &str)> {
        self.active
            .iter()
            .find_map(|a| a.profile.map(|p| (p, a.name.as_str())))
    }

    /// The fan boost in force: the largest any active rule asks for, with that rule's name.
    pub fn fan_boost(&self) -> Option<(u8, &str)> {
        self.active
            .iter()
            .filter_map(|a| a.fan_boost.map(|b| (b, a.name.as_str())))
            .fold(None, |best, x| match best {
                Some((b, _)) if b >= x.0 => best,
                _ => Some(x),
            })
    }

    /// Names of the active rules shown in the tray.
    pub fn tray(&self) -> Vec<&str> {
        self.active
            .iter()
            .filter(|a| a.tray)
            .map(|a| a.name.as_str())
            .collect()
    }
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
        actions: AlertActions::default(),
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
            actions: AlertActions::default(),
        },
        AlertRule {
            id: "health-warnings".into(),
            name: "Health warnings".into(),
            enabled: false,
            kind: AlertKind::Health { problems: false },
            for_secs: 0,
            cooldown_secs: 1800,
            sound: false,
            actions: AlertActions::default(),
        },
    ]
}

impl AlertRule {
    /// The sensors its conditions watch.
    pub fn sensors(&self) -> Vec<&str> {
        match &self.kind {
            AlertKind::Threshold { sensor, .. } => vec![sensor.as_str()],
            AlertKind::Health { .. } => Vec::new(),
            AlertKind::Conditions { conditions, .. } => {
                let mut v: Vec<&str> = Vec::new();
                for c in conditions {
                    if !v.contains(&c.sensor.as_str()) {
                        v.push(&c.sensor);
                    }
                }
                v
            }
        }
    }

    /// Its conditions (a threshold rule as one condition); none for a health rule.
    pub fn conditions(&self) -> (bool, Vec<Condition>) {
        match &self.kind {
            AlertKind::Threshold {
                sensor,
                above,
                value,
            } => (
                false,
                vec![Condition {
                    sensor: sensor.clone(),
                    test: if *above {
                        ConditionTest::Above { value: *value }
                    } else {
                        ConditionTest::Below { value: *value }
                    },
                    hysteresis: 0.0,
                }],
            ),
            AlertKind::Health { .. } => (false, Vec::new()),
            AlertKind::Conditions { any, conditions } => (*any, conditions.clone()),
        }
    }
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
        match &r.kind {
            AlertKind::Threshold { sensor, value, .. } => {
                if sensor.trim().is_empty() || !value.is_finite() {
                    return Err(format!("rule {:?}: pick a sensor and a number", r.name));
                }
            }
            AlertKind::Health { .. } => {
                if r.actions.lasting() {
                    return Err(format!(
                        "rule {:?}: System Health rules can only notify and record",
                        r.name
                    ));
                }
            }
            AlertKind::Conditions { conditions, .. } => {
                if conditions.is_empty() || conditions.len() > MAX_CONDITIONS {
                    return Err(format!(
                        "rule {:?}: 1 to {MAX_CONDITIONS} conditions",
                        r.name
                    ));
                }
                for c in conditions {
                    check_condition(c).map_err(|e| format!("rule {:?}: {e}", r.name))?;
                }
            }
        }
        if let Some(b) = r.actions.fan_boost {
            if !(MIN_FAN_BOOST..=MAX_FAN_BOOST).contains(&b) {
                return Err(format!(
                    "rule {:?}: raise fans by {MIN_FAN_BOOST} to {MAX_FAN_BOOST} points",
                    r.name
                ));
            }
        }
    }
    Ok(())
}

fn check_condition(c: &Condition) -> Result<(), String> {
    if c.sensor.trim().is_empty() {
        return Err("pick a sensor for each condition".into());
    }
    if !c.hysteresis.is_finite() || c.hysteresis < 0.0 {
        return Err("the clearing margin is a number of 0 or more".into());
    }
    match &c.test {
        ConditionTest::Above { value } | ConditionTest::Below { value } => {
            if !value.is_finite() {
                return Err("enter a number for each limit".into());
            }
        }
        ConditionTest::Rises { by, secs } | ConditionTest::Falls { by, secs } => {
            if !by.is_finite() || *by <= 0.0 {
                return Err("a rise or fall is a number above 0".into());
            }
            if !(MIN_RATE_SECS..=MAX_RATE_SECS).contains(secs) {
                return Err(format!(
                    "a rise or fall is measured over {MIN_RATE_SECS} to {MAX_RATE_SECS} s"
                ));
            }
        }
        ConditionTest::AboveUsual { by } | ConditionTest::BelowUsual { by } => {
            if !by.is_finite() || *by <= 0.0 {
                return Err("the distance from usual is a number above 0".into());
            }
        }
    }
    Ok(())
}

/// Reads the saved rules; first run (no file) gets [`default_rules`]. An invalid file is set
/// aside as `alerts.bad.json` (see [`crate::files`]) and the defaults are used, so a hand edit
/// gone wrong never leaves the user without alerts.
pub fn load(path: &std::path::Path) -> Vec<AlertRule> {
    match crate::files::read_json::<Vec<AlertRule>>(path, |rules| validate(rules)) {
        Ok(Some(rules)) => rules,
        Ok(None) => default_rules(),
        Err(e) => {
            log::warn!(
                "alerts: {} not used: {e}; using the default rules",
                path.display()
            );
            default_rules()
        }
    }
}

/// Writes the rules (through a temporary file, so a crash never leaves half a file).
pub fn save(path: &std::path::Path, rules: &[AlertRule]) -> std::io::Result<()> {
    crate::files::write_json(path, &rules)
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

/// A sensor's usual reading now, by registry id.
pub type UsualFn<'a> = &'a dyn Fn(&str) -> Option<f64>;

/// What a tick offers the rules besides sensor readings.
#[derive(Default)]
pub struct TickInputs<'a> {
    /// System Health changes this tick.
    pub health: &'a [HealthEvent],
    /// A sensor's usual reading now (for "above usual"); `None` while there is none.
    pub usual: Option<UsualFn<'a>>,
    /// Vyro drives at least one fan (a fan boost can apply).
    pub fans_on: bool,
    /// This tick's programs, for diagnostic records.
    pub processes: &'a [ProcessSnapshot],
}

/// The part a sensor belongs to, as System Health names it (CPU, GPU, Fans, …), else the
/// sensor itself: alerts about the same part within [`DUPLICATE_SECS`] notify once.
pub fn part(sensor: &str) -> String {
    let prefix = sensor.split('.').next().unwrap_or(sensor);
    match prefix {
        "cpu" => "CPU",
        "gpu" => "GPU",
        "memory" | "ram" => "Memory",
        "storage" => "Storage",
        "board" => "Board thermals",
        "fan" => "Fans",
        "voltage" => "Voltages",
        _ => sensor,
    }
    .to_owned()
}

/// Sensors every diagnostic record keeps after the rule's own (those the PC has).
const DIAGNOSTIC_SENSORS: [&str; 10] = [
    "cpu.temp.package",
    "cpu.usage.total",
    "cpu.power.package",
    "cpu.clock.effective",
    "gpu.0.temp.edge",
    "gpu.0.temp.hotspot",
    "gpu.0.usage",
    "gpu.0.power",
    "memory.used",
    "memory.available",
];
/// Fan headers looked at for a record.
const DIAGNOSTIC_FANS: u16 = 8;

/// The readings and busiest programs of this moment, for a rule's record.
pub fn diagnostic(
    rule: &AlertRule,
    reading: &dyn Fn(&str) -> Option<Reading>,
    processes: &[ProcessSnapshot],
) -> Diagnostic {
    let fans: Vec<String> = (0..DIAGNOSTIC_FANS).map(|i| format!("fan.{i}")).collect();
    let mut ids: Vec<&str> = rule.sensors();
    for id in DIAGNOSTIC_SENSORS
        .iter()
        .copied()
        .chain(fans.iter().map(String::as_str))
    {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let readings = ids
        .into_iter()
        .filter_map(|id| {
            reading(id).map(|r| DiagnosticReading {
                id: id.to_owned(),
                name: r.name,
                value: r.value,
                unit: r.unit,
            })
        })
        .take(DIAGNOSTIC_READINGS)
        .collect();
    let mut busy: Vec<&ProcessSnapshot> = processes
        .iter()
        .filter(|p| {
            let n = p.name.to_ascii_lowercase();
            n != "idle" && n != "system idle process"
        })
        .collect();
    let load = |p: &ProcessSnapshot| p.cpu_pct.unwrap_or(0.0).max(p.gpu_pct.unwrap_or(0.0));
    busy.sort_by(|a, b| load(b).total_cmp(&load(a)));
    let programs = busy
        .into_iter()
        .take(DIAGNOSTIC_PROGRAMS)
        .map(|p| DiagnosticProgram {
            name: p.name.clone(),
            cpu_pct: p.cpu_pct,
            gpu_pct: p.gpu_pct,
        })
        .collect();
    Diagnostic { readings, programs }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Status;

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
            actions: AlertActions::default(),
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
        assert!(dir.join("alerts.bad.json").exists());
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

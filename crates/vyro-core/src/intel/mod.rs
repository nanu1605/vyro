//! Vyro Intelligence (P16): findings about this PC from its own history, each with the
//! evidence behind it, whether it was measured or inferred, and how sure Vyro is. Layered:
//!
//! 1. **Derived metrics** ([`derive`]): thermal headroom, temperature for the load, fan
//!    response, FPS per watt, clock stability.
//! 2. **Baselines**: the recent half hour against the past week at the same load, for the
//!    profile in force where there is enough of it, the same game's earlier sessions, and the
//!    time before a lasting shift ([`stats::change_point`]) so a change three days ago is
//!    measured at full size and dated.
//! 3. **Detection** ([`detect`]): hotter than usual, a slower fan, heat limiting, little
//!    headroom, power steps, what set a game's frame rate, background load during a game, a
//!    game running slower than it used to.
//! 4. **Explanation** ([`cause`]): cause chains from PC changes (P19), profile switches, fan
//!    control and busy programs, under fixed rules that keep a weak correlation from reading
//!    as a cause.
//!
//! Everything here but [`runner`] is pure. Temperatures in texts are written as
//! "<number> °C" (never a difference) so the app can show them in °F.

mod cause;
mod derive;
mod detect;
pub mod digest;
pub mod runner;
mod stats;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::changes::HardwareChange;
use crate::fan::FanEvent;
use crate::gaming::markers::GameEvent;
use crate::gaming::GameSession;
use crate::history::HistoryPoint;
use crate::profiles::Profile;
use crate::registry::SensorTree;
use crate::snapshot::Status;

pub use digest::MinuteDigest;
pub use runner::Intel;

/// How a finding or a cause is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Basis {
    /// Read from the sensors or the logs.
    Measured,
    /// Worked out from measurements.
    Inferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    fn lower(self) -> Self {
        match self {
            Self::High => Self::Medium,
            _ => Self::Low,
        }
    }
}

/// How strongly a cause is tied to what it explains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CauseRole {
    /// It can produce the effect, came just before it began, the effect lasted, and nothing
    /// else that could have done it happened then (or both sides are measured together).
    Likely,
    /// One of several things that could have done it.
    Possible,
    /// Happened around the same time, but is not shown to be the cause.
    Related,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CauseKind {
    /// A PC change (P19): an update, a setting, a part.
    Change,
    Profile,
    FanControl,
    /// A busy program.
    Program,
    /// Another measured finding.
    Measured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FindingKind {
    Hotter,
    Cooling,
    Throttling,
    LowHeadroom,
    PowerChange,
    Bottleneck,
    BackgroundLoad,
    GameSlower,
}

/// A measurement behind a finding: a value over a stretch of time, and what it usually is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Evidence {
    pub label: String,
    /// The sensor to open in History.
    pub metric: Option<String>,
    /// Unix seconds.
    pub from: i64,
    pub to: i64,
    pub value: f64,
    pub baseline: Option<f64>,
    /// Display unit; "°C" values are temperatures (shown in °F if chosen).
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Cause {
    pub role: CauseRole,
    pub basis: Basis,
    pub confidence: Confidence,
    pub kind: CauseKind,
    pub text: String,
    /// When it happened (unix seconds).
    pub ts: Option<i64>,
    /// The PC change (P19) it is.
    pub change_id: Option<i64>,
    pub evidence: Vec<Evidence>,
    /// What explains this cause in turn.
    pub because: Vec<Cause>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Finding {
    /// The same finding keeps its key from one pass to the next.
    pub key: String,
    pub kind: FindingKind,
    /// `warn`: worth a look; `ok`: an explanation.
    pub status: Status,
    pub basis: Basis,
    pub confidence: Confidence,
    pub title: String,
    pub detail: String,
    /// When it happened (unix seconds); `None` for "the last half hour".
    pub ts: Option<i64>,
    /// When it began, where a lasting shift dates it.
    pub since: Option<i64>,
    pub evidence: Vec<Evidence>,
    pub causes: Vec<Cause>,
    /// Around the same time, not shown to be the cause.
    pub related: Vec<Cause>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VitalKind {
    /// Degrees left before the CPU's limit (99 °C).
    HeadroomCpu,
    /// Degrees left before the GPU's limit (92 °C).
    HeadroomGpu,
    /// CPU temperature against its usual at the current load (a difference).
    TempForLoad,
    /// CPU fan speed against its usual at the current CPU temperature.
    FanResponse,
    /// Frames per watt (CPU + GPU) in the last game session.
    Efficiency,
    /// How much the CPU clock varies under sustained full load (%; lower is steadier).
    ClockStability,
}

/// A derived metric, now against usual.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Vital {
    pub kind: VitalKind,
    pub value: Option<f64>,
    pub usual: Option<f64>,
    pub unit: String,
    pub basis: Basis,
    /// What it was measured at ("at 20 % load", "Cyberpunk2077.exe").
    pub detail: Option<String>,
    pub metric: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Insights {
    /// When they were worked out (unix seconds).
    pub ts: i64,
    pub vitals: Vec<Vital>,
    /// Warnings first, then explanations; newest first within each.
    pub findings: Vec<Finding>,
    /// How much history the baselines had, seconds.
    pub history_secs: i64,
}

/// What the app needs every tick: the keys of the findings worth a look (for the tab dot).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InsightBrief {
    pub attention: Vec<String>,
}

impl Insights {
    pub fn brief(&self) -> InsightBrief {
        InsightBrief {
            attention: self
                .findings
                .iter()
                .filter(|f| f.status == Status::Warn)
                .map(|f| f.key.clone())
                .collect(),
        }
    }
}

/// Series by metric id.
pub type SeriesMap = HashMap<String, Vec<HistoryPoint>>;

pub struct Inputs<'a> {
    pub now: i64,
    /// Past 7 days (17-minute buckets): baselines.
    pub week: &'a SeriesMap,
    /// Past 24 h (144 s buckets): recent shifts, throttling, power steps.
    pub day: &'a SeriesMap,
    /// Past hour (10 s buckets): the current state.
    pub hour: &'a SeriesMap,
    /// Sensor display names by id.
    pub names: &'a HashMap<String, String>,
    /// Minute digests of the past week, oldest first.
    pub digests: &'a [MinuteDigest],
    /// Logged game sessions, newest first.
    pub sessions: &'a [GameSession],
    /// Markers of the recent sessions, by session id.
    pub markers: &'a HashMap<i64, Vec<GameEvent>>,
    /// The PC change history (P19).
    pub changes: &'a [HardwareChange],
    /// Fan control log, oldest first.
    pub fan_events: &'a [FanEvent],
    /// The profile in force.
    pub profile: Option<Profile>,
}

/// The metrics [`analyze`] uses that this machine records.
pub fn wanted(tree: &SensorTree) -> Vec<String> {
    let mut ids = Vec::new();
    for g in &tree.groups {
        for s in g.sensors.iter().filter(|s| s.history) {
            let id = s.id.as_str();
            let gpu = id
                .strip_prefix("gpu.")
                .and_then(|r| r.split_once('.'))
                .map(|(_, m)| m);
            let keep = matches!(
                id,
                "cpu.usage.total"
                    | "cpu.temp.package"
                    | "cpu.clock.effective"
                    | "cpu.clock.current"
                    | "cpu.power.package"
            ) || matches!(
                gpu,
                Some("usage" | "temp.edge" | "temp.hotspot" | "clock.core" | "power")
            ) || (id.starts_with("fan.") && s.unit == crate::Unit::Rpm);
            if keep {
                ids.push(s.id.clone());
            }
        }
    }
    ids
}

pub fn analyze(inp: &Inputs) -> Insights {
    let state = derive::derive(inp);
    let mut findings = detect::detect(inp, &state);
    // Worth a look first; newest first within each ("now" is newest).
    let rank = |f: &Finding| u8::from(f.status != Status::Warn);
    let when = |f: &Finding| f.ts.unwrap_or(inp.now);
    findings.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| when(b).cmp(&when(a))));
    let mut vitals = derive::vitals(inp, &state);
    for v in &mut vitals {
        v.value = v.value.map(round);
        v.usual = v.usual.map(round);
    }
    for f in &mut findings {
        tidy(&mut f.evidence);
        for c in f.causes.iter_mut().chain(f.related.iter_mut()) {
            tidy_cause(c);
        }
    }
    Insights {
        ts: inp.now,
        vitals,
        findings,
        history_secs: state.history_secs,
    }
}

/// Hundredths are plenty, and they cross the wire exactly.
fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn tidy(evidence: &mut [Evidence]) {
    for e in evidence {
        e.value = round(e.value);
        e.baseline = e.baseline.map(round);
    }
}

fn tidy_cause(c: &mut Cause) {
    tidy(&mut c.evidence);
    for b in &mut c.because {
        tidy_cause(b);
    }
}

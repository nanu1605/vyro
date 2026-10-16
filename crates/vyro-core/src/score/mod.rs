//! Vyro Performance Score (P17): how well this PC runs compared with how it should, not how
//! fast it is. Formula v1 is fixed by `docs/performance-score.md` (signed off 2026-10-15): nine
//! observed inputs, each turned into a 0–100 sub-score by fixed breakpoints, weighted and
//! averaged over those with data. A change to any input, breakpoint or weight is a new formula
//! version; every result keeps its own.
//!
//! [`compute`] is pure: it takes the [`Measures`] gathered from the history store, the drives,
//! the event log and the intelligence pass (`measure`), so the same measures always give the
//! same score.

pub mod measure;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::intel::{Basis, Confidence};
use crate::snapshot::Status;

/// The formula these results follow.
pub const FORMULA_VERSION: u32 = 1;
/// Below this share of the weight with data there is no score.
pub const MIN_COVERAGE: f64 = 0.6;
/// The window the history inputs look back over.
pub const WINDOW_SECS: i64 = 7 * 86_400;
/// Stability looks back this far in the event log.
pub const STABILITY_SECS: i64 = 30 * 86_400;
/// Cooling and the change from usual need this much history (P16's baselines).
pub const BASELINE_SECS: i64 = 3 * 86_400;
/// Stored results are kept this long.
pub const KEEP_SECS: i64 = 365 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScoreInputId {
    Thermals,
    SustainedClocks,
    Throttling,
    Cooling,
    Storage,
    Memory,
    Network,
    Stability,
    Deviation,
}

impl ScoreInputId {
    pub const ALL: [Self; 9] = [
        Self::Thermals,
        Self::SustainedClocks,
        Self::Throttling,
        Self::Cooling,
        Self::Storage,
        Self::Memory,
        Self::Network,
        Self::Stability,
        Self::Deviation,
    ];

    /// Formula v1's weights; they add up to 100.
    pub const fn weight(self) -> u32 {
        match self {
            Self::Thermals | Self::Throttling | Self::Stability => 15,
            Self::Network => 5,
            _ => 10,
        }
    }

    pub const fn basis(self) -> Basis {
        match self {
            Self::Deviation => Basis::Inferred,
            _ => Basis::Measured,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScoreBand {
    Excellent,
    Good,
    Fair,
    NeedsAttention,
}

impl ScoreBand {
    pub fn of(score: u32) -> Self {
        match score {
            90.. => Self::Excellent,
            75..=89 => Self::Good,
            50..=74 => Self::Fair,
            _ => Self::NeedsAttention,
        }
    }
}

/// One measured value behind an input's sub-score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScorePart {
    pub label: String,
    pub value: f64,
    /// "°C", "%", "ms", "" for a count, or "health" for a drive's verdict (0 good, 1 caution,
    /// 2 bad).
    pub unit: String,
    /// A temperature difference (no °F offset), not a reading.
    pub difference: bool,
    /// This part's 0–100 score; the input takes the worst of its parts.
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScoreInput {
    pub id: ScoreInputId,
    pub weight: u32,
    /// 0–100; `None` when it has no data ([`Self::missing`] says why).
    pub score: Option<f64>,
    pub basis: Basis,
    pub parts: Vec<ScorePart>,
    pub missing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PerformanceScore {
    /// When it was worked out (unix seconds).
    pub ts: i64,
    pub formula: u32,
    /// 0–100; `None` while less than [`MIN_COVERAGE`] of the weight has data.
    pub score: Option<u32>,
    pub band: Option<ScoreBand>,
    /// Share of the weight with data, 0–1.
    pub coverage: f64,
    pub window_secs: i64,
    pub inputs: Vec<ScoreInput>,
}

/// One stored daily result, for the trend.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScorePoint {
    pub ts: i64,
    pub score: u32,
    pub formula: u32,
}

/// The score now and the stored daily results, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScoreReport {
    pub latest: PerformanceScore,
    pub history: Vec<ScorePoint>,
}

/// A device's temperature in loaded minutes against its limit.
#[derive(Debug, Clone, PartialEq)]
pub struct Headroom {
    /// "CPU", "GPU #0".
    pub device: String,
    pub median_c: f64,
    pub limit_c: f64,
}

/// How much a device's clock sagged over its long full-load stretches.
#[derive(Debug, Clone, PartialEq)]
pub struct Sag {
    pub device: String,
    pub pct: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cooling {
    /// CPU temperature against its usual for the load (a difference, °C).
    pub hotter_c: Option<f64>,
    /// CPU fan speed against its usual at the same temperature, as a fraction.
    pub fan_ratio: Option<f64>,
    /// P16 traced the hotter CPU / the slower fan to the owner's own choice.
    pub hotter_chosen: bool,
    pub fan_chosen: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageHealth {
    /// Worst internal drive's SMART verdict (`None`: none readable).
    pub worst: Option<Status>,
    /// Worst SSD's life left, %.
    pub life_pct: Option<f64>,
    /// Free space on the system drive, %.
    pub system_free_pct: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StabilityCounts {
    /// Kernel-Power 41.
    pub shutdowns: u32,
    /// WHEA-Logger 17, 19, 47.
    pub corrected: u32,
    /// WHEA-Logger 18.
    pub fatal: u32,
}

/// Everything formula v1 reads; `None` / empty where there is no data.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measures {
    pub thermals: Vec<Headroom>,
    pub sag: Vec<Sag>,
    /// Share of loaded time with heat or current limiting, 0–1; `None` when nothing reports
    /// limit flags.
    pub throttle: Option<f64>,
    /// `None` before P16's baselines exist.
    pub cooling: Option<Cooling>,
    pub storage: Option<StorageHealth>,
    /// RAM in use, p95, %.
    pub memory_p95: Option<f64>,
    /// Ping: round trip p95 of minute averages (ms), and lost replies (%).
    pub ping_p95_ms: Option<f64>,
    pub loss_pct: Option<f64>,
    pub stability: Option<StabilityCounts>,
    /// Confidence of each P16 warning now; `None` before P16's baselines exist.
    pub deviation: Option<Vec<Confidence>>,
    /// Why history-based inputs are missing when there is too little history.
    pub thin: Option<String>,
    /// Share of loaded time the CPU / GPU held its power limit, 0–1 (diagnostics only; not a
    /// formula v1 input).
    pub power_cpu: Option<f64>,
    pub power_gpu: Option<f64>,
}

/// `x` on straight lines between `points` (x rising), flat beyond the ends.
pub fn interp(points: &[(f64, f64)], x: f64) -> f64 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 0.0;
    };
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    for w in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    last.1
}

/// Formula v1's breakpoints, (input, sub-score), input rising.
pub mod curves {
    /// Thermal headroom (°C); below 0 the score is 0.
    pub const HEADROOM: [(f64, f64); 6] = [
        (-0.001, 0.0),
        (0.0, 20.0),
        (5.0, 50.0),
        (10.0, 70.0),
        (15.0, 85.0),
        (25.0, 100.0),
    ];
    /// Clock sag (%).
    pub const SAG: [(f64, f64); 4] = [(2.0, 100.0), (5.0, 75.0), (10.0, 40.0), (20.0, 0.0)];
    /// Share of loaded time throttled (%).
    pub const THROTTLE: [(f64, f64); 5] = [
        (0.0, 100.0),
        (0.5, 90.0),
        (2.0, 70.0),
        (5.0, 40.0),
        (15.0, 0.0),
    ];
    /// Hotter than usual for the load (°C).
    pub const HOTTER: [(f64, f64); 4] = [(2.0, 100.0), (5.0, 80.0), (10.0, 40.0), (15.0, 0.0)];
    /// Fan speed against usual (%).
    pub const FAN: [(f64, f64); 3] = [(50.0, 20.0), (70.0, 60.0), (90.0, 100.0)];
    /// SSD life left (%).
    pub const LIFE: [(f64, f64); 3] = [(0.0, 30.0), (10.0, 70.0), (30.0, 100.0)];
    /// Free space on the system drive (%).
    pub const FREE: [(f64, f64); 4] = [(2.0, 30.0), (5.0, 60.0), (10.0, 85.0), (15.0, 100.0)];
    /// RAM in use, p95 (%).
    pub const MEMORY: [(f64, f64); 4] = [(75.0, 100.0), (85.0, 80.0), (92.0, 50.0), (98.0, 10.0)];
    /// Ping round trip, p95 (ms).
    pub const PING: [(f64, f64); 4] = [(40.0, 100.0), (80.0, 80.0), (150.0, 50.0), (300.0, 10.0)];
    /// Lost replies (%).
    pub const LOSS: [(f64, f64); 4] = [(0.0, 100.0), (1.0, 80.0), (3.0, 50.0), (10.0, 0.0)];
}

fn part(
    label: impl Into<String>,
    value: f64,
    unit: &str,
    difference: bool,
    score: f64,
) -> ScorePart {
    ScorePart {
        label: label.into(),
        value,
        unit: unit.into(),
        difference,
        score,
    }
}

/// The input from its parts (the worst counts), or missing for `why`.
fn input(id: ScoreInputId, parts: Vec<ScorePart>, why: &str) -> ScoreInput {
    let score = parts.iter().map(|p| p.score).reduce(f64::min);
    ScoreInput {
        id,
        weight: id.weight(),
        score,
        basis: id.basis(),
        missing: score.is_none().then(|| why.to_owned()),
        parts,
    }
}

/// Unexpected shutdowns in 30 days.
fn shutdown_score(n: u32) -> f64 {
    match n {
        0 => 100.0,
        1 => 70.0,
        2 => 45.0,
        _ => 20.0,
    }
}

/// Hardware errors in 30 days.
fn hardware_error_score(c: StabilityCounts) -> f64 {
    match (c.fatal, c.corrected) {
        (1.., _) => 0.0,
        (_, 0) => 100.0,
        (_, 1..=9) => 80.0,
        (_, 10..=99) => 50.0,
        _ => 20.0,
    }
}

fn deviation_score(warnings: &[Confidence]) -> f64 {
    let cost: f64 = warnings
        .iter()
        .map(|c| match c {
            Confidence::High => 25.0,
            Confidence::Medium => 15.0,
            Confidence::Low => 10.0,
        })
        .sum();
    (100.0 - cost).max(0.0)
}

/// Works out the score from `m` at `now` (formula v1).
pub fn compute(m: &Measures, now: i64) -> PerformanceScore {
    use curves::*;
    let thin = m.thin.as_deref();
    let history_why = |default: &'static str| thin.unwrap_or(default).to_owned();

    let thermals = input(
        ScoreInputId::Thermals,
        m.thermals
            .iter()
            .map(|h| {
                let room = h.limit_c - h.median_c;
                part(
                    format!("{} headroom under load", h.device),
                    room,
                    "°C",
                    true,
                    interp(&HEADROOM, room),
                )
            })
            .collect(),
        &history_why("Fewer than 60 minutes under load in the past week."),
    );
    let sustained = input(
        ScoreInputId::SustainedClocks,
        m.sag
            .iter()
            .map(|s| {
                part(
                    format!("{} clock sag over long full load", s.device),
                    s.pct,
                    "%",
                    false,
                    interp(&SAG, s.pct),
                )
            })
            .collect(),
        &history_why("No stretch of 5 minutes or more at full load in the past week."),
    );
    let throttling = input(
        ScoreInputId::Throttling,
        m.throttle
            .map(|share| {
                let pct = share * 100.0;
                part(
                    "Loaded time limited by heat or current",
                    pct,
                    "%",
                    false,
                    interp(&THROTTLE, pct),
                )
            })
            .into_iter()
            .collect(),
        &history_why(
            "This CPU and GPU report no limit flags (or there was no load in the past week).",
        ),
    );
    let cooling = input(
        ScoreInputId::Cooling,
        m.cooling
            .as_ref()
            .map(|c| {
                let mut parts = Vec::new();
                if let Some(h) = c.hotter_c {
                    let s = if c.hotter_chosen {
                        100.0
                    } else {
                        interp(&HOTTER, h)
                    };
                    parts.push(part("CPU hotter than usual for the load", h, "°C", true, s));
                }
                if let Some(r) = c.fan_ratio {
                    let pct = r * 100.0;
                    let s = if c.fan_chosen {
                        100.0
                    } else {
                        interp(&FAN, pct)
                    };
                    parts.push(part("CPU fan speed against usual", pct, "%", false, s));
                }
                parts
            })
            .unwrap_or_default(),
        &history_why("Needs 3 days of history for the usual temperatures and fan speeds."),
    );
    let storage = input(
        ScoreInputId::Storage,
        m.storage
            .as_ref()
            .map(|s| {
                let mut parts = Vec::new();
                if let Some(st) = s.worst {
                    let (v, score) = match st {
                        Status::Fail => (2.0, 0.0),
                        Status::Warn => (1.0, 50.0),
                        _ => (0.0, 100.0),
                    };
                    parts.push(part("Worst drive health", v, "health", false, score));
                }
                if let Some(l) = s.life_pct {
                    parts.push(part("SSD life left", l, "%", false, interp(&LIFE, l)));
                }
                if let Some(f) = s.system_free_pct {
                    parts.push(part(
                        "Free on the system drive",
                        f,
                        "%",
                        false,
                        interp(&FREE, f),
                    ));
                }
                parts
            })
            .unwrap_or_default(),
        "No drive information.",
    );
    let memory = input(
        ScoreInputId::Memory,
        m.memory_p95
            .map(|p| {
                part(
                    "RAM in use (95th percentile)",
                    p,
                    "%",
                    false,
                    interp(&MEMORY, p),
                )
            })
            .into_iter()
            .collect(),
        &history_why("No memory readings in the past week."),
    );
    let mut net = Vec::new();
    if let Some(p) = m.ping_p95_ms {
        net.push(part(
            "Ping (95th percentile)",
            p,
            "ms",
            false,
            interp(&PING, p),
        ));
    }
    if let (Some(l), true) = (m.loss_pct, m.ping_p95_ms.is_some()) {
        net.push(part("Lost replies", l, "%", false, interp(&LOSS, l)));
    }
    let network = input(
        ScoreInputId::Network,
        net,
        "Ping is off, or the PC was offline.",
    );
    let stability = input(
        ScoreInputId::Stability,
        m.stability
            .map(|c| {
                vec![
                    part(
                        "Unexpected shutdowns (30 days)",
                        f64::from(c.shutdowns),
                        "",
                        false,
                        shutdown_score(c.shutdowns),
                    ),
                    part(
                        if c.fatal > 0 {
                            "Fatal hardware errors (30 days)"
                        } else {
                            "Corrected hardware errors (30 days)"
                        },
                        f64::from(if c.fatal > 0 { c.fatal } else { c.corrected }),
                        "",
                        false,
                        hardware_error_score(c),
                    ),
                ]
            })
            .unwrap_or_default(),
        "The Windows event log could not be read.",
    );
    let deviation = input(
        ScoreInputId::Deviation,
        m.deviation
            .as_ref()
            .map(|w| {
                vec![part(
                    "Insights warnings now",
                    f64::from(u32::try_from(w.len()).unwrap_or(u32::MAX)),
                    "",
                    false,
                    deviation_score(w),
                )]
            })
            .unwrap_or_default(),
        &history_why("Needs 3 days of history for Insights' baselines."),
    );

    let inputs = vec![
        thermals, sustained, throttling, cooling, storage, memory, network, stability, deviation,
    ];
    let total: u32 = inputs.iter().map(|i| i.weight).sum();
    let (sum, have) = inputs
        .iter()
        .filter_map(|i| i.score.map(|s| (s * f64::from(i.weight), i.weight)))
        .fold((0.0, 0u32), |(s, w), (x, y)| (s + x, w + y));
    let coverage = f64::from(have) / f64::from(total);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let score = (have > 0 && coverage >= MIN_COVERAGE - 1e-9)
        .then(|| (sum / f64::from(have)).round().clamp(0.0, 100.0) as u32);
    PerformanceScore {
        ts: now,
        formula: FORMULA_VERSION,
        score,
        band: score.map(ScoreBand::of),
        coverage,
        window_secs: WINDOW_SECS,
        inputs,
    }
}

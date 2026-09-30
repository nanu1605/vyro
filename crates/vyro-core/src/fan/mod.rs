//! Fan control (Phase 12b): what the user configured, what the controller reports, and the
//! pure rules that turn sensor readings into duties. The controller (`controller`) applies the
//! duties through `sio::control`; it runs only in the service, only on allow-listed boards,
//! and hands cooling back to the BIOS on any failure.
//!
//! Rules, in order: control off → nothing is driven; CPU or GPU at its emergency limit →
//! every controlled header at 100 %; a curve whose source sensor has no reading → that header
//! at 100 %; otherwise the curve (with hysteresis and ramp limits) or the fixed speed, never
//! below the header's floor.

pub mod controller;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::open_device;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::sio::control::Board;
use crate::snapshot::FanKind;

/// Emergency sensors (the same ones the default alerts watch).
pub const CPU_TEMP: &str = "cpu.temp.package";
pub const GPU_TEMP: &str = "gpu.0.temp.edge";
pub const MAX_POINTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CurvePoint {
    pub temp_c: f64,
    pub pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanCurve {
    /// Registry id of the temperature the curve follows.
    pub source: String,
    /// 2–8 points, temperatures rising.
    pub points: Vec<CurvePoint>,
    /// The speed only falls once the temperature is this far below where it last rose.
    pub hysteresis_c: f64,
    /// Largest change per second, up and down (percentage points).
    pub ramp_up_pct_s: f64,
    pub ramp_down_pct_s: f64,
}

impl Default for FanCurve {
    /// "Balanced": quiet at idle, full speed near the CPU's own limit.
    fn default() -> Self {
        let p = |temp_c, pct| CurvePoint { temp_c, pct };
        Self {
            source: CPU_TEMP.into(),
            points: vec![p(40.0, 30.0), p(60.0, 45.0), p(75.0, 70.0), p(85.0, 100.0)],
            hysteresis_c: 3.0,
            ramp_up_pct_s: 10.0,
            ramp_down_pct_s: 5.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "lowercase")]
#[ts(export)]
pub enum HeaderMode {
    /// The BIOS curve (Vyro does not touch the header).
    Bios,
    Fixed {
        pct: f64,
    },
    Curve {
        curve: FanCurve,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeaderConfig {
    pub index: u16,
    pub mode: HeaderMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanConfig {
    /// Off by default; the UI asks for confirmation before turning it on.
    pub enabled: bool,
    pub headers: Vec<HeaderConfig>,
    pub emergency_cpu_c: f64,
    pub emergency_gpu_c: f64,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            headers: Vec::new(),
            emergency_cpu_c: 95.0,
            emergency_gpu_c: 90.0,
        }
    }
}

impl FanConfig {
    pub fn mode(&self, index: u16) -> &HeaderMode {
        self.headers
            .iter()
            .find(|h| h.index == index)
            .map_or(&HeaderMode::Bios, |h| &h.mode)
    }

    /// Whether any header is driven by Vyro.
    pub fn drives_any(&self) -> bool {
        self.enabled && self.headers.iter().any(|h| h.mode != HeaderMode::Bios)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "lowercase")]
#[ts(export)]
pub enum FanCapability {
    /// Fan control is possible on this board.
    Writable { board: String },
    /// A supported chip, but control is not validated here.
    ReadOnly { reason: String },
    /// No PawnIO, no supported chip, or not running in the service.
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HeaderStatus {
    pub index: u16,
    pub name: String,
    pub kind: FanKind,
    /// Lowest speed Vyro sets on this header.
    pub min_pct: u8,
    /// Whether this header can be driven (pumps wait for pump control).
    pub controllable: bool,
    pub rpm: Option<f64>,
    /// Duty the chip drives now.
    pub duty_pct: Option<f64>,
    /// Under Vyro's control (else the BIOS curve).
    pub software: bool,
    /// What Vyro asked for.
    pub target_pct: Option<f64>,
    /// Current temperature of the curve's source, for the editor's live marker.
    pub source_c: Option<f64>,
    /// False when the header reads 0 RPM while driven: nothing plugged in, or a hub or splitter
    /// without a speed wire (its fans still follow the header).
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanStatus {
    pub capability: FanCapability,
    pub config: FanConfig,
    /// Vyro is driving at least one header right now.
    pub active: bool,
    /// Why every header (or one) runs at full speed, while it lasts.
    pub override_reason: Option<String>,
    pub last_error: Option<String>,
    pub headers: Vec<HeaderStatus>,
}

impl FanStatus {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            capability: FanCapability::Unavailable {
                reason: reason.into(),
            },
            config: FanConfig::default(),
            active: false,
            override_reason: None,
            last_error: None,
            headers: Vec::new(),
        }
    }
}

/// Speed on `points` at `temp`: linear between points, flat beyond the ends.
pub fn evaluate(points: &[CurvePoint], temp: f64) -> f64 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 100.0;
    };
    if temp <= first.temp_c {
        return first.pct;
    }
    if temp >= last.temp_c {
        return last.pct;
    }
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if temp <= b.temp_c {
            let span = b.temp_c - a.temp_c;
            if span <= 0.0 {
                return b.pct;
            }
            return a.pct + (b.pct - a.pct) * (temp - a.temp_c) / span;
        }
    }
    last.pct
}

/// Checks a configuration before it is applied or saved. `board` is the allow-listed board
/// (`None` when control is not possible here: then it may not be enabled).
pub fn validate(cfg: &FanConfig, board: Option<&Board>) -> Result<(), String> {
    if !(40.0..=105.0).contains(&cfg.emergency_cpu_c) {
        return Err("CPU emergency limit must be 40–105 °C".into());
    }
    if !(40.0..=105.0).contains(&cfg.emergency_gpu_c) {
        return Err("GPU emergency limit must be 40–105 °C".into());
    }
    if cfg.enabled && board.is_none() {
        return Err("fan control is not available on this PC".into());
    }
    let mut seen = Vec::new();
    for h in &cfg.headers {
        if seen.contains(&h.index) {
            return Err(format!("header {} is listed twice", h.index));
        }
        seen.push(h.index);
        if h.mode == HeaderMode::Bios {
            continue;
        }
        let header = board
            .and_then(|b| b.headers.iter().find(|x| x.index == h.index))
            .ok_or_else(|| format!("header {} cannot be controlled", h.index))?;
        if header.kind == FanKind::Pump {
            return Err(format!(
                "{} stays on the BIOS curve until pump control",
                header.name
            ));
        }
        match &h.mode {
            HeaderMode::Bios => {}
            HeaderMode::Fixed { pct } => {
                if !(0.0..=100.0).contains(pct) {
                    return Err(format!("{}: speed must be 0–100 %", header.name));
                }
            }
            HeaderMode::Curve { curve } => {
                validate_curve(curve).map_err(|e| format!("{}: {e}", header.name))?
            }
        }
    }
    Ok(())
}

fn validate_curve(c: &FanCurve) -> Result<(), String> {
    if c.source.is_empty() {
        return Err("choose a temperature to follow".into());
    }
    if !(2..=MAX_POINTS).contains(&c.points.len()) {
        return Err(format!("a curve has 2–{MAX_POINTS} points"));
    }
    for p in &c.points {
        if !(0.0..=150.0).contains(&p.temp_c) || !(0.0..=100.0).contains(&p.pct) {
            return Err("points must be 0–150 °C and 0–100 %".into());
        }
    }
    if c.points.windows(2).any(|w| w[1].temp_c <= w[0].temp_c) {
        return Err("point temperatures must rise from left to right".into());
    }
    if !(0.0..=15.0).contains(&c.hysteresis_c) {
        return Err("hysteresis must be 0–15 °C".into());
    }
    if !(1.0..=100.0).contains(&c.ramp_up_pct_s) || !(1.0..=100.0).contains(&c.ramp_down_pct_s) {
        return Err("ramp rates must be 1–100 % per second".into());
    }
    Ok(())
}

/// Per-header state between ticks.
#[derive(Debug, Clone, Copy, Default)]
struct Run {
    /// Temperature the curve is evaluated at (follows rises at once, falls after hysteresis).
    held_c: Option<f64>,
    /// Speed last asked for.
    pct: Option<f64>,
}

/// What to drive this tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decision {
    /// `(header, percent)` for every header Vyro drives; headers not listed go to the BIOS.
    pub targets: Vec<(u16, f64)>,
    /// Why speeds were forced to 100 %.
    pub override_reason: Option<String>,
    /// Source temperature of each curve header.
    pub sources: Vec<(u16, f64)>,
}

/// Turns readings into speeds tick by tick.
#[derive(Debug, Default)]
pub struct Runtime {
    runs: HashMap<u16, Run>,
    last_s: Option<f64>,
}

impl Runtime {
    /// Starts a header's ramp from the speed it runs at now, so taking over does not jump.
    pub fn seed(&mut self, header: u16, pct: f64) {
        self.runs.entry(header).or_default().pct.get_or_insert(pct);
    }

    pub fn reset(&mut self) {
        self.runs.clear();
        self.last_s = None;
    }

    /// `now_s` is a monotonic time in seconds; `read` gives a sensor's current value.
    pub fn decide(
        &mut self,
        cfg: &FanConfig,
        board: &Board,
        now_s: f64,
        read: impl Fn(&str) -> Option<f64>,
    ) -> Decision {
        // No elapsed time on the first tick: seeded headers start where they are.
        let dt = self.last_s.map_or(0.0, |t| (now_s - t).max(0.0));
        self.last_s = Some(now_s);
        let mut out = Decision::default();
        if !cfg.enabled {
            self.runs.clear();
            return out;
        }
        let finite = |id: &str| read(id).filter(|v| v.is_finite());
        let emergency = match (finite(CPU_TEMP), finite(GPU_TEMP)) {
            (Some(c), _) if c >= cfg.emergency_cpu_c => Some(format!(
                "CPU at {c:.0} °C (limit {:.0} °C): fans at full speed",
                cfg.emergency_cpu_c
            )),
            (_, Some(g)) if g >= cfg.emergency_gpu_c => Some(format!(
                "GPU at {g:.0} °C (limit {:.0} °C): fans at full speed",
                cfg.emergency_gpu_c
            )),
            _ => None,
        };
        for h in board.headers {
            let mode = cfg.mode(h.index);
            if *mode == HeaderMode::Bios || h.kind == FanKind::Pump {
                self.runs.remove(&h.index);
                continue;
            }
            let run = self.runs.entry(h.index).or_default();
            let floor = f64::from(h.min_pct);
            let want = if emergency.is_some() {
                None
            } else {
                match mode {
                    HeaderMode::Bios => continue,
                    HeaderMode::Fixed { pct } => Some(*pct),
                    HeaderMode::Curve { curve } => match finite(&curve.source) {
                        Some(t) => {
                            out.sources.push((h.index, t));
                            let held = run
                                .held_c
                                .map_or(t, |held| held.clamp(t, t + curve.hysteresis_c.max(0.0)));
                            run.held_c = Some(held);
                            let target = evaluate(&curve.points, held);
                            Some(ramp(run.pct, target, dt, curve))
                        }
                        None => {
                            out.override_reason.get_or_insert_with(|| {
                                format!("{} has no reading: {} at full speed", curve.source, h.name)
                            });
                            None
                        }
                    },
                }
            };
            let pct = want.unwrap_or(100.0).clamp(floor, 100.0);
            run.pct = Some(pct);
            out.targets.push((h.index, pct));
        }
        if emergency.is_some() {
            out.override_reason = emergency;
        }
        out
    }
}

fn ramp(from: Option<f64>, to: f64, dt: f64, c: &FanCurve) -> f64 {
    let Some(from) = from else {
        return to;
    };
    if to >= from {
        to.min(from + c.ramp_up_pct_s * dt)
    } else {
        to.max(from - c.ramp_down_pct_s * dt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sio::control::ALLOW_LIST;

    fn board() -> &'static Board {
        &ALLOW_LIST[0]
    }

    fn curve(points: &[(f64, f64)]) -> FanCurve {
        FanCurve {
            points: points
                .iter()
                .map(|&(temp_c, pct)| CurvePoint { temp_c, pct })
                .collect(),
            ..FanCurve::default()
        }
    }

    fn cfg(modes: &[(u16, HeaderMode)]) -> FanConfig {
        FanConfig {
            enabled: true,
            headers: modes
                .iter()
                .map(|(index, mode)| HeaderConfig {
                    index: *index,
                    mode: mode.clone(),
                })
                .collect(),
            ..FanConfig::default()
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn evaluates_between_and_beyond_points() {
        let c = curve(&[(40.0, 30.0), (60.0, 50.0), (80.0, 100.0)]);
        assert!(close(evaluate(&c.points, 20.0), 30.0));
        assert!(close(evaluate(&c.points, 50.0), 40.0));
        assert!(close(evaluate(&c.points, 70.0), 75.0));
        assert!(close(evaluate(&c.points, 95.0), 100.0));
        assert!(close(evaluate(&[], 50.0), 100.0));
    }

    #[test]
    fn validation_guards_every_field() {
        let b = Some(board());
        let ok = cfg(&[(
            0,
            HeaderMode::Curve {
                curve: FanCurve::default(),
            },
        )]);
        assert_eq!(validate(&ok, b), Ok(()));
        assert!(validate(&ok, None).is_err(), "enabled without a board");
        let bios_only = FanConfig {
            enabled: false,
            ..ok.clone()
        };
        assert!(validate(&bios_only, None).is_err(), "curves need a board");
        assert_eq!(validate(&FanConfig::default(), None), Ok(()));

        let pump = cfg(&[(1, HeaderMode::Fixed { pct: 80.0 })]);
        assert!(validate(&pump, b).unwrap_err().contains("pump"));
        let unlisted = cfg(&[(5, HeaderMode::Fixed { pct: 80.0 })]);
        assert!(validate(&unlisted, b).is_err());
        let twice = cfg(&[(0, HeaderMode::Bios), (0, HeaderMode::Bios)]);
        assert!(validate(&twice, b).is_err());
        let over = cfg(&[(0, HeaderMode::Fixed { pct: 120.0 })]);
        assert!(validate(&over, b).is_err());
        let falling = cfg(&[(
            0,
            HeaderMode::Curve {
                curve: curve(&[(60.0, 50.0), (50.0, 60.0)]),
            },
        )]);
        assert!(validate(&falling, b).is_err());
        let one = cfg(&[(
            0,
            HeaderMode::Curve {
                curve: curve(&[(60.0, 50.0)]),
            },
        )]);
        assert!(validate(&one, b).is_err());
        let mut limit = ok.clone();
        limit.emergency_cpu_c = 30.0;
        assert!(validate(&limit, b).is_err());
        limit.emergency_cpu_c = 50.0; // the field check lowers it to force an override
        assert_eq!(validate(&limit, b), Ok(()));
    }

    #[test]
    fn nothing_is_driven_when_off_or_on_bios() {
        let mut rt = Runtime::default();
        let mut c = cfg(&[(0, HeaderMode::Fixed { pct: 50.0 })]);
        c.enabled = false;
        assert!(rt
            .decide(&c, board(), 0.0, |_| Some(50.0))
            .targets
            .is_empty());
        let bios = cfg(&[(0, HeaderMode::Bios)]);
        assert!(rt
            .decide(&bios, board(), 1.0, |_| Some(50.0))
            .targets
            .is_empty());
        assert!(!bios.drives_any());
    }

    #[test]
    fn fixed_speeds_respect_the_floor_and_pumps_are_skipped() {
        let mut rt = Runtime::default();
        let c = cfg(&[
            (0, HeaderMode::Fixed { pct: 5.0 }),
            (1, HeaderMode::Fixed { pct: 80.0 }),
            (2, HeaderMode::Fixed { pct: 55.0 }),
        ]);
        let d = rt.decide(&c, board(), 0.0, |_| Some(40.0));
        assert_eq!(d.targets, vec![(0, 20.0), (2, 55.0)]);
        assert_eq!(d.override_reason, None);
    }

    #[test]
    fn curve_ramps_and_holds_with_hysteresis() {
        let mut rt = Runtime::default();
        let c = cfg(&[(
            0,
            HeaderMode::Curve {
                curve: curve(&[(40.0, 30.0), (80.0, 100.0)]),
            },
        )]);
        let at = |rt: &mut Runtime, t: f64, temp: f64| {
            rt.decide(&c, board(), t, move |id| (id == CPU_TEMP).then_some(temp))
                .targets[0]
                .1
        };
        // First tick jumps to the curve (nothing to ramp from).
        assert!(close(at(&mut rt, 0.0, 40.0), 30.0));
        // Hot: the curve says 100 %, the ramp allows 10 %/s.
        assert!(close(at(&mut rt, 1.0, 80.0), 40.0));
        assert!(close(at(&mut rt, 2.0, 80.0), 50.0));
        for t in 3..10 {
            at(&mut rt, f64::from(t), 80.0);
        }
        assert!(close(at(&mut rt, 10.0, 80.0), 100.0));
        // 2 °C cooler is inside the 3 °C hysteresis: still 100 %.
        assert!(close(at(&mut rt, 11.0, 78.0), 100.0));
        // 10 °C cooler: held at 73 °C (70 + 3) → 87.75 %, reached at 5 %/s.
        assert!(close(at(&mut rt, 12.0, 70.0), 95.0));
        for t in 13..20 {
            at(&mut rt, f64::from(t), 70.0);
        }
        assert!(close(at(&mut rt, 20.0, 70.0), 87.75));
    }

    #[test]
    fn a_seeded_header_starts_from_its_current_speed() {
        let mut rt = Runtime::default();
        rt.seed(0, 60.0);
        let c = cfg(&[(0, HeaderMode::Fixed { pct: 30.0 })]);
        // Fixed speeds are not ramped.
        assert_eq!(
            rt.decide(&c, board(), 0.0, |_| None).targets,
            vec![(0, 30.0)]
        );
        let mut rt = Runtime::default();
        rt.seed(0, 60.0);
        let c = cfg(&[(
            0,
            HeaderMode::Curve {
                curve: curve(&[(40.0, 30.0), (80.0, 30.0)]),
            },
        )]);
        rt.decide(&c, board(), 0.0, |_| Some(50.0));
        // The first tick has no elapsed time: it stays at 60 % and ramps down from there.
        let d = rt.decide(&c, board(), 1.0, |_| Some(50.0));
        assert_eq!(d.targets, vec![(0, 55.0)]);
    }

    #[test]
    fn a_missing_source_runs_that_header_at_full_speed() {
        let mut rt = Runtime::default();
        let gpu_curve = FanCurve {
            source: "gpu.0.temp.hotspot".into(),
            ..FanCurve::default()
        };
        let c = cfg(&[
            (
                0,
                HeaderMode::Curve {
                    curve: FanCurve::default(),
                },
            ),
            (2, HeaderMode::Curve { curve: gpu_curve }),
        ]);
        let d = rt.decide(&c, board(), 0.0, |id| {
            (id == CPU_TEMP)
                .then_some(40.0)
                .or((id == "gpu.0.temp.hotspot").then_some(f64::NAN))
        });
        assert_eq!(d.targets, vec![(0, 30.0), (2, 100.0)]);
        assert!(d.override_reason.unwrap().contains("gpu.0.temp.hotspot"));
        assert_eq!(d.sources, vec![(0, 40.0)]);
    }

    #[test]
    fn emergency_limits_force_full_speed_everywhere() {
        let mut rt = Runtime::default();
        let mut c = cfg(&[
            (0, HeaderMode::Fixed { pct: 30.0 }),
            (
                2,
                HeaderMode::Curve {
                    curve: FanCurve::default(),
                },
            ),
        ]);
        let d = rt.decide(&c, board(), 0.0, |id| match id {
            CPU_TEMP => Some(96.0),
            _ => None,
        });
        assert_eq!(d.targets, vec![(0, 100.0), (2, 100.0)]);
        assert!(d.override_reason.unwrap().starts_with("CPU at 96 °C"));
        c.emergency_cpu_c = 100.0;
        let d = rt.decide(&c, board(), 1.0, |id| match id {
            CPU_TEMP => Some(60.0),
            GPU_TEMP => Some(91.0),
            _ => None,
        });
        assert!(d.override_reason.unwrap().starts_with("GPU at 91 °C"));
        // Once it cools, the curve ramps down from 100 % instead of dropping.
        let d = rt.decide(&c, board(), 2.0, |id| (id == CPU_TEMP).then_some(40.0));
        assert_eq!(d.targets, vec![(0, 30.0), (2, 95.0)]);
        assert_eq!(d.override_reason, None);
    }

    #[test]
    fn config_round_trips_as_json() {
        let c = cfg(&[
            (
                0,
                HeaderMode::Curve {
                    curve: FanCurve::default(),
                },
            ),
            (2, HeaderMode::Fixed { pct: 40.0 }),
            (3, HeaderMode::Bios),
        ]);
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""mode":"curve""#));
        assert_eq!(serde_json::from_str::<FanConfig>(&json).unwrap(), c);
    }
}

//! Fan control (Phase 12b): what the user configured, what the controller reports, and the
//! pure rules that turn sensor readings into duties. The controller (`controller`) applies the
//! duties through `sio::control`; it runs only in the service, only on allow-listed boards,
//! and hands cooling back to the BIOS on any failure.
//!
//! Rules, in order: control off → nothing is driven; a pump that stops reporting a speed →
//! control off and every header back to the BIOS; CPU or GPU at its emergency limit → every
//! controlled header at 100 %; a curve whose source sensor has no reading → that header at
//! 100 %; otherwise the curve (with hysteresis and ramp limits) or the fixed speed, never below
//! the header's floor.
//!
//! Pumps (12c): the pump header is driven only after the owner confirms it drives a pump
//! ([`FanConfig::pumps`]); a pump plugged into a fan header can be marked the same way. A
//! pump never goes below [`PUMP_MIN_PCT`] %.
//!
//! Profiles (12e): each P9c profile can have its own header settings
//! ([`FanConfig::profiles`]); the others use [`FanConfig::headers`]. Turning control on or off,
//! the safety limits, pumps and names apply to every profile.

pub mod controller;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::open_device;

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::profiles::Profile;
use crate::sio::control::{Board, Header};
use crate::snapshot::FanKind;

pub use crate::sio::control::PUMP_MIN_PCT;

/// Emergency sensors (the same ones the default alerts watch).
pub const CPU_TEMP: &str = "cpu.temp.package";
pub const GPU_TEMP: &str = "gpu.0.temp.edge";
pub const MAX_POINTS: usize = 8;
/// A driven pump that has reported a speed and then reads below this...
pub const PUMP_STALL_RPM: f64 = 300.0;
/// ...on this many ticks in a row has stopped: control ends and the BIOS takes over.
pub const PUMP_STALL_TICKS: u32 = 3;

/// Longest name the owner can give a header.
pub const MAX_LABEL: usize = 32;

/// Registry id of a header's fan speed.
pub fn rpm_id(header: u16) -> String {
    format!("fan.{header}")
}

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
    /// Headers the owner confirmed drive a pump. The board's pump header can be driven only
    /// once listed; every listed header keeps the pump floor and the stall check.
    #[serde(default)]
    pub pumps: Vec<u16>,
    /// Names the owner gave headers ("Case fans"), shown instead of the board's.
    #[serde(default)]
    pub labels: BTreeMap<u16, String>,
    /// Header settings of profiles that do not use [`Self::headers`].
    #[serde(default)]
    pub profiles: BTreeMap<Profile, Vec<HeaderConfig>>,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            headers: Vec::new(),
            emergency_cpu_c: 95.0,
            emergency_gpu_c: 90.0,
            pumps: Vec::new(),
            labels: BTreeMap::new(),
            profiles: BTreeMap::new(),
        }
    }
}

impl FanConfig {
    /// The configuration in force while `profile` is active: its own header settings, if it
    /// has any.
    pub fn effective(&self, profile: Profile) -> FanConfig {
        let mut c = self.clone();
        if let Some(own) = c.profiles.remove(&profile) {
            c.headers = own;
        }
        c.profiles.clear();
        c
    }

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

    /// Whether `header` is treated as a pump (the board says so, or the owner did).
    pub fn is_pump(&self, header: &Header) -> bool {
        header.kind == FanKind::Pump || self.pumps.contains(&header.index)
    }

    /// Whether Vyro may drive `header` (a pump header only once the owner confirmed it).
    pub fn may_drive(&self, header: &Header) -> bool {
        header.kind != FanKind::Pump || self.pumps.contains(&header.index)
    }

    /// Lowest speed for `header`: its own floor, raised to the pump floor for a pump.
    pub fn floor(&self, header: &Header) -> u8 {
        if self.is_pump(header) {
            header.min_pct.max(PUMP_MIN_PCT)
        } else {
            header.min_pct
        }
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
    /// Lowest speed Vyro sets on this header (marking it as a pump raises it to
    /// [`FanStatus::pump_min_pct`]).
    pub min_pct: u8,
    /// Whether this header can be driven on this board (a pump header still needs the owner's
    /// confirmation, [`FanConfig::pumps`]).
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
    /// Lowest speed for any header treated as a pump.
    pub pump_min_pct: u8,
    /// The profile whose header settings are in force.
    pub profile: Profile,
    /// An alert running the driven headers faster, while it lasts.
    #[serde(default)]
    pub boost: Option<FanBoost>,
}

/// An alert's fan action in force (P18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanBoost {
    /// Percentage points on top of each driven header's speed.
    pub pct: u8,
    /// The alert rule asking for it.
    pub rule: String,
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
            pump_min_pct: PUMP_MIN_PCT,
            profile: Profile::default(),
            boost: None,
        }
    }
}

/// Fan control as each sensor snapshot reports it, so the header chip and the tray follow it
/// without asking the service.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanBrief {
    /// Vyro drives at least one header.
    pub active: bool,
    /// A safety rule runs the driven headers at full speed.
    pub full_speed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FanEventKind {
    /// Control turned on.
    On,
    /// Control turned off by the owner.
    Off,
    /// "Return to BIOS".
    Bios,
    /// A safety rule set full speed.
    FullSpeed,
    /// Full speed no longer needed.
    Normal,
    /// A fault ended control (stopped pump, a register that did not keep a write, I/O errors,
    /// another program driving the fans).
    Fault,
    /// Sensor readings stopped: the BIOS took over until they resume.
    Watchdog,
    /// A BIOS state left by a crash was written back at start.
    Recovered,
    /// A header lost Vyro's setting and was written again.
    Rewritten,
    Sleep,
    Wake,
    /// Another profile's header settings are now in force.
    Profile,
    /// An alert raised the fans Vyro drives (P18).
    Boost,
    /// That alert cleared: the fans follow their settings again.
    BoostEnd,
}

/// One entry of the fan control log (kept in the history store, kind [`EVENT_KIND`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanEvent {
    /// Unix seconds.
    pub ts: i64,
    pub kind: FanEventKind,
    pub message: String,
}

/// History-store event kind of [`FanEvent`]s.
pub const EVENT_KIND: &str = "fan";

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
    let find = |index: u16| board.and_then(|b| b.headers.iter().find(|x| x.index == index));
    for (i, p) in cfg.pumps.iter().enumerate() {
        if cfg.pumps[..i].contains(p) || (board.is_some() && find(*p).is_none()) {
            return Err(format!("header {p} cannot be marked as a pump"));
        }
    }
    for (index, label) in &cfg.labels {
        if board.is_some() && find(*index).is_none() {
            return Err(format!("header {index} cannot be named"));
        }
        if label.trim().is_empty() || label.trim() != label || label.chars().count() > MAX_LABEL {
            return Err(format!(
                "names are 1–{MAX_LABEL} characters without spaces at either end"
            ));
        }
    }
    validate_headers(cfg, &cfg.headers, board)?;
    for (profile, headers) in &cfg.profiles {
        validate_headers(cfg, headers, board)
            .map_err(|e| format!("{}: {e}", profile.spec().name))?;
    }
    Ok(())
}

/// One header list (shared, or a profile's) against the board and the pump confirmations.
fn validate_headers(
    cfg: &FanConfig,
    headers: &[HeaderConfig],
    board: Option<&Board>,
) -> Result<(), String> {
    let find = |index: u16| board.and_then(|b| b.headers.iter().find(|x| x.index == index));
    let mut seen = Vec::new();
    for h in headers {
        if seen.contains(&h.index) {
            return Err(format!("header {} is listed twice", h.index));
        }
        seen.push(h.index);
        if h.mode == HeaderMode::Bios {
            continue;
        }
        let header =
            find(h.index).ok_or_else(|| format!("header {} cannot be controlled", h.index))?;
        if !cfg.may_drive(header) {
            return Err(format!(
                "confirm that {} drives your pump before Vyro controls it",
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
    /// A pump has reported a speed since Vyro took it over.
    spun: bool,
    /// Ticks in a row a pump read below [`PUMP_STALL_RPM`] after it spun.
    stalled: u32,
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
    /// A fault that ends control (a stopped pump): every header goes back to the BIOS.
    pub fault: Option<String>,
}

/// Turns readings into speeds tick by tick.
#[derive(Debug, Default)]
pub struct Runtime {
    runs: HashMap<u16, Run>,
    last_s: Option<f64>,
    /// Percentage points an alert adds to every driven header (P18): on top of its curve
    /// (ramped like any change) or fixed speed. Safety full speed is unaffected.
    pub boost: f64,
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
        let boost = self.boost.clamp(0.0, 100.0);
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
            if *mode == HeaderMode::Bios || !cfg.may_drive(h) {
                self.runs.remove(&h.index);
                continue;
            }
            let run = self.runs.entry(h.index).or_default();
            if cfg.is_pump(h) {
                if let Some(rpm) = read(&rpm_id(h.index)).filter(|v| v.is_finite()) {
                    if rpm >= PUMP_STALL_RPM {
                        run.spun = true;
                        run.stalled = 0;
                    } else if run.spun {
                        run.stalled += 1;
                        if run.stalled >= PUMP_STALL_TICKS {
                            out.fault.get_or_insert_with(|| {
                                format!("{} reads {rpm:.0} RPM: the pump may have stopped", h.name)
                            });
                        }
                    }
                }
            }
            let floor = f64::from(cfg.floor(h));
            let want = if emergency.is_some() {
                None
            } else {
                match mode {
                    HeaderMode::Bios => continue,
                    HeaderMode::Fixed { pct } => Some(*pct + boost),
                    HeaderMode::Curve { curve } => match finite(&curve.source) {
                        Some(t) => {
                            out.sources.push((h.index, t));
                            let held = run
                                .held_c
                                .map_or(t, |held| held.clamp(t, t + curve.hysteresis_c.max(0.0)));
                            run.held_c = Some(held);
                            let target = evaluate(&curve.points, held) + boost;
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

        let mut pump = cfg(&[(1, HeaderMode::Fixed { pct: 80.0 })]);
        assert!(
            validate(&pump, b).unwrap_err().contains("pump"),
            "unconfirmed"
        );
        pump.pumps = vec![1];
        assert_eq!(validate(&pump, b), Ok(()));
        pump.pumps = vec![1, 1];
        assert!(validate(&pump, b).is_err(), "listed twice");
        pump.pumps = vec![1, 6];
        assert!(validate(&pump, b).is_err(), "not a header");
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
    fn confirmed_pumps_are_driven_never_below_the_pump_floor() {
        let mut rt = Runtime::default();
        let mut c = cfg(&[
            (1, HeaderMode::Fixed { pct: 30.0 }),
            (2, HeaderMode::Fixed { pct: 30.0 }),
        ]);
        // The pump header once confirmed; a pump on System Fan 1 marked by the owner.
        c.pumps = vec![1, 2];
        let d = rt.decide(&c, board(), 0.0, |id| {
            Some(if id.starts_with("fan.") { 4000.0 } else { 40.0 })
        });
        assert_eq!(d.targets, vec![(1, 60.0), (2, 60.0)]);
        assert_eq!(c.floor(&board().headers[2]), PUMP_MIN_PCT);
        assert_eq!(c.floor(&board().headers[0]), 20);
        assert!(!FanConfig::default().may_drive(&board().headers[1]));
        assert!(c.may_drive(&board().headers[1]));
    }

    #[test]
    fn a_pump_that_stops_reporting_a_speed_ends_control() {
        let mut rt = Runtime::default();
        let mut c = cfg(&[(1, HeaderMode::Fixed { pct: 70.0 })]);
        c.pumps = vec![1];
        let rpm = |v: f64| move |id: &str| (id == "fan.1").then_some(v);
        assert_eq!(rt.decide(&c, board(), 0.0, rpm(3800.0)).fault, None);
        // One or two low reads may be a glitch.
        assert_eq!(rt.decide(&c, board(), 1.0, rpm(0.0)).fault, None);
        assert_eq!(rt.decide(&c, board(), 2.0, rpm(0.0)).fault, None);
        let d = rt.decide(&c, board(), 3.0, rpm(0.0));
        assert!(d.fault.unwrap().contains("Pump Fan reads 0 RPM"));
        // A reading in between starts the count again.
        let mut rt = Runtime::default();
        for (t, v) in [3800.0, 0.0, 0.0, 3700.0, 0.0, 0.0].into_iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let d = rt.decide(&c, board(), t as f64, rpm(v));
            assert_eq!(d.fault, None);
        }
        // A pump header that never reported a speed (no tach wire) is not judged by it.
        let mut rt = Runtime::default();
        for t in 0..5 {
            assert_eq!(rt.decide(&c, board(), f64::from(t), rpm(0.0)).fault, None);
        }
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
    fn an_alert_boost_raises_driven_headers_only() {
        let mut rt = Runtime::default();
        let c = cfg(&[
            (0, HeaderMode::Fixed { pct: 40.0 }),
            (1, HeaderMode::Bios),
            (
                2,
                HeaderMode::Curve {
                    curve: curve(&[(40.0, 30.0), (80.0, 100.0)]),
                },
            ),
        ]);
        let read = |temp: f64| move |id: &str| (id == CPU_TEMP).then_some(temp);
        assert_eq!(
            rt.decide(&c, board(), 0.0, read(40.0)).targets,
            vec![(0, 40.0), (2, 30.0)]
        );
        rt.boost = 20.0;
        // Fixed jumps; the curve ramps up at its 10 %/s; the BIOS header is left alone.
        let d = rt.decide(&c, board(), 1.0, read(40.0));
        assert_eq!(d.targets, vec![(0, 60.0), (2, 40.0)]);
        let d = rt.decide(&c, board(), 2.0, read(40.0));
        assert_eq!(d.targets, vec![(0, 60.0), (2, 50.0)]);
        // Never past 100 %.
        for t in 3..12 {
            rt.decide(&c, board(), f64::from(t), read(80.0));
        }
        let d = rt.decide(&c, board(), 12.0, read(80.0));
        assert_eq!(d.targets, vec![(0, 60.0), (2, 100.0)]);
        // Over: back down at the curve's ramp.
        rt.boost = 0.0;
        let d = rt.decide(&c, board(), 13.0, read(40.0));
        assert_eq!(d.targets, vec![(0, 40.0), (2, 95.0)]);
        // Nothing at all with control off.
        rt.boost = 50.0;
        let off = FanConfig {
            enabled: false,
            ..c.clone()
        };
        assert!(rt
            .decide(&off, board(), 14.0, read(40.0))
            .targets
            .is_empty());
    }

    #[test]
    fn a_profile_with_its_own_settings_replaces_the_shared_ones() {
        let mut c = cfg(&[(0, HeaderMode::Fixed { pct: 40.0 })]);
        c.profiles.insert(
            Profile::Gaming,
            vec![HeaderConfig {
                index: 0,
                mode: HeaderMode::Fixed { pct: 70.0 },
            }],
        );
        assert_eq!(
            c.effective(Profile::Gaming).mode(0),
            &HeaderMode::Fixed { pct: 70.0 }
        );
        assert_eq!(
            c.effective(Profile::Silent).mode(0),
            &HeaderMode::Fixed { pct: 40.0 }
        );
        assert!(c.effective(Profile::Gaming).profiles.is_empty());
        assert_eq!(validate(&c, Some(board())), Ok(()));
        // A profile's list is checked like the shared one.
        c.profiles.insert(
            Profile::Work,
            vec![HeaderConfig {
                index: 1,
                mode: HeaderMode::Fixed { pct: 70.0 },
            }],
        );
        assert!(validate(&c, Some(board()))
            .unwrap_err()
            .starts_with("Work: "));
    }

    #[test]
    fn header_names_are_checked() {
        let b = Some(board());
        let mut c = cfg(&[]);
        c.labels.insert(2, "Case fans".into());
        assert_eq!(validate(&c, b), Ok(()));
        for bad in ["", " Case", &"x".repeat(MAX_LABEL + 1)] {
            c.labels.insert(2, bad.into());
            assert!(validate(&c, b).is_err(), "{bad:?}");
        }
        c.labels.clear();
        c.labels.insert(9, "Nowhere".into());
        assert!(validate(&c, b).is_err());
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
        let mut c = c;
        c.labels.insert(2, "Case fans".into());
        c.profiles.insert(Profile::Gaming, vec![]);
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""mode":"curve""#));
        assert!(json.contains(r#""labels":{"2":"Case fans"}"#), "{json}");
        assert!(json.contains(r#""profiles":{"gaming":[]}"#), "{json}");
        assert_eq!(serde_json::from_str::<FanConfig>(&json).unwrap(), c);
        // A file from before pumps, names and profiles still loads.
        let old = r#"{"enabled":false,"headers":[],"emergencyCpuC":95,"emergencyGpuC":90}"#;
        assert_eq!(
            serde_json::from_str::<FanConfig>(old).unwrap(),
            FanConfig::default()
        );
    }
}

//! Benchmark mode (P20): Vyro's own workloads, watched by its own sensors and compared only with
//! this PC's earlier runs. Method v1 is fixed by `docs/benchmark.md`: each area runs a warm-up
//! and 3 measured passes, and a result is the median with its spread. Any change to a workload,
//! a duration or a metric is a new method version; every run keeps its own.
//!
//! The load runs in the caller's process ([`runner::run`]): the app runs it, so quitting Vyro
//! always ends it. Every worker checks for a stop at least every [`STOP_CHECK`], and the runner
//! waits for all of them before it returns.

pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod runner;
pub mod storage;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod win;

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::{SensorSnapshot, Status};

/// The method these results follow.
pub const METHOD_VERSION: u32 = 1;
/// Measured passes per area (after one warm-up).
pub const PASSES: usize = 3;
/// A difference counts as faster or slower only beyond this, or beyond either run's spread.
pub const NOISE_FLOOR: f64 = 0.03;
/// Runs kept in the history store.
pub const KEEP_RUNS: usize = 100;
/// Longest a worker goes without checking for a stop.
pub const STOP_CHECK: Duration = Duration::from_millis(50);
/// No reading for this long stops a run: it can no longer watch the temperatures.
pub const STALE_SECS: f64 = 10.0;
/// Background CPU use above this before the first area marks the run busy, %.
pub const BUSY_PCT: f64 = 10.0;
/// Time spent watching the PC before the first area.
pub const SETTLE_SECS: f64 = 3.0;
/// Temperatures at which a run stops (Health's problem limits), °C, on this many readings in a
/// row.
pub const CPU_STOP_C: f64 = crate::health::CPU_TEMP.1;
pub const GPU_STOP_C: f64 = crate::health::GPU_TEMP.1;
pub const HOT_READINGS: u32 = 2;
/// The storage area's file, and the free space it needs on that drive.
pub const FILE_BYTES: u64 = 1 << 30;
pub const MIN_FREE_BYTES: u64 = 2 << 30;
/// The memory area needs this much RAM available, MB.
pub const MIN_AVAILABLE_MB: f64 = 1536.0;
/// Sustained (Full only): loops and their length.
pub const SUSTAIN_LOOPS: usize = 10;
pub const SUSTAIN_LOOP_SECS: f64 = 60.0;
/// Sustained stability at or above this reads as steady, %.
pub const STEADY_PCT: f64 = 97.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BenchMode {
    #[default]
    Quick,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AreaId {
    CpuSingle,
    CpuMulti,
    Memory,
    Storage,
    GpuCompute,
    Graphics,
    Sustained,
    Efficiency,
    Stability,
}

impl AreaId {
    /// The areas a mode runs, in order (Efficiency and Stability are worked out from them).
    pub fn plan(mode: BenchMode) -> Vec<AreaId> {
        let mut v = vec![
            Self::CpuSingle,
            Self::CpuMulti,
            Self::Memory,
            Self::Storage,
            Self::GpuCompute,
            Self::Graphics,
        ];
        if mode == BenchMode::Full {
            v.push(Self::Sustained);
        }
        v
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::CpuSingle => "CPU single-thread",
            Self::CpuMulti => "CPU multi-thread",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::GpuCompute => "GPU compute",
            Self::Graphics => "Graphics",
            Self::Sustained => "Sustained",
            Self::Efficiency => "Efficiency",
            Self::Stability => "Stability",
        }
    }

    /// Planned length in seconds, for the progress bar and the watchdog.
    pub const fn planned_secs(self) -> f64 {
        match self {
            Self::CpuSingle | Self::CpuMulti => 14.0,
            Self::Memory => 18.0,
            Self::Storage => 40.0,
            Self::GpuCompute => 16.0,
            Self::Graphics => 26.0,
            Self::Sustained => SUSTAIN_LOOPS as f64 * SUSTAIN_LOOP_SECS,
            Self::Efficiency | Self::Stability => 0.0,
        }
    }
}

/// Planned length of a whole run, s.
pub fn planned_secs(mode: BenchMode) -> f64 {
    SETTLE_SECS
        + AreaId::plan(mode)
            .iter()
            .map(|a| a.planned_secs())
            .sum::<f64>()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Better {
    Higher,
    Lower,
    /// Shown, never compared (e.g. multi-thread scaling).
    Shown,
}

/// One measured figure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Metric {
    /// Stable id across runs, e.g. "cpu.single".
    pub key: String,
    pub label: String,
    /// "pts", "GB/s", "ns", "MB/s", "IOPS", "GFLOPS", "fps", "%", "pts/W", "fps/W", "×".
    pub unit: String,
    /// The median of the measured passes (or the worked-out value).
    pub value: f64,
    /// (highest − lowest) ÷ median over the passes; `None` for worked-out values.
    pub spread: Option<f64>,
    /// The measured passes, in order.
    pub passes: Vec<f64>,
    pub better: Better,
}

impl Metric {
    /// From measured passes: the median and the spread.
    pub fn measured(key: &str, label: &str, unit: &str, better: Better, passes: Vec<f64>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            unit: unit.into(),
            value: median(&passes),
            spread: spread(&passes),
            passes,
            better,
        }
    }

    /// A value worked out from others (no passes of its own).
    pub fn derived(key: &str, label: &str, unit: &str, better: Better, value: f64) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            unit: unit.into(),
            value,
            spread: None,
            passes: Vec::new(),
            better,
        }
    }
}

/// Median of the values; 0 when there are none.
pub fn median(v: &[f64]) -> f64 {
    let mut s: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

/// (highest − lowest) ÷ median; `None` with fewer than 2 values or a zero median.
pub fn spread(v: &[f64]) -> Option<f64> {
    if v.len() < 2 {
        return None;
    }
    let m = median(v);
    let hi = v.iter().copied().fold(f64::MIN, f64::max);
    let lo = v.iter().copied().fold(f64::MAX, f64::min);
    (m > 0.0).then(|| (hi - lo) / m)
}

/// What the sensors saw while an area ran.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Watch {
    pub secs: f64,
    pub cpu_temp_max: Option<f64>,
    pub cpu_power_max: Option<f64>,
    pub cpu_power_avg: Option<f64>,
    pub cpu_mhz_avg: Option<f64>,
    pub gpu_temp_max: Option<f64>,
    pub gpu_power_max: Option<f64>,
    pub gpu_power_avg: Option<f64>,
    pub gpu_mhz_avg: Option<f64>,
    /// Seconds with the CPU's thermal or current flag or the GPU's thermal flag; `None` when
    /// nothing reports flags.
    pub throttled_secs: Option<f64>,
    /// Seconds the fans ran at full speed for safety.
    pub full_speed_secs: f64,
}

/// One loop of the Sustained area.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SustainLoop {
    /// CPU points in this loop.
    pub cpu: Option<f64>,
    /// GPU frames per second in this loop.
    pub gpu: Option<f64>,
    pub cpu_mhz: Option<f64>,
    pub gpu_mhz: Option<f64>,
    pub cpu_temp_max: Option<f64>,
    pub gpu_temp_max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AreaResult {
    pub area: AreaId,
    pub metrics: Vec<Metric>,
    pub watch: Option<Watch>,
    /// Why the area has no result (skipped, unsupported, stopped).
    pub missing: Option<String>,
    /// What it ran on, e.g. the GPU's name or the drive.
    pub detail: Option<String>,
    /// Sustained only: each loop.
    #[serde(default)]
    pub loops: Vec<SustainLoop>,
}

impl AreaResult {
    pub fn missing(area: AreaId, why: impl Into<String>) -> Self {
        Self {
            area,
            metrics: Vec::new(),
            watch: None,
            missing: Some(why.into()),
            detail: None,
            loops: Vec::new(),
        }
    }

    pub fn metric(&self, key: &str) -> Option<&Metric> {
        self.metrics.iter().find(|m| m.key == key)
    }
}

/// The setup the run had, for comparing runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BenchSetup {
    pub vyro: String,
    pub cpu: Option<String>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub gpu: Option<String>,
    pub gpu_driver: Option<String>,
    pub ram_gb: Option<f64>,
    pub ram_mts: Option<u32>,
    pub board: Option<String>,
    pub bios: Option<String>,
    pub windows: Option<String>,
    pub power_plan: Option<String>,
    pub profile: Option<String>,
    pub fan_profile: Option<String>,
    /// The drive the storage area ran on.
    pub drive: Option<String>,
    /// Background CPU use before the first area, %.
    pub busy_pct: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum RunStatus {
    #[default]
    Completed,
    /// Stopped before the end (by the owner or a safety rule); see `reason`.
    Stopped,
}

/// Errors found during the run (area 9).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BenchErrors {
    /// Wrong checksums from the CPU, memory and GPU workloads.
    pub calc: u32,
    /// Data read back from the storage file that does not match what was written.
    pub storage: u32,
    /// WHEA hardware errors logged during the run; `None` when the log cannot be read.
    pub hardware: Option<u32>,
    /// Direct3D "device removed" (a driver reset).
    pub gpu_resets: u32,
}

impl BenchErrors {
    pub fn total(&self) -> u32 {
        self.calc + self.storage + self.hardware.unwrap_or(0) + self.gpu_resets
    }
}

/// A finished (or stopped) run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BenchRun {
    /// Store id; 0 before it is kept.
    pub id: i64,
    /// Start, Unix seconds.
    pub ts: i64,
    pub method: u32,
    pub mode: BenchMode,
    pub status: RunStatus,
    /// Why it stopped early.
    pub reason: Option<String>,
    pub secs: f64,
    /// Background CPU use before the start was above [`BUSY_PCT`].
    pub busy: bool,
    /// Things worth knowing about the run (no CPU temperature, storage skipped, …).
    pub notes: Vec<String>,
    pub setup: BenchSetup,
    pub areas: Vec<AreaResult>,
    pub errors: BenchErrors,
}

impl BenchRun {
    pub fn area(&self, id: AreaId) -> Option<&AreaResult> {
        self.areas.iter().find(|a| a.area == id)
    }

    pub fn metrics(&self) -> impl Iterator<Item = (AreaId, &Metric)> {
        self.areas
            .iter()
            .flat_map(|a| a.metrics.iter().map(move |m| (a.area, m)))
    }
}

/// A stored run in the list: the headline metrics without the details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunSummary {
    pub id: i64,
    pub ts: i64,
    pub method: u32,
    pub mode: BenchMode,
    pub status: RunStatus,
    pub busy: bool,
    pub errors: u32,
    /// Every metric's key and value.
    pub metrics: Vec<SummaryMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SummaryMetric {
    pub key: String,
    pub value: f64,
    pub spread: Option<f64>,
}

/// Where a run is, for the progress view.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BenchProgress {
    pub mode: BenchMode,
    /// The area running; `None` while settling and at the end.
    pub area: Option<AreaId>,
    /// e.g. "Pass 2 of 3".
    pub step: String,
    /// The whole run, 0–1.
    pub done: f64,
    pub elapsed: f64,
    pub planned: f64,
    /// Areas finished so far.
    pub finished: Vec<AreaResult>,
}

/// What stands in the way of a run, and what it will note.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Preflight {
    /// Reasons the run cannot start now.
    pub blockers: Vec<String>,
    /// Things the result will note.
    pub notes: Vec<String>,
    /// The GPU the GPU areas will use.
    pub gpu: Option<String>,
    /// The drive and folder the storage area will use.
    pub drive: Option<String>,
    pub quick_secs: f64,
    pub full_secs: f64,
}

/// One reading of what the runner watches, taken from a snapshot.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    /// When it arrived.
    pub at: Option<Instant>,
    pub cpu_temp: Option<f64>,
    pub cpu_power: Option<f64>,
    pub cpu_mhz: Option<f64>,
    pub cpu_usage: Option<f64>,
    /// Thermal or current limiting; `None` when the CPU reports no flags.
    pub cpu_throttle: Option<bool>,
    pub gpus: Vec<GpuReading>,
    pub full_speed: bool,
    /// Health problems with the CPU, GPU, fans or board thermals.
    pub problems: Vec<String>,
    /// Temperatures at Health's warning level or above.
    pub warnings: Vec<String>,
    pub game: bool,
    pub available_mb: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuReading {
    pub name: String,
    pub integrated: Option<bool>,
    pub temp: Option<f64>,
    pub power: Option<f64>,
    pub mhz: Option<f64>,
    pub throttle: Option<bool>,
}

impl Sample {
    pub fn from_snapshot(snap: &SensorSnapshot, at: Instant) -> Self {
        let c = &snap.cpu;
        let flags = [c.throttle_thermal, c.throttle_current_limit];
        let cpu_throttle = flags
            .iter()
            .any(Option::is_some)
            .then(|| flags.contains(&Some(true)));
        let watched = ["CPU", "GPU", "Fans", "Board thermals"];
        let problems = snap
            .health
            .subsystems
            .iter()
            .filter(|s| s.status == Status::Fail && watched.contains(&s.name.as_str()))
            .flat_map(|s| {
                if s.messages.is_empty() {
                    vec![format!("{}: problem", s.name)]
                } else {
                    s.messages.clone()
                }
            })
            .collect();
        let mut warnings = Vec::new();
        if let Some(t) = c.temp_package.filter(|t| *t >= crate::health::CPU_TEMP.0) {
            warnings.push(format!("CPU at {t:.0} °C"));
        }
        for g in &snap.gpu {
            if let Some(t) = g.temp_edge.filter(|t| *t >= crate::health::GPU_TEMP.0) {
                warnings.push(format!("{} at {t:.0} °C", g.name));
            }
        }
        Self {
            at: Some(at),
            cpu_temp: c.temp_package,
            cpu_power: c.power_pkg_w,
            cpu_mhz: c.mhz_effective.or(c.mhz_current),
            cpu_usage: c.usage_total,
            cpu_throttle,
            gpus: snap
                .gpu
                .iter()
                .map(|g| GpuReading {
                    name: g.name.clone(),
                    integrated: g.integrated,
                    temp: g.temp_edge,
                    power: g.power_w,
                    mhz: g.mhz_core,
                    throttle: g.throttle_thermal,
                })
                .collect(),
            full_speed: snap.fan_control.full_speed,
            problems,
            warnings,
            game: snap.game.is_some(),
            available_mb: snap.memory.available_mb,
        }
    }

    /// The GPU the run watches: the one named `name` if any matches, else the first discrete,
    /// else the first.
    pub fn gpu(&self, name: Option<&str>) -> Option<&GpuReading> {
        let norm = |s: &str| s.to_ascii_lowercase().replace(['(', ')'], "");
        name.and_then(|n| {
            let n = norm(n);
            self.gpus.iter().filter(|g| !g.name.is_empty()).find(|g| {
                let g = norm(&g.name);
                g == n || g.contains(&n) || n.contains(&g)
            })
        })
        .or_else(|| self.gpus.iter().find(|g| g.integrated == Some(false)))
        .or_else(|| self.gpus.first())
    }
}

/// The newest reading, as the app's snapshot sink last left it.
pub trait Probe: Send + Sync {
    fn sample(&self) -> Option<Sample>;
}

/// Whether a run can start, from the newest reading and what the run would use.
pub fn preflight(
    sample: Option<&Sample>,
    running: bool,
    gpu: Option<String>,
    drive: Option<String>,
    free_bytes: Option<u64>,
) -> Preflight {
    let mut p = Preflight {
        gpu,
        drive,
        quick_secs: planned_secs(BenchMode::Quick),
        full_secs: planned_secs(BenchMode::Full),
        ..Preflight::default()
    };
    if running {
        p.blockers.push("A benchmark is already running.".into());
    }
    match sample {
        None => p
            .blockers
            .push("No sensor readings yet: the run could not watch the temperatures.".into()),
        Some(s) => {
            if s.game {
                p.blockers.push("A game is running: close it first.".into());
            }
            for w in &s.warnings {
                p.blockers.push(format!(
                    "{w}, already at Health's warning level: let it cool first."
                ));
            }
            for w in &s.problems {
                p.blockers.push(format!("Health reports a problem: {w}."));
            }
            if s.cpu_temp.is_none() {
                p.notes.push(no_cpu_temp_note());
            }
        }
    }
    if p.gpu.is_none() {
        p.notes
            .push("No Direct3D 11 GPU found: GPU compute, Graphics and the GPU half of Sustained are skipped.".into());
    }
    if let Some(free) = free_bytes.filter(|f| *f < MIN_FREE_BYTES) {
        p.notes.push(storage_skip_note(free));
    }
    p
}

pub(crate) fn no_cpu_temp_note() -> String {
    "CPU temperature is not read (PawnIO): the run cannot stop on CPU heat; the CPU's own \
     protection still applies."
        .into()
}

pub(crate) fn storage_skip_note(free: u64) -> String {
    format!(
        "Storage is skipped: {:.1} GiB free on the drive with the temporary folder, 2 GiB needed.",
        free as f64 / f64::from(1u32 << 30)
    )
}

/// Turns off Windows' power throttling (EcoQoS) for this process while a run goes on, so a
/// minimised Vyro is not slowed down; `false` hands the decision back to Windows.
pub fn full_speed_process(on: bool) {
    #[cfg(windows)]
    win::full_speed_process(on);
    #[cfg(not(windows))]
    let _ = on;
}

/// Free bytes on the drive holding `path`; `None` when unknown.
pub fn free_bytes(path: &std::path::Path) -> Option<u64> {
    #[cfg(windows)]
    return win::free_bytes(path);
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

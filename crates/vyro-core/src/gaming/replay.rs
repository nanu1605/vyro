//! Game session replay (P15): one sample per tick while a game is captured, kept with the
//! session's markers in the history store and read back downsampled for the replay view.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::markers::{Frames, GameEvent};
use super::GameSession;
use crate::snapshot::SensorSnapshot;

/// Most samples a replay is sent with.
pub const MAX_POINTS: usize = 1500;

/// One tick of a captured game.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameSample {
    /// Milliseconds from the session start.
    pub t: i64,
    pub fps: Option<f64>,
    /// The slowest stretch since the previous sample, as a frame rate.
    pub fps_min: Option<f64>,
    pub frame_ms: Option<f64>,
    /// The worst frame since the previous sample.
    pub frame_max_ms: Option<f64>,
    pub cpu_usage: Option<f64>,
    pub gpu_usage: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub gpu_temp: Option<f64>,
    pub cpu_power: Option<f64>,
    pub gpu_power: Option<f64>,
}

/// A session with its samples and markers, for the replay view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameTimeline {
    pub session: GameSession,
    /// Spacing of the samples, ms: the update rate, or the bucket width once downsampled.
    pub step_ms: i64,
    pub samples: Vec<GameSample>,
    pub events: Vec<GameEvent>,
}

/// Readings stored per sample, in column order, with the factor that makes each an integer
/// (0.1 FPS, 0.01 ms, 0.1 %, 0.1 °C, 0.1 W): SQLite keeps small integers in 1–3 bytes where a
/// REAL takes 8.
pub(crate) const COLUMNS: [(&str, f64); 10] = [
    ("fps", 10.0),
    ("fps_min", 10.0),
    ("frame_ms", 100.0),
    ("frame_max_ms", 100.0),
    ("cpu_usage", 10.0),
    ("gpu_usage", 10.0),
    ("cpu_temp", 10.0),
    ("gpu_temp", 10.0),
    ("cpu_power", 10.0),
    ("gpu_power", 10.0),
];

impl GameSample {
    /// This tick's sample: frame figures from the capture, the rest from the snapshot.
    pub fn new(t: i64, frames: &Frames, frame_ms: Option<f64>, snap: &SensorSnapshot) -> Self {
        let gpu = snap.gpu.first();
        Self {
            t,
            fps: frames.fps,
            fps_min: frames.min_fps,
            frame_ms,
            frame_max_ms: frames.worst_ms,
            cpu_usage: snap.cpu.usage_total,
            gpu_usage: gpu.and_then(|g| g.usage),
            cpu_temp: snap.cpu.temp_package,
            gpu_temp: gpu.and_then(|g| g.temp_edge),
            cpu_power: snap.cpu.power_pkg_w,
            gpu_power: gpu.and_then(|g| g.power_w),
        }
    }

    /// The readings in [`COLUMNS`] order.
    pub(crate) fn values(&self) -> [Option<f64>; 10] {
        [
            self.fps,
            self.fps_min,
            self.frame_ms,
            self.frame_max_ms,
            self.cpu_usage,
            self.gpu_usage,
            self.cpu_temp,
            self.gpu_temp,
            self.cpu_power,
            self.gpu_power,
        ]
    }

    pub(crate) fn from_values(t: i64, v: [Option<f64>; 10]) -> Self {
        let [fps, fps_min, frame_ms, frame_max_ms, cpu_usage, gpu_usage, cpu_temp, gpu_temp, cpu_power, gpu_power] =
            v;
        Self {
            t,
            fps,
            fps_min,
            frame_ms,
            frame_max_ms,
            cpu_usage,
            gpu_usage,
            cpu_temp,
            gpu_temp,
            cpu_power,
            gpu_power,
        }
    }
}

/// Stored form of a reading (see [`COLUMNS`]).
pub(crate) fn encode(v: Option<f64>, scale: f64) -> Option<i64> {
    v.filter(|v| v.is_finite())
        .map(|v| (v * scale).round() as i64)
}

/// At most `max` samples, and their spacing in ms. Longer sessions are bucketed: each bucket
/// keeps its slowest stretch (`fps_min`) and worst frame (`frame_max_ms`), so a stutter
/// survives, and averages the rest.
pub fn downsample(samples: Vec<GameSample>, max: usize) -> (Vec<GameSample>, i64) {
    let (Some(first), Some(last)) = (samples.first().copied(), samples.last().copied()) else {
        return (samples, 1000);
    };
    if samples.len() <= max.max(1) {
        let mut gaps: Vec<i64> = samples.windows(2).map(|w| w[1].t - w[0].t).collect();
        gaps.sort_unstable();
        let step = gaps.get(gaps.len() / 2).copied().unwrap_or(1000);
        return (samples, step);
    }
    let max = i64::try_from(max.max(1)).unwrap_or(i64::MAX);
    let width = ((last.t - first.t) / max + 1).max(1);
    let mut out: Vec<GameSample> = Vec::new();
    let mut group: Vec<GameSample> = Vec::new();
    let mut bucket = 0;
    for s in samples {
        let b = (s.t - first.t) / width;
        if b != bucket && !group.is_empty() {
            out.push(merge(first.t + bucket * width, &group));
            group.clear();
        }
        bucket = b;
        group.push(s);
    }
    if !group.is_empty() {
        out.push(merge(first.t + bucket * width, &group));
    }
    (out, width)
}

fn merge(t: i64, group: &[GameSample]) -> GameSample {
    let pick = |f: fn(&GameSample) -> Option<f64>| group.iter().filter_map(f);
    let avg = |f: fn(&GameSample) -> Option<f64>| {
        let (sum, n) = pick(f).fold((0.0, 0u32), |(s, n), v| (s + v, n + 1));
        (n > 0).then(|| sum / f64::from(n))
    };
    GameSample {
        t,
        fps: avg(|s| s.fps),
        fps_min: pick(|s| s.fps_min).reduce(f64::min),
        frame_ms: avg(|s| s.frame_ms),
        frame_max_ms: pick(|s| s.frame_max_ms).reduce(f64::max),
        cpu_usage: avg(|s| s.cpu_usage),
        gpu_usage: avg(|s| s.gpu_usage),
        cpu_temp: avg(|s| s.cpu_temp),
        gpu_temp: avg(|s| s.gpu_temp),
        cpu_power: avg(|s| s.cpu_power),
        gpu_power: avg(|s| s.gpu_power),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(t: i64, fps: f64) -> GameSample {
        GameSample {
            t,
            fps: Some(fps),
            fps_min: Some(fps - 10.0),
            frame_ms: Some(1000.0 / fps),
            frame_max_ms: Some(1000.0 / fps + 2.0),
            gpu_temp: Some(60.0),
            ..GameSample::default()
        }
    }

    #[test]
    fn short_sessions_pass_through_with_their_tick() {
        let s: Vec<_> = (0..10).map(|i| sample(i * 2000, 100.0)).collect();
        let (out, step) = downsample(s.clone(), MAX_POINTS);
        assert_eq!((out, step), (s, 2000));
        assert_eq!(downsample(Vec::new(), 10), (Vec::new(), 1000));
    }

    #[test]
    fn downsampling_keeps_the_extremes() {
        // Two hours at 2 Hz, one hitch and one slow stretch.
        let mut s: Vec<_> = (0..14_400).map(|i| sample(i * 500, 140.0)).collect();
        s[9001].frame_max_ms = Some(212.5);
        s[3000].fps_min = Some(41.0);
        s[5000].gpu_temp = None;
        let (out, step) = downsample(s, MAX_POINTS);
        assert!(out.len() <= MAX_POINTS, "{}", out.len());
        assert!(out.len() > MAX_POINTS - 10);
        assert_eq!(step, 4800);
        assert!(out.windows(2).all(|w| w[1].t - w[0].t == step));
        let worst = out
            .iter()
            .filter_map(|p| p.frame_max_ms)
            .fold(0.0, f64::max);
        assert_eq!(worst, 212.5);
        let slowest = out
            .iter()
            .filter_map(|p| p.fps_min)
            .fold(f64::MAX, f64::min);
        assert_eq!(slowest, 41.0);
        assert!(out
            .iter()
            .all(|p| p.fps == Some(140.0) && p.gpu_temp == Some(60.0)));
    }

    #[test]
    fn readings_round_trip_through_their_stored_form() {
        let s = GameSample {
            t: 1500,
            fps: Some(141.63),
            frame_ms: Some(7.0612),
            cpu_temp: Some(71.25),
            ..GameSample::default()
        };
        let stored: Vec<Option<i64>> = s
            .values()
            .iter()
            .zip(COLUMNS)
            .map(|(v, (_, scale))| encode(*v, scale))
            .collect();
        assert_eq!(stored[0], Some(1416));
        assert_eq!(stored[2], Some(706));
        assert_eq!(stored[1], None);
        assert_eq!(encode(Some(f64::NAN), 10.0), None);
    }
}

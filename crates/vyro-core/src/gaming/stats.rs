//! Frame statistics. FPS is the frame count over the summed frame times; the 1 % and 0.1 % lows
//! are the frame rates at the 99th and 99.9th percentile frame times (as PresentMon reports
//! them).

use std::collections::VecDeque;

use super::csv::Frame;
use super::{Bound, FramePoint};

/// Averages (FPS, busy times, latency) cover the last second.
pub const AVG_MS: f64 = 1_000.0;
/// Lows cover the last ten seconds, so one hitch stays visible for a while.
pub const LOWS_MS: f64 = 10_000.0;
/// Frame-time graph: the last ten seconds in 100 ms buckets, aligned to the capture clock so a
/// bucket keeps its value as the graph scrolls.
pub const BUCKET_MS: f64 = 100.0;
pub const TIMELINE_LEN: usize = 100;
/// A busy share of the frame time at or above this marks the limiting side.
const BOUND_SHARE: f64 = 0.85;

#[derive(Debug, Clone, Copy)]
struct Sample {
    start: f64,
    frame: f64,
    cpu: Option<f64>,
    gpu: Option<f64>,
    latency: Option<f64>,
    displayed: bool,
}

/// Rolling window of recent frames.
#[derive(Debug, Default)]
pub struct Window {
    samples: VecDeque<Sample>,
}

/// Figures over the rolling window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Live {
    pub fps: Option<f64>,
    pub frame_ms: Option<f64>,
    pub low1_fps: Option<f64>,
    pub low01_fps: Option<f64>,
    pub cpu_busy_ms: Option<f64>,
    pub gpu_busy_ms: Option<f64>,
    pub latency_ms: Option<f64>,
    pub displayed_fps: Option<f64>,
    pub bound: Option<Bound>,
    /// Oldest first; `None` for a bucket without frames.
    pub timeline: Vec<Option<FramePoint>>,
}

impl Window {
    pub fn push(&mut self, f: &Frame) {
        // A new capture restarts the clock: start over.
        if self.samples.back().is_some_and(|s| f.start_ms < s.start) {
            self.samples.clear();
        }
        self.samples.push_back(Sample {
            start: f.start_ms,
            frame: f.frame_ms,
            cpu: f.cpu_busy_ms,
            gpu: f.gpu_busy_ms,
            latency: f.display_latency_ms,
            displayed: f.displayed_ms.is_some(),
        });
        // A bucket more than the lows need, so the oldest graph bucket is whole.
        let oldest = f.start_ms - LOWS_MS - BUCKET_MS;
        while self.samples.front().is_some_and(|s| s.start < oldest) {
            self.samples.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn live(&self) -> Live {
        let Some(last) = self.samples.back() else {
            return Live::default();
        };
        let since = last.start - AVG_MS;
        let recent: Vec<&Sample> = self.samples.iter().filter(|s| s.start > since).collect();
        let total: f64 = recent.iter().map(|s| s.frame).sum();
        let n = recent.len() as f64;
        let rate = |count: f64| (total > 0.0).then(|| count * 1000.0 / total);
        let mean = |f: fn(&Sample) -> Option<f64>| {
            let v: Vec<f64> = recent.iter().filter_map(|s| f(s)).collect();
            (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
        };
        let frame_ms = (n > 0.0).then(|| total / n);
        let cpu = mean(|s| s.cpu);
        let gpu = mean(|s| s.gpu);
        let lows_from = last.start - LOWS_MS;
        let mut all: Vec<f64> = self
            .samples
            .iter()
            .filter(|s| s.start >= lows_from)
            .map(|s| s.frame)
            .collect();
        all.sort_by(f64::total_cmp);
        Live {
            fps: rate(n),
            frame_ms,
            low1_fps: fps_at(percentile(&all, 99.0)),
            low01_fps: fps_at(percentile(&all, 99.9)),
            cpu_busy_ms: cpu,
            gpu_busy_ms: gpu,
            latency_ms: mean(|s| s.latency),
            displayed_fps: rate(recent.iter().filter(|s| s.displayed).count() as f64),
            bound: bound(frame_ms, cpu, gpu),
            timeline: self.timeline(last.start),
        }
    }

    /// Average and worst frame time per complete bucket; the bucket still filling is left out.
    fn timeline(&self, now: f64) -> Vec<Option<FramePoint>> {
        let end = (now / BUCKET_MS).floor() as i64;
        let first = end - TIMELINE_LEN as i64;
        let mut sums = vec![(0.0f64, 0u32, 0.0f64); TIMELINE_LEN];
        for s in &self.samples {
            let k = (s.start / BUCKET_MS).floor() as i64;
            if k >= first && k < end {
                let b = &mut sums[(k - first) as usize];
                b.0 += s.frame;
                b.1 += 1;
                b.2 = b.2.max(s.frame);
            }
        }
        sums.into_iter()
            .map(|(sum, n, max)| {
                (n > 0).then(|| FramePoint {
                    avg_ms: (sum / f64::from(n)) as f32,
                    max_ms: max as f32,
                })
            })
            .collect()
    }
}

/// Nearest-rank percentile of sorted values.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.clamp(1, sorted.len()) - 1).copied()
}

fn fps_at(ms: Option<f64>) -> Option<f64> {
    ms.filter(|&m| m > 0.0).map(|m| 1000.0 / m)
}

/// What limits the frame rate: the side that is busy for (nearly) the whole frame, or a cap
/// (V-Sync, a limiter) when neither is.
pub fn bound(frame: Option<f64>, cpu: Option<f64>, gpu: Option<f64>) -> Option<Bound> {
    let frame = frame.filter(|&f| f > 0.0)?;
    let share = |v: Option<f64>| v.map(|v| v / frame);
    match (share(cpu), share(gpu)) {
        (c, Some(g)) if g >= BOUND_SHARE && g >= c.unwrap_or(0.0) => Some(Bound::Gpu),
        (Some(c), _) if c >= BOUND_SHARE => Some(Bound::Cpu),
        (Some(_), Some(_)) => Some(Bound::Capped),
        _ => None,
    }
}

/// Whole-session totals: frame times in a 0.1 ms histogram (up to 1 s), so a long session
/// costs a fixed 40 KB and still gives exact-enough lows.
#[derive(Debug, Clone)]
pub struct Totals {
    bins: Vec<u32>,
    pub frames: u64,
    pub total_ms: f64,
    cpu: (f64, u64),
    gpu: (f64, u64),
    pub max_frame_ms: f64,
}

const BIN_MS: f64 = 0.1;
const BINS: usize = 10_000;

impl Default for Totals {
    fn default() -> Self {
        Self {
            bins: vec![0; BINS + 1],
            frames: 0,
            total_ms: 0.0,
            cpu: (0.0, 0),
            gpu: (0.0, 0),
            max_frame_ms: 0.0,
        }
    }
}

impl Totals {
    pub fn push(&mut self, f: &Frame) {
        let bin = ((f.frame_ms / BIN_MS) as usize).min(BINS);
        self.bins[bin] += 1;
        self.frames += 1;
        self.total_ms += f.frame_ms;
        self.max_frame_ms = self.max_frame_ms.max(f.frame_ms);
        if let Some(c) = f.cpu_busy_ms {
            self.cpu.0 += c;
            self.cpu.1 += 1;
        }
        if let Some(g) = f.gpu_busy_ms {
            self.gpu.0 += g;
            self.gpu.1 += 1;
        }
    }

    pub fn fps(&self) -> Option<f64> {
        (self.total_ms > 0.0).then(|| self.frames as f64 * 1000.0 / self.total_ms)
    }

    /// Frame rate at the `p`th percentile frame time (upper edge of its bin).
    pub fn low(&self, p: f64) -> Option<f64> {
        if self.frames == 0 {
            return None;
        }
        let rank = ((p / 100.0) * self.frames as f64).ceil().max(1.0) as u64;
        let mut seen = 0;
        for (i, &n) in self.bins.iter().enumerate() {
            seen += u64::from(n);
            if seen >= rank {
                let ms = if i == BINS {
                    self.max_frame_ms
                } else {
                    (i + 1) as f64 * BIN_MS
                };
                return fps_at(Some(ms));
            }
        }
        None
    }

    pub fn bound(&self) -> Option<Bound> {
        let avg = |(sum, n): (f64, u64)| (n > 0).then(|| sum / n as f64);
        let frame = (self.frames > 0).then(|| self.total_ms / self.frames as f64);
        bound(frame, avg(self.cpu), avg(self.gpu))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(start: f64, ms: f64) -> Frame {
        Frame {
            app: "g.exe".into(),
            pid: 1,
            runtime: "DXGI".into(),
            present_mode: None,
            start_ms: start,
            frame_ms: ms,
            cpu_busy_ms: Some(ms * 0.4),
            gpu_busy_ms: Some(ms * 0.95),
            display_latency_ms: Some(10.0),
            displayed_ms: Some(ms),
        }
    }

    /// `n` frames of `ms` each, then one hitch of `hitch` ms.
    fn run(w: &mut Window, t: &mut Totals, n: usize, ms: f64, hitch: f64) {
        let mut at = 0.0;
        for _ in 0..n {
            let f = frame(at, ms);
            w.push(&f);
            t.push(&f);
            at += ms;
        }
        let f = frame(at, hitch);
        w.push(&f);
        t.push(&f);
    }

    #[test]
    fn steady_144_with_a_hitch() {
        let (mut w, mut t) = (Window::default(), Totals::default());
        run(&mut w, &mut t, 1439, 1000.0 / 144.0, 50.0);
        let l = w.live();
        // The last second holds the hitch: 144 frames over ~1.04 s.
        let fps = l.fps.unwrap();
        assert!((130.0..144.0).contains(&fps), "{fps}");
        // 1440 frames in the window: the 99th percentile is still a normal frame…
        assert!((l.low1_fps.unwrap() - 144.0).abs() < 0.5);
        // …the 99.9th (rank 1439 of 1440) too; only the max is the hitch.
        assert!(l.low01_fps.unwrap() > 140.0);
        assert_eq!(l.bound, Some(Bound::Gpu));
        assert_eq!(l.timeline.len(), TIMELINE_LEN);
        // The hitch starts the bucket still filling, so it is not drawn yet.
        let last = l.timeline.last().unwrap().unwrap();
        assert!((last.avg_ms - 1000.0 / 144.0).abs() < 0.01);
        assert!(l.timeline.iter().flatten().all(|p| p.max_ms < 7.0));
        assert_eq!(t.frames, 1440);
        assert!(
            (t.fps().unwrap() - 1440.0 * 1000.0 / (1439.0 * 1000.0 / 144.0 + 50.0)).abs() < 1e-6
        );
        assert!(
            (t.low(99.0).unwrap() - 1000.0 / 7.0).abs() < 1.0,
            "bin edge 7.0 ms"
        );
        assert!((t.low(99.99).unwrap() - 20.0).abs() < 0.1, "the hitch");
        assert_eq!(t.bound(), Some(Bound::Gpu));
    }

    #[test]
    fn stutter_pulls_the_lows_down() {
        let mut w = Window::default();
        let mut at = 0.0;
        for i in 0..600 {
            let ms = if i % 50 == 0 { 40.0 } else { 10.0 };
            w.push(&frame(at, ms));
            at += ms;
        }
        let l = w.live();
        assert!((l.low1_fps.unwrap() - 25.0).abs() < 1e-9);
        assert!(l.fps.unwrap() > 90.0);
    }

    #[test]
    fn old_frames_leave_and_a_restart_clears() {
        let mut w = Window::default();
        w.push(&frame(0.0, 10.0));
        w.push(&frame(20_000.0, 10.0));
        assert_eq!(w.samples.len(), 1);
        w.push(&frame(5.0, 10.0));
        assert_eq!(w.samples.len(), 1, "clock went back: new capture");
        assert!(Window::default().live().fps.is_none());
    }

    #[test]
    fn timeline_buckets_average_and_keep_the_worst_frame() {
        let mut w = Window::default();
        let mut at = 0.0;
        // 2.5 ms frames for 3 s, one 30 ms hitch at 1.5 s.
        while at < 3_000.0 {
            let ms = if (1_500.0..1_502.5).contains(&at) {
                30.0
            } else {
                2.5
            };
            w.push(&frame(at, ms));
            at += ms;
        }
        let t = w.live().timeline;
        assert_eq!(t.len(), TIMELINE_LEN);
        // 3 s of capture: the 29 complete buckets are the newest; older ones are empty.
        let filled: Vec<FramePoint> = t.iter().flatten().copied().collect();
        assert_eq!(filled.len(), 29);
        assert!(t[..TIMELINE_LEN - 29].iter().all(Option::is_none));
        let hitch = filled.iter().filter(|p| p.max_ms == 30.0).count();
        assert_eq!(hitch, 1);
        assert!(filled.iter().all(|p| p.avg_ms >= 2.5 && p.avg_ms < 4.0));
    }

    #[test]
    fn bound_rules() {
        assert_eq!(bound(Some(10.0), Some(9.5), Some(5.0)), Some(Bound::Cpu));
        assert_eq!(bound(Some(10.0), Some(9.5), Some(9.8)), Some(Bound::Gpu));
        assert_eq!(bound(Some(16.7), Some(4.0), Some(6.0)), Some(Bound::Capped));
        assert_eq!(bound(Some(10.0), None, None), None);
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0], 50.0), Some(2.0));
        assert_eq!(percentile(&[], 99.0), None);
    }
}

//! Overlay 2.0 (P14): what the companion overlay shows and how much room its window needs. Pure:
//! the app places the window and fills the rows from each snapshot.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Bound, GameSnapshot};
use crate::snapshot::SensorSnapshot;

/// How much the overlay shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum OverlayMode {
    /// The frame rate on one line.
    Minimal,
    /// Frame rate, 1 % low, CPU and GPU use and temperature.
    #[default]
    Performance,
    /// Everything: frame time, power, VRAM, latency, what limits the frame rate.
    Full,
    /// Frame rate over a large frame-time graph.
    Graph,
}

/// One reading the overlay can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum OverlayMetric {
    Fps,
    Low1,
    Low01,
    FrameTime,
    Latency,
    CpuUsage,
    CpuTemp,
    CpuPower,
    GpuUsage,
    GpuTemp,
    GpuPower,
    Vram,
    Bound,
}

impl OverlayMetric {
    pub const ALL: [Self; 13] = [
        Self::Fps,
        Self::Low1,
        Self::Low01,
        Self::FrameTime,
        Self::Latency,
        Self::CpuUsage,
        Self::CpuTemp,
        Self::CpuPower,
        Self::GpuUsage,
        Self::GpuTemp,
        Self::GpuPower,
        Self::Vram,
        Self::Bound,
    ];

    /// Short label shown next to the value.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fps => "FPS",
            Self::Low1 => "1% low",
            Self::Low01 => "0.1% low",
            Self::FrameTime => "Frame",
            Self::Latency => "Latency",
            Self::CpuUsage => "CPU",
            Self::CpuTemp => "CPU temp",
            Self::CpuPower => "CPU power",
            Self::GpuUsage => "GPU",
            Self::GpuTemp => "GPU temp",
            Self::GpuPower => "GPU power",
            Self::Vram => "VRAM",
            Self::Bound => "Limit",
        }
    }
}

/// The readings a mode shows unless the owner picked their own.
pub fn default_metrics(mode: OverlayMode) -> &'static [OverlayMetric] {
    use OverlayMetric as M;
    match mode {
        OverlayMode::Minimal => &[M::Fps],
        OverlayMode::Performance => &[
            M::Fps,
            M::Low1,
            M::CpuUsage,
            M::CpuTemp,
            M::GpuUsage,
            M::GpuTemp,
        ],
        OverlayMode::Full => &M::ALL,
        OverlayMode::Graph => &[M::Fps, M::Low1],
    }
}

/// The readings shown: `custom` (in the fixed order of [`OverlayMetric::ALL`], each once) when
/// the owner picked some, else the mode's.
pub fn metrics(mode: OverlayMode, custom: &[OverlayMetric]) -> Vec<OverlayMetric> {
    if custom.is_empty() {
        return default_metrics(mode).to_vec();
    }
    OverlayMetric::ALL
        .into_iter()
        .filter(|m| custom.contains(m))
        .collect()
}

/// One reading as the overlay shows it. Values keep the engine's units (°C, W, MB); the page
/// formats them (temperatures in the owner's unit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OverlayRow {
    pub metric: OverlayMetric,
    pub label: String,
    pub value: Option<f64>,
    /// What limits the frame rate ("GPU", "CPU", "Capped").
    pub text: Option<String>,
}

/// The rows for `shown`, from this tick's snapshot and game. The frame figures are empty while
/// the game is paused (no frames).
pub fn rows(
    shown: &[OverlayMetric],
    snap: &SensorSnapshot,
    game: &GameSnapshot,
) -> Vec<OverlayRow> {
    use OverlayMetric as M;
    let live = game.state == super::CaptureState::Capturing;
    let frames = |v: Option<f64>| v.filter(|_| live);
    let gpu = snap.gpu.first();
    shown
        .iter()
        .map(|&metric| {
            let value = match metric {
                M::Fps => frames(game.fps),
                M::Low1 => frames(game.low1_fps),
                M::Low01 => frames(game.low01_fps),
                M::FrameTime => frames(game.frame_ms),
                M::Latency => frames(game.latency_ms),
                M::CpuUsage => snap.cpu.usage_total,
                M::CpuTemp => snap.cpu.temp_package,
                M::CpuPower => snap.cpu.power_pkg_w,
                M::GpuUsage => gpu.and_then(|g| g.usage),
                M::GpuTemp => gpu.and_then(|g| g.temp_edge),
                M::GpuPower => gpu.and_then(|g| g.power_w),
                M::Vram => gpu.and_then(|g| g.vram_used_mb),
                M::Bound => None,
            };
            let text = (metric == M::Bound)
                .then(|| game.bound.filter(|_| live))
                .flatten()
                .map(|b| {
                    match b {
                        Bound::Gpu => "GPU",
                        Bound::Cpu => "CPU",
                        Bound::Capped => "Capped",
                    }
                    .to_owned()
                });
            OverlayRow {
                metric,
                label: metric.label().to_owned(),
                value: value.filter(|v| v.is_finite()),
                text,
            }
        })
        .collect()
}

/// Scale limits, percent.
pub const SCALE_MIN: u16 = 75;
pub const SCALE_MAX: u16 = 200;

/// Room the window needs, logical pixels, at least as large as the plate the page draws (the
/// window is transparent and click-through, and the page pins the plate to the window's corner
/// on the screen's corner, so spare room is invisible). Frame rate and 1 % low sit in the
/// header; the other readings take two columns (one line in Minimal mode).
pub fn size(
    mode: OverlayMode,
    shown: &[OverlayMetric],
    graph: bool,
    compact: bool,
    scale_pct: u16,
) -> (f64, f64) {
    let rest = shown
        .iter()
        .filter(|m| !matches!(m, OverlayMetric::Fps | OverlayMetric::Low1))
        .count();
    let (row, pad) = if compact { (17.0, 10.0) } else { (21.0, 16.0) };
    let (w, h) = match mode {
        OverlayMode::Minimal => {
            // The frame rate, then each other reading, wrapping past 744 px (the plate's widest).
            let w = 130.0 + 111.0 * (shown.len().saturating_sub(1)) as f64;
            let lines = (w / 744.0).ceil().max(1.0);
            let line = if compact { 38.0 } else { 46.0 };
            (w.min(760.0), line + (lines - 1.0) * 20.0)
        }
        OverlayMode::Graph => {
            let rows = rest.div_ceil(2) as f64;
            (320.0, 52.0 + 120.0 + rows * row + pad)
        }
        OverlayMode::Performance | OverlayMode::Full => {
            let rows = rest.div_ceil(2) as f64;
            let graph_h = if graph { 52.0 } else { 0.0 };
            // Full is wider: its two columns hold the longer labels ("GPU power").
            let w = match (mode, compact) {
                (OverlayMode::Full, false) => 300.0,
                (OverlayMode::Full, true) => 260.0,
                (_, false) => 268.0,
                (_, true) => 236.0,
            };
            (w, 52.0 + rows * row + graph_h + pad)
        }
    };
    let s = f64::from(scale_pct.clamp(SCALE_MIN, SCALE_MAX)) / 100.0;
    ((w * s).ceil(), (h * s).ceil())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_have_their_readings_and_custom_lists_keep_a_fixed_order() {
        assert_eq!(default_metrics(OverlayMode::Minimal), &[OverlayMetric::Fps]);
        assert_eq!(default_metrics(OverlayMode::Full).len(), 13);
        let picked = metrics(
            OverlayMode::Minimal,
            &[
                OverlayMetric::GpuTemp,
                OverlayMetric::Fps,
                OverlayMetric::GpuTemp,
            ],
        );
        assert_eq!(picked, vec![OverlayMetric::Fps, OverlayMetric::GpuTemp]);
        assert_eq!(
            metrics(OverlayMode::Graph, &[]),
            vec![OverlayMetric::Fps, OverlayMetric::Low1]
        );
    }

    #[test]
    fn the_window_grows_with_what_it_shows() {
        let perf = metrics(OverlayMode::Performance, &[]);
        let full = metrics(OverlayMode::Full, &[]);
        let (pw, ph) = size(OverlayMode::Performance, &perf, false, false, 100);
        let (fw, fh) = size(OverlayMode::Full, &full, false, false, 100);
        assert!(fw > pw && fh > ph, "wider columns, more rows");
        let (_, with_graph) = size(OverlayMode::Performance, &perf, true, false, 100);
        assert!(with_graph > ph);
        let (cw, ch) = size(OverlayMode::Performance, &perf, false, true, 100);
        assert!(cw < pw && ch < ph, "compact is smaller");
        let (mw, mh) = size(
            OverlayMode::Minimal,
            &[OverlayMetric::Fps],
            false,
            false,
            100,
        );
        assert!(mw < pw && mh < ph);
        let (gw, gh) = size(
            OverlayMode::Graph,
            &metrics(OverlayMode::Graph, &[]),
            true,
            false,
            100,
        );
        assert!(gw > pw && gh > ph);
    }

    #[test]
    fn same_numbers_as_the_window_page() {
        // src/overlay.test.ts checks its mirror against these.
        let perf = metrics(OverlayMode::Performance, &[]);
        assert_eq!(
            size(
                OverlayMode::Minimal,
                &[OverlayMetric::Fps],
                false,
                false,
                100
            ),
            (130.0, 46.0)
        );
        assert_eq!(
            size(OverlayMode::Performance, &perf, true, false, 100),
            (268.0, 162.0)
        );
        assert_eq!(
            size(OverlayMode::Performance, &perf, false, true, 150),
            (354.0, 144.0)
        );
    }

    #[test]
    fn minimal_wraps_onto_more_lines() {
        let all = metrics(OverlayMode::Full, &[]);
        assert_eq!(
            size(OverlayMode::Minimal, &all[..3], false, false, 100),
            (352.0, 46.0)
        );
        assert_eq!(
            size(OverlayMode::Minimal, &all, false, false, 100),
            (760.0, 66.0)
        );
        assert_eq!(
            size(OverlayMode::Minimal, &all, false, true, 100),
            (760.0, 58.0)
        );
    }

    #[test]
    fn scale_multiplies_and_is_bounded() {
        let m = [OverlayMetric::Fps];
        let (w1, h1) = size(OverlayMode::Minimal, &m, false, false, 100);
        let (w2, h2) = size(OverlayMode::Minimal, &m, false, false, 200);
        assert_eq!((w2, h2), (w1 * 2.0, h1 * 2.0));
        assert_eq!(
            size(OverlayMode::Minimal, &m, false, false, 900),
            (w2, h2),
            "capped at 200 %"
        );
        assert_eq!(
            size(OverlayMode::Minimal, &m, false, false, 10),
            size(OverlayMode::Minimal, &m, false, false, 75)
        );
    }

    #[test]
    fn rows_follow_the_snapshot_and_go_blank_while_paused() {
        use crate::gaming::CaptureState;
        let mut snap = SensorSnapshot::default();
        snap.cpu.temp_package = Some(71.5);
        snap.cpu.power_pkg_w = Some(88.0);
        let mut game = GameSnapshot {
            state: CaptureState::Capturing,
            detail: None,
            app: "Game.exe".into(),
            pid: 1,
            runtime: None,
            present_mode: None,
            fps: Some(141.6),
            frame_ms: Some(7.1),
            low1_fps: Some(117.0),
            low01_fps: None,
            cpu_busy_ms: None,
            gpu_busy_ms: None,
            latency_ms: None,
            displayed_fps: None,
            bound: Some(Bound::Gpu),
            started: 0,
            frames: 10,
            session_fps: None,
            session_low1_fps: None,
            timeline: Vec::new(),
        };
        let shown = [
            OverlayMetric::Fps,
            OverlayMetric::CpuTemp,
            OverlayMetric::CpuPower,
            OverlayMetric::GpuTemp,
            OverlayMetric::Bound,
        ];
        let r = rows(&shown, &snap, &game);
        assert_eq!(r[0].value, Some(141.6));
        assert_eq!((r[1].value, r[1].label.as_str()), (Some(71.5), "CPU temp"));
        assert_eq!(r[2].value, Some(88.0));
        assert_eq!(r[3].value, None, "no GPU");
        assert_eq!(r[4].text.as_deref(), Some("GPU"));
        game.state = CaptureState::Idle;
        let r = rows(&shown, &snap, &game);
        assert_eq!(r[0].value, None, "paused: no frame rate");
        assert_eq!(r[1].value, Some(71.5), "sensors still read");
        assert_eq!(r[4].text, None);
    }

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_string(&OverlayMode::Graph).unwrap(),
            r#""graph""#
        );
        assert_eq!(
            serde_json::to_string(&OverlayMetric::FrameTime).unwrap(),
            r#""frameTime""#
        );
    }
}

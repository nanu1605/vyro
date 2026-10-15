//! Moments worth marking in a game session's replay (P15): frame-rate drops and stutters,
//! thermal and power limits, other clock limits, and what the player did (switched away and
//! back, changed profile, showed or hid the overlay). Pure: [`super::Gaming`] feeds it one tick
//! at a time.

use std::collections::{HashMap, VecDeque};
use std::mem::{discriminant, Discriminant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{FramePoint, GameContext};
use crate::profiles::Profile;
use crate::snapshot::SensorSnapshot;

/// Markers kept per kind and session, so a game that stutters all session long still leaves
/// room for the rest.
pub const MAX_PER_KIND: usize = 200;
/// The frame rate a drop is measured against: the median over this long…
const WINDOW_MS: i64 = 30_000;
/// …once there is at least this much of it.
const WARMUP_MS: i64 = 10_000;
/// A drop: below this share of the median frame rate…
const DROP: f64 = 0.6;
/// …until back above this share.
const RECOVER: f64 = 0.8;
/// A stutter: a 100 ms stretch whose worst frame is this many times the median frame time…
const STUTTER_RATIO: f64 = 3.0;
/// …and at least this long (a 12 ms frame at 300 FPS goes unnoticed).
const STUTTER_MIN_MS: f64 = 25.0;
/// The same kind of frame-rate moment again within this time is the same moment.
const DEBOUNCE_MS: i64 = 10_000;
/// A limit must have been off this long to be marked again (a GPU riding its power limit
/// flickers in and out of it).
const REARM_MS: i64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Part {
    Cpu,
    Gpu,
}

/// What happened at a moment of the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum Marker {
    /// The frame rate fell below 60 % of the last half-minute's median (`usual`).
    FpsDrop {
        fps: f64,
        usual: f64,
    },
    /// A frame took `ms`, against a median frame time of `usual`.
    Stutter {
        ms: f64,
        usual: f64,
    },
    /// Clocks held back by temperature.
    Thermal {
        part: Part,
    },
    /// Clocks held back by the power limit.
    PowerLimit {
        part: Part,
    },
    /// Another clock limit (CPU current limit, GPU hardware slowdown).
    Throttle {
        part: Part,
        reason: String,
    },
    /// The player switched away from the game…
    Left,
    /// …and back to it.
    Back,
    /// The profile in force changed (`auto`: the game chose it).
    Profile {
        profile: Profile,
        auto: bool,
    },
    Overlay {
        shown: bool,
    },
}

/// One marker on the session's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GameEvent {
    /// Milliseconds from the session start.
    pub t: i64,
    pub marker: Marker,
}

/// This tick's frame figures, from the 100 ms timeline.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frames {
    pub fps: Option<f64>,
    /// The slowest 100 ms stretch since the previous tick, as a frame rate.
    pub min_fps: Option<f64>,
    /// The worst frame since the previous tick, ms.
    pub worst_ms: Option<f64>,
    /// Median frame time over the timeline (the last 15 s), ms.
    pub median_ms: Option<f64>,
}

impl Frames {
    /// `since_ms`: time since the previous tick; the stretches it covers are the newest ones.
    pub fn new(fps: Option<f64>, timeline: &[Option<FramePoint>], since_ms: i64) -> Self {
        let n = u64::try_from(since_ms.max(1))
            .map_or(1, |ms| ms.div_ceil(100))
            .try_into()
            .unwrap_or(usize::MAX)
            .min(timeline.len());
        let recent = timeline[timeline.len() - n..].iter().flatten();
        let (slowest, worst) = recent.fold((None::<f64>, None::<f64>), |(a, w), p| {
            let avg = f64::from(p.avg_ms);
            let max = f64::from(p.max_ms);
            (
                Some(a.map_or(avg, |a| a.max(avg))),
                Some(w.map_or(max, |w| w.max(max))),
            )
        });
        let avgs: Vec<f64> = timeline
            .iter()
            .flatten()
            .map(|p| f64::from(p.avg_ms))
            .collect();
        Self {
            fps,
            min_fps: slowest.filter(|ms| *ms > 0.0).map(|ms| 1000.0 / ms),
            worst_ms: worst,
            median_ms: median(avgs),
        }
    }
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[derive(Debug, Clone, Copy)]
struct Limit {
    on: bool,
    off_since: i64,
}

/// Turns ticks into markers: edges, a hysteresis on drops, debouncing, a cap per kind.
#[derive(Debug)]
pub struct Detector {
    fps: VecDeque<(i64, f64)>,
    dropped: bool,
    last_drop: Option<i64>,
    last_stutter: Option<i64>,
    limits: HashMap<String, Limit>,
    context: Option<GameContext>,
    counts: HashMap<Discriminant<Marker>, usize>,
}

impl Detector {
    /// `context`: the app's view when the session starts (changes after it are marked).
    pub fn new(context: Option<GameContext>) -> Self {
        Self {
            fps: VecDeque::new(),
            dropped: false,
            last_drop: None,
            last_stutter: None,
            limits: HashMap::new(),
            context,
            counts: HashMap::new(),
        }
    }

    /// One tick of a captured game at `t` ms from the session start.
    pub fn step(&mut self, t: i64, frames: &Frames, snap: &SensorSnapshot) -> Vec<GameEvent> {
        let mut out = Vec::new();
        if let Some(fps) = frames.fps.filter(|v| v.is_finite() && *v > 0.0) {
            self.fps.push_back((t, fps));
            while self.fps.front().is_some_and(|(at, _)| *at < t - WINDOW_MS) {
                self.fps.pop_front();
            }
            let span = self.fps.front().map_or(0, |(at, _)| t - at);
            if let Some(usual) = median(self.fps.iter().map(|(_, v)| *v).collect()) {
                if self.dropped && fps >= RECOVER * usual {
                    self.dropped = false;
                } else if !self.dropped
                    && span >= WARMUP_MS
                    && fps < DROP * usual
                    && self.last_drop.is_none_or(|at| t - at >= DEBOUNCE_MS)
                {
                    self.dropped = true;
                    self.last_drop = Some(t);
                    out.push(Marker::FpsDrop {
                        fps: round1(fps),
                        usual: round1(usual),
                    });
                }
            }
        }
        if let (Some(worst), Some(usual)) = (frames.worst_ms, frames.median_ms) {
            if worst >= STUTTER_RATIO * usual
                && worst >= STUTTER_MIN_MS
                && self.last_stutter.is_none_or(|at| t - at >= DEBOUNCE_MS)
            {
                self.last_stutter = Some(t);
                out.push(Marker::Stutter {
                    ms: round1(worst),
                    usual: round1(usual),
                });
            }
        }

        let cpu = &snap.cpu;
        let gpu = snap.gpu.first();
        let on = |v: Option<bool>| v == Some(true);
        let mut limits = vec![
            (
                "cpu.thermal".to_owned(),
                on(cpu.throttle_thermal),
                Marker::Thermal { part: Part::Cpu },
            ),
            (
                "cpu.power".to_owned(),
                on(cpu.throttle_power_limit),
                Marker::PowerLimit { part: Part::Cpu },
            ),
            (
                "cpu.current".to_owned(),
                on(cpu.throttle_current_limit),
                Marker::Throttle {
                    part: Part::Cpu,
                    reason: "current limit".into(),
                },
            ),
            (
                "gpu.thermal".to_owned(),
                on(gpu.and_then(|g| g.throttle_thermal)),
                Marker::Thermal { part: Part::Gpu },
            ),
            (
                "gpu.power".to_owned(),
                on(gpu.and_then(|g| g.throttle_power_limit)),
                Marker::PowerLimit { part: Part::Gpu },
            ),
        ];
        // Thermal and power reasons are the flags above; the rest are their own limits.
        let other = |r: &&String| !r.contains("thermal") && !r.contains("power");
        let reasons: Vec<&String> = gpu
            .map(|g| g.throttle_reasons.iter().filter(other).collect())
            .unwrap_or_default();
        for r in &reasons {
            limits.push((
                format!("gpu.{r}"),
                true,
                Marker::Throttle {
                    part: Part::Gpu,
                    reason: (*r).clone(),
                },
            ));
        }
        // Reasons seen before and gone now are off.
        let gone: Vec<String> = self
            .limits
            .keys()
            .filter(|k| k.starts_with("gpu.") && !matches!(k.as_str(), "gpu.thermal" | "gpu.power"))
            .filter(|k| !reasons.iter().any(|r| k.as_str() == format!("gpu.{r}")))
            .cloned()
            .collect();
        for key in gone {
            self.limit(t, &key, false);
        }
        for (key, active, marker) in limits {
            if self.limit(t, &key, active) {
                out.push(marker);
            }
        }
        self.keep(t, out)
    }

    /// The app's view changed: marks switching away and back, the profile and the overlay.
    pub fn context(&mut self, t: i64, context: Option<GameContext>) -> Vec<GameEvent> {
        let mut out = Vec::new();
        if let (Some(old), Some(new)) = (self.context, context) {
            if old.in_front != new.in_front {
                out.push(if new.in_front {
                    Marker::Back
                } else {
                    Marker::Left
                });
            }
            if (old.profile, old.auto) != (new.profile, new.auto) {
                out.push(Marker::Profile {
                    profile: new.profile,
                    auto: new.auto,
                });
            }
            if old.overlay != new.overlay {
                out.push(Marker::Overlay { shown: new.overlay });
            }
        }
        if context.is_some() {
            self.context = context;
        }
        self.keep(t, out)
    }

    /// Whether the limit `key` just came on, having been off long enough.
    fn limit(&mut self, t: i64, key: &str, on: bool) -> bool {
        let l = self.limits.entry(key.to_owned()).or_insert(Limit {
            on: false,
            off_since: i64::MIN / 2,
        });
        let fire = on && !l.on && t - l.off_since >= REARM_MS;
        if l.on && !on {
            l.off_since = t;
        }
        l.on = on;
        fire
    }

    fn keep(&mut self, t: i64, markers: Vec<Marker>) -> Vec<GameEvent> {
        markers
            .into_iter()
            .filter(|m| {
                let n = self.counts.entry(discriminant(m)).or_insert(0);
                *n += 1;
                *n <= MAX_PER_KIND
            })
            .map(|marker| GameEvent { t, marker })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::GpuSnapshot;

    fn steady(ms: f32) -> Vec<Option<FramePoint>> {
        vec![
            Some(FramePoint {
                avg_ms: ms,
                max_ms: ms + 1.0
            });
            150
        ]
    }

    fn snap() -> SensorSnapshot {
        let mut s = SensorSnapshot::default();
        s.gpu.push(GpuSnapshot::default());
        s
    }

    fn kinds(events: &[GameEvent]) -> Vec<&Marker> {
        events.iter().map(|e| &e.marker).collect()
    }

    #[test]
    fn frames_read_the_newest_stretches_and_the_median() {
        let mut tl = steady(10.0);
        tl[140] = Some(FramePoint {
            avg_ms: 20.0,
            max_ms: 80.0,
        });
        tl[149] = None;
        // The last second covers stretch 140.
        let f = Frames::new(Some(100.0), &tl, 1000);
        assert_eq!(f.worst_ms, Some(80.0));
        assert_eq!(f.min_fps, Some(50.0));
        assert_eq!(f.median_ms, Some(10.0));
        // The last half-second does not.
        let f = Frames::new(Some(100.0), &tl, 500);
        assert_eq!(f.worst_ms, Some(11.0));
        assert_eq!(Frames::new(None, &[], 1000), Frames::default());
    }

    #[test]
    fn a_steady_game_has_no_markers_and_a_dip_one() {
        let mut d = Detector::new(None);
        let s = snap();
        let tl = steady(7.0);
        let f = |fps| Frames {
            fps: Some(fps),
            ..Frames::new(None, &tl, 1000)
        };
        for i in 0..20 {
            assert!(d.step(i * 1000, &f(140.0), &s).is_empty(), "tick {i}");
        }
        // Down to 70 FPS for a few seconds: one marker, not one per tick.
        let mut all = Vec::new();
        for i in 20..25 {
            all.extend(d.step(i * 1000, &f(70.0), &s));
        }
        assert_eq!(
            kinds(&all),
            [&Marker::FpsDrop {
                fps: 70.0,
                usual: 140.0
            }]
        );
        // Recovered, then a second dip past the debounce is marked again.
        for i in 25..40 {
            assert!(d.step(i * 1000, &f(140.0), &s).is_empty());
        }
        assert_eq!(d.step(40_000, &f(60.0), &s).len(), 1);
    }

    #[test]
    fn no_drop_before_ten_seconds_of_frames() {
        let mut d = Detector::new(None);
        let s = snap();
        let f = |fps| Frames {
            fps: Some(fps),
            ..Frames::default()
        };
        for i in 0..5 {
            d.step(i * 1000, &f(140.0), &s);
        }
        assert!(d.step(5000, &f(30.0), &s).is_empty());
    }

    #[test]
    fn a_hitch_is_a_stutter_unless_it_is_short() {
        let mut d = Detector::new(None);
        let s = snap();
        let mut tl = steady(7.0);
        tl[149] = Some(FramePoint {
            avg_ms: 9.0,
            max_ms: 60.0,
        });
        let f = Frames::new(Some(140.0), &tl, 1000);
        assert_eq!(
            kinds(&d.step(1000, &f, &s)),
            [&Marker::Stutter {
                ms: 60.0,
                usual: 7.0
            }]
        );
        // Debounced: the next tick's hitch is the same moment.
        assert!(d.step(2000, &f, &s).is_empty());
        assert_eq!(d.step(12_000, &f, &s).len(), 1);
        // At 300 FPS a 12 ms frame is 3.6× the median but too short to notice.
        let mut d = Detector::new(None);
        let mut tl = steady(3.3);
        tl[149] = Some(FramePoint {
            avg_ms: 4.0,
            max_ms: 12.0,
        });
        assert!(d
            .step(1000, &Frames::new(Some(300.0), &tl, 1000), &s)
            .is_empty());
    }

    #[test]
    fn limits_are_marked_on_their_edge_and_rearm_after_thirty_seconds() {
        let mut d = Detector::new(None);
        let f = Frames::default();
        let mut s = snap();
        assert!(d.step(0, &f, &s).is_empty());
        s.gpu[0].throttle_power_limit = Some(true);
        s.gpu[0].throttle_reasons = vec!["power cap".into(), "hardware slowdown".into()];
        s.cpu.throttle_thermal = Some(true);
        let first = d.step(1000, &f, &s);
        assert_eq!(
            kinds(&first),
            [
                &Marker::Thermal { part: Part::Cpu },
                &Marker::PowerLimit { part: Part::Gpu },
                &Marker::Throttle {
                    part: Part::Gpu,
                    reason: "hardware slowdown".into()
                },
            ]
        );
        assert!(d.step(2000, &f, &s).is_empty(), "still on");
        // Flickers off and on: the same limit.
        let off = snap();
        d.step(3000, &f, &off);
        assert!(d.step(4000, &f, &s).is_empty());
        // Off for half a minute: marked again.
        d.step(5000, &f, &off);
        assert_eq!(d.step(36_000, &f, &s).len(), 3);
    }

    #[test]
    fn context_changes_are_marked() {
        let ctx = |in_front, profile, overlay| {
            Some(GameContext {
                in_front,
                profile,
                auto: true,
                overlay,
            })
        };
        let mut d = Detector::new(ctx(true, Profile::Gaming, false));
        assert!(d.context(500, ctx(true, Profile::Gaming, false)).is_empty());
        assert_eq!(
            kinds(&d.context(1000, ctx(false, Profile::Gaming, false))),
            [&Marker::Left]
        );
        let back = d.context(2000, ctx(true, Profile::Silent, true));
        assert_eq!(
            kinds(&back),
            [
                &Marker::Back,
                &Marker::Profile {
                    profile: Profile::Silent,
                    auto: true
                },
                &Marker::Overlay { shown: true },
            ]
        );
        assert_eq!(back[0].t, 2000);
        // The game going away is not a marker.
        assert!(d.context(3000, None).is_empty());
    }

    #[test]
    fn each_kind_is_capped() {
        let ctx = |in_front| {
            Some(GameContext {
                in_front,
                profile: Profile::Gaming,
                auto: true,
                overlay: false,
            })
        };
        let mut d = Detector::new(ctx(true));
        let mut left = 0;
        for i in 0..1000 {
            left += d
                .context(i, ctx(i % 2 == 0))
                .iter()
                .filter(|e| e.marker == Marker::Left)
                .count();
        }
        assert_eq!(left, MAX_PER_KIND);
    }

    #[test]
    fn markers_serialize_with_their_kind() {
        let e = GameEvent {
            t: 1500,
            marker: Marker::PowerLimit { part: Part::Gpu },
        };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"t":1500,"marker":{"kind":"powerLimit","part":"gpu"}}"#
        );
    }
}

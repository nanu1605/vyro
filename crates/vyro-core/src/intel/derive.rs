//! Derived metrics and their baselines (P16, layers 1–2). For the CPU and each GPU: the
//! temperature over the last half hour against what the past week says it runs at for the same
//! load, for the profile in force where there is enough of it, and whether that relation
//! shifted for good (when). For each fan: its speed against its usual at the same CPU
//! temperature. Plus the vital signs shown on top of Insights.

use std::collections::HashMap;

use super::stats::{buckets, change_point, fit, join, mean, median, variation};
use super::{Basis, Inputs, Vital, VitalKind};
use crate::health::{CPU_TEMP, GPU_TEMP};
use crate::profiles::Profile;
use crate::HistoryRange;

/// The last half hour is "now"; baselines leave out the last hour.
pub(super) const RECENT_SECS: i64 = 1800;
pub(super) const BASELINE_GAP_SECS: i64 = 3600;
/// Full load, for throttling, headroom and full-load clocks.
pub(super) const FULL_LOAD: f64 = 85.0;
const HOUR: i64 = 3600;

/// Where a lasting shift began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Onset {
    /// Its cause would lie between these (unix seconds).
    pub lo: i64,
    pub hi: i64,
    /// The first data after the shift.
    pub at: i64,
    /// Found over the week (at least 6 h on each side), not just the last day.
    pub long: bool,
}

/// The CPU's or a GPU's temperature for its load.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TempState {
    /// `None`: the CPU.
    pub gpu: Option<usize>,
    pub label: String,
    pub temp: String,
    pub usage: String,
    /// Last half hour: mean temperature and load.
    pub t: f64,
    pub u: f64,
    /// Usual temperature at that load (before the shift, if there was one).
    pub expected: f64,
    pub sigma: f64,
    pub onset: Option<Onset>,
    /// The baseline is the profile in force (`false`: every profile, though others ran).
    pub mixed: bool,
}

/// A fan's speed for the CPU temperature.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FanState {
    pub id: String,
    pub name: String,
    /// Last half hour: median speed and mean CPU temperature.
    pub rpm: f64,
    pub t: f64,
    /// Usual speed at that temperature (before the shift, if there was one).
    pub usual: f64,
    pub onset: Option<Onset>,
    pub mixed: bool,
}

pub(super) struct State {
    pub cpu: Option<TempState>,
    pub gpus: Vec<TempState>,
    pub fans: Vec<FanState>,
    pub gpu_ids: Vec<usize>,
    /// How far back the week's history reaches.
    pub history_secs: i64,
}

pub(super) fn derive(inp: &Inputs) -> State {
    let gpu_ids = gpu_indices(inp);
    let cpu = temp_state(inp, None, "CPU", "cpu.temp.package", "cpu.usage.total");
    let gpus = gpu_ids
        .iter()
        .filter_map(|&g| {
            let label = if gpu_ids.len() == 1 {
                "GPU".to_owned()
            } else {
                format!("GPU #{g}")
            };
            temp_state(
                inp,
                Some(g),
                &label,
                &format!("gpu.{g}.temp.edge"),
                &format!("gpu.{g}.usage"),
            )
        })
        .collect();
    let mut fan_ids: Vec<&String> = inp.week.keys().filter(|k| k.starts_with("fan.")).collect();
    fan_ids.sort();
    let fans = fan_ids
        .into_iter()
        .filter_map(|f| fan_state(inp, f))
        .collect();
    let oldest = inp
        .week
        .values()
        .filter_map(|s| s.first().map(|p| p.ts))
        .min();
    State {
        cpu,
        gpus,
        fans,
        gpu_ids,
        history_secs: oldest.map_or(0, |o| (inp.now - o).max(0)),
    }
}

fn gpu_indices(inp: &Inputs) -> Vec<usize> {
    let mut v: Vec<usize> = inp
        .week
        .keys()
        .chain(inp.day.keys())
        .filter_map(|k| k.strip_prefix("gpu.")?.split_once('.')?.0.parse().ok())
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The profile most minutes of `[ts, ts + width)` ran with (from the digests).
pub(super) fn bucket_profile(inp: &Inputs, ts: i64, width: i64) -> Option<Profile> {
    let d = inp.digests;
    let start = d.partition_point(|x| x.ts < ts);
    let mut counts: HashMap<Profile, u32> = HashMap::new();
    for x in d[start..].iter().take_while(|x| x.ts < ts + width) {
        if let Some(p) = x.profile {
            *counts.entry(p).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(p, _)| p)
}

/// Keeps the baseline points that ran with the profile in force when there are at least
/// `min` of them. Also says whether other profiles were left in (a mixed baseline).
fn same_profile<T: Copy>(
    inp: &Inputs,
    points: Vec<(i64, T)>,
    width: i64,
    min: usize,
) -> (Vec<(i64, T)>, bool) {
    let Some(now) = inp.profile else {
        return (points, false);
    };
    let tagged: Vec<(Option<Profile>, (i64, T))> = points
        .into_iter()
        .map(|p| (bucket_profile(inp, p.0, width), p))
        .collect();
    let same = tagged.iter().filter(|(p, _)| *p == Some(now)).count();
    let other = tagged
        .iter()
        .filter(|(p, _)| p.is_some_and(|p| p != now))
        .count();
    if same >= min {
        let kept = tagged
            .into_iter()
            .filter(|(p, _)| *p == Some(now))
            .map(|(_, x)| x)
            .collect();
        return (kept, false);
    }
    // Too little of this profile: every profile, and say so when others are a real part.
    let mixed = other * 10 >= tagged.len().max(1);
    (tagged.into_iter().map(|(_, x)| x).collect(), mixed)
}

/// A lasting shift of `(time, residual)`: over the week (hourly means, 6 h on each side) or,
/// failing that, over the last day (3 h before, 30 min after). `up`: a rise, else a fall.
/// Returns where it began and the level before it.
pub(super) fn lasting_shift(
    week: &[(i64, f64)],
    day: &[(i64, f64)],
    up: bool,
    min: f64,
) -> Option<(Onset, f64)> {
    let size = |before: f64, after: f64| if up { after - before } else { before - after };
    // The shift must hold from where it is dated: a split placed too early (a recent shift
    // diluted with hours from before it) fails on the first half of what follows it.
    let holds = |points: &[(i64, f64)], s: &super::stats::Shift| {
        let after: Vec<f64> = points
            .iter()
            .filter(|p| p.0 >= s.first_after)
            .map(|p| p.1)
            .collect();
        let half = mean(after[..after.len().div_ceil(2)].iter().copied());
        size(s.before, s.after) >= min.max(2.5 * s.sigma)
            && half.is_some_and(|h| size(s.before, h) >= min)
    };
    let hourly = buckets(week, HOUR);
    if let Some(s) = change_point(&hourly, HOUR, 6 * HOUR, 6 * HOUR) {
        if holds(&hourly, &s) {
            let onset = Onset {
                lo: s.last_before - 6 * HOUR,
                hi: s.first_after + HOUR,
                at: s.first_after,
                long: true,
            };
            return Some((onset, s.before));
        }
    }
    let step = HistoryRange::H24.step_secs();
    let s = change_point(day, step, 3 * HOUR, RECENT_SECS)?;
    holds(day, &s).then(|| {
        (
            Onset {
                lo: s.last_before - RECENT_SECS,
                hi: s.first_after + 600,
                at: s.first_after,
                long: false,
            },
            s.before,
        )
    })
}

fn std_dev(v: impl IntoIterator<Item = f64>, around: f64) -> Option<f64> {
    mean(v.into_iter().map(|x| (x - around).powi(2))).map(f64::sqrt)
}

fn temp_state(
    inp: &Inputs,
    gpu: Option<usize>,
    label: &str,
    temp: &str,
    usage: &str,
) -> Option<TempState> {
    let week_step = HistoryRange::D7.step_secs();
    let all = join(inp.week, usage, temp);
    let base: Vec<(i64, (f64, f64))> = all
        .iter()
        .filter(|(ts, ..)| *ts < inp.now - BASELINE_GAP_SECS)
        .map(|&(ts, u, t)| (ts, (u, t)))
        .collect();
    let (base, mixed) = same_profile(inp, base, week_step, 24);
    if base.len() < 24 {
        return None;
    }
    let pts: Vec<(f64, f64)> = base.iter().map(|(_, p)| *p).collect();
    let (a, b, sigma) = fit(&pts)?;
    let recent: Vec<(i64, f64, f64)> = join(inp.hour, usage, temp)
        .into_iter()
        .filter(|(ts, ..)| *ts >= inp.now - RECENT_SECS)
        .collect();
    if recent.len() < 30 {
        return None;
    }
    let u = mean(recent.iter().map(|r| r.1))?;
    let t = mean(recent.iter().map(|r| r.2))?;
    let model = |u: f64| a + b * u;
    // The temperature for the load over time: the baseline's points and everything since.
    let kept: std::collections::HashSet<i64> = base.iter().map(|(ts, _)| *ts).collect();
    let resid_week: Vec<(i64, f64)> = all
        .iter()
        .filter(|(ts, ..)| kept.contains(ts) || *ts >= inp.now - BASELINE_GAP_SECS)
        .map(|&(ts, u, t)| (ts, t - model(u)))
        .collect();
    let resid_day: Vec<(i64, f64)> = join(inp.day, usage, temp)
        .into_iter()
        .map(|(ts, u, t)| (ts, t - model(u)))
        .collect();
    let shift = lasting_shift(&resid_week, &resid_day, true, 4.0);
    let (onset, level, sigma) = match shift {
        Some((o, level)) => {
            // The spread before the shift: the shift itself must not widen the baseline.
            let before = resid_week.iter().filter(|(ts, _)| *ts < o.at).map(|r| r.1);
            let s = if o.long { std_dev(before, level) } else { None };
            (Some(o), level, s.unwrap_or(sigma))
        }
        None => (None, 0.0, sigma),
    };
    Some(TempState {
        gpu,
        label: label.to_owned(),
        temp: temp.to_owned(),
        usage: usage.to_owned(),
        t,
        u,
        expected: model(u) + level,
        sigma,
        onset,
        mixed,
    })
}

fn fan_state(inp: &Inputs, fan: &str) -> Option<FanState> {
    const BIN: f64 = 5.0;
    #[allow(clippy::cast_possible_truncation)]
    let bin = |t: f64| (t / BIN).floor() as i64;
    let week_step = HistoryRange::D7.step_secs();
    let all = join(inp.week, "cpu.temp.package", fan);
    let base: Vec<(i64, (f64, f64))> = all
        .iter()
        .filter(|(ts, ..)| *ts < inp.now - BASELINE_GAP_SECS)
        .map(|&(ts, t, rpm)| (ts, (t, rpm)))
        .collect();
    let (base, mixed) = same_profile(inp, base, week_step, 24);
    let mut by_bin: HashMap<i64, Vec<f64>> = HashMap::new();
    for (_, (t, rpm)) in &base {
        by_bin.entry(bin(*t)).or_default().push(*rpm);
    }
    let usual_by_bin: HashMap<i64, f64> = by_bin
        .into_iter()
        .filter(|(_, v)| v.len() >= 4)
        .filter_map(|(k, v)| Some((k, median(v)?)))
        .collect();
    let recent: Vec<(f64, f64)> = join(inp.hour, "cpu.temp.package", fan)
        .into_iter()
        .filter(|(ts, ..)| *ts >= inp.now - RECENT_SECS)
        .map(|(_, t, rpm)| (t, rpm))
        .collect();
    if recent.len() < 30 {
        return None;
    }
    let t = mean(recent.iter().map(|r| r.0))?;
    let rpm = median(recent.iter().map(|r| r.1).collect())?;
    let usual = *usual_by_bin.get(&bin(t))?;
    let resid = |pts: Vec<(i64, f64, f64)>| -> Vec<(i64, f64)> {
        pts.into_iter()
            .filter_map(|(ts, t, rpm)| Some((ts, rpm - usual_by_bin.get(&bin(t))?)))
            .collect()
    };
    let week_resid = resid(all);
    let shift = lasting_shift(
        &week_resid,
        &resid(join(inp.day, "cpu.temp.package", fan)),
        false,
        150.0f64.max(0.2 * usual),
    );
    let (onset, level) = shift.map_or((None, 0.0), |(o, l)| (Some(o), l));
    Some(FanState {
        id: fan.to_owned(),
        name: inp
            .names
            .get(fan)
            .cloned()
            .unwrap_or_else(|| fan.to_owned()),
        rpm,
        t,
        usual: usual + level,
        onset,
        mixed,
    })
}

/// The recent mean of a metric (last half hour), straight from the hour series.
fn recent_mean(inp: &Inputs, id: &str) -> Option<f64> {
    mean(
        inp.hour
            .get(id)?
            .iter()
            .filter(|p| p.ts >= inp.now - RECENT_SECS)
            .map(|p| p.avg),
    )
}

/// Spread of the CPU clock over the full-load buckets of `map` before `before` (%).
fn clock_spread(map: &super::SeriesMap, before: i64) -> Option<f64> {
    let clock = if map.contains_key("cpu.clock.effective") {
        "cpu.clock.effective"
    } else {
        "cpu.clock.current"
    };
    let v: Vec<f64> = join(map, "cpu.usage.total", clock)
        .into_iter()
        .filter(|(ts, u, _)| *u >= FULL_LOAD && *ts < before)
        .map(|(_, _, c)| c)
        .collect();
    (v.len() >= 10)
        .then(|| variation(&v))
        .flatten()
        .map(|x| x * 100.0)
}

pub(super) fn vitals(inp: &Inputs, st: &State) -> Vec<Vital> {
    let vital =
        |kind, value, usual, unit: &str, detail: Option<String>, metric: Option<&str>| Vital {
            kind,
            value,
            usual,
            unit: unit.to_owned(),
            basis: Basis::Measured,
            detail,
            metric: metric.map(str::to_owned),
        };
    let load = |u: f64| Some(format!("at {u:.0} % load"));
    let mut out = Vec::new();

    // Headroom: degrees left before the problem limit, now and usually at this load.
    let cpu_t = st
        .cpu
        .as_ref()
        .map(|c| c.t)
        .or_else(|| recent_mean(inp, "cpu.temp.package"));
    out.push(vital(
        VitalKind::HeadroomCpu,
        cpu_t.map(|t| CPU_TEMP.1 - t),
        st.cpu.as_ref().map(|c| CPU_TEMP.1 - c.expected),
        "°C",
        st.cpu.as_ref().and_then(|c| load(c.u)),
        Some("cpu.temp.package"),
    ));
    let g = st.gpus.first();
    let gpu_id = st.gpu_ids.first().map(|g| format!("gpu.{g}.temp.edge"));
    let gpu_t = g
        .map(|g| g.t)
        .or_else(|| gpu_id.as_deref().and_then(|id| recent_mean(inp, id)));
    out.push(vital(
        VitalKind::HeadroomGpu,
        gpu_t.map(|t| GPU_TEMP.1 - t),
        g.map(|g| GPU_TEMP.1 - g.expected),
        "°C",
        g.and_then(|g| load(g.u)),
        gpu_id.as_deref(),
    ));

    // Temperature for the load: a difference from usual.
    out.push(vital(
        VitalKind::TempForLoad,
        st.cpu.as_ref().map(|c| c.t - c.expected),
        st.cpu.as_ref().map(|_| 0.0),
        "°C",
        st.cpu.as_ref().and_then(|c| load(c.u)),
        Some("cpu.temp.package"),
    ));

    // Fan response: the CPU fan, else the first fan with a baseline.
    let fan = st
        .fans
        .iter()
        .find(|f| f.name.to_ascii_lowercase().contains("cpu"))
        .or(st.fans.first());
    out.push(vital(
        VitalKind::FanResponse,
        fan.map(|f| f.rpm),
        fan.map(|f| f.usual),
        "RPM",
        fan.map(|f| format!("{} at {:.0} °C", f.name, f.t)),
        fan.map(|f| f.id.as_str()),
    ));

    // Frames per watt in the last game session, against the same game's earlier sessions.
    let per_watt =
        |s: &crate::gaming::GameSession| Some(s.avg_fps? / s.power_avg.filter(|w| *w > 1.0)?);
    let last = inp
        .sessions
        .iter()
        .find(|s| s.started >= inp.now - 7 * 86_400 && per_watt(s).is_some());
    let usual = last.and_then(|l| {
        let earlier: Vec<f64> = inp
            .sessions
            .iter()
            .filter(|s| s.app == l.app && s.started < l.started)
            .filter_map(per_watt)
            .collect();
        (earlier.len() >= 2).then(|| median(earlier)).flatten()
    });
    out.push(vital(
        VitalKind::Efficiency,
        last.and_then(per_watt),
        usual,
        "FPS/W",
        last.map(|s| s.app.clone()),
        None,
    ));

    // Clock stability under sustained full load: today against the rest of the week.
    let now_spread = clock_spread(inp.day, i64::MAX);
    let usual_spread = clock_spread(inp.week, inp.now - 86_400);
    out.push(vital(
        VitalKind::ClockStability,
        now_spread,
        usual_spread,
        "%",
        now_spread.map(|_| "under full load, last 24 h".to_owned()),
        Some(if inp.day.contains_key("cpu.clock.effective") {
            "cpu.clock.effective"
        } else {
            "cpu.clock.current"
        }),
    ));
    out
}

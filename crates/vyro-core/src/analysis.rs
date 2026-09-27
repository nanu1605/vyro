//! Insights (P9d): plain-language findings from the history store. Everything here is pure: the
//! engine fetches three windows of the relevant series (the past week for baselines, the past
//! day for events, the past hour for the current state) and [`analyze`] compares them.
//!
//! - **Unusual temperature**: the CPU's and each GPU's temperature against what the past week
//!   says it runs at for the same load (a straight-line fit of temperature on load).
//! - **Throttling**: full load at a high temperature with clocks well below the usual full-load
//!   clock.
//! - **Cooling**: a fan spinning much slower than it usually does at the current CPU
//!   temperature.
//! - **Power changes**: sustained steps in CPU + GPU power, explained by the component and load
//!   that changed.
//!
//! Temperatures in texts are written as "<number> °C" so the app can show them in °F.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::history::HistoryPoint;
use crate::registry::SensorTree;
use crate::snapshot::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum InsightKind {
    UnusualTemp,
    Throttling,
    Cooling,
    PowerChange,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Insight {
    pub kind: InsightKind,
    /// `warn` for something to look at, `ok` for an explanation.
    pub status: Status,
    pub title: String,
    pub detail: String,
    /// When it happened (unix seconds); `None` for "the last half hour".
    pub ts: Option<i64>,
    /// Sensors to open in History.
    pub metrics: Vec<String>,
}

/// Series by metric id.
pub type SeriesMap = HashMap<String, Vec<HistoryPoint>>;

pub struct Inputs<'a> {
    pub now: i64,
    /// Past 7 days, coarse buckets: baselines.
    pub week: &'a SeriesMap,
    /// Past 24 h: throttling and power changes.
    pub day: &'a SeriesMap,
    /// Past hour, 10 s buckets: the current state.
    pub hour: &'a SeriesMap,
    /// Sensor display names by id.
    pub names: &'a HashMap<String, String>,
}

/// The last half hour is "now"; baselines leave out the last hour.
const RECENT_SECS: i64 = 1800;
const BASELINE_GAP_SECS: i64 = 3600;
/// Full load, for throttling and full-load clocks.
const FULL_LOAD: f64 = 85.0;

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

pub fn analyze(inp: &Inputs) -> Vec<Insight> {
    let mut out = Vec::new();
    let gpus = gpu_indices(inp);
    // Temperature against load.
    if let Some(i) = unusual_temp(inp, "CPU", "cpu.temp.package", "cpu.usage.total") {
        out.push(i);
    }
    for g in &gpus {
        let name = format!("GPU #{g}");
        let temp = format!("gpu.{g}.temp.edge");
        let usage = format!("gpu.{g}.usage");
        if let Some(i) = unusual_temp(inp, &name, &temp, &usage) {
            out.push(i);
        }
    }
    // Throttling.
    let cpu_clock = if inp.day.contains_key("cpu.clock.effective") {
        "cpu.clock.effective"
    } else {
        "cpu.clock.current"
    };
    if let Some(i) = throttling(
        inp,
        "CPU",
        "cpu.usage.total",
        "cpu.temp.package",
        88.0,
        cpu_clock,
    ) {
        out.push(i);
    }
    for g in &gpus {
        let name = format!("GPU #{g}");
        let (usage, clock) = (format!("gpu.{g}.usage"), format!("gpu.{g}.clock.core"));
        let hot = if inp.day.contains_key(&format!("gpu.{g}.temp.hotspot")) {
            (format!("gpu.{g}.temp.hotspot"), 100.0)
        } else {
            (format!("gpu.{g}.temp.edge"), 83.0)
        };
        if let Some(i) = throttling(inp, &name, &usage, &hot.0, hot.1, &clock) {
            out.push(i);
        }
    }
    // Fans.
    let mut fans: Vec<&String> = inp.week.keys().filter(|k| k.starts_with("fan.")).collect();
    fans.sort();
    for fan in fans {
        if let Some(i) = cooling(inp, fan) {
            out.push(i);
        }
    }
    // Power.
    out.extend(power_changes(inp, &gpus));
    out
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

fn name<'a>(inp: &'a Inputs, id: &'a str) -> &'a str {
    inp.names.get(id).map_or(id, String::as_str)
}

/// Bucket averages of `a` and `b` at the same timestamps.
fn join(map: &SeriesMap, a: &str, b: &str) -> Vec<(i64, f64, f64)> {
    let (Some(a), Some(b)) = (map.get(a), map.get(b)) else {
        return Vec::new();
    };
    let by_ts: HashMap<i64, f64> = b.iter().map(|p| (p.ts, p.avg)).collect();
    a.iter()
        .filter_map(|p| by_ts.get(&p.ts).map(|&v| (p.ts, p.avg, v)))
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn mean(v: impl IntoIterator<Item = f64>) -> Option<f64> {
    let (sum, n) = v
        .into_iter()
        .fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| sum / n as f64)
}

fn percentile(mut v: Vec<f64>, p: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v.get(i).copied()
}

/// Least-squares `y = a + b·x`, and the residual standard deviation.
#[allow(clippy::cast_precision_loss)]
fn fit(points: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = points.len() as f64;
    let mx = mean(points.iter().map(|p| p.0))?;
    let my = mean(points.iter().map(|p| p.1))?;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    // Too little spread in load to tell its effect: compare with the plain average.
    let b = if sxx / n >= 25.0 { sxy / sxx } else { 0.0 };
    let a = my - b * mx;
    let var = points
        .iter()
        .map(|p| (p.1 - a - b * p.0).powi(2))
        .sum::<f64>()
        / n;
    Some((a, b, var.sqrt()))
}

fn unusual_temp(inp: &Inputs, label: &str, temp: &str, usage: &str) -> Option<Insight> {
    let base: Vec<(f64, f64)> = join(inp.week, usage, temp)
        .into_iter()
        .filter(|(ts, ..)| *ts < inp.now - BASELINE_GAP_SECS)
        .map(|(_, u, t)| (u, t))
        .collect();
    if base.len() < 24 {
        return None;
    }
    let (a, b, sigma) = fit(&base)?;
    let recent: Vec<(i64, f64, f64)> = join(inp.hour, usage, temp)
        .into_iter()
        .filter(|(ts, ..)| *ts >= inp.now - RECENT_SECS)
        .collect();
    if recent.len() < 30 {
        return None;
    }
    let u = mean(recent.iter().map(|r| r.1))?;
    let t = mean(recent.iter().map(|r| r.2))?;
    let expected = a + b * u;
    if t < 50.0 || t - expected < (2.5 * sigma).max(6.0) {
        return None;
    }
    Some(Insight {
        kind: InsightKind::UnusualTemp,
        status: Status::Warn,
        title: format!("{label} running hotter than usual"),
        detail: format!(
            "Over the last 30 minutes: {t:.0} °C at {u:.0} % load. At that load it usually runs \
             near {expected:.0} °C (past week). Dust, a changed fan curve, a warmer room or a \
             cooler problem can cause this."
        ),
        ts: None,
        metrics: vec![temp.to_owned(), usage.to_owned()],
    })
}

fn throttling(
    inp: &Inputs,
    label: &str,
    usage: &str,
    temp: &str,
    hot: f64,
    clock: &str,
) -> Option<Insight> {
    // Usual full-load clock: from the week and the day, full-load buckets only.
    let full: Vec<f64> = [inp.week, inp.day]
        .into_iter()
        .flat_map(|m| join(m, usage, clock))
        .filter(|(_, u, _)| *u >= FULL_LOAD)
        .map(|(_, _, c)| c)
        .collect();
    if full.len() < 3 {
        return None;
    }
    let usual = percentile(full, 0.75)?;
    let temps: HashMap<i64, f64> = inp.day.get(temp)?.iter().map(|p| (p.ts, p.avg)).collect();
    let slow: Vec<(i64, f64, f64)> = join(inp.day, usage, clock)
        .into_iter()
        .filter_map(|(ts, u, c)| {
            let t = *temps.get(&ts)?;
            (u >= FULL_LOAD && t >= hot && c <= 0.9 * usual).then_some((ts, c, t))
        })
        .collect();
    let worst = slow.iter().min_by(|a, b| a.1.total_cmp(&b.1))?;
    let times = if slow.len() == 1 {
        "Once".to_owned()
    } else {
        format!("{} times", slow.len())
    };
    Some(Insight {
        kind: InsightKind::Throttling,
        status: Status::Warn,
        title: format!("{label} slowed down by heat"),
        detail: format!(
            "{times} in the last 24 hours it ran at full load at {:.0} °C and only {:.0} MHz, \
             below its usual {usual:.0} MHz at full load. Better cooling (or a lower power \
             limit) keeps the clocks up.",
            worst.2, worst.1
        ),
        ts: Some(worst.0),
        metrics: vec![clock.to_owned(), temp.to_owned(), usage.to_owned()],
    })
}

fn cooling(inp: &Inputs, fan: &str) -> Option<Insight> {
    const BIN: f64 = 5.0;
    let bin = |t: f64| (t / BIN).floor();
    let base: Vec<(f64, f64)> = join(inp.week, "cpu.temp.package", fan)
        .into_iter()
        .filter(|(ts, ..)| *ts < inp.now - BASELINE_GAP_SECS)
        .map(|(_, t, rpm)| (t, rpm))
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
    let rpm = percentile(recent.iter().map(|r| r.1).collect(), 0.5)?;
    let usual: Vec<f64> = base
        .iter()
        .filter(|(bt, _)| bin(*bt) == bin(t))
        .map(|(_, r)| *r)
        .collect();
    if usual.len() < 4 {
        return None;
    }
    let usual = percentile(usual, 0.5)?;
    if usual < 300.0 || rpm > 0.6 * usual {
        return None;
    }
    let fan_name = name(inp, fan);
    Some(Insight {
        kind: InsightKind::Cooling,
        status: Status::Warn,
        title: format!("{fan_name} slower than usual"),
        detail: format!(
            "{rpm:.0} RPM with the CPU at {t:.0} °C, where it usually spins near {usual:.0} RPM. \
             Check the fan and its cable, or whether the fan curve in the BIOS changed."
        ),
        ts: None,
        metrics: vec![fan.to_owned(), "cpu.temp.package".to_owned()],
    })
}

/// One day bucket: timestamp, CPU power, each GPU's power.
type PowerRow = (i64, f64, Vec<f64>);

/// Sustained steps in CPU + GPU power over the day, newest last (at most three).
fn power_changes(inp: &Inputs, gpus: &[usize]) -> Vec<Insight> {
    const K: usize = 4; // buckets on each side (about 10 minutes)
    let Some(cpu) = inp.day.get("cpu.power.package") else {
        return Vec::new();
    };
    let gpu_power: Vec<(usize, HashMap<i64, f64>)> = gpus
        .iter()
        .filter_map(|g| {
            let s = inp.day.get(&format!("gpu.{g}.power"))?;
            Some((*g, s.iter().map(|p| (p.ts, p.avg)).collect()))
        })
        .collect();
    // Only buckets where every component reported.
    let rows: Vec<PowerRow> = cpu
        .iter()
        .filter_map(|p| {
            let g: Option<Vec<f64>> = gpu_power
                .iter()
                .map(|(_, m)| m.get(&p.ts).copied())
                .collect();
            Some((p.ts, p.avg, g?))
        })
        .collect();
    if rows.len() < 2 * K + 1 {
        return Vec::new();
    }
    let total = |r: &PowerRow| r.1 + r.2.iter().sum::<f64>();
    let side_mean = |from: usize, to: usize, f: &dyn Fn(&PowerRow) -> f64| {
        mean(rows[from..to].iter().map(f)).unwrap_or(0.0)
    };
    // Candidate steps, strongest first, then kept if not near a stronger one.
    let mut steps: Vec<(usize, f64)> = (K..=rows.len() - K)
        .filter_map(|i| {
            let before = side_mean(i - K, i, &total);
            let after = side_mean(i, i + K, &total);
            let d = after - before;
            (d.abs() >= 30.0f64.max(0.4 * before.min(after))).then_some((i, d))
        })
        .collect();
    steps.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    let mut kept: Vec<usize> = Vec::new();
    for (i, _) in steps {
        if kept.iter().all(|k| k.abs_diff(i) >= 2 * K) {
            kept.push(i);
        }
    }
    kept.sort_unstable();
    let start = kept.len().saturating_sub(3);
    kept[start..]
        .iter()
        .map(|&i| {
            let before = side_mean(i - K, i, &total);
            let after = side_mean(i, i + K, &total);
            let mut parts: Vec<(f64, String)> = Vec::new();
            let cpu_d = side_mean(i, i + K, &|r| r.1) - side_mean(i - K, i, &|r| r.1);
            let load = |id: &str| {
                let s = inp.day.get(id)?;
                let (t0, t1, t2) = (rows[i - K].0, rows[i].0, rows[i + K - 1].0);
                let b = mean(s.iter().filter(|p| p.ts >= t0 && p.ts < t1).map(|p| p.avg))?;
                let a = mean(s.iter().filter(|p| p.ts >= t1 && p.ts <= t2).map(|p| p.avg))?;
                Some(format!(" (load {b:.0} % → {a:.0} %)"))
            };
            parts.push((
                cpu_d,
                format!(
                    "CPU {cpu_d:+.0} W{}",
                    load("cpu.usage.total").unwrap_or_default()
                ),
            ));
            for (j, (g, _)) in gpu_power.iter().enumerate() {
                let d = side_mean(i, i + K, &|r| r.2[j]) - side_mean(i - K, i, &|r| r.2[j]);
                parts.push((
                    d,
                    format!(
                        "GPU #{g} {d:+.0} W{}",
                        load(&format!("gpu.{g}.usage")).unwrap_or_default()
                    ),
                ));
            }
            parts.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
            let because: Vec<String> = parts
                .into_iter()
                .filter(|(d, _)| d.abs() >= 5.0)
                .map(|(_, s)| s)
                .collect();
            let rose = after > before;
            let mut metrics = vec!["cpu.power.package".to_owned()];
            metrics.extend(gpu_power.iter().map(|(g, _)| format!("gpu.{g}.power")));
            Insight {
                kind: InsightKind::PowerChange,
                status: Status::Ok,
                title: format!(
                    "Power {} to {after:.0} W",
                    if rose { "rose" } else { "dropped" }
                ),
                detail: format!(
                    "CPU + GPU power went from {before:.0} W to {after:.0} W{}",
                    if because.is_empty() {
                        ".".to_owned()
                    } else {
                        format!(": {}.", because.join(", "))
                    }
                ),
                ts: Some(rows[i].0),
                metrics,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn series(from: i64, step: i64, values: impl IntoIterator<Item = f64>) -> Vec<HistoryPoint> {
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| HistoryPoint {
                ts: from + i as i64 * step,
                avg: v,
                min: v,
                max: v,
            })
            .collect()
    }

    /// A week of 17-minute buckets where temperature = 35 + 0.4 × load, loads cycling 0–100 %.
    fn week() -> SeriesMap {
        let n = 590;
        let from = NOW - 7 * 86_400;
        let load: Vec<f64> = (0..n).map(|i| f64::from((i * 37 % 101) as u32)).collect();
        let temp: Vec<f64> = load.iter().map(|l| 35.0 + 0.4 * l).collect();
        let fan: Vec<f64> = temp.iter().map(|t| 500.0 + 20.0 * t).collect();
        let clock: Vec<f64> = load
            .iter()
            .map(|l| if *l >= 85.0 { 5200.0 } else { 3000.0 })
            .collect();
        HashMap::from([
            ("cpu.usage.total".into(), series(from, 1020, load)),
            ("cpu.temp.package".into(), series(from, 1020, temp)),
            ("fan.0".into(), series(from, 1020, fan)),
            ("cpu.clock.effective".into(), series(from, 1020, clock)),
        ])
    }

    fn hour(temp: f64, load: f64, fan: f64) -> SeriesMap {
        let from = NOW - 3600;
        HashMap::from([
            ("cpu.usage.total".into(), series(from, 10, vec![load; 360])),
            ("cpu.temp.package".into(), series(from, 10, vec![temp; 360])),
            ("fan.0".into(), series(from, 10, vec![fan; 360])),
        ])
    }

    fn run(week: &SeriesMap, day: &SeriesMap, hour: &SeriesMap) -> Vec<Insight> {
        let names = HashMap::from([("fan.0".to_owned(), "CPU Fan".to_owned())]);
        analyze(&Inputs {
            now: NOW,
            week,
            day,
            hour,
            names: &names,
        })
    }

    #[test]
    fn quiet_when_everything_matches_the_baseline() {
        // 20 % load → 43 °C, fan 1,360 RPM: exactly as usual.
        let insights = run(&week(), &SeriesMap::new(), &hour(43.0, 20.0, 1360.0));
        assert!(insights.is_empty(), "{insights:?}");
    }

    #[test]
    fn flags_a_hotter_cpu_for_the_same_load_and_a_slower_fan() {
        // 20 % load usually means 43 °C; 58 °C now, fan at a third of its usual speed.
        let insights = run(&week(), &SeriesMap::new(), &hour(58.0, 20.0, 600.0));
        let hot = insights
            .iter()
            .find(|i| i.kind == InsightKind::UnusualTemp)
            .expect("hot");
        assert_eq!(hot.title, "CPU running hotter than usual");
        assert!(hot.detail.contains("58 °C at 20 % load"), "{}", hot.detail);
        assert!(hot.detail.contains("near 43 °C"), "{}", hot.detail);
        let fan = insights
            .iter()
            .find(|i| i.kind == InsightKind::Cooling)
            .expect("fan");
        assert_eq!(fan.title, "CPU Fan slower than usual");
        assert!(
            fan.detail.starts_with("600 RPM with the CPU at 58 °C"),
            "{}",
            fan.detail
        );
    }

    #[test]
    fn finds_heat_throttling_at_full_load() {
        let from = NOW - 86_400;
        let load = [20.0, 95.0, 96.0, 97.0, 30.0];
        let day = HashMap::from([
            ("cpu.usage.total".into(), series(from, 144, load)),
            (
                "cpu.temp.package".into(),
                series(from, 144, [50.0, 80.0, 95.0, 96.0, 55.0]),
            ),
            (
                "cpu.clock.effective".into(),
                series(from, 144, [3000.0, 5200.0, 4300.0, 4100.0, 3000.0]),
            ),
        ]);
        let insights = run(&week(), &day, &SeriesMap::new());
        let t = insights
            .iter()
            .find(|i| i.kind == InsightKind::Throttling)
            .expect("throttling");
        assert_eq!(t.title, "CPU slowed down by heat");
        assert!(t.detail.starts_with("2 times"), "{}", t.detail);
        assert!(t.detail.contains("96 °C and only 4100 MHz"), "{}", t.detail);
        assert!(t.detail.contains("usual 5200 MHz"), "{}", t.detail);
        assert_eq!(t.ts, Some(from + 3 * 144));
    }

    #[test]
    fn explains_a_power_step_by_the_component_that_changed() {
        let from = NOW - 86_400;
        let n = 20;
        let step = |lo: f64, hi: f64| (0..n).map(move |i| if i < 10 { lo } else { hi });
        let day = HashMap::from([
            (
                "cpu.power.package".into(),
                series(from, 144, step(30.0, 38.0)),
            ),
            ("cpu.usage.total".into(), series(from, 144, step(5.0, 12.0))),
            ("gpu.0.power".into(), series(from, 144, step(15.0, 150.0))),
            ("gpu.0.usage".into(), series(from, 144, step(3.0, 97.0))),
        ]);
        let insights = run(&SeriesMap::new(), &day, &SeriesMap::new());
        let p: Vec<&Insight> = insights
            .iter()
            .filter(|i| i.kind == InsightKind::PowerChange)
            .collect();
        assert_eq!(p.len(), 1, "{p:?}");
        assert_eq!(p[0].title, "Power rose to 188 W");
        assert_eq!(
            p[0].detail,
            "CPU + GPU power went from 45 W to 188 W: GPU #0 +135 W (load 3 % → 97 %), CPU +8 W (load 5 % → 12 %)."
        );
        assert_eq!(p[0].ts, Some(from + 10 * 144));
        assert_eq!(p[0].status, Status::Ok);
    }

    #[test]
    fn needs_enough_history() {
        let short: SeriesMap = week()
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().take(10).collect()))
            .collect();
        assert!(run(&short, &SeriesMap::new(), &hour(80.0, 20.0, 600.0)).is_empty());
    }
}

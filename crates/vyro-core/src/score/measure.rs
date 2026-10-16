//! Gathering formula v1's [`Measures`] (pure): from the week's 1-minute history, the minute
//! digests, the drives, the event-log counts and the intelligence pass.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{Cooling, Headroom, Measures, Sag, StabilityCounts, StorageHealth, BASELINE_SECS};
use crate::health::GPU_TEMP;
use crate::history::HistoryPoint;
use crate::info::DriveInfo;
use crate::intel::digest::MinuteDigest;
use crate::intel::stats::{mean, percentile};
use crate::intel::{FindingKind, Insights};
use crate::snapshot::Status;

/// A loaded minute: the CPU or a GPU averaged at least this much load.
pub const LOADED_PCT: f64 = 50.0;
/// A full-load minute, for clock sag.
pub const FULL_PCT: f64 = 85.0;
/// Shortest full-load stretch counted for clock sag, minutes.
pub const MIN_STRETCH: usize = 5;
/// Fewest minutes behind thermals, memory and ping.
pub const MIN_MINUTES: usize = 60;
/// About a day of history before the history inputs count.
pub const MIN_SPAN_SECS: i64 = 23 * 3600;

/// 1-minute series by metric (minute start → average).
pub type MinuteMap = HashMap<String, BTreeMap<i64, f64>>;

/// From history points to minute maps.
pub fn minute_map(series: HashMap<String, Vec<HistoryPoint>>) -> MinuteMap {
    series
        .into_iter()
        .map(|(id, pts)| {
            let m = pts
                .into_iter()
                .filter(|p| p.avg.is_finite())
                .map(|p| (p.ts - p.ts.rem_euclid(60), p.avg))
                .collect();
            (id, m)
        })
        .collect()
}

/// The CPU's design limit: 100 °C for Intel Core, 95 °C for AMD Ryzen 7000/9000 and for any
/// CPU Vyro cannot place.
pub fn cpu_limit(brand: Option<&str>) -> f64 {
    match brand {
        Some(b) if b.contains("Intel") && b.contains("Core") => 100.0,
        _ => 95.0,
    }
}

/// The metrics formula v1 reads, for GPUs `gpus` (their registry indices).
pub fn metrics(gpus: &[u32]) -> Vec<String> {
    let mut v: Vec<String> = [
        "cpu.usage.total",
        "cpu.temp.package",
        "cpu.clock.effective",
        "memory.used",
        "memory.available",
        "network.ping",
        "network.loss",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for g in gpus {
        v.push(format!("gpu.{g}.usage"));
        v.push(format!("gpu.{g}.temp.edge"));
        v.push(format!("gpu.{g}.clock.core"));
    }
    v
}

/// GPU indices with a usage sensor among registry ids.
pub fn gpu_indices<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<u32> {
    let mut out: Vec<u32> = ids
        .into_iter()
        .filter_map(|id| {
            let rest = id.strip_prefix("gpu.")?.strip_suffix(".usage")?;
            rest.parse().ok()
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn series<'a>(m: &'a MinuteMap, id: &str) -> Option<&'a BTreeMap<i64, f64>> {
    m.get(id).filter(|s| !s.is_empty())
}

/// Minutes in which the CPU or a GPU averaged at least [`LOADED_PCT`].
pub fn loaded(m: &MinuteMap, gpus: &[u32]) -> BTreeSet<i64> {
    let mut out = BTreeSet::new();
    let ids = std::iter::once("cpu.usage.total".to_owned())
        .chain(gpus.iter().map(|g| format!("gpu.{g}.usage")));
    for id in ids {
        if let Some(s) = series(m, &id) {
            out.extend(s.iter().filter(|(_, v)| **v >= LOADED_PCT).map(|(t, _)| *t));
        }
    }
    out
}

/// Median temperature of each device in loaded minutes against its limit.
pub fn thermals(
    m: &MinuteMap,
    gpus: &[u32],
    loaded: &BTreeSet<i64>,
    cpu_limit_c: f64,
) -> Vec<Headroom> {
    let median_in = |id: &str| -> Option<f64> {
        let s = series(m, id)?;
        let v: Vec<f64> = loaded.iter().filter_map(|t| s.get(t).copied()).collect();
        (v.len() >= MIN_MINUTES)
            .then(|| percentile(v, 0.5))
            .flatten()
    };
    let mut out = Vec::new();
    if let Some(t) = median_in("cpu.temp.package") {
        out.push(Headroom {
            device: "CPU".into(),
            median_c: t,
            limit_c: cpu_limit_c,
        });
    }
    for g in gpus {
        if let Some(t) = median_in(&format!("gpu.{g}.temp.edge")) {
            out.push(Headroom {
                device: format!("GPU #{g}"),
                median_c: t,
                limit_c: GPU_TEMP.0,
            });
        }
    }
    out
}

/// Clock sag over the device's full-load stretches (weighted by length), %.
fn sag_of(usage: &BTreeMap<i64, f64>, clock: &BTreeMap<i64, f64>) -> Option<f64> {
    let mut runs: Vec<Vec<f64>> = Vec::new();
    let mut run: Vec<f64> = Vec::new();
    let mut last: Option<i64> = None;
    for (t, u) in usage {
        let clk = clock.get(t).copied();
        let continues = last.is_some_and(|l| t - l == 60);
        if *u >= FULL_PCT && clk.is_some() {
            if !continues && !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            run.extend(clk);
            last = Some(*t);
        } else {
            if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            last = None;
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    let (mut sum, mut weight) = (0.0, 0.0);
    for r in runs.iter().filter(|r| r.len() >= MIN_STRETCH) {
        // Minute 1 may be partial: the early clock is minutes 2–3, the late one the last two.
        let early = mean(r[1..3].iter().copied())?;
        let late = mean(r[r.len() - 2..].iter().copied())?;
        if early <= 0.0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let w = r.len() as f64;
        sum += (1.0 - late / early).max(0.0) * 100.0 * w;
        weight += w;
    }
    (weight > 0.0).then(|| sum / weight)
}

pub fn sag(m: &MinuteMap, gpus: &[u32]) -> Vec<Sag> {
    let mut out = Vec::new();
    if let (Some(u), Some(c)) = (
        series(m, "cpu.usage.total"),
        series(m, "cpu.clock.effective"),
    ) {
        if let Some(pct) = sag_of(u, c) {
            out.push(Sag {
                device: "CPU".into(),
                pct,
            });
        }
    }
    for g in gpus {
        if let (Some(u), Some(c)) = (
            series(m, &format!("gpu.{g}.usage")),
            series(m, &format!("gpu.{g}.clock.core")),
        ) {
            if let Some(pct) = sag_of(u, c) {
                out.push(Sag {
                    device: format!("GPU #{g}"),
                    pct,
                });
            }
        }
    }
    out
}

/// Share of loaded time with the CPU's heat or current limit or the GPU's heat limit reported;
/// `None` when no minute reports any of those flags, or nothing ran loaded.
pub fn throttle(digests: &[MinuteDigest], loaded: &BTreeSet<i64>) -> Option<f64> {
    let reports = digests.iter().any(|d| {
        let t = d.throttle;
        t.cpu_heat.is_some() || t.cpu_current.is_some() || t.gpu_heat.is_some()
    });
    if !reports {
        return None;
    }
    let mut minutes = 0u32;
    let mut secs = 0u32;
    for d in digests.iter().filter(|d| loaded.contains(&d.ts)) {
        let t = d.throttle;
        let cpu = u32::from(t.cpu_heat.unwrap_or(0)) + u32::from(t.cpu_current.unwrap_or(0));
        secs += cpu.min(60).max(u32::from(t.gpu_heat.unwrap_or(0)).min(60));
        minutes += 1;
    }
    (minutes > 0).then(|| f64::from(secs) / (60.0 * f64::from(minutes)))
}

/// Share of loaded time the CPU and the GPU held their power limit (PPT, power target), each
/// `None` where nothing reports that flag.
pub fn power_limit(digests: &[MinuteDigest], loaded: &BTreeSet<i64>) -> (Option<f64>, Option<f64>) {
    let share = |pick: fn(&MinuteDigest) -> Option<u8>| -> Option<f64> {
        digests.iter().any(|d| pick(d).is_some()).then_some(())?;
        let (mut minutes, mut secs) = (0u32, 0u32);
        for d in digests.iter().filter(|d| loaded.contains(&d.ts)) {
            secs += u32::from(pick(d).unwrap_or(0)).min(60);
            minutes += 1;
        }
        (minutes > 0).then(|| f64::from(secs) / (60.0 * f64::from(minutes)))
    };
    (
        share(|d| d.throttle.cpu_power),
        share(|d| d.throttle.gpu_power),
    )
}

/// RAM in use, 95th percentile of the minutes, %.
pub fn memory_p95(m: &MinuteMap) -> Option<f64> {
    let (used, avail) = (series(m, "memory.used")?, series(m, "memory.available")?);
    let v: Vec<f64> = used
        .iter()
        .filter_map(|(t, u)| {
            let a = avail.get(t)?;
            (u + a > 0.0).then(|| u / (u + a) * 100.0)
        })
        .collect();
    (v.len() >= MIN_MINUTES)
        .then(|| percentile(v, 0.95))
        .flatten()
}

/// Ping round trip p95 of minute averages (ms), and the lost replies (%).
pub fn network(m: &MinuteMap) -> (Option<f64>, Option<f64>) {
    let Some(ping) = series(m, "network.ping").filter(|s| s.len() >= MIN_MINUTES) else {
        return (None, None);
    };
    let p95 = percentile(ping.values().copied().collect(), 0.95);
    let loss = series(m, "network.loss").and_then(|s| mean(s.values().copied()));
    (p95, loss)
}

/// The internal drives' health, life left and the system drive's free space.
pub fn storage(drives: &[DriveInfo]) -> Option<StorageHealth> {
    let internal: Vec<&DriveInfo> = drives.iter().filter(|d| !d.removable).collect();
    if internal.is_empty() {
        return None;
    }
    let rank = |s: Status| match s {
        Status::Fail => 2,
        Status::Warn => 1,
        _ => 0,
    };
    let worst = internal
        .iter()
        .map(|d| d.health)
        .filter(|s| matches!(s, Status::Ok | Status::Warn | Status::Fail))
        .max_by_key(|s| rank(*s));
    let life_pct = internal
        .iter()
        .filter_map(|d| d.health_pct)
        .reduce(f64::min);
    let system_free_pct = internal
        .iter()
        .find(|d| d.system)
        .and_then(|d| {
            d.volumes
                .iter()
                .filter_map(|v| Some((v.total_mb?, v.free_mb?)))
                .filter(|(t, _)| *t > 0.0)
                .max_by(|a, b| a.0.total_cmp(&b.0))
        })
        .map(|(total, free)| free / total * 100.0);
    Some(StorageHealth {
        worst,
        life_pct,
        system_free_pct,
    })
}

/// Cooling from P16's vital signs; `None` before its baselines exist.
pub fn cooling(i: &Insights) -> Option<Cooling> {
    use crate::intel::VitalKind;
    if i.history_secs < BASELINE_SECS {
        return None;
    }
    let vital = |k: VitalKind| i.vitals.iter().find(|v| v.kind == k);
    let hotter_c = vital(VitalKind::TempForLoad).and_then(|v| v.value);
    let fan_ratio = vital(VitalKind::FanResponse).and_then(|v| match (v.value, v.usual) {
        (Some(now), Some(usual)) if usual > 0.0 => Some(now / usual),
        _ => None,
    });
    if hotter_c.is_none() && fan_ratio.is_none() {
        return None;
    }
    // An explanation (not a warning) traces it to the owner's own choice.
    let chosen = |k: FindingKind| {
        i.findings
            .iter()
            .any(|f| f.kind == k && f.status == Status::Ok)
    };
    Some(Cooling {
        hotter_c,
        fan_ratio,
        hotter_chosen: chosen(FindingKind::Hotter),
        fan_chosen: chosen(FindingKind::Cooling),
    })
}

/// The confidence of each P16 warning; `None` before its baselines exist.
pub fn deviation(i: &Insights) -> Option<Vec<crate::intel::Confidence>> {
    (i.history_secs >= BASELINE_SECS).then(|| {
        i.findings
            .iter()
            .filter(|f| f.status == Status::Warn)
            .map(|f| f.confidence)
            .collect()
    })
}

/// Everything formula v1 reads. `span_secs`: how much minute history there is.
#[allow(clippy::too_many_arguments)]
pub fn gather(
    m: &MinuteMap,
    gpus: &[u32],
    digests: &[MinuteDigest],
    span_secs: i64,
    cpu_brand: Option<&str>,
    drives: &[DriveInfo],
    stability: Option<StabilityCounts>,
    insights: Option<&Insights>,
) -> Measures {
    let mut out = Measures {
        storage: storage(drives),
        stability,
        cooling: insights.and_then(cooling),
        deviation: insights.and_then(deviation),
        ..Measures::default()
    };
    if span_secs < MIN_SPAN_SECS {
        #[allow(clippy::cast_precision_loss)]
        let hours = (span_secs.max(0) as f64 / 3600.0).round();
        out.thin = Some(format!(
            "Vyro has {hours} h of history; the score needs about a day."
        ));
        return out;
    }
    let busy = loaded(m, gpus);
    out.thermals = thermals(m, gpus, &busy, cpu_limit(cpu_brand));
    out.sag = sag(m, gpus);
    out.throttle = throttle(digests, &busy);
    (out.power_cpu, out.power_gpu) = power_limit(digests, &busy);
    out.memory_p95 = memory_p95(m);
    (out.ping_p95_ms, out.loss_pct) = network(m);
    out
}

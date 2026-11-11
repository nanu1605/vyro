//! Detection (P16, layer 3): findings from the derived metrics, the minute digests and the
//! game sessions, each explained with [`cause::explain`].

use std::collections::{BTreeMap, HashMap, HashSet};

use super::cause::{self, Candidate, Effect, Explained, Link, What, Window};
use super::derive::{FanState, State, TempState, FULL_LOAD, RECENT_SECS};
use super::stats::{join, mad, mean, median, percentile};
use super::{
    Basis, Cause, CauseKind, CauseRole, Confidence, Evidence, Finding, FindingKind, Inputs,
    MinuteDigest,
};
use crate::gaming::markers::Marker;
use crate::gaming::{Bound, GameSession};
use crate::health::{CPU_TEMP, GPU_TEMP};
use crate::snapshot::Status;
use crate::HistoryRange;

const DAY: i64 = 86_400;
/// Under three days of history, baselines are thinner: one step less sure.
const FULL_HISTORY_SECS: i64 = 3 * DAY;
/// A program is busy in a minute of play from this much use (% of the machine).
const BUSY_CPU: f64 = 10.0;
const BUSY_GPU: f64 = 15.0;
/// Background load needs this many busy minutes in a session.
const BUSY_MINUTES: usize = 2;
/// Frame drops tied to a program: at least this many in its busy minutes, at this many
/// times the rate of the other minutes.
const DROPS_MIN: usize = 5;
const DROPS_RATIO: f64 = 3.0;
/// A game is slower than usual below this share of its usual frame rate…
const SLOWER: f64 = 0.9;
/// …and outside the spread of its earlier sessions.
const SLOWER_MADS: f64 = 2.0;
/// Sessions shorter than this say little about a game's frame rate.
const MIN_SESSION_SECS: i64 = 300;

fn ev(
    label: impl Into<String>,
    metric: Option<&str>,
    from: i64,
    to: i64,
    value: f64,
    baseline: Option<f64>,
    unit: &str,
) -> Evidence {
    Evidence {
        label: label.into(),
        metric: metric.map(str::to_owned),
        from,
        to,
        value,
        baseline,
        unit: unit.to_owned(),
    }
}

/// One step less sure for a thin or mixed baseline.
fn baseline_confidence(st: &State, mixed: bool) -> Confidence {
    let mut c = Confidence::High;
    if st.history_secs < FULL_HISTORY_SECS {
        c = c.lower();
    }
    if mixed {
        c = c.lower();
    }
    c
}

fn window_of(onset: Option<super::derive::Onset>, now: i64) -> Window {
    onset.map_or(
        Window {
            lo: now - DAY,
            hi: now,
            lasted: false,
        },
        |o| Window {
            lo: o.lo,
            hi: o.hi,
            lasted: true,
        },
    )
}

const EXPECTED: &str = " Expected after the change below, which you made.";

pub(super) fn detect(inp: &Inputs, st: &State) -> Vec<Finding> {
    let cands = cause::candidates(inp);
    let mut out = Vec::new();

    // Fans first: a fan slower than usual explains a hotter CPU.
    let mut fans: Vec<(Finding, Explained)> = st
        .fans
        .iter()
        .filter_map(|f| cooling(inp, st, f, &cands))
        .collect();
    let mut hot: Vec<(Finding, Explained, Option<usize>)> = Vec::new();
    if let Some(c) = &st.cpu {
        let links: Vec<Link> = fans
            .iter()
            .map(|(f, e)| Link {
                cause: Cause {
                    role: CauseRole::Likely,
                    basis: Basis::Measured,
                    confidence: Confidence::High,
                    kind: CauseKind::Measured,
                    text: f.title.clone(),
                    ts: f.since,
                    change_id: None,
                    evidence: f.evidence.clone(),
                    because: e.causes.clone(),
                },
                chosen: e.chosen,
                uses: e.uses.clone(),
            })
            .collect();
        if let Some((f, e)) = hotter(inp, st, c, &cands, links) {
            // The fans it builds on are told inside it.
            if f.causes.iter().any(|c| c.kind == CauseKind::Measured) {
                fans.clear();
            }
            hot.push((f, e, None));
        }
    }
    for g in &st.gpus {
        if let Some((f, e)) = hotter(inp, st, g, &cands, Vec::new()) {
            hot.push((f, e, g.gpu));
        }
    }
    out.extend(fans.into_iter().map(|(f, _)| f));

    out.extend(throttling(inp, st, &cands, &hot));
    out.extend(hot.into_iter().map(|(f, ..)| f));
    out.extend(low_headroom(inp, st));
    out.extend(power_changes(inp, st));
    out.extend(games(inp, &cands));
    out
}

fn cooling(
    inp: &Inputs,
    st: &State,
    f: &FanState,
    cands: &[Candidate],
) -> Option<(Finding, Explained)> {
    if f.usual < 300.0 || f.rpm > 0.6 * f.usual {
        return None;
    }
    let ex = cause::explain(
        Effect::Cooling,
        window_of(f.onset, inp.now),
        cands,
        Vec::new(),
    );
    let tail = if ex.chosen {
        EXPECTED
    } else if ex.causes.is_empty() {
        " Check the fan and its cable, or whether the fan curve in the BIOS changed."
    } else {
        ""
    };
    let finding = Finding {
        key: format!("cooling:{}", f.id),
        kind: FindingKind::Cooling,
        status: if ex.chosen { Status::Ok } else { Status::Warn },
        basis: Basis::Measured,
        confidence: baseline_confidence(st, f.mixed),
        title: format!("{} slower than usual", f.name),
        detail: format!(
            "{:.0} RPM with the CPU at {:.0} °C, where it usually spins near {:.0} RPM.{tail}",
            f.rpm, f.t, f.usual
        ),
        ts: None,
        since: f.onset.map(|o| o.at),
        evidence: vec![
            ev(
                format!("{} speed", f.name),
                Some(&f.id),
                inp.now - RECENT_SECS,
                inp.now,
                f.rpm,
                Some(f.usual),
                "RPM",
            ),
            ev(
                "CPU temperature",
                Some("cpu.temp.package"),
                inp.now - RECENT_SECS,
                inp.now,
                f.t,
                None,
                "°C",
            ),
        ],
        causes: ex.causes.clone(),
        related: ex.related.clone(),
    };
    Some((finding, ex))
}

fn part_key(c: &TempState) -> String {
    c.gpu
        .map_or_else(|| "cpu".to_owned(), |g| format!("gpu{g}"))
}

fn hotter(
    inp: &Inputs,
    st: &State,
    c: &TempState,
    cands: &[Candidate],
    links: Vec<Link>,
) -> Option<(Finding, Explained)> {
    if c.t < 50.0 || c.t - c.expected < (2.5 * c.sigma).max(6.0) {
        return None;
    }
    let effect = if c.gpu.is_some() {
        Effect::HotterGpu
    } else {
        Effect::HotterCpu
    };
    let ex = cause::explain(effect, window_of(c.onset, inp.now), cands, links);
    let before = if c.onset.is_some() {
        format!(
            "At that load it ran near {:.0} °C before this began.",
            c.expected
        )
    } else {
        format!(
            "At that load it usually runs near {:.0} °C ({}).",
            c.expected,
            if c.mixed {
                "past week"
            } else {
                "past week, this profile"
            }
        )
    };
    let tail = if ex.chosen {
        EXPECTED
    } else if ex.causes.is_empty() {
        " Dust, a changed fan curve, a warmer room or a cooler problem can cause this."
    } else {
        ""
    };
    let day = c
        .onset
        .map_or_else(|| "now".to_owned(), |o| (o.at.div_euclid(DAY)).to_string());
    let finding = Finding {
        key: format!("hotter:{}:{day}", part_key(c)),
        kind: FindingKind::Hotter,
        status: if ex.chosen { Status::Ok } else { Status::Warn },
        basis: Basis::Measured,
        confidence: baseline_confidence(st, c.mixed),
        title: format!("{} running hotter than usual", c.label),
        detail: format!(
            "Over the last 30 minutes: {:.0} °C at {:.0} % load. {before}{tail}",
            c.t, c.u
        ),
        ts: None,
        since: c.onset.map(|o| o.at),
        evidence: vec![
            ev(
                format!("{} temperature", c.label),
                Some(&c.temp),
                inp.now - RECENT_SECS,
                inp.now,
                c.t,
                Some(c.expected),
                "°C",
            ),
            ev(
                format!("{} load", c.label),
                Some(&c.usage),
                inp.now - RECENT_SECS,
                inp.now,
                c.u,
                None,
                "%",
            ),
        ],
        causes: cause::trim(ex.causes.clone(), 3),
        related: ex.related.clone(),
    };
    Some((finding, ex))
}

/// Heat limiting over the last day: measured where the part reports it, else inferred from
/// clocks at full load and high temperature.
fn throttling(
    inp: &Inputs,
    st: &State,
    cands: &[Candidate],
    hot: &[(Finding, Explained, Option<usize>)],
) -> Vec<Finding> {
    let since = inp.now - DAY;
    let recent: Vec<&MinuteDigest> = inp.digests.iter().filter(|d| d.ts >= since).collect();
    let mut out = Vec::new();
    let parts: Vec<(Option<usize>, String)> = std::iter::once((None, "CPU".to_owned()))
        .chain(st.gpu_ids.iter().map(|&g| {
            let label = if st.gpu_ids.len() == 1 {
                "GPU".to_owned()
            } else {
                format!("GPU #{g}")
            };
            (Some(g), label)
        }))
        .collect();
    for (gpu, label) in parts {
        let secs = |d: &MinuteDigest| match gpu {
            None => d.throttle.cpu_heat,
            // The digest follows the first GPU.
            Some(0) => d.throttle.gpu_heat,
            Some(_) => None,
        };
        let reported = recent.iter().any(|d| secs(d).is_some());
        let temp = gpu.map_or_else(
            || "cpu.temp.package".to_owned(),
            |g| format!("gpu.{g}.temp.edge"),
        );
        let (mut finding, worst) = if reported {
            let total: u32 = recent.iter().map(|d| u32::from(secs(d).unwrap_or(0))).sum();
            if total < 60 {
                continue;
            }
            let worst = recent
                .iter()
                .max_by_key(|d| (secs(d).unwrap_or(0), d.ts))
                .map(|d| d.ts);
            let minutes = (f64::from(total) / 60.0).round();
            let f = Finding {
                key: format!("throttling:{}:{}", gpu.map_or_else(|| "cpu".to_owned(), |g| format!("gpu{g}")), inp.now.div_euclid(DAY)),
                kind: FindingKind::Throttling,
                status: Status::Warn,
                basis: Basis::Measured,
                confidence: Confidence::High,
                title: format!("{label} slowed down by heat"),
                detail: format!(
                    "In the last 24 hours the {label} reported holding its clocks back for heat for {minutes:.0} {} in all.",
                    if minutes == 1.0 { "minute" } else { "minutes" }
                ),
                ts: worst,
                since: None,
                evidence: vec![ev("Heat limiting, last 24 h", Some(&temp), since, inp.now, minutes, None, "min")],
                causes: Vec::new(),
                related: Vec::new(),
            };
            (f, worst)
        } else {
            let Some(f) = inferred_throttling(inp, st, gpu, &label) else {
                continue;
            };
            let ts = f.ts;
            (f, ts)
        };
        // Running hotter than usual explains being held back by heat.
        let links: Vec<Link> = hot
            .iter()
            .filter(|(h, _, g)| {
                *g == gpu && h.since.is_none_or(|s| worst.is_none_or(|w| w >= s - 3600))
            })
            .map(|(h, e, _)| Link {
                cause: Cause {
                    role: CauseRole::Likely,
                    basis: Basis::Measured,
                    confidence: Confidence::High,
                    kind: CauseKind::Measured,
                    text: h.title.clone(),
                    ts: h.since,
                    change_id: None,
                    evidence: h.evidence.clone(),
                    because: h.causes.clone(),
                },
                chosen: e.chosen,
                uses: e.uses.clone(),
            })
            .collect();
        let w = Window {
            lo: since,
            hi: inp.now,
            lasted: false,
        };
        let ex = cause::explain(Effect::Throttling { gpu: gpu.is_some() }, w, cands, links);
        if ex.chosen {
            finding.status = Status::Ok;
            finding.detail.push_str(EXPECTED);
        }
        finding.causes = cause::trim(ex.causes, 3);
        finding.related = ex.related;
        out.push(finding);
    }
    out
}

/// Full load at a high temperature with clocks well below the usual full-load clock.
fn inferred_throttling(
    inp: &Inputs,
    st: &State,
    gpu: Option<usize>,
    label: &str,
) -> Option<Finding> {
    let (usage, temp, hot, clock) = match gpu {
        None => {
            let clock = if inp.day.contains_key("cpu.clock.effective") {
                "cpu.clock.effective"
            } else {
                "cpu.clock.current"
            };
            (
                "cpu.usage.total".to_owned(),
                "cpu.temp.package".to_owned(),
                88.0,
                clock.to_owned(),
            )
        }
        Some(g) => {
            let (temp, hot) = if inp.day.contains_key(&format!("gpu.{g}.temp.hotspot")) {
                (format!("gpu.{g}.temp.hotspot"), 100.0)
            } else {
                (format!("gpu.{g}.temp.edge"), 83.0)
            };
            (
                format!("gpu.{g}.usage"),
                temp,
                hot,
                format!("gpu.{g}.clock.core"),
            )
        }
    };
    let full: Vec<f64> = [inp.week, inp.day]
        .into_iter()
        .flat_map(|m| join(m, &usage, &clock))
        .filter(|(_, u, _)| *u >= FULL_LOAD)
        .map(|(_, _, c)| c)
        .collect();
    if full.len() < 3 {
        return None;
    }
    let usual = percentile(full, 0.75)?;
    let temps: HashMap<i64, f64> = inp.day.get(&temp)?.iter().map(|p| (p.ts, p.avg)).collect();
    let slow: Vec<(i64, f64, f64)> = join(inp.day, &usage, &clock)
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
    Some(Finding {
        key: format!(
            "throttling:{}:{}",
            gpu.map_or_else(|| "cpu".to_owned(), |g| format!("gpu{g}")),
            inp.now.div_euclid(DAY)
        ),
        kind: FindingKind::Throttling,
        status: Status::Warn,
        basis: Basis::Inferred,
        confidence: baseline_confidence(st, false).min(Confidence::Medium),
        title: format!("{label} slowed down by heat"),
        detail: format!(
            "{times} in the last 24 hours it ran at full load at {:.0} °C and only {:.0} MHz, \
             below its usual {usual:.0} MHz at full load. It does not report heat limiting \
             itself, so this is worked out from its clocks.",
            worst.2, worst.1
        ),
        ts: Some(worst.0),
        since: None,
        evidence: vec![
            ev(
                "Clock at full load",
                Some(&clock),
                worst.0,
                worst.0 + HistoryRange::H24.step_secs(),
                worst.1,
                Some(usual),
                "MHz",
            ),
            ev(
                "Temperature",
                Some(&temp),
                worst.0,
                worst.0 + HistoryRange::H24.step_secs(),
                worst.2,
                None,
                "°C",
            ),
        ],
        causes: Vec::new(),
        related: Vec::new(),
    })
}

/// Sustained full load within 3 °C of the problem limit.
fn low_headroom(inp: &Inputs, st: &State) -> Vec<Finding> {
    let step = HistoryRange::H24.step_secs();
    let mut parts = vec![(
        None,
        "CPU".to_owned(),
        "cpu.temp.package".to_owned(),
        "cpu.usage.total".to_owned(),
        CPU_TEMP.1,
    )];
    for &g in &st.gpu_ids {
        let label = if st.gpu_ids.len() == 1 {
            "GPU".to_owned()
        } else {
            format!("GPU #{g}")
        };
        parts.push((
            Some(g),
            label,
            format!("gpu.{g}.temp.edge"),
            format!("gpu.{g}.usage"),
            GPU_TEMP.1,
        ));
    }
    parts
        .into_iter()
        .filter_map(|(gpu, label, temp, usage, limit)| {
            let close: Vec<(i64, f64, f64)> = join(inp.day, &usage, &temp)
                .into_iter()
                .filter(|(_, u, t)| *u >= FULL_LOAD && *t >= limit - 3.0)
                .collect();
            #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
            let secs = close.len() as i64 * step;
            if secs < 300 {
                return None;
            }
            let worst = close.iter().max_by(|a, b| a.2.total_cmp(&b.2))?;
            #[allow(clippy::cast_precision_loss)]
            let minutes = (secs as f64 / 60.0).round();
            Some(Finding {
                key: format!("headroom:{}:{}", gpu.map_or_else(|| "cpu".to_owned(), |g| format!("gpu{g}")), inp.now.div_euclid(DAY)),
                kind: FindingKind::LowHeadroom,
                status: Status::Warn,
                basis: Basis::Measured,
                confidence: Confidence::High,
                title: format!("{label} close to its temperature limit"),
                detail: format!(
                    "Under full load it reached {:.0} °C for about {minutes:.0} minutes in the last 24 hours; its limit is {limit:.0} °C. Better cooling keeps it further from the limit.",
                    worst.2
                ),
                ts: Some(worst.0),
                since: None,
                evidence: vec![ev(format!("{label} temperature at full load"), Some(&temp), worst.0, worst.0 + step, worst.2, Some(limit), "°C")],
                causes: Vec::new(),
                related: Vec::new(),
            })
        })
        .collect()
}

/// One day bucket: timestamp, CPU power, each GPU's power.
type PowerRow = (i64, f64, Vec<f64>);

/// The program whose use rose (or fell) by most of a load change between two spans.
fn program_behind(
    inp: &Inputs,
    t0: i64,
    t1: i64,
    t2: i64,
    d_load: f64,
    gpu: bool,
) -> Option<Candidate> {
    if d_load.abs() < 10.0 {
        return None;
    }
    let avg = |from: i64, to: i64| -> HashMap<&str, f64> {
        let span: Vec<&MinuteDigest> = inp
            .digests
            .iter()
            .filter(|d| d.ts >= from && d.ts < to)
            .collect();
        let mut sum: HashMap<&str, f64> = HashMap::new();
        for d in &span {
            for b in &d.busy {
                *sum.entry(b.name.as_str()).or_default() += if gpu { b.gpu } else { b.cpu };
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let n = span.len().max(1) as f64;
        sum.into_iter().map(|(k, v)| (k, v / n)).collect()
    };
    let (before, after) = (avg(t0, t1), avg(t1, t2));
    let names: HashSet<&str> = before.keys().chain(after.keys()).copied().collect();
    let (name, d) = names
        .into_iter()
        .map(|n| {
            (
                n,
                after.get(n).unwrap_or(&0.0) - before.get(n).unwrap_or(&0.0),
            )
        })
        .max_by(|a, b| (a.1 * d_load.signum()).total_cmp(&(b.1 * d_load.signum())))?;
    if d * d_load.signum() < 0.5 * d_load.abs() {
        return None;
    }
    let what = if gpu { "GPU" } else { "CPU" };
    Some(Candidate {
        what: What::Program,
        ts: t1,
        text: format!(
            "{name}: about {:.0} % → {:.0} % {what}",
            before.get(name).unwrap_or(&0.0),
            after.get(name).unwrap_or(&0.0)
        ),
        change_id: None,
        evidence: Vec::new(),
        chosen: false,
    })
}

/// Sustained steps in CPU + GPU power over the day, newest last (at most three), explained by
/// the component and load that changed and, where one did most of it, the program.
fn power_changes(inp: &Inputs, st: &State) -> Vec<Finding> {
    const K: usize = 4; // buckets on each side (about 10 minutes)
    let Some(cpu) = inp.day.get("cpu.power.package") else {
        return Vec::new();
    };
    let gpu_power: Vec<(usize, HashMap<i64, f64>)> = st
        .gpu_ids
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
            let (t0, t1, t2) = (rows[i - K].0, rows[i].0, rows[i + K - 1].0);
            let mut parts: Vec<(f64, String, Option<Candidate>)> = Vec::new();
            let cpu_d = side_mean(i, i + K, &|r| r.1) - side_mean(i - K, i, &|r| r.1);
            let load = |id: &str| {
                let s = inp.day.get(id)?;
                let b = mean(s.iter().filter(|p| p.ts >= t0 && p.ts < t1).map(|p| p.avg))?;
                let a = mean(s.iter().filter(|p| p.ts >= t1 && p.ts <= t2).map(|p| p.avg))?;
                Some((b, a))
            };
            let text = |l: Option<(f64, f64)>| {
                l.map(|(b, a)| format!(" (load {b:.0} % → {a:.0} %)"))
                    .unwrap_or_default()
            };
            let end = t2 + HistoryRange::H24.step_secs();
            let l = load("cpu.usage.total");
            parts.push((
                cpu_d,
                format!("CPU {cpu_d:+.0} W{}", text(l)),
                l.and_then(|(b, a)| program_behind(inp, t0, t1, end, a - b, false)),
            ));
            for (j, (g, _)) in gpu_power.iter().enumerate() {
                let d = side_mean(i, i + K, &|r| r.2[j]) - side_mean(i - K, i, &|r| r.2[j]);
                let l = load(&format!("gpu.{g}.usage"));
                parts.push((
                    d,
                    format!("GPU #{g} {d:+.0} W{}", text(l)),
                    l.and_then(|(b, a)| program_behind(inp, t0, t1, end, a - b, true)),
                ));
            }
            parts.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
            // The program behind the component that changed most.
            let program = parts.first().and_then(|p| p.2.clone());
            let because: Vec<String> = parts
                .into_iter()
                .filter(|(d, ..)| d.abs() >= 5.0)
                .map(|(_, s, _)| s)
                .collect();
            let rose = after > before;
            let mut metrics = vec!["cpu.power.package".to_owned()];
            metrics.extend(gpu_power.iter().map(|(g, _)| format!("gpu.{g}.power")));
            let w = Window {
                lo: t0,
                hi: end,
                lasted: true,
            };
            let ex = cause::explain(
                Effect::Load,
                w,
                &program.into_iter().collect::<Vec<_>>(),
                Vec::new(),
            );
            Finding {
                key: format!("power:{t1}"),
                kind: FindingKind::PowerChange,
                status: Status::Ok,
                basis: Basis::Measured,
                confidence: Confidence::High,
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
                ts: Some(t1),
                since: None,
                evidence: metrics
                    .iter()
                    .take(1)
                    .map(|m| {
                        ev(
                            "CPU + GPU power",
                            Some(m),
                            t0,
                            end,
                            after,
                            Some(before),
                            "W",
                        )
                    })
                    .collect(),
                causes: ex.causes,
                related: Vec::new(),
            }
        })
        .collect()
}

/// A session's end (unix seconds).
fn end_of(s: &GameSession) -> i64 {
    s.ended.unwrap_or(s.started + s.secs)
}

/// The digests of a session's minutes.
fn minutes_of<'a>(inp: &'a Inputs, s: &GameSession) -> Vec<&'a MinuteDigest> {
    let end = end_of(s);
    inp.digests
        .iter()
        .filter(|d| {
            d.ts >= s.started - 59 && d.ts <= end && d.game.as_deref() == Some(s.app.as_str())
        })
        .collect()
}

/// The busiest background program of a session, as a finding and as a candidate cause.
fn background(inp: &Inputs, s: &GameSession) -> Option<(Finding, Candidate)> {
    let minutes = minutes_of(inp, s);
    let mut busy: BTreeMap<&str, Vec<&MinuteDigest>> = BTreeMap::new();
    for d in &minutes {
        for b in &d.busy {
            if b.cpu >= BUSY_CPU || b.gpu >= BUSY_GPU {
                busy.entry(b.name.as_str()).or_default().push(d);
            }
        }
    }
    let (name, during) = busy.into_iter().max_by(|a, b| {
        a.1.len().cmp(&b.1.len()).then_with(|| {
            let cpu = |v: &[&MinuteDigest], n: &str| v.iter().map(|d| d.use_of(n).0).sum::<f64>();
            cpu(&a.1, a.0).total_cmp(&cpu(&b.1, b.0))
        })
    })?;
    if during.len() < BUSY_MINUTES {
        return None;
    }
    let cpu = mean(during.iter().map(|d| d.use_of(name).0)).unwrap_or(0.0);
    let gpu = mean(during.iter().map(|d| d.use_of(name).1)).unwrap_or(0.0);
    let busy_at: HashSet<i64> = during.iter().map(|d| d.ts).collect();
    let quiet = minutes.len() - busy_at.len();
    // Frame drops and stutters in its busy minutes against the others.
    let drops: Vec<i64> = inp
        .markers
        .get(&s.id)
        .map(|m| {
            m.iter()
                .filter(|e| matches!(e.marker, Marker::FpsDrop { .. } | Marker::Stutter { .. }))
                .map(|e| (s.started + e.t / 1000).div_euclid(60) * 60)
                .collect()
        })
        .unwrap_or_default();
    let in_busy = drops.iter().filter(|t| busy_at.contains(t)).count();
    let game_minutes: HashSet<i64> = minutes.iter().map(|d| d.ts).collect();
    let in_quiet = drops
        .iter()
        .filter(|t| game_minutes.contains(t) && !busy_at.contains(t))
        .count();
    #[allow(clippy::cast_precision_loss)]
    let (rate_busy, rate_quiet) = (
        in_busy as f64 / busy_at.len() as f64,
        if quiet > 0 {
            in_quiet as f64 / quiet as f64
        } else {
            0.0
        },
    );
    let tied = in_busy >= DROPS_MIN && quiet >= 2 && rate_busy >= DROPS_RATIO * rate_quiet;
    let first = during.iter().map(|d| d.ts).min().unwrap_or(s.started);
    let last = during.iter().map(|d| d.ts).max().unwrap_or(s.started) + 60;
    let usage = if gpu >= BUSY_GPU && gpu > cpu {
        format!("about {gpu:.0} % of the GPU")
    } else {
        format!("about {cpu:.0} % of the CPU")
    };
    let cand = Candidate {
        what: What::Program,
        ts: first,
        text: format!("{name} used {usage} during play"),
        change_id: None,
        evidence: vec![ev(
            format!("{name} CPU use"),
            None,
            first,
            last,
            cpu,
            None,
            "%",
        )],
        chosen: false,
    };
    let mut evidence = vec![ev(
        format!("{name} CPU use"),
        None,
        first,
        last,
        cpu,
        None,
        "%",
    )];
    if !drops.is_empty() && quiet > 0 {
        evidence.push(ev(
            "Frame drops per minute (busy vs other minutes)",
            Some("game.fps"),
            s.started,
            end_of(s),
            rate_busy,
            Some(rate_quiet),
            "/min",
        ));
    }
    let (title, detail, causes, related) = if tied {
        (
            format!("Frame drops in {} while {name} was busy", s.app),
            format!(
                "{name} used {usage} for {} of the {} minutes played. The frame rate dropped {in_busy} times in those minutes, against {in_quiet} in the other {quiet}.",
                during.len(),
                minutes.len()
            ),
            vec![cand.cause(CauseRole::Likely, Confidence::Medium)],
            Vec::new(),
        )
    } else {
        let why = if drops.is_empty() {
            " No frame drops were marked in this session.".to_owned()
        } else {
            format!(" Frame drops did not follow its busy minutes ({in_busy} in them, {in_quiet} in the other {quiet}), so it is not shown as their cause.")
        };
        (
            format!("{name} busy during {}", s.app),
            format!(
                "{name} used {usage} for {} of the {} minutes played.{why} Closing it while playing frees that for the game.",
                during.len(),
                minutes.len()
            ),
            Vec::new(),
            if drops.is_empty() {
                Vec::new()
            } else {
                vec![cand.cause(CauseRole::Related, Confidence::Low)]
            },
        )
    };
    let finding = Finding {
        key: format!("background:{}:{name}", s.id),
        kind: FindingKind::BackgroundLoad,
        status: Status::Warn,
        basis: Basis::Measured,
        confidence: Confidence::High,
        title,
        detail,
        ts: Some(first),
        since: None,
        evidence,
        causes,
        related,
    };
    Some((finding, cand))
}

/// What set the frame rate in a session.
fn bottleneck(inp: &Inputs, s: &GameSession, program: Option<&Candidate>) -> Option<Finding> {
    let minutes = minutes_of(inp, s);
    let mut counts: HashMap<Bound, usize> = HashMap::new();
    for d in &minutes {
        if let Some(b) = d.bound {
            *counts.entry(b).or_default() += 1;
        }
    }
    let total: usize = counts.values().sum();
    let (bound, share, confidence) = if total >= 5 {
        let (b, n) = counts
            .into_iter()
            .max_by_key(|(b, n)| (*n, matches!(b, Bound::Gpu)))?;
        #[allow(clippy::cast_precision_loss)]
        let share = 100.0 * n as f64 / total as f64;
        (b, Some(share), Confidence::High)
    } else {
        (s.bound?, None, Confidence::Medium)
    };
    let mins = (s.secs + 30) / 60;
    let share_text = share.map_or_else(
        || "most of the time".to_owned(),
        |p| format!("{p:.0} % of the time"),
    );
    let (title, detail) = match bound {
        Bound::Gpu => (
            format!("GPU set the frame rate in {}", s.app),
            format!("In the last session ({mins} min) the GPU was the limit {share_text}: it was busy for nearly all of every frame. Lower graphics settings or a faster GPU raise the frame rate."),
        ),
        Bound::Cpu => (
            format!("CPU set the frame rate in {}", s.app),
            format!("In the last session ({mins} min) the game's CPU work was the limit {share_text}, with the GPU waiting. Fewer background programs or lower CPU-heavy settings (view distance, crowds) help."),
        ),
        Bound::Capped => (
            format!("Frame rate capped in {}", s.app),
            format!("In the last session ({mins} min) V-Sync or a frame limiter held the frame rate {share_text}; neither the CPU nor the GPU was the limit."),
        ),
    };
    let causes = if bound == Bound::Cpu {
        let w = Window {
            lo: s.started - 60,
            hi: end_of(s),
            lasted: true,
        };
        let cands: Vec<Candidate> = program.cloned().into_iter().collect();
        cause::explain(Effect::Load, w, &cands, Vec::new()).causes
    } else {
        Vec::new()
    };
    let mut evidence = Vec::new();
    if let Some(u) = s.gpu_usage_avg {
        evidence.push(ev(
            "GPU load (session average)",
            Some("gpu.0.usage"),
            s.started,
            end_of(s),
            u,
            None,
            "%",
        ));
    }
    if let Some(u) = s.cpu_usage_avg {
        evidence.push(ev(
            "CPU load (session average)",
            Some("cpu.usage.total"),
            s.started,
            end_of(s),
            u,
            None,
            "%",
        ));
    }
    Some(Finding {
        key: format!("bound:{}", s.id),
        kind: FindingKind::Bottleneck,
        status: Status::Ok,
        basis: Basis::Measured,
        confidence,
        title,
        detail,
        ts: Some(s.started),
        since: None,
        evidence,
        causes,
        related: Vec::new(),
    })
}

/// The newest sessions of a game averaging well below its earlier ones.
fn slower(
    inp: &Inputs,
    app: &str,
    cands: &[Candidate],
    programs: &HashMap<i64, Candidate>,
) -> Option<Finding> {
    let list: Vec<&GameSession> = inp
        .sessions
        .iter()
        .filter(|s| {
            s.app == app && s.ended.is_some() && s.secs >= MIN_SESSION_SECS && s.avg_fps.is_some()
        })
        .collect();
    let fps = |s: &GameSession| s.avg_fps.unwrap_or(0.0);
    let usual = |base: &[&GameSession]| -> Option<(f64, f64)> {
        let v: Vec<f64> = base.iter().map(|s| fps(s)).collect();
        Some((median(v.clone())?, mad(&v)?))
    };
    let is_slow =
        |s: &GameSession, (m, d): (f64, f64)| fps(s) <= SLOWER * m && m - fps(s) > SLOWER_MADS * d;
    if list.len() < 4 {
        return None;
    }
    let first_guess = usual(&list[1..])?;
    if !is_slow(list[0], first_guess) {
        return None;
    }
    let k = list.iter().take_while(|s| is_slow(s, first_guess)).count();
    let base = &list[k..];
    if base.len() < 3 {
        return None;
    }
    let (m, d) = usual(base)?;
    if !is_slow(list[0], (m, d)) {
        return None;
    }
    let first_slow = list[k - 1];
    let prev = base[0];
    let w = Window {
        lo: prev.started,
        hi: end_of(first_slow),
        lasted: k >= 2,
    };
    let mut all: Vec<Candidate> = cands.to_vec();
    // The profile it ran with, where the digests did not see the switch.
    let usual_profile = {
        let mut c: HashMap<_, usize> = HashMap::new();
        for s in base {
            *c.entry(s.profile).or_default() += 1;
        }
        c.into_iter().max_by_key(|(_, n)| *n).and_then(|(p, _)| p)
    };
    if let (Some(now), Some(was)) = (first_slow.profile, usual_profile) {
        let seen = all
            .iter()
            .any(|c| c.what == What::Profile && c.ts >= w.lo && c.ts <= w.hi);
        if now != was && !seen {
            all.push(Candidate {
                what: What::Profile,
                ts: first_slow.started,
                text: format!(
                    "Played with the {} profile; earlier sessions used {}",
                    cause::profile_name(now),
                    cause::profile_name(was)
                ),
                change_id: None,
                evidence: Vec::new(),
                chosen: true,
            });
        }
    }
    all.extend(
        list[..k]
            .iter()
            .filter_map(|s| programs.get(&s.id).cloned()),
    );
    let ex = cause::explain(Effect::GameSlower, w, &all, Vec::new());
    let last = list[0];
    let sessions = if k == 1 {
        "The last session".to_owned()
    } else {
        format!("The last {k} sessions")
    };
    let avg = mean(list[..k].iter().map(|s| fps(s))).unwrap_or(fps(last));
    let mut detail = format!(
        "{sessions} averaged {avg:.0} FPS; earlier sessions averaged {m:.0} FPS (median of {}). In-game settings, the map or the scene can also explain this.",
        base.len()
    );
    let usual_bound = {
        let mut c: HashMap<_, usize> = HashMap::new();
        for s in base {
            *c.entry(s.bound).or_default() += 1;
        }
        c.into_iter().max_by_key(|(_, n)| *n).and_then(|(b, _)| b)
    };
    if let (Some(Bound::Cpu), Some(Bound::Gpu)) = (last.bound, usual_bound) {
        detail.push_str(" The CPU set the frame rate this time, where it was the GPU before.");
    }
    if ex.chosen {
        detail.push_str(EXPECTED);
    }
    Some(Finding {
        key: format!("slower:{app}:{}", first_slow.id),
        kind: FindingKind::GameSlower,
        status: if ex.chosen { Status::Ok } else { Status::Warn },
        basis: Basis::Measured,
        confidence: if base.len() >= 5 {
            Confidence::High
        } else {
            Confidence::Medium
        },
        title: format!("{app} running slower than usual"),
        detail,
        ts: Some(last.started),
        since: Some(first_slow.started),
        evidence: vec![ev(
            "Average FPS, last session",
            Some("game.fps"),
            last.started,
            end_of(last),
            fps(last),
            Some(m),
            "FPS",
        )],
        causes: ex.causes,
        related: ex.related,
    })
}

/// Background load, what set the frame rate and whether a game runs slower, for the past
/// week's sessions.
fn games(inp: &Inputs, cands: &[Candidate]) -> Vec<Finding> {
    let week: Vec<&GameSession> = inp
        .sessions
        .iter()
        .filter(|s| s.ended.is_some() && s.started >= inp.now - 7 * DAY)
        .collect();
    let mut out = Vec::new();
    let mut programs: HashMap<i64, Candidate> = HashMap::new();
    for s in week.iter().take(5) {
        if let Some((f, c)) = background(inp, s) {
            out.push(f);
            programs.insert(s.id, c);
        }
    }
    if let Some(s) = week.first() {
        out.extend(bottleneck(inp, s, programs.get(&s.id)));
    }
    let mut apps: Vec<&str> = Vec::new();
    for s in &week {
        if !apps.contains(&s.app.as_str()) {
            apps.push(&s.app);
        }
    }
    for app in apps.into_iter().take(5) {
        out.extend(slower(inp, app, cands, &programs));
    }
    out
}

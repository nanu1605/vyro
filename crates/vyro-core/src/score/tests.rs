//! Formula v1 (docs/performance-score.md): its breakpoints, the coverage rule, the worked
//! example and synthetic weeks.

use std::collections::HashMap;

use super::measure::{self, MinuteMap};
use super::*;
use crate::info::{DriveInfo, VolumeInfo};
use crate::intel::digest::{MinuteDigest, Throttle};

const NOW: i64 = 1_800_000_000 - 1_800_000_000 % 86_400;

/// The worked example of the formula document (reference PC).
fn example() -> Measures {
    Measures {
        thermals: vec![
            Headroom {
                device: "CPU".into(),
                median_c: 78.0,
                limit_c: 95.0,
            },
            Headroom {
                device: "GPU #0".into(),
                median_c: 62.0,
                limit_c: 85.0,
            },
        ],
        sag: vec![
            Sag {
                device: "CPU".into(),
                pct: 1.4,
            },
            Sag {
                device: "GPU #0".into(),
                pct: 0.8,
            },
        ],
        throttle: Some(0.0),
        cooling: Some(Cooling {
            hotter_c: Some(1.0),
            fan_ratio: Some(0.98),
            ..Cooling::default()
        }),
        storage: Some(StorageHealth {
            worst: Some(Status::Ok),
            life_pct: Some(96.0),
            system_free_pct: Some(41.0),
        }),
        memory_p95: Some(61.0),
        ping_p95_ms: Some(18.0),
        loss_pct: Some(0.1),
        stability: Some(StabilityCounts::default()),
        deviation: Some(Vec::new()),
        thin: None,
        power_cpu: None,
        power_gpu: None,
    }
}

fn sub(s: &PerformanceScore, id: ScoreInputId) -> Option<f64> {
    s.inputs.iter().find(|i| i.id == id).unwrap().score
}

#[test]
fn weights_add_up_to_100_and_bands_split_where_the_document_says() {
    let total: u32 = ScoreInputId::ALL.iter().map(|i| i.weight()).sum();
    assert_eq!(total, 100);
    assert_eq!(ScoreBand::of(100), ScoreBand::Excellent);
    assert_eq!(ScoreBand::of(90), ScoreBand::Excellent);
    assert_eq!(ScoreBand::of(89), ScoreBand::Good);
    assert_eq!(ScoreBand::of(75), ScoreBand::Good);
    assert_eq!(ScoreBand::of(74), ScoreBand::Fair);
    assert_eq!(ScoreBand::of(50), ScoreBand::Fair);
    assert_eq!(ScoreBand::of(49), ScoreBand::NeedsAttention);
}

#[test]
fn every_breakpoint_of_the_tables() {
    use curves::*;
    let at = |c: &[(f64, f64)], x: f64| interp(c, x);
    for (x, y) in [
        (25.0, 100.0),
        (30.0, 100.0),
        (15.0, 85.0),
        (10.0, 70.0),
        (5.0, 50.0),
        (0.0, 20.0),
        (-3.0, 0.0),
    ] {
        assert_eq!(at(&HEADROOM, x), y, "headroom {x}");
    }
    assert_eq!(
        at(&HEADROOM, 20.0),
        92.5,
        "between breakpoints: a straight line"
    );
    for (x, y) in [
        (1.0, 100.0),
        (2.0, 100.0),
        (5.0, 75.0),
        (10.0, 40.0),
        (20.0, 0.0),
        (40.0, 0.0),
    ] {
        assert_eq!(at(&SAG, x), y, "sag {x}");
    }
    for (x, y) in [
        (0.0, 100.0),
        (0.5, 90.0),
        (2.0, 70.0),
        (5.0, 40.0),
        (15.0, 0.0),
    ] {
        assert_eq!(at(&THROTTLE, x), y, "throttle {x}");
    }
    for (x, y) in [
        (-4.0, 100.0),
        (2.0, 100.0),
        (5.0, 80.0),
        (10.0, 40.0),
        (15.0, 0.0),
    ] {
        assert_eq!(at(&HOTTER, x), y, "hotter {x}");
    }
    for (x, y) in [
        (110.0, 100.0),
        (90.0, 100.0),
        (70.0, 60.0),
        (50.0, 20.0),
        (30.0, 20.0),
    ] {
        assert_eq!(at(&FAN, x), y, "fan {x}");
    }
    for (x, y) in [(80.0, 100.0), (30.0, 100.0), (10.0, 70.0), (0.0, 30.0)] {
        assert_eq!(at(&LIFE, x), y, "life {x}");
    }
    for (x, y) in [
        (50.0, 100.0),
        (15.0, 100.0),
        (10.0, 85.0),
        (5.0, 60.0),
        (2.0, 30.0),
        (0.5, 30.0),
    ] {
        assert_eq!(at(&FREE, x), y, "free {x}");
    }
    for (x, y) in [
        (60.0, 100.0),
        (75.0, 100.0),
        (85.0, 80.0),
        (92.0, 50.0),
        (98.0, 10.0),
        (100.0, 10.0),
    ] {
        assert_eq!(at(&MEMORY, x), y, "memory {x}");
    }
    for (x, y) in [
        (10.0, 100.0),
        (40.0, 100.0),
        (80.0, 80.0),
        (150.0, 50.0),
        (300.0, 10.0),
    ] {
        assert_eq!(at(&PING, x), y, "ping {x}");
    }
    for (x, y) in [(0.0, 100.0), (1.0, 80.0), (3.0, 50.0), (10.0, 0.0)] {
        assert_eq!(at(&LOSS, x), y, "loss {x}");
    }
}

#[test]
fn the_worked_example_scores_98() {
    let s = compute(&example(), NOW);
    assert_eq!(s.formula, 1);
    assert_eq!(s.score, Some(98));
    assert_eq!(s.band, Some(ScoreBand::Excellent));
    assert!((s.coverage - 1.0).abs() < 1e-9);
    assert_eq!(
        sub(&s, ScoreInputId::Thermals),
        Some(88.0),
        "the CPU's 17 °C, worse than the GPU's 23"
    );
    assert_eq!(sub(&s, ScoreInputId::Network).map(f64::round), Some(98.0));
    assert!(s.inputs.iter().all(|i| i.missing.is_none()));
    assert_eq!(
        s.inputs
            .iter()
            .find(|i| i.id == ScoreInputId::Deviation)
            .unwrap()
            .basis,
        Basis::Inferred
    );
    // The same measures always give the same result.
    assert_eq!(compute(&example(), NOW), s);
}

#[test]
fn missing_inputs_share_out_their_weight_until_coverage_is_too_low() {
    // No network and no load: 85 % of the weight, scored on what is left.
    let mut m = example();
    m.ping_p95_ms = None;
    m.sag.clear();
    let s = compute(&m, NOW);
    assert!((s.coverage - 0.85).abs() < 1e-9);
    assert_eq!(s.score, Some(98));
    let net = s
        .inputs
        .iter()
        .find(|i| i.id == ScoreInputId::Network)
        .unwrap();
    assert_eq!(net.score, None);
    assert_eq!(
        net.missing.as_deref(),
        Some("Ping is off, or the PC was offline.")
    );
    // A first day: only drives and the event log (25 %), no score.
    let m = Measures {
        storage: example().storage,
        stability: Some(StabilityCounts::default()),
        thin: Some("Vyro has 5 h of history; the score needs about a day.".into()),
        ..Measures::default()
    };
    let s = compute(&m, NOW);
    assert_eq!((s.score, s.band), (None, None));
    assert!((s.coverage - 0.25).abs() < 1e-9);
    let thermals = s
        .inputs
        .iter()
        .find(|i| i.id == ScoreInputId::Thermals)
        .unwrap();
    assert_eq!(
        thermals.missing.as_deref(),
        Some("Vyro has 5 h of history; the score needs about a day.")
    );
    // Exactly 60 %: scored.
    let m = Measures {
        thermals: example().thermals,
        throttle: Some(0.0),
        stability: Some(StabilityCounts::default()),
        memory_p95: Some(50.0),
        ping_p95_ms: Some(20.0),
        ..Measures::default()
    };
    assert!(compute(&m, NOW).score.is_some());
}

#[test]
fn stability_drives_cooling_and_insights() {
    let with = |f: &dyn Fn(&mut Measures)| {
        let mut m = example();
        f(&mut m);
        compute(&m, NOW)
    };
    let stab = |c: StabilityCounts| sub(&with(&|m| m.stability = Some(c)), ScoreInputId::Stability);
    let n = |shutdowns, corrected, fatal| StabilityCounts {
        shutdowns,
        corrected,
        fatal,
    };
    assert_eq!(stab(n(1, 0, 0)), Some(70.0));
    assert_eq!(stab(n(2, 0, 0)), Some(45.0));
    assert_eq!(stab(n(5, 0, 0)), Some(20.0));
    assert_eq!(stab(n(0, 4, 0)), Some(80.0));
    assert_eq!(stab(n(0, 40, 0)), Some(50.0));
    assert_eq!(stab(n(0, 400, 0)), Some(20.0));
    assert_eq!(stab(n(0, 0, 1)), Some(0.0));
    // The worst drive counts; without SMART only the free space does.
    let drive = |worst, life, free| {
        sub(
            &with(&|m| {
                m.storage = Some(StorageHealth {
                    worst,
                    life_pct: life,
                    system_free_pct: free,
                });
            }),
            ScoreInputId::Storage,
        )
    };
    assert_eq!(
        drive(Some(Status::Warn), Some(80.0), Some(40.0)),
        Some(50.0)
    );
    assert_eq!(drive(Some(Status::Fail), Some(80.0), Some(40.0)), Some(0.0));
    assert_eq!(drive(None, None, Some(5.0)), Some(60.0));
    // A fan slower by the owner's choice is not a fault.
    let fan = |chosen| {
        sub(
            &with(&|m| {
                m.cooling = Some(Cooling {
                    hotter_c: Some(1.0),
                    fan_ratio: Some(0.6),
                    fan_chosen: chosen,
                    ..Cooling::default()
                });
            }),
            ScoreInputId::Cooling,
        )
    };
    assert_eq!(fan(false), Some(40.0));
    assert_eq!(fan(true), Some(100.0));
    let dev = sub(
        &with(&|m| m.deviation = Some(vec![Confidence::High, Confidence::Medium])),
        ScoreInputId::Deviation,
    );
    assert_eq!(dev, Some(60.0));
}

// --- Measures from a synthetic week -------------------------------------------------------

const MIN: i64 = 60;
const DAY: i64 = 86_400;

/// A week of minutes: idle most of the day, two hours of gaming each evening.
struct Week {
    m: MinuteMap,
    digests: Vec<MinuteDigest>,
}

fn week(throttled_secs: u8, clock_sag: f64) -> Week {
    let mut m: MinuteMap = HashMap::new();
    let mut put = |id: &str, t: i64, v: f64| {
        m.entry(id.to_owned()).or_default().insert(t, v);
    };
    let mut digests = Vec::new();
    let start = NOW - 7 * DAY;
    for i in 0..(7 * DAY / MIN) {
        let t = start + i * MIN;
        let minute_of_day = (t - start) % DAY / MIN;
        let gaming = (1200..1320).contains(&minute_of_day);
        let k = f64::from(u32::try_from((minute_of_day - 1200).max(0)).unwrap());
        // The clock drifts down by `clock_sag` % over each two-hour session.
        let clock = 5200.0 * (1.0 - clock_sag / 100.0 * k / 119.0);
        put("cpu.usage.total", t, if gaming { 92.0 } else { 6.0 });
        put("cpu.temp.package", t, if gaming { 76.0 } else { 42.0 });
        put(
            "cpu.clock.effective",
            t,
            if gaming { clock } else { 3000.0 },
        );
        put("gpu.0.usage", t, if gaming { 97.0 } else { 2.0 });
        put("gpu.0.temp.edge", t, if gaming { 64.0 } else { 35.0 });
        put("gpu.0.clock.core", t, if gaming { 2800.0 } else { 300.0 });
        put("memory.used", t, if gaming { 18_000.0 } else { 12_000.0 });
        put(
            "memory.available",
            t,
            if gaming { 14_000.0 } else { 20_000.0 },
        );
        put("network.ping", t, 19.0);
        put("network.loss", t, 0.0);
        digests.push(MinuteDigest {
            ts: t,
            profile: None,
            game: None,
            bound: None,
            throttle: Throttle {
                cpu_heat: Some(if gaming { throttled_secs } else { 0 }),
                cpu_power: Some(0),
                cpu_current: Some(0),
                gpu_heat: Some(0),
                gpu_power: Some(0),
            },
            busy: Vec::new(),
        });
    }
    Week { m, digests }
}

fn drive(health: Status, life: f64, free_pct: f64) -> DriveInfo {
    DriveInfo {
        health,
        health_pct: Some(life),
        system: true,
        volumes: vec![VolumeInfo {
            letter: "C:".into(),
            total_mb: Some(1_000_000.0),
            free_mb: Some(10_000.0 * free_pct),
            ..VolumeInfo::default()
        }],
        ..DriveInfo::default()
    }
}

fn score_of(w: &Week, drives: &[DriveInfo], stab: StabilityCounts) -> PerformanceScore {
    let m = measure::gather(
        &w.m,
        &[0],
        &w.digests,
        7 * DAY,
        Some("AMD Ryzen 7 9700X 8-Core Processor"),
        drives,
        Some(stab),
        None,
    );
    compute(&m, NOW)
}

#[test]
fn a_healthy_week_scores_excellent() {
    let w = week(0, 0.5);
    let s = score_of(
        &w,
        &[drive(Status::Ok, 96.0, 40.0)],
        StabilityCounts::default(),
    );
    // Cooling and the change from usual wait for Insights; the rest is measured.
    assert!((s.coverage - 0.8).abs() < 1e-9, "{}", s.coverage);
    assert!(s.score.unwrap() >= 90, "{s:#?}");
    let t = s
        .inputs
        .iter()
        .find(|i| i.id == ScoreInputId::Thermals)
        .unwrap();
    assert_eq!(
        t.parts[0].value, 19.0,
        "95 °C limit, 76 °C median under load"
    );
    assert_eq!(sub(&s, ScoreInputId::Throttling), Some(100.0));
}

#[test]
fn throttling_a_failing_drive_and_crashes_lower_the_band() {
    let good_drive = [drive(Status::Ok, 96.0, 40.0)];
    let bad_drive = [drive(Status::Fail, 4.0, 3.0)];
    let crashes = StabilityCounts {
        shutdowns: 3,
        corrected: 0,
        fatal: 0,
    };
    // Heat limiting 5 % of the loaded time and a 6 % clock sag: Good.
    let warm = week(3, 6.0);
    let s = score_of(&warm, &good_drive, StabilityCounts::default());
    assert_eq!(sub(&s, ScoreInputId::Throttling), Some(40.0));
    assert_eq!(s.score, Some(83), "{s:#?}");
    assert_eq!(s.band, Some(ScoreBand::Good));
    // With a failing drive and three crashes: Fair.
    let s = score_of(&warm, &bad_drive, crashes);
    assert_eq!(sub(&s, ScoreInputId::Storage), Some(0.0));
    assert_eq!(sub(&s, ScoreInputId::Stability), Some(20.0));
    assert_eq!(s.band, Some(ScoreBand::Fair), "{s:#?}");
    // A third of every loaded minute at the heat limit and a 12 % sag, alone: Fair; with the
    // drive and the crashes too: Needs attention.
    let hot = week(20, 12.0);
    let s = score_of(&hot, &good_drive, StabilityCounts::default());
    assert_eq!(sub(&s, ScoreInputId::Throttling), Some(0.0));
    assert_eq!(s.band, Some(ScoreBand::Fair), "{s:#?}");
    let s = score_of(&hot, &bad_drive, crashes);
    assert_eq!(s.band, Some(ScoreBand::NeedsAttention), "{s:#?}");
}

#[test]
fn measures_from_minutes() {
    let w = week(0, 10.0);
    let busy = measure::loaded(&w.m, &[0]);
    assert_eq!(busy.len(), 7 * 120);
    let sag = measure::sag(&w.m, &[0]);
    assert_eq!(sag[0].device, "CPU");
    // Minutes 2–3 against the last two of a 120-minute drift to 10 % below.
    assert!((sag[0].pct - 9.84).abs() < 0.01, "{sag:?}");
    assert_eq!(sag[1].pct, 0.0, "the GPU held its clock");
    // No flags at all: missing, not zero.
    let mut quiet = w.digests.clone();
    for d in &mut quiet {
        d.throttle = Throttle::default();
    }
    assert_eq!(measure::throttle(&quiet, &busy), None);
    let mem = measure::memory_p95(&w.m).unwrap();
    assert!((mem - 56.25).abs() < 0.01, "{mem}");
    assert_eq!(measure::network(&w.m), (Some(19.0), Some(0.0)));
    assert_eq!(
        measure::cpu_limit(Some("13th Gen Intel(R) Core(TM) i7-13700K")),
        100.0
    );
    assert_eq!(
        measure::cpu_limit(Some("AMD Ryzen 7 9700X 8-Core Processor")),
        95.0
    );
    assert_eq!(measure::cpu_limit(None), 95.0);
    assert_eq!(
        measure::gpu_indices([
            "gpu.0.usage",
            "gpu.1.usage",
            "gpu.0.temp.edge",
            "cpu.usage.total"
        ]),
        [0, 1]
    );
}

#[test]
fn a_first_day_has_no_history_inputs() {
    let w = week(0, 0.5);
    let m = measure::gather(&w.m, &[0], &w.digests, 5 * 3600, None, &[], None, None);
    assert!(m.thermals.is_empty() && m.memory_p95.is_none() && m.throttle.is_none());
    assert_eq!(
        m.thin.as_deref(),
        Some("Vyro has 5 h of history; the score needs about a day.")
    );
    assert_eq!(compute(&m, NOW).score, None);
}

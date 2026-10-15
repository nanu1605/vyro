//! The intelligence engine against synthetic history: the P9d cases it grew out of, and the
//! P16 acceptance scenarios (induced load, heat and background interference; no weak
//! correlation shown as a cause).

use std::collections::HashMap;

use super::digest::{Busy, MinuteDigest, Throttle};
use super::*;
use crate::changes::{ChangeCategory, ChangeKind, ComponentClass, HardwareChange};
use crate::fan::{FanEvent, FanEventKind};
use crate::gaming::markers::{GameEvent, Marker};
use crate::gaming::{Bound, GameSession};
use crate::history::HistoryPoint;
use crate::profiles::Profile;
use crate::snapshot::Status;

/// A whole hour.
const NOW: i64 = 1_800_000_000;
const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

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

/// Everything [`analyze`] reads, owned.
#[derive(Default)]
struct World {
    week: SeriesMap,
    day: SeriesMap,
    hour: SeriesMap,
    digests: Vec<MinuteDigest>,
    sessions: Vec<GameSession>,
    markers: HashMap<i64, Vec<GameEvent>>,
    changes: Vec<HardwareChange>,
    fan_events: Vec<FanEvent>,
    profile: Option<Profile>,
}

impl World {
    fn run(&self) -> Insights {
        let names = HashMap::from([("fan.0".to_owned(), "CPU Fan".to_owned())]);
        analyze(&Inputs {
            now: NOW,
            week: &self.week,
            day: &self.day,
            hour: &self.hour,
            names: &names,
            digests: &self.digests,
            sessions: &self.sessions,
            markers: &self.markers,
            changes: &self.changes,
            fan_events: &self.fan_events,
            profile: self.profile,
        })
    }
}

/// A PC whose CPU runs at 35 °C + 0.4 °C per % of load and whose CPU fan spins at 500 RPM +
/// 20 RPM per °C, with changes from given moments on.
#[derive(Clone, Copy)]
struct Pc {
    /// From then on, this much hotter at any load.
    hotter: Option<(i64, f64)>,
    /// From then on, the fan at this share of its speed.
    fan: Option<(i64, f64)>,
    /// Load over the last half hour.
    load_now: f64,
    /// History begins here.
    since: i64,
}

impl Default for Pc {
    fn default() -> Self {
        Self {
            hotter: None,
            fan: None,
            load_now: 20.0,
            since: NOW - 7 * DAY,
        }
    }
}

impl Pc {
    fn load(&self, ts: i64, step: i64) -> f64 {
        if ts >= NOW - 1800 {
            self.load_now
        } else {
            f64::from(u32::try_from((ts / step) * 37 % 101).unwrap())
        }
    }

    fn temp(&self, ts: i64, load: f64) -> f64 {
        35.0 + 0.4 * load
            + self
                .hotter
                .filter(|(t, _)| ts >= *t)
                .map_or(0.0, |(_, d)| d)
    }

    fn rpm(&self, ts: i64, temp: f64) -> f64 {
        (500.0 + 20.0 * temp) * self.fan.filter(|(t, _)| ts >= *t).map_or(1.0, |(_, f)| f)
    }

    fn map(&self, from: i64, step: i64) -> SeriesMap {
        let from = from.max(self.since);
        let mut m = SeriesMap::new();
        let mut ts = from;
        while ts < NOW {
            let l = self.load(ts, step);
            let t = self.temp(ts, l);
            for (id, v) in [
                ("cpu.usage.total", l),
                ("cpu.temp.package", t),
                ("fan.0", self.rpm(ts, t)),
            ] {
                m.entry(id.to_owned()).or_default().push(HistoryPoint {
                    ts,
                    avg: v,
                    min: v,
                    max: v,
                });
            }
            ts += step;
        }
        m
    }

    fn world(&self) -> World {
        World {
            week: self.map(NOW - 7 * DAY, 1020),
            day: self.map(NOW - DAY, 144),
            hour: self.map(NOW - HOUR, 10),
            ..Default::default()
        }
    }
}

fn change(
    id: i64,
    ts: i64,
    class: ComponentClass,
    kind: ChangeKind,
    field: Option<&str>,
) -> HardwareChange {
    let category = match field {
        Some("BIOS") => ChangeCategory::Firmware,
        Some("Driver") => ChangeCategory::Driver,
        Some("Speed") => ChangeCategory::Settings,
        _ => ChangeCategory::Hardware,
    };
    HardwareChange {
        id,
        ts,
        category,
        kind,
        class: Some(class),
        slot: None,
        name: format!("{class:?}"),
        field: field.map(str::to_owned),
        before: Some("a".into()),
        after: Some("b".into()),
        last_seen: None,
    }
}

fn digest(ts: i64) -> MinuteDigest {
    MinuteDigest {
        ts,
        profile: None,
        game: None,
        bound: None,
        throttle: Throttle::default(),
        busy: Vec::new(),
    }
}

fn session(id: i64, started: i64, secs: i64, fps: f64) -> GameSession {
    GameSession {
        id,
        app: "Game.exe".into(),
        runtime: None,
        started,
        secs,
        frames: 0,
        avg_fps: Some(fps),
        low1_fps: None,
        low01_fps: None,
        max_frame_ms: None,
        bound: Some(Bound::Gpu),
        cpu_temp_max: None,
        gpu_temp_max: None,
        cpu_usage_avg: Some(30.0),
        gpu_usage_avg: Some(95.0),
        profile: Some(Profile::Gaming),
        ended: Some(started + secs),
        has_timeline: true,
        power_avg: Some(250.0),
    }
}

fn find(i: &Insights, kind: FindingKind) -> &Finding {
    i.findings
        .iter()
        .find(|f| f.kind == kind)
        .unwrap_or_else(|| panic!("no {kind:?} in {:#?}", i.findings))
}

fn has(i: &Insights, kind: FindingKind) -> bool {
    i.findings.iter().any(|f| f.kind == kind)
}

// ---- The P9d cases ----

#[test]
fn quiet_when_everything_matches_the_baseline() {
    let i = Pc::default().world().run();
    assert!(i.findings.is_empty(), "{:#?}", i.findings);
    let v = |k: VitalKind| i.vitals.iter().find(|v| v.kind == k).unwrap();
    // 20 % load → 43 °C, as usual; fan 1,360 RPM, as usual.
    let t = v(VitalKind::TempForLoad).value.unwrap();
    assert!(t.abs() < 0.5, "{t}");
    assert!((v(VitalKind::HeadroomCpu).value.unwrap() - 56.0).abs() < 0.5);
    let fan = v(VitalKind::FanResponse);
    assert!((fan.value.unwrap() - 1360.0).abs() < 1.0);
    assert!((fan.usual.unwrap() - 1360.0).abs() < 60.0, "{fan:?}");
    assert_eq!(v(VitalKind::Efficiency).value, None);
    assert!(i.history_secs >= 7 * DAY - 1020);
    assert!(i.brief().attention.is_empty());
}

#[test]
fn a_slower_fan_explains_a_hotter_cpu() {
    // The last half hour: 58 °C at 20 % load (usually 43 °C), fan at a third of its speed.
    let pc = Pc {
        hotter: Some((NOW - 1800, 15.0)),
        fan: Some((NOW - 1800, 0.36)),
        ..Pc::default()
    };
    let i = pc.world().run();
    let hot = find(&i, FindingKind::Hotter);
    assert_eq!(hot.title, "CPU running hotter than usual");
    assert!(hot.detail.contains("58 °C at 20 % load"), "{}", hot.detail);
    assert!(hot.detail.contains("near 43 °C"), "{}", hot.detail);
    assert_eq!(hot.status, Status::Warn);
    assert_eq!(hot.basis, Basis::Measured);
    let c = &hot.causes[0];
    assert_eq!(
        (c.kind, c.role, c.basis, c.confidence),
        (
            CauseKind::Measured,
            CauseRole::Likely,
            Basis::Measured,
            Confidence::High
        )
    );
    assert_eq!(c.text, "CPU Fan slower than usual");
    // The fan is told inside the CPU finding, not again on its own.
    assert!(!has(&i, FindingKind::Cooling), "{:#?}", i.findings);
    assert_eq!(i.brief().attention, std::slice::from_ref(&hot.key));
}

/// A week whose load cycles 0–100 %, temperature = 35 + 0.4 × load, fan = 500 + 20 × temp,
/// and a CPU clock of 5200 MHz at full load (3000 MHz otherwise).
fn p9d_week() -> SeriesMap {
    let n = 590;
    let from = NOW - 7 * DAY;
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

#[test]
fn infers_heat_throttling_from_clocks_where_the_cpu_does_not_report_it() {
    let from = NOW - DAY;
    let w = World {
        week: p9d_week(),
        day: HashMap::from([
            (
                "cpu.usage.total".into(),
                series(from, 144, [20.0, 95.0, 96.0, 97.0, 30.0]),
            ),
            (
                "cpu.temp.package".into(),
                series(from, 144, [50.0, 80.0, 95.0, 96.0, 55.0]),
            ),
            (
                "cpu.clock.effective".into(),
                series(from, 144, [3000.0, 5200.0, 4300.0, 4100.0, 3000.0]),
            ),
        ]),
        ..Default::default()
    };
    let i = w.run();
    let t = find(&i, FindingKind::Throttling);
    assert_eq!(t.title, "CPU slowed down by heat");
    assert_eq!(t.basis, Basis::Inferred);
    assert_eq!(t.confidence, Confidence::Medium);
    assert!(t.detail.starts_with("2 times"), "{}", t.detail);
    assert!(t.detail.contains("96 °C and only 4100 MHz"), "{}", t.detail);
    assert!(t.detail.contains("usual 5200 MHz"), "{}", t.detail);
    assert_eq!(t.ts, Some(from + 3 * 144));
}

#[test]
fn measures_heat_throttling_where_the_cpu_reports_it() {
    let mut w = World::default();
    for m in 0..120 {
        let mut d = digest(NOW - 2 * HOUR + m * 60);
        d.throttle.cpu_heat = Some(if (40..44).contains(&m) { 30 } else { 0 });
        w.digests.push(d);
    }
    let i = w.run();
    let t = find(&i, FindingKind::Throttling);
    assert_eq!((t.basis, t.confidence), (Basis::Measured, Confidence::High));
    assert!(t.detail.contains("for 2 minutes in all"), "{}", t.detail);
    assert_eq!(t.ts, Some(NOW - 2 * HOUR + 43 * 60));
}

#[test]
fn explains_a_power_step_by_the_component_and_the_program_that_changed() {
    let from = NOW - DAY;
    let n = 20;
    let step = |lo: f64, hi: f64| (0..n).map(move |i| if i < 10 { lo } else { hi });
    let mut w = World {
        day: HashMap::from([
            (
                "cpu.power.package".into(),
                series(from, 144, step(30.0, 38.0)),
            ),
            ("cpu.usage.total".into(), series(from, 144, step(5.0, 12.0))),
            ("gpu.0.power".into(), series(from, 144, step(15.0, 150.0))),
            ("gpu.0.usage".into(), series(from, 144, step(3.0, 97.0))),
        ]),
        ..Default::default()
    };
    for m in 10..40 {
        let mut d = digest(from + m * 60);
        if from + m * 60 >= from + 10 * 144 {
            d.busy.push(Busy {
                name: "Render.exe".into(),
                cpu: 4.0,
                gpu: 90.0,
            });
        }
        w.digests.push(d);
    }
    let i = w.run();
    let p: Vec<&Finding> = i
        .findings
        .iter()
        .filter(|f| f.kind == FindingKind::PowerChange)
        .collect();
    assert_eq!(p.len(), 1, "{p:?}");
    assert_eq!(p[0].title, "Power rose to 188 W");
    assert_eq!(
        p[0].detail,
        "CPU + GPU power went from 45 W to 188 W: GPU #0 +135 W (load 3 % → 97 %), CPU +8 W (load 5 % → 12 %)."
    );
    assert_eq!(p[0].ts, Some(from + 10 * 144));
    assert_eq!(p[0].status, Status::Ok);
    let c = &p[0].causes[0];
    assert_eq!(
        (c.kind, c.role, c.confidence),
        (CauseKind::Program, CauseRole::Likely, Confidence::Medium)
    );
    assert_eq!(c.text, "Render.exe: about 0 % → 90 % GPU");
}

#[test]
fn needs_enough_history() {
    let pc = Pc {
        hotter: Some((NOW - 1800, 25.0)),
        fan: Some((NOW - 1800, 0.3)),
        since: NOW - 3 * HOUR,
        ..Pc::default()
    };
    let i = pc.world().run();
    assert!(
        !has(&i, FindingKind::Hotter) && !has(&i, FindingKind::Cooling),
        "{:#?}",
        i.findings
    );
}

// ---- Acceptance: induced load ----

#[test]
fn load_alone_is_not_hotter_than_usual() {
    // Full load for the last half hour: 71 °C, exactly what that load always gives.
    let i = Pc {
        load_now: 90.0,
        ..Pc::default()
    }
    .world()
    .run();
    assert!(!has(&i, FindingKind::Hotter), "{:#?}", i.findings);
    let h = i
        .vitals
        .iter()
        .find(|v| v.kind == VitalKind::HeadroomCpu)
        .unwrap();
    assert!((h.value.unwrap() - 28.0).abs() < 0.5, "{h:?}");
}

// ---- Acceptance: induced heat, and causes ----

/// 8 °C hotter for the same load since three days ago.
fn bios_world(changes: Vec<HardwareChange>) -> World {
    let mut w = Pc {
        hotter: Some((NOW - 3 * DAY, 8.0)),
        ..Pc::default()
    }
    .world();
    w.changes = changes;
    w
}

#[test]
fn dates_a_lasting_shift_and_names_the_one_change_that_can_explain_it() {
    let t = NOW - 3 * DAY;
    let i = bios_world(vec![
        change(
            1,
            t + 120,
            ComponentClass::Board,
            ChangeKind::Changed,
            Some("BIOS"),
        ),
        change(
            2,
            t + 200,
            ComponentClass::Monitor,
            ChangeKind::FirstSeen,
            None,
        ),
    ])
    .run();
    let hot = find(&i, FindingKind::Hotter);
    let since = hot.since.expect("dated");
    assert!((since - t).abs() <= HOUR, "since {since}, shift at {t}");
    assert!(hot.detail.contains("before this began"), "{}", hot.detail);
    assert_eq!(hot.confidence, Confidence::High);
    assert_eq!(hot.causes.len(), 1, "{:#?}", hot.causes);
    let c = &hot.causes[0];
    assert_eq!(
        (c.kind, c.role, c.basis, c.confidence),
        (
            CauseKind::Change,
            CauseRole::Likely,
            Basis::Inferred,
            Confidence::Medium
        )
    );
    assert_eq!(c.change_id, Some(1));
    assert_eq!(c.text, "BIOS: a → b");
    // A monitor connected at the same time is never a cause.
    assert_eq!(hot.related.len(), 1);
    assert_eq!(hot.related[0].role, CauseRole::Related);
    assert_eq!(hot.related[0].text, "New monitor connected: Monitor");
}

#[test]
fn two_changes_at_once_are_each_only_possible() {
    let t = NOW - 3 * DAY;
    let i = bios_world(vec![
        change(
            1,
            t + 120,
            ComponentClass::Board,
            ChangeKind::Changed,
            Some("BIOS"),
        ),
        change(
            2,
            t + 300,
            ComponentClass::Memory,
            ChangeKind::Changed,
            Some("Speed"),
        ),
    ])
    .run();
    let hot = find(&i, FindingKind::Hotter);
    assert_eq!(hot.causes.len(), 2);
    assert!(
        hot.causes
            .iter()
            .all(|c| c.role == CauseRole::Possible && c.confidence == Confidence::Low),
        "{:#?}",
        hot.causes
    );
}

#[test]
fn a_change_after_the_shift_began_is_not_a_cause() {
    let t = NOW - 3 * DAY;
    let i = bios_world(vec![change(
        1,
        t + 10 * HOUR,
        ComponentClass::Board,
        ChangeKind::Changed,
        Some("BIOS"),
    )])
    .run();
    let hot = find(&i, FindingKind::Hotter);
    assert!(hot.causes.is_empty() && hot.related.is_empty(), "{hot:#?}");
    assert!(hot.detail.contains("Dust"), "{}", hot.detail);
    assert_eq!(hot.status, Status::Warn);
}

#[test]
fn two_days_of_history_lower_the_confidence() {
    let i = Pc {
        hotter: Some((NOW - 1800, 15.0)),
        since: NOW - 2 * DAY,
        ..Pc::default()
    }
    .world()
    .run();
    assert_eq!(find(&i, FindingKind::Hotter).confidence, Confidence::Medium);
}

#[test]
fn turning_fan_control_on_explains_the_slower_fan_and_the_hotter_cpu() {
    let t = NOW - 2 * HOUR;
    let mut w = Pc {
        hotter: Some((t, 10.0)),
        fan: Some((t, 0.5)),
        ..Pc::default()
    }
    .world();
    w.fan_events.push(FanEvent {
        ts: t - 60,
        kind: FanEventKind::On,
        message: "Fan control turned on".into(),
    });
    let i = w.run();
    let hot = find(&i, FindingKind::Hotter);
    // A change the owner made: expected, not a problem.
    assert_eq!(hot.status, Status::Ok, "{hot:#?}");
    assert!(hot.detail.ends_with("which you made."), "{}", hot.detail);
    let fan = &hot.causes[0];
    assert_eq!(
        (fan.kind, fan.role, fan.confidence),
        (CauseKind::Measured, CauseRole::Likely, Confidence::High)
    );
    let root = &fan.because[0];
    assert_eq!(
        (root.kind, root.role, root.confidence),
        (CauseKind::FanControl, CauseRole::Likely, Confidence::Medium)
    );
    assert_eq!(root.text, "Fan control turned on");
    // The fan control event explains the fan; it is not counted again for the CPU.
    assert_eq!(hot.causes.len(), 1, "{:#?}", hot.causes);
    assert!(!has(&i, FindingKind::Cooling));
    assert!(i.brief().attention.is_empty());
}

#[test]
fn the_silent_profile_explains_a_slower_fan() {
    let t = NOW - 2 * HOUR;
    let mut w = Pc {
        fan: Some((t, 0.5)),
        ..Pc::default()
    }
    .world();
    let mut m = NOW - 7 * DAY;
    while m < NOW {
        let mut d = digest(m);
        d.profile = Some(if m >= t {
            Profile::Silent
        } else {
            Profile::Desktop
        });
        w.digests.push(d);
        m += 60;
    }
    w.profile = Some(Profile::Silent);
    let i = w.run();
    let fan = find(&i, FindingKind::Cooling);
    assert_eq!(fan.status, Status::Ok, "{fan:#?}");
    // Dated where it began, not hours earlier (a recent shift diluted into an older split).
    let since = fan.since.expect("dated");
    assert!((since - t).abs() <= 600, "since {since}, switch at {t}");
    assert_eq!(fan.causes[0].kind, CauseKind::Profile);
    assert_eq!(fan.causes[0].role, CauseRole::Likely);
    assert_eq!(
        fan.causes[0].text,
        "Profile switched from Desktop to Silent"
    );
    // Little history on Silent: the baseline mixes profiles, so one step less sure.
    assert_eq!(fan.confidence, Confidence::Medium);
}

#[test]
fn with_enough_history_on_the_profile_its_own_baseline_is_used() {
    // Silent every other day for the past week, slower fans on Silent all along.
    let silent = |ts: i64| (ts - (NOW - 7 * DAY)).div_euclid(DAY) % 2 == 1 || ts >= NOW - DAY;
    let pc = Pc::default();
    let mut w = pc.world();
    for map in [&mut w.week, &mut w.day, &mut w.hour] {
        if let Some(fan) = map.get_mut("fan.0") {
            for p in fan.iter_mut().filter(|p| silent(p.ts)) {
                p.avg *= 0.5;
            }
        }
    }
    let mut m = NOW - 7 * DAY;
    while m < NOW {
        let mut d = digest(m);
        d.profile = Some(if silent(m) {
            Profile::Silent
        } else {
            Profile::Desktop
        });
        w.digests.push(d);
        m += 60;
    }
    w.profile = Some(Profile::Silent);
    let i = w.run();
    assert!(!has(&i, FindingKind::Cooling), "{:#?}", i.findings);
}

// ---- Acceptance: background interference ----

/// A 40-minute session two hours ago; MsMpEng.exe busy in minutes 10–19.
fn game_world(drops: impl Fn(i64) -> usize) -> World {
    let started = NOW - 2 * HOUR;
    let mut w = World {
        sessions: vec![session(7, started, 2400, 100.0)],
        ..Default::default()
    };
    let mut events = Vec::new();
    for m in 0..40 {
        let mut d = digest(started + m * 60);
        d.game = Some("Game.exe".into());
        d.bound = Some(Bound::Gpu);
        if (10..20).contains(&m) {
            d.busy.push(Busy {
                name: "MsMpEng.exe".into(),
                cpu: 25.0,
                gpu: 0.0,
            });
        }
        w.digests.push(d);
        for k in 0..drops(m) {
            events.push(GameEvent {
                t: (m * 60 + 5 + 10 * k as i64) * 1000,
                marker: Marker::FpsDrop {
                    fps: 40.0,
                    usual: 100.0,
                },
            });
        }
    }
    w.markers.insert(7, events);
    w
}

#[test]
fn frame_drops_while_a_program_is_busy_name_it_as_the_likely_cause() {
    let i = game_world(|m| {
        if (10..20).contains(&m) {
            2
        } else {
            usize::from(m == 30)
        }
    })
    .run();
    let b = find(&i, FindingKind::BackgroundLoad);
    assert_eq!(
        b.title,
        "Frame drops in Game.exe while MsMpEng.exe was busy"
    );
    assert_eq!((b.basis, b.status), (Basis::Measured, Status::Warn));
    assert!(
        b.detail.contains("for 10 of the 40 minutes"),
        "{}",
        b.detail
    );
    let c = &b.causes[0];
    assert_eq!(
        (c.kind, c.role, c.basis, c.confidence),
        (
            CauseKind::Program,
            CauseRole::Likely,
            Basis::Inferred,
            Confidence::Medium
        )
    );
    // What set the frame rate, measured.
    let g = find(&i, FindingKind::Bottleneck);
    assert_eq!(g.title, "GPU set the frame rate in Game.exe");
    assert!(g.detail.contains("100 % of the time"), "{}", g.detail);
    assert_eq!(g.confidence, Confidence::High);
}

#[test]
fn a_busy_program_without_matching_drops_is_not_called_their_cause() {
    let i = game_world(|_| 1).run();
    let b = find(&i, FindingKind::BackgroundLoad);
    assert_eq!(b.title, "MsMpEng.exe busy during Game.exe");
    assert!(b.causes.is_empty());
    assert_eq!(b.related.len(), 1);
    assert_eq!(b.related[0].role, CauseRole::Related);
    assert!(
        b.detail.contains("not shown as their cause"),
        "{}",
        b.detail
    );
}

#[test]
fn a_game_slower_since_a_driver_update_names_it() {
    // Two sessions at 80 FPS after five near 100; the driver changed in between.
    let mut w = World::default();
    for (id, fps) in [
        (8, 80.0),
        (7, 82.0),
        (6, 100.0),
        (5, 98.0),
        (4, 102.0),
        (3, 101.0),
        (2, 99.0),
    ] {
        w.sessions
            .push(session(id, NOW - (9 - id) * 12 * HOUR, 1800, fps));
    }
    w.changes = vec![
        change(
            1,
            NOW - 30 * HOUR,
            ComponentClass::Gpu,
            ChangeKind::Changed,
            Some("Driver"),
        ),
        change(
            2,
            NOW - 29 * HOUR,
            ComponentClass::Monitor,
            ChangeKind::FirstSeen,
            None,
        ),
    ];
    let i = w.run();
    let s = find(&i, FindingKind::GameSlower);
    assert_eq!(s.title, "Game.exe running slower than usual");
    assert!(
        s.detail
            .starts_with("The last 2 sessions averaged 81 FPS; earlier sessions averaged 100 FPS"),
        "{}",
        s.detail
    );
    assert_eq!(s.confidence, Confidence::High);
    assert_eq!(s.since, Some(NOW - 24 * HOUR));
    assert_eq!(s.causes.len(), 1, "{:#?}", s.causes);
    assert_eq!(
        (s.causes[0].role, s.causes[0].change_id),
        (CauseRole::Likely, Some(1))
    );
    assert_eq!(s.related[0].change_id, Some(2));
    // One slow session is not yet a lasting change: the driver is only possible.
    w.sessions.remove(1);
    let i = w.run();
    let s = find(&i, FindingKind::GameSlower);
    assert_eq!(s.causes[0].role, CauseRole::Possible);
}

#[test]
fn findings_survive_the_wire_and_warnings_come_first() {
    let mut w = game_world(|m| if (10..20).contains(&m) { 2 } else { 0 });
    let pc = Pc {
        hotter: Some((NOW - 1800, 15.0)),
        ..Pc::default()
    }
    .world();
    (w.week, w.day, w.hour) = (pc.week, pc.day, pc.hour);
    let i = w.run();
    assert!(i.findings.len() >= 3, "{:#?}", i.findings);
    let warn_then_ok = i
        .findings
        .windows(2)
        .all(|p| !(p[0].status == Status::Ok && p[1].status == Status::Warn));
    assert!(warn_then_ok);
    let json = serde_json::to_string(&i).unwrap();
    // Values are rounded to hundredths, so they survive the trip exactly.
    assert_eq!(serde_json::from_str::<Insights>(&json).unwrap(), i);
    assert!(json.contains("\"historySecs\""));
}

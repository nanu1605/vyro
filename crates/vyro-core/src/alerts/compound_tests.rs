//! P18: compound conditions, rates, usual readings, noise controls and actions.

use super::*;
use crate::profiles::Profile;
use crate::snapshot::Status;

/// Readings from (id, value) pairs: temperatures in °C, the rest in percent.
fn reads<'a>(values: &'a [(&'a str, f64)]) -> impl Fn(&str) -> Option<Reading> + 'a {
    move |id| {
        values.iter().find(|(i, _)| *i == id).map(|(i, v)| Reading {
            value: *v,
            unit: if i.contains("temp") {
                Unit::Celsius
            } else {
                Unit::Percent
            },
            name: (*i).to_owned(),
        })
    }
}

fn cond(sensor: &str, test: ConditionTest) -> Condition {
    Condition {
        sensor: sensor.into(),
        test,
        hysteresis: 0.0,
    }
}

fn rule(id: &str, any: bool, conditions: Vec<Condition>) -> AlertRule {
    AlertRule {
        id: id.into(),
        name: id.into(),
        enabled: true,
        kind: AlertKind::Conditions { any, conditions },
        for_secs: 10,
        cooldown_secs: 0,
        sound: false,
        actions: AlertActions::default(),
    }
}

fn hot_cpu_and_gpu(any: bool) -> AlertRule {
    rule(
        "hot",
        any,
        vec![
            cond("cpu.temp", ConditionTest::Above { value: 85.0 }),
            cond("gpu.0.temp", ConditionTest::Above { value: 80.0 }),
        ],
    )
}

/// Ticks `from..=to` with the same readings; the alerts fired.
fn run(a: &mut AlertEngine, from: i64, to: i64, values: &[(&str, f64)]) -> Vec<AlertFired> {
    (from..=to)
        .flat_map(|ts| a.evaluate(ts, reads(values), &[]))
        .collect()
}

#[test]
fn all_conditions_must_hold_for_the_hold_time() {
    let mut a = AlertEngine::new(vec![hot_cpu_and_gpu(false)]);
    // The CPU alone, then the GPU alone: nothing.
    assert!(run(&mut a, 0, 60, &[("cpu.temp", 95.0), ("gpu.0.temp", 60.0)]).is_empty());
    assert!(run(&mut a, 61, 120, &[("cpu.temp", 50.0), ("gpu.0.temp", 88.0)]).is_empty());
    // Both, but for less than the 10 s hold.
    assert!(run(
        &mut a,
        121,
        129,
        &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]
    )
    .is_empty());
    assert!(run(
        &mut a,
        130,
        130,
        &[("cpu.temp", 90.0), ("gpu.0.temp", 70.0)]
    )
    .is_empty());
    // Both for 10 s: one alert naming both readings.
    let f = run(
        &mut a,
        131,
        150,
        &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)],
    );
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].ts, 141);
    assert_eq!(
        f[0].message,
        "cpu.temp at 90.0 °C (above 85.0 °C) and gpu.0.temp at 85.0 °C (above 80.0 °C) for 10 s"
    );
}

#[test]
fn any_condition_is_enough_and_a_missing_reading_decides_nothing() {
    let mut a = AlertEngine::new(vec![hot_cpu_and_gpu(true)]);
    let f = run(&mut a, 0, 10, &[("cpu.temp", 50.0), ("gpu.0.temp", 88.0)]);
    assert_eq!(f.len(), 1);
    assert_eq!(
        f[0].message,
        "gpu.0.temp at 88.0 °C (above 80.0 °C) for 10 s"
    );

    // All of them, one sensor gone: it neither fires nor re-arms.
    let mut a = AlertEngine::new(vec![hot_cpu_and_gpu(false)]);
    assert_eq!(
        run(&mut a, 0, 10, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]).len(),
        1
    );
    assert!(run(&mut a, 11, 20, &[("cpu.temp", 90.0)]).is_empty());
    assert!(
        run(&mut a, 21, 40, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]).is_empty(),
        "still latched: the gap did not re-arm it"
    );
}

#[test]
fn hysteresis_keeps_a_hovering_reading_from_firing_again() {
    let hover = |a: &mut AlertEngine| -> usize {
        // 86, 84, 86, 84 …: around the 85 °C limit, never 3 °C below it.
        (0..120)
            .map(|ts| {
                let v = if ts % 20 < 10 { 86.0 } else { 84.0 };
                a.evaluate(ts, reads(&[("cpu.temp", v)]), &[]).len()
            })
            .sum()
    };
    let mut plain = rule(
        "cpu",
        false,
        vec![cond("cpu.temp", ConditionTest::Above { value: 85.0 })],
    );
    plain.for_secs = 5;
    let mut damped = plain.clone();
    let AlertKind::Conditions { conditions, .. } = &mut damped.kind else {
        unreachable!()
    };
    conditions[0].hysteresis = 3.0;
    assert_eq!(hover(&mut AlertEngine::new(vec![plain])), 6);
    assert_eq!(hover(&mut AlertEngine::new(vec![damped.clone()])), 1);
    // Clearing past the margin re-arms it.
    let mut a = AlertEngine::new(vec![damped]);
    assert_eq!(run(&mut a, 0, 5, &[("cpu.temp", 86.0)]).len(), 1);
    run(&mut a, 6, 6, &[("cpu.temp", 81.9)]);
    assert_eq!(run(&mut a, 7, 12, &[("cpu.temp", 86.0)]).len(), 1);
}

#[test]
fn a_fast_rise_fires_a_slow_climb_does_not() {
    let mut r = rule(
        "spike",
        false,
        vec![cond(
            "cpu.temp",
            ConditionTest::Rises { by: 15.0, secs: 30 },
        )],
    );
    r.for_secs = 0;
    let mut a = AlertEngine::new(vec![r]);
    // 0.25 °C a second: 7.5 °C in any 30 s.
    let slow: usize = (0..120)
        .map(|ts| {
            let v = 40.0 + 0.25 * f64::from(u8::try_from(ts).unwrap());
            a.evaluate(ts, reads(&[("cpu.temp", v)]), &[]).len()
        })
        .sum();
    assert_eq!(slow, 0);
    // 70 → 88 °C in 10 s: 15 °C above the lowest reading of the last 30 s by 125 s.
    let mut fired = Vec::new();
    for (i, ts) in (120..=130).enumerate() {
        let v = 70.0 + 1.8 * f64::from(u8::try_from(i).unwrap());
        fired.extend(a.evaluate(ts, reads(&[("cpu.temp", v)]), &[]));
    }
    assert_eq!(fired.len(), 1);
    assert_eq!(
        fired[0].message,
        "cpu.temp rose from 63.8 °C to 79.0 °C within 30 s"
    );
}

#[test]
fn above_usual_needs_a_usual_reading() {
    let mut r = rule(
        "usual",
        false,
        vec![cond("cpu.temp", ConditionTest::AboveUsual { by: 8.0 })],
    );
    r.for_secs = 0;
    let mut a = AlertEngine::new(vec![r]);
    assert_eq!(a.usual_sensors(), ["cpu.temp"]);
    let tick = |a: &mut AlertEngine, ts, v, usual: Option<f64>| {
        let u = move |_: &str| usual;
        a.evaluate_with(
            ts,
            reads(&[("cpu.temp", v)]),
            &TickInputs {
                usual: Some(&u),
                ..TickInputs::default()
            },
        )
    };
    assert!(tick(&mut a, 0, 90.0, None).is_empty(), "no history yet");
    assert!(tick(&mut a, 1, 66.0, Some(60.0)).is_empty());
    let f = tick(&mut a, 2, 69.0, Some(60.0));
    assert_eq!(f[0].message, "cpu.temp at 69.0 °C, usually 60.0 °C");
}

#[test]
fn actions_last_while_the_conditions_hold_and_end_after_they_clear() {
    let mut r = hot_cpu_and_gpu(false);
    r.cooldown_secs = 600;
    r.actions = AlertActions {
        notify: true,
        tray: true,
        profile: Some(Profile::Gaming),
        fan_boost: Some(20),
        record: false,
    };
    let mut quiet = rule(
        "quiet",
        false,
        vec![cond("cpu.temp", ConditionTest::Above { value: 80.0 })],
    );
    quiet.actions.profile = Some(Profile::Silent);
    quiet.actions.fan_boost = Some(30);
    let mut a = AlertEngine::new(vec![r, quiet]);
    let hot = [("cpu.temp", 90.0), ("gpu.0.temp", 85.0)];
    run(&mut a, 0, 9, &hot);
    assert!(a.brief().active.is_empty(), "not fired yet");
    let fired = run(&mut a, 10, 10, &hot);
    assert_eq!(
        fired.iter().find(|f| f.rule_id == "hot").unwrap().actions,
        [
            "Gaming profile while it holds",
            "Fans not raised: fan control is off",
            "Shown in the tray while it holds"
        ]
    );
    let b = a.brief();
    assert_eq!(b.active.len(), 2);
    // The first rule's profile; the largest boost.
    assert_eq!(b.profile(), Some((Profile::Gaming, "hot")));
    assert_eq!(b.fan_boost(), Some((30, "quiet")));
    assert_eq!(b.tray(), ["hot"]);
    // Clear: active for another 30 s, then not. A blip back above restarts the count.
    let cool = [("cpu.temp", 50.0), ("gpu.0.temp", 50.0)];
    run(&mut a, 11, 30, &cool);
    run(&mut a, 31, 31, &hot);
    run(&mut a, 32, 61, &cool);
    assert_eq!(a.brief().active.len(), 2);
    run(&mut a, 62, 62, &cool);
    assert!(a.brief().active.is_empty());
}

#[test]
fn a_lost_sensor_pauses_actions_until_its_reading_is_back() {
    let mut r = hot_cpu_and_gpu(false);
    r.actions.fan_boost = Some(20);
    let mut a = AlertEngine::new(vec![r]);
    run(&mut a, 0, 10, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]);
    assert_eq!(a.brief().fan_boost(), Some((20, "hot")));
    run(&mut a, 11, 41, &[("cpu.temp", 90.0)]);
    assert_eq!(a.brief().fan_boost(), None);
    // The reading is back and the rule still holds: its actions too, without a second alert.
    assert!(run(&mut a, 42, 42, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]).is_empty());
    assert_eq!(a.brief().fan_boost(), Some((20, "hot")));
    // Then they end as usual, 30 s after it clears.
    run(&mut a, 43, 73, &[("cpu.temp", 50.0), ("gpu.0.temp", 50.0)]);
    assert_eq!(a.brief().fan_boost(), None);
}

#[test]
fn turning_a_rule_off_ends_its_actions() {
    let mut r = hot_cpu_and_gpu(false);
    r.actions.profile = Some(Profile::Silent);
    let mut a = AlertEngine::new(vec![r.clone()]);
    run(&mut a, 0, 10, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]);
    assert!(a.brief().profile().is_some());
    r.enabled = false;
    a.set_rules(vec![r.clone()]).unwrap();
    assert!(a.brief().active.is_empty());
    run(&mut a, 11, 11, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]);
    assert!(a.brief().active.is_empty());
    // On again while it still holds: its actions are back, without a second alert.
    r.enabled = true;
    a.set_rules(vec![r]).unwrap();
    assert!(run(&mut a, 12, 12, &[("cpu.temp", 90.0), ("gpu.0.temp", 85.0)]).is_empty());
    assert!(a.brief().profile().is_some());
}

#[test]
fn alerts_about_the_same_part_within_a_minute_notify_once() {
    let mut temp = rule(
        "cpu-temp",
        false,
        vec![cond("cpu.temp", ConditionTest::Above { value: 90.0 })],
    );
    temp.for_secs = 0;
    temp.cooldown_secs = 600;
    let mut load = rule(
        "cpu-load",
        false,
        vec![cond("cpu.usage", ConditionTest::Above { value: 95.0 })],
    );
    load.for_secs = 0;
    let mut gpu = rule(
        "gpu",
        false,
        vec![cond("gpu.0.temp", ConditionTest::Above { value: 80.0 })],
    );
    gpu.for_secs = 0;
    let mut a = AlertEngine::new(vec![
        temp,
        load,
        gpu,
        AlertRule {
            for_secs: 0,
            ..default_rules().remove(2)
        },
    ]);
    let f = run(&mut a, 0, 0, &[("cpu.temp", 95.0), ("gpu.0.temp", 85.0)]);
    assert!(f.iter().all(|f| f.notify), "different parts");
    let f = run(&mut a, 30, 30, &[("cpu.temp", 95.0), ("cpu.usage", 99.0)]);
    assert_eq!(f.len(), 1);
    assert!(!f[0].notify);
    assert_eq!(f[0].also.as_deref(), Some("cpu-temp"));
    // A hardware problem about the CPU still notifies.
    let problem = HealthEvent {
        ts: 40,
        subsystem: "CPU".into(),
        status: Status::Fail,
        message: "CPU at 100 °C".into(),
    };
    let f = a.evaluate(40, reads(&[]), std::slice::from_ref(&problem));
    assert!(f[0].notify);
    // A minute on, the CPU notifies again.
    run(&mut a, 41, 99, &[("cpu.usage", 10.0)]);
    let f = run(&mut a, 100, 100, &[("cpu.usage", 99.0)]);
    assert!(f[0].notify && f[0].also.is_none());
    // Logged either way.
    assert_eq!(a.log().len(), 5);
}

#[test]
fn a_rule_that_only_logs_and_its_diagnostic_record() {
    let mut r = rule(
        "rec",
        false,
        vec![cond("gpu.0.temp", ConditionTest::Above { value: 80.0 })],
    );
    r.for_secs = 0;
    r.actions.notify = false;
    r.actions.record = true;
    let mut a = AlertEngine::new(vec![r]);
    let proc = |pid, name: &str, cpu: f64, gpu: Option<f64>| ProcessSnapshot {
        pid,
        name: name.into(),
        cpu_pct: Some(cpu),
        ram_mb: None,
        gpu_pct: gpu,
    };
    let procs = [
        proc(0, "Idle", 80.0, None),
        proc(1, "a.exe", 1.0, None),
        proc(2, "game.exe", 20.0, Some(97.0)),
        proc(3, "b.exe", 2.0, None),
        proc(4, "c.exe", 3.0, None),
        proc(5, "d.exe", 4.0, None),
        proc(6, "scan.exe", 30.0, None),
    ];
    let f = a.evaluate_with(
        0,
        reads(&[
            ("gpu.0.temp", 85.0),
            ("cpu.temp.package", 70.0),
            ("fan.1", 1200.0),
        ]),
        &TickInputs {
            processes: &procs,
            ..TickInputs::default()
        },
    );
    assert!(!f[0].notify);
    assert_eq!(f[0].actions, ["No notification (logged only)"]);
    let d = f[0].diagnostic.as_ref().unwrap();
    let ids: Vec<&str> = d.readings.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["gpu.0.temp", "cpu.temp.package", "fan.1"]);
    let names: Vec<&str> = d.programs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["game.exe", "scan.exe", "d.exe", "c.exe", "b.exe"]);
}

#[test]
fn checks_new_rules() {
    let ok = hot_cpu_and_gpu(false);
    assert_eq!(validate(std::slice::from_ref(&ok)), Ok(()));
    let bad = |f: &dyn Fn(&mut AlertRule)| {
        let mut r = ok.clone();
        f(&mut r);
        validate(&[r]).unwrap_err()
    };
    assert!(bad(&|r| r.kind = AlertKind::Conditions {
        any: false,
        conditions: Vec::new()
    })
    .contains("conditions"));
    assert!(bad(&|r| r.actions.fan_boost = Some(80)).contains("raise fans"));
    assert!(bad(&|r| {
        r.kind = AlertKind::Conditions {
            any: false,
            conditions: vec![cond(
                "cpu.temp",
                ConditionTest::Rises {
                    by: 5.0,
                    secs: 3600,
                },
            )],
        }
    })
    .contains("measured over"));
    assert!(bad(&|r| {
        r.kind = AlertKind::Conditions {
            any: false,
            conditions: vec![Condition {
                hysteresis: -1.0,
                ..cond("cpu.temp", ConditionTest::Above { value: 1.0 })
            }],
        }
    })
    .contains("margin"));
    // Names count characters, as the editor does, not bytes: 80 accented letters are fine.
    let mut named = ok.clone();
    named.name = "é".repeat(80);
    assert_eq!(validate(&[named]), Ok(()));
    assert!(bad(&|r| r.name = "é".repeat(81)).contains("1–80 characters"));
    let mut health = default_rules().remove(2);
    health.actions.profile = Some(Profile::Silent);
    assert!(validate(&[health.clone()])
        .unwrap_err()
        .contains("only notify and record"));
    health.actions = AlertActions {
        record: true,
        ..AlertActions::default()
    };
    assert_eq!(validate(&[health]), Ok(()));
}

#[test]
fn rules_saved_before_p18_load_and_new_ones_keep_their_shape() {
    let old = r#"{"id":"x","name":"X","enabled":true,"kind":{"type":"threshold","sensor":"cpu.temp.package","above":true,"value":90.0},"forSecs":5,"cooldownSecs":60,"sound":true}"#;
    let r: AlertRule = serde_json::from_str(old).unwrap();
    assert_eq!(r.actions, AlertActions::default());
    assert!(r.actions.notify);
    let mut r = hot_cpu_and_gpu(true);
    r.kind = AlertKind::Conditions {
        any: true,
        conditions: vec![cond("cpu.temp", ConditionTest::AboveUsual { by: 8.0 })],
    };
    r.actions.fan_boost = Some(20);
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["kind"]["type"], "conditions");
    assert_eq!(json["kind"]["conditions"][0]["test"]["type"], "aboveUsual");
    assert_eq!(json["actions"]["fanBoost"], 20);
    let back: AlertRule = serde_json::from_value(json).unwrap();
    assert_eq!(back, r);
    // An alert stored before P18 reads as one that notified.
    let fired: AlertFired =
        serde_json::from_str(r#"{"ts":1,"ruleId":"x","title":"t","message":"m","sound":true}"#)
            .unwrap();
    assert!(fired.notify && fired.actions.is_empty() && fired.diagnostic.is_none());
}

#[test]
fn which_part_a_sensor_belongs_to() {
    assert_eq!(part("cpu.temp.package"), "CPU");
    assert_eq!(part("gpu.1.usage"), "GPU");
    assert_eq!(part("fan.3"), "Fans");
    assert_eq!(part("network.ping"), "network.ping");
}

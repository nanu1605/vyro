use std::sync::Arc;
use std::time::{Duration, Instant};

use super::controller::{Options, SimDevice, TuneControl, MARKER_FILE};
use super::search::{self, Next, Search};
use super::*;
use crate::snapshot::GpuSnapshot;

fn limits() -> Limits {
    Limits {
        core: Some(Range {
            min: -500,
            max: 1000,
        }),
        mem: Some(Range {
            min: -1000,
            max: 1500,
        }),
        power: Some(Range { min: 50, max: 115 }),
        power_default_w: Some(200.0),
    }
}

fn custom(id: &str, s: Settings) -> Preset {
    Preset {
        id: id.into(),
        name: id.into(),
        settings: s,
        kind: PresetKind::Custom,
        note: None,
    }
}

const PLUS: Settings = Settings {
    core_mhz: 90,
    mem_mhz: 500,
    power_pct: 110,
};

#[test]
fn family_and_laptop_from_the_driver() {
    assert_eq!(Family::from_arch("Ada"), Some(Family::Ada));
    assert_eq!(Family::from_arch("Blackwell"), Some(Family::Blackwell));
    assert_eq!(Family::from_arch("Turing"), Some(Family::Turing));
    assert_eq!(Family::from_arch("Pascal"), None);
    assert_eq!(Family::from_arch("Hopper"), None);
    assert!(is_laptop("NVIDIA GeForce RTX 4070 Laptop GPU"));
    assert!(is_laptop("NVIDIA GeForce RTX 2080 with Max-Q Design"));
    assert!(!is_laptop("NVIDIA GeForce RTX 4090"));
}

#[test]
fn suggestions_are_scaled_to_the_driver() {
    let all = suggestions(Some(Family::Ada), false, &limits());
    let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, [STOCK, QUIET, EFFICIENT, PERFORMANCE]);
    let perf = all.iter().find(|p| p.id == PERFORMANCE).unwrap().settings;
    assert_eq!(
        perf,
        Settings {
            core_mhz: 100,
            mem_mhz: 600,
            power_pct: 115
        }
    );
    let quiet = all.iter().find(|p| p.id == QUIET).unwrap().settings;
    assert_eq!(
        quiet,
        Settings {
            core_mhz: 0,
            mem_mhz: 0,
            power_pct: 75
        }
    );

    // A laptop gets smaller offsets, and without a power limit to change, Quiet would be
    // stock: it is left out, and Performance is offsets only.
    let laptop = Limits {
        power: None,
        power_default_w: None,
        ..limits()
    };
    let all = suggestions(Some(Family::Ada), true, &laptop);
    assert!(all.iter().all(|p| p.id != QUIET));
    let perf = all.iter().find(|p| p.id == PERFORMANCE).unwrap().settings;
    assert_eq!(
        perf,
        Settings {
            core_mhz: 75,
            mem_mhz: 400,
            power_pct: 100
        }
    );

    // A narrow range clamps the offsets; a power range that stops at 100 % keeps it there.
    let narrow = Limits {
        core: Some(Range { min: 0, max: 50 }),
        mem: None,
        power: Some(Range { min: 80, max: 100 }),
        power_default_w: Some(80.0),
    };
    let all = suggestions(Some(Family::Blackwell), false, &narrow);
    let perf = all.iter().find(|p| p.id == PERFORMANCE).unwrap().settings;
    assert_eq!(
        perf,
        Settings {
            core_mhz: 50,
            mem_mhz: 0,
            power_pct: 100
        }
    );
    let quiet = all.iter().find(|p| p.id == QUIET).unwrap().settings;
    assert_eq!(quiet.power_pct, 80, "75 % clamped to the driver's lowest");

    // No family: only Stock.
    assert_eq!(suggestions(None, false, &limits()).len(), 1);
}

#[test]
fn every_family_has_offsets_below_the_typical_card() {
    for family in [
        Family::Turing,
        Family::Ampere,
        Family::Ada,
        Family::Blackwell,
    ] {
        for laptop in [false, true] {
            let all = suggestions(Some(family), laptop, &limits());
            let perf = all.iter().find(|p| p.id == PERFORMANCE).unwrap().settings;
            assert!((25..=150).contains(&perf.core_mhz), "{family:?} {laptop}");
            assert!((200..=800).contains(&perf.mem_mhz), "{family:?} {laptop}");
            let eff = all.iter().find(|p| p.id == EFFICIENT).unwrap().settings;
            assert!(eff.core_mhz < perf.core_mhz && eff.power_pct < 100);
        }
    }
}

#[test]
fn validation() {
    let l = limits();
    let sugg = suggestions(Some(Family::Ada), false, &l);
    let ok = TuneConfig {
        enabled: true,
        presets: vec![custom("mine", PLUS)],
        preset: "mine".into(),
        ..TuneConfig::default()
    };
    assert!(validate(&ok, Some(&l), &sugg).is_ok());
    // Not possible here.
    assert!(validate(&ok, None, &sugg).is_err());
    assert!(validate(&TuneConfig::default(), None, &sugg).is_ok());
    // Outside the driver's range.
    let mut c = ok.clone();
    c.presets[0].settings.core_mhz = 1200;
    assert!(validate(&c, Some(&l), &sugg).is_err());
    // A setting the driver does not allow must stay stock.
    let no_power = Limits { power: None, ..l };
    assert!(validate(&ok, Some(&no_power), &sugg).is_err());
    // Ids: not Vyro's, not twice, plain.
    let mut c = ok.clone();
    c.presets[0].id = PERFORMANCE.into();
    assert!(validate(&c, Some(&l), &sugg).is_err());
    let mut c = ok.clone();
    c.presets.push(custom("mine", Settings::STOCK));
    assert!(validate(&c, Some(&l), &sugg).is_err());
    let mut c = ok.clone();
    c.presets[0].id = "a/b".into();
    assert!(validate(&c, Some(&l), &sugg).is_err());
    // Names.
    let mut c = ok.clone();
    c.presets[0].name = " padded".into();
    assert!(validate(&c, Some(&l), &sugg).is_err());
    // Only the owner's presets are saved.
    let mut c = ok.clone();
    c.presets[0].kind = PresetKind::Suggested;
    assert!(validate(&c, Some(&l), &sugg).is_err());
    // The preset in force and the profiles' presets exist.
    let mut c = ok.clone();
    c.preset = "gone".into();
    assert!(validate(&c, Some(&l), &sugg).is_err());
    let mut c = ok.clone();
    c.profiles
        .insert(crate::profiles::Profile::Gaming, "gone".into());
    assert!(validate(&c, Some(&l), &sugg).is_err());
    let mut c = ok;
    c.profiles
        .insert(crate::profiles::Profile::Gaming, PERFORMANCE.into());
    assert!(validate(&c, Some(&l), &sugg).is_ok());
}

#[test]
fn tested_settings() {
    let mut c = TuneConfig::default();
    assert_eq!(
        c.tested_at(&Settings::STOCK),
        Some(0),
        "stock always counts"
    );
    assert_eq!(c.tested_at(&PLUS), None);
    c.mark_tested(PLUS, 100);
    c.mark_tested(PLUS, 200);
    assert_eq!(c.tested_at(&PLUS), Some(200));
    assert_eq!(c.tested.len(), 1);
    for i in 0..40 {
        c.mark_tested(
            Settings {
                core_mhz: i,
                ..PLUS
            },
            i64::from(i),
        );
    }
    assert_eq!(c.tested.len(), MAX_TESTED);
}

#[test]
fn temperature_guard_needs_two_readings() {
    let mut g = TempGuard::default();
    assert_eq!(g.check(Some(91.0), None), None);
    assert!(g.check(Some(90.0), None).is_some());
    let mut g = TempGuard::default();
    assert_eq!(g.check(Some(91.0), None), None);
    assert_eq!(
        g.check(Some(70.0), None),
        None,
        "a cool reading starts over"
    );
    assert_eq!(g.check(Some(91.0), None), None);
    let mut g = TempGuard::default();
    assert_eq!(g.check(Some(80.0), Some(106.0)), None);
    let why = g.check(Some(80.0), Some(105.0)).unwrap();
    assert!(why.contains("hotspot"), "{why}");
    let mut g = TempGuard::default();
    assert_eq!(g.check(None, None), None);
    assert_eq!(g.check(None, None), None);
}

#[test]
fn crash_verdicts() {
    let m = Marker {
        since: 1000,
        preset: "mine".into(),
        settings: PLUS,
        trial: false,
    };
    assert_eq!(
        crash_verdict(None, &[(2000, CrashKind::Shutdown)]),
        Verdict::Clean
    );
    assert_eq!(crash_verdict(Some(m.clone()), &[]), Verdict::Unclean);
    // Before the settings went on: not theirs.
    assert_eq!(
        crash_verdict(Some(m.clone()), &[(900, CrashKind::Shutdown)]),
        Verdict::Unclean
    );
    // The first one after counts (a few seconds of slack).
    let v = crash_verdict(
        Some(m.clone()),
        &[(1500, CrashKind::Shutdown), (998, CrashKind::DriverReset)],
    );
    assert_eq!(
        v,
        Verdict::Crashed {
            kind: CrashKind::DriverReset,
            at: 998,
            marker: m
        }
    );
}

#[test]
fn drift_compares_only_what_the_driver_allows() {
    let l = limits();
    assert!(!drifted(&PLUS, &PLUS, &l));
    assert!(drifted(
        &PLUS,
        &Settings {
            core_mhz: 0,
            ..PLUS
        },
        &l
    ));
    assert!(
        !drifted(
            &PLUS,
            &Settings {
                power_pct: 109,
                ..PLUS
            },
            &l
        ),
        "power within 1 %"
    );
    assert!(drifted(
        &PLUS,
        &Settings {
            power_pct: 100,
            ..PLUS
        },
        &l
    ));
    let no_mem = Limits { mem: None, ..l };
    assert!(!drifted(&PLUS, &Settings { mem_mhz: 0, ..PLUS }, &no_mem));
}

#[test]
fn describing_settings() {
    assert_eq!(describe(&Settings::STOCK), "stock");
    assert_eq!(
        describe(&PLUS),
        "core +90 MHz, memory +500 MHz, power 110 %"
    );
    assert_eq!(
        describe(&Settings {
            core_mhz: -30,
            ..Settings::STOCK
        }),
        "core -30 MHz"
    );
}

// ---- Find my best ----

/// Runs a search against a card that is stable up to `core_max` and `mem_max`, with memory
/// bandwidth that drops past `mem_fast`.
fn run_search(
    start: Settings,
    l: Limits,
    core_max: i32,
    mem_max: i32,
    mem_fast: i32,
) -> (Next, u32) {
    let mut s = Search::new(start, l);
    let mut next = s.first();
    let mut tests = 0;
    loop {
        let Next::Test { settings, secs, .. } = next else {
            return (next, tests);
        };
        tests += 1;
        assert!(tests < 100, "the search ends");
        assert!(l.allows(&settings));
        let long = secs == search::CONFIRM_SECS;
        let passed = settings.core_mhz <= core_max && settings.mem_mhz <= mem_max;
        let bandwidth = (s.phase() == search::Phase::Memory).then(|| {
            let m = f64::from(settings.mem_mhz);
            if settings.mem_mhz <= mem_fast {
                500.0 + m / 10.0
            } else {
                500.0
            }
        });
        let _ = long;
        next = s.result(passed, bandwidth);
    }
}

#[test]
fn search_climbs_then_keeps_a_margin() {
    let start = Settings {
        core_mhz: 100,
        mem_mhz: 600,
        power_pct: 100,
    };
    let (end, tests) = run_search(start, limits(), 175, 1100, 5000);
    // Core: 100..175 pass, 190 fails → 175 − 15 = 160. Memory: 600..1100 pass, 1200 fails →
    // 1100 − 100 = 1000. Power at the highest allowed.
    assert_eq!(
        end,
        Next::Done(Settings {
            core_mhz: 160,
            mem_mhz: 1000,
            power_pct: 115
        })
    );
    assert!(tests < 20, "{tests}");
}

#[test]
fn search_stops_memory_when_it_gets_slower() {
    let start = Settings {
        core_mhz: 100,
        mem_mhz: 600,
        power_pct: 100,
    };
    // Stable to +1400, but past +900 the bandwidth drops: errors being corrected.
    let (end, _) = run_search(start, limits(), 400, 1400, 900);
    let Next::Done(s) = end else {
        panic!("{end:?}")
    };
    assert_eq!(s.mem_mhz, 800, "the last faster step, less the margin");
    assert_eq!(
        s.core_mhz,
        100 + search::CORE_CLIMB - search::CORE_MARGIN,
        "core climb is capped"
    );
}

#[test]
fn search_lowers_a_result_that_fails_the_long_test() {
    let start = Settings {
        core_mhz: 100,
        mem_mhz: 600,
        power_pct: 100,
    };
    let mut s = Search::new(start, limits());
    let mut next = s.first();
    let mut confirms = 0;
    while let Next::Test { settings, secs, .. } = next {
        let passed = if secs == search::CONFIRM_SECS {
            confirms += 1;
            confirms == 2
        } else {
            settings.core_mhz <= 130 && settings.mem_mhz <= 700
        };
        next = s.result(passed, None);
    }
    // 130 − 15 = 115 and 700 − 100 = 600 failed the long test; one step lower passed.
    assert_eq!(
        next,
        Next::Done(Settings {
            core_mhz: 100,
            mem_mhz: 500,
            power_pct: 115
        })
    );
}

#[test]
fn search_gives_up_when_nothing_passes() {
    let start = Settings {
        core_mhz: 100,
        mem_mhz: 600,
        power_pct: 100,
    };
    let (end, _) = run_search(start, limits(), -1, -1, 0);
    assert!(matches!(end, Next::GaveUp(_)), "{end:?}");
    let none = Limits {
        core: None,
        mem: None,
        ..limits()
    };
    assert!(matches!(Search::new(start, none).first(), Next::GaveUp(_)));
    // Memory only (no core range).
    let mem_only = Limits {
        core: None,
        ..limits()
    };
    let (end, _) = run_search(start, mem_only, 0, 800, 5000);
    assert_eq!(
        end,
        Next::Done(Settings {
            core_mhz: 0,
            mem_mhz: 700,
            power_pct: 115
        })
    );
}

// ---- The controller ----

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn fast(dir: Option<&std::path::Path>) -> Options {
    Options {
        dir: dir.map(std::path::Path::to_path_buf),
        check_every: Duration::from_millis(20),
        ..Options::memory()
    }
}

fn on(preset: &str) -> TuneConfig {
    TuneConfig {
        enabled: true,
        preset: preset.into(),
        presets: vec![custom("mine", PLUS)],
        ..TuneConfig::default()
    }
}

fn snapshot(edge: f64) -> GpuSnapshot {
    GpuSnapshot {
        name: "NVIDIA GeForce RTX 4070 (simulated)".into(),
        temp_edge: Some(edge),
        ..GpuSnapshot::default()
    }
}

#[test]
fn applies_reads_back_and_restores_at_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let dev = SimDevice::new();
    let c = TuneControl::start(Box::new(dev), fast(Some(dir.path())));
    assert!(matches!(c.capability(), TuneCapability::Writable { .. }));
    assert!(!c.brief().tuned);
    c.set_config(on("mine")).unwrap();
    wait_for("tuned", || c.status().current == Some(PLUS));
    let st = c.status();
    assert_eq!(st.applied.as_deref(), Some("mine"));
    assert_eq!(
        st.config.gpu.as_deref(),
        Some("GPU-sim"),
        "bound to the card"
    );
    assert!(c.brief().tuned);
    assert!(dir.path().join(MARKER_FILE).exists());
    c.shutdown();
    assert!(
        !dir.path().join(MARKER_FILE).exists(),
        "stock again, marker gone"
    );
}

#[test]
fn return_to_stock_and_apply_again() {
    let c = TuneControl::start(Box::new(SimDevice::new()), fast(None));
    c.set_config(on("mine")).unwrap();
    wait_for("tuned", || c.brief().tuned);
    c.restore().unwrap();
    wait_for("stock", || c.status().current == Some(Settings::STOCK));
    assert!(c.status().held.unwrap().contains("Returned to stock"));
    c.set_config(on("mine")).unwrap();
    wait_for("tuned again", || c.brief().tuned);
}

#[test]
fn profiles_have_their_own_preset() {
    let c = TuneControl::start(Box::new(SimDevice::new()), fast(None));
    let mut cfg = on(STOCK);
    cfg.profiles
        .insert(crate::profiles::Profile::Gaming, "mine".into());
    c.set_config(cfg).unwrap();
    wait_for("stock", || c.status().current == Some(Settings::STOCK));
    c.set_profile(crate::profiles::Profile::Gaming).unwrap();
    wait_for("gaming preset", || c.status().current == Some(PLUS));
    c.set_profile(crate::profiles::Profile::Desktop).unwrap();
    wait_for("back to stock", || {
        c.status().current == Some(Settings::STOCK)
    });
}

#[test]
fn heat_holds_the_card_at_stock() {
    let c = TuneControl::start(Box::new(SimDevice::new()), fast(None));
    c.set_config(on("mine")).unwrap();
    wait_for("tuned", || c.brief().tuned);
    c.on_tick(&[snapshot(92.0)]);
    assert!(c.brief().tuned, "one reading is not enough");
    c.on_tick(&[snapshot(93.0)]);
    wait_for("stock", || c.status().current == Some(Settings::STOCK));
    assert!(c.brief().held);
    assert!(c.status().held.unwrap().contains("93 °C"));
    // A profile change does not undo it; applying again does.
    c.set_profile(crate::profiles::Profile::Gaming).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(c.status().current, Some(Settings::STOCK));
    c.set_config(on("mine")).unwrap();
    wait_for("tuned again", || c.status().current == Some(PLUS));
}

#[test]
fn another_program_changing_the_card_ends_tuning() {
    let dev = SimDevice::new();
    let other = Arc::clone(&dev.other);
    let c = TuneControl::start(Box::new(dev), fast(None));
    c.set_config(on("mine")).unwrap();
    wait_for("tuned", || c.status().current == Some(PLUS));
    for _ in 0..3 {
        *crate::lock(&other) = Some(Settings {
            core_mhz: 200,
            ..Settings::STOCK
        });
        wait_for("rewritten or held", || {
            crate::lock(&other).is_none() || c.brief().held
        });
    }
    wait_for("held", || c.brief().held);
    let held = c.status().held.unwrap();
    assert!(held.contains("Another program"), "{held}");
    let kinds: Vec<TuneEventKind> = c.events().iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&TuneEventKind::Rewritten));
    assert!(kinds.contains(&TuneEventKind::Fault));
}

#[test]
fn a_failed_write_holds_at_stock() {
    let dev = SimDevice::new();
    let fail = Arc::clone(&dev.fail);
    let c = TuneControl::start(Box::new(dev), fast(None));
    *crate::lock(&fail) = true;
    c.set_config(on("mine")).unwrap();
    wait_for("held", || c.brief().held);
    assert!(c.status().last_error.unwrap().contains("refused"));
}

#[test]
fn a_crash_while_tuned_turns_apply_at_start_off() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = on("mine");
    cfg.apply_at_start = true;
    cfg.mark_tested(PLUS, 50);
    cfg.gpu = Some("GPU-sim".into());
    crate::files::write_json(&dir.path().join(controller::CONFIG_FILE), &cfg).unwrap();
    let marker = Marker {
        since: 1000,
        preset: "mine".into(),
        settings: PLUS,
        trial: false,
    };
    crate::files::write_json(&dir.path().join(MARKER_FILE), &marker).unwrap();
    let opts = Options {
        crashes: Box::new(|_| vec![(1200, CrashKind::Shutdown)]),
        ..fast(Some(dir.path()))
    };
    let c = TuneControl::start(Box::new(SimDevice::new()), opts);
    let st = c.status();
    assert!(!st.config.apply_at_start);
    let why = st.held.unwrap();
    assert!(why.contains("stopped without shutting down"), "{why}");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(c.status().current, Some(Settings::STOCK));
    assert!(!dir.path().join(MARKER_FILE).exists());
    let saved: TuneConfig =
        crate::files::read_json(&dir.path().join(controller::CONFIG_FILE), |_| Ok(()))
            .unwrap()
            .unwrap();
    assert!(!saved.apply_at_start, "saved off");
}

#[test]
fn apply_at_start_only_for_tested_settings() {
    let start = |tested: bool| {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = on("mine");
        cfg.apply_at_start = true;
        cfg.gpu = Some("GPU-sim".into());
        if tested {
            cfg.mark_tested(PLUS, 50);
        }
        crate::files::write_json(&dir.path().join(controller::CONFIG_FILE), &cfg).unwrap();
        let c = TuneControl::start(Box::new(SimDevice::new()), fast(Some(dir.path())));
        std::thread::sleep(Duration::from_millis(60));
        (c.status(), dir)
    };
    let (st, _d) = start(true);
    assert_eq!(st.current, Some(PLUS));
    let (st, _d) = start(false);
    assert_eq!(st.current, Some(Settings::STOCK));
    assert!(st.held.unwrap().contains("stability test"));
}

#[test]
fn presets_from_another_card_are_not_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = on("mine");
    cfg.apply_at_start = true;
    cfg.gpu = Some("GPU-old".into());
    cfg.mark_tested(PLUS, 50);
    crate::files::write_json(&dir.path().join(controller::CONFIG_FILE), &cfg).unwrap();
    let c = TuneControl::start(Box::new(SimDevice::new()), fast(Some(dir.path())));
    std::thread::sleep(Duration::from_millis(60));
    let st = c.status();
    assert_eq!(st.current, Some(Settings::STOCK));
    assert!(st.held.unwrap().contains("another card"));
    // Applying binds them to this card; the old card's test results do not carry over.
    c.set_config(st.config).unwrap();
    let st = c.status();
    assert_eq!(st.config.gpu.as_deref(), Some("GPU-sim"));
    assert!(st.config.tested.is_empty());
}

#[test]
fn a_trial_ends_by_itself_and_marks_tested_settings() {
    let c = TuneControl::start(Box::new(SimDevice::new()), fast(None));
    let trial = |s| Trial {
        settings: s,
        step: "test".into(),
        done: 0.0,
    };
    assert!(c.trial(Some(trial(PLUS))).is_err(), "tuning is off");
    c.set_config(on(STOCK)).unwrap();
    c.trial(Some(trial(PLUS))).unwrap();
    wait_for("on trial", || c.status().current == Some(PLUS));
    assert!(c.status().trial.is_some());
    assert_eq!(
        c.status().applied,
        None,
        "a trial is not the preset in force"
    );
    c.trial(None).unwrap();
    wait_for("back", || c.status().current == Some(Settings::STOCK));
    c.tested(PLUS, true, "").unwrap();
    let st = c.status();
    assert!(st.config.tested_at(&PLUS).is_some());
    let mine = st.presets.iter().find(|p| p.preset.id == "mine").unwrap();
    assert!(
        mine.tested_at.is_some(),
        "a preset with those settings shows as tested"
    );
    let out = Settings {
        core_mhz: 5000,
        ..PLUS
    };
    assert!(
        c.trial(Some(trial(out))).is_err(),
        "outside the driver's range"
    );
}

#[test]
fn sleep_gives_stock_and_waking_applies_again() {
    let c = TuneControl::start(Box::new(SimDevice::new()), fast(None));
    c.set_config(on("mine")).unwrap();
    wait_for("tuned", || c.status().current == Some(PLUS));
    c.set_suspended(true);
    assert_eq!(
        c.status().current,
        Some(Settings::STOCK),
        "stock before set_suspended returns"
    );
    c.set_suspended(false);
    wait_for("tuned again", || c.status().current == Some(PLUS));
}

#[test]
fn cards_vyro_does_not_tune() {
    let old = SimDevice::new().with_info(
        GpuInfo {
            name: "NVIDIA GeForce GTX 1080".into(),
            uuid: "GPU-1080".into(),
            family: None,
            laptop: false,
            driver: "x".into(),
        },
        limits(),
    );
    let c = TuneControl::start(Box::new(old), fast(None));
    assert!(matches!(c.capability(), TuneCapability::ReadOnly { .. }));
    assert!(c.set_config(on("mine")).is_err());
    let ada = GpuInfo {
        name: "NVIDIA GeForce RTX 4060 Laptop GPU".into(),
        uuid: "GPU-4060".into(),
        family: Some(Family::Ada),
        laptop: true,
        driver: "x".into(),
    };
    let locked = SimDevice::new().with_info(ada, Limits::default());
    let c = TuneControl::start(Box::new(locked), fast(None));
    assert!(matches!(c.capability(), TuneCapability::ReadOnly { .. }));
}

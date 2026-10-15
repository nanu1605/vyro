use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::gpu::{Gpu, GpuError, GpuFactory};
use super::runner::{self, Control, RunInputs, Scale};
use super::*;

fn no_stop() -> AtomicBool {
    AtomicBool::new(false)
}

#[test]
fn median_and_spread() {
    assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
    assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
    assert_eq!(median(&[]), 0.0);
    let s = spread(&[95.0, 100.0, 105.0]).unwrap();
    assert!((s - 0.1).abs() < 1e-12);
    assert_eq!(spread(&[1.0]), None);
    let m = Metric::measured("k", "K", "pts", Better::Higher, vec![10.0, 12.0, 11.0]);
    assert_eq!((m.value, m.passes.len()), (11.0, 3));
}

#[test]
fn cpu_rounds_are_deterministic_and_checked() {
    let a = cpu::Inputs::new();
    let b = cpu::Inputs::new();
    let r = cpu::reference(&a);
    assert_eq!(r, cpu::reference(&b), "same inputs, same checksum");
    let errors = AtomicU32::new(0);
    let until = Instant::now() + Duration::from_millis(30);
    let n = cpu::spin(&a, r, until, &no_stop(), &errors, None);
    assert!(n > 0);
    assert_eq!(errors.load(Ordering::SeqCst), 0);
    // A round that disagrees with the reference counts as an error.
    let until = Instant::now() + Duration::from_millis(10);
    let n = cpu::spin(&a, r ^ 1, until, &no_stop(), &errors, None);
    assert_eq!(u64::from(errors.load(Ordering::SeqCst)), n);
    // A stop ends it at once.
    let stop = AtomicBool::new(true);
    let until = Instant::now() + Duration::from_secs(60);
    assert_eq!(cpu::spin(&a, r, until, &stop, &errors, None), 0);
    let pts = cpu::pass(&a, r, 2, 0.03, &no_stop(), &AtomicU32::new(0));
    assert!(pts > 0.0);
}

#[test]
fn memory_bandwidth_is_checked_and_the_chain_is_one_cycle() {
    let mut buf = memory::Buffers::new(1 << 15).unwrap();
    let errors = AtomicU32::new(0);
    assert!(memory::read(&buf, 3, 0.02, &no_stop(), &errors) > 0.0);
    assert!(memory::copy(&mut buf, 3, 0.02, &no_stop(), &errors) > 0.0);
    assert_eq!(errors.load(Ordering::SeqCst), 0);
    assert_eq!(buf.bytes(), 1 << 18);
    let chain = memory::Chain::new(1 << 16).unwrap();
    // 64 KiB of 4-byte slots, one per 64-byte line: 1024 lines in a single cycle.
    assert_eq!(chain.cycle_len(), 1024);
    assert!(chain.latency(0.01, &no_stop()).unwrap() > 0.0);
    assert_eq!(chain.latency(1.0, &AtomicBool::new(true)), None);
}

#[test]
fn storage_reads_back_what_it_wrote_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("vyro-bench-1.tmp"), b"left by a crash").unwrap();
    std::fs::write(dir.path().join("keep.tmp"), b"not ours").unwrap();
    let file = storage::TempFile::create(dir.path(), 8 << 20).unwrap();
    assert!(
        !dir.path().join("vyro-bench-1.tmp").exists(),
        "stale file deleted"
    );
    assert!(dir.path().join("keep.tmp").exists());
    let errors = AtomicU32::new(0);
    let w = storage::sequential(&file, true, None, &no_stop(), &errors).unwrap();
    let r = storage::sequential(&file, false, Some(5.0), &no_stop(), &errors).unwrap();
    let iops = storage::random_read(&file, 0.02, 7, &no_stop(), &errors).unwrap();
    assert!(w > 0.0 && r > 0.0 && iops > 0.0);
    assert_eq!(errors.load(Ordering::SeqCst), 0);
    // A sector that changed behind our back is found.
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(file.path())
            .unwrap();
        f.seek(SeekFrom::Start(3 * storage::SECTOR as u64)).unwrap();
        f.write_all(&[0u8; 8]).unwrap();
    }
    storage::sequential(&file, false, None, &no_stop(), &errors).unwrap();
    assert_eq!(errors.load(Ordering::SeqCst), 1);
    let path = file.path().to_owned();
    drop(file);
    assert!(!path.exists(), "deleted when done");
}

#[test]
fn frame_stats_give_the_rate_and_the_one_percent_low() {
    let mut times = vec![10.0; 99];
    times.push(40.0);
    let (fps, low) = gpu::frame_stats(&times, 1.0).unwrap();
    assert_eq!(fps, 100.0);
    assert_eq!(low, 100.0, "the 99th percentile of 100 frames is the 99th");
    times.push(50.0);
    let (_, low) = gpu::frame_stats(&times, 1.0).unwrap();
    assert_eq!(low, 25.0);
    assert_eq!(gpu::frame_stats(&[], 1.0), None);
    assert_ne!(gpu::check_hash(1), gpu::check_hash(2));
}

// ---------------------------------------------------------------------------------------------
// The runner, with a fake GPU and fake readings.

struct FakeGpu {
    reset_in_frames: bool,
}

impl Gpu for FakeGpu {
    fn name(&self) -> String {
        "Fake GPU".into()
    }
    fn compute(&mut self, secs: f64, stop: &AtomicBool) -> Result<f64, GpuError> {
        busy(secs, stop);
        Ok(1000.0)
    }
    fn bandwidth(&mut self, secs: f64, stop: &AtomicBool) -> Result<f64, GpuError> {
        busy(secs, stop);
        Ok(400.0)
    }
    fn check(&mut self) -> Result<u32, GpuError> {
        Ok(0)
    }
    fn frames(
        &mut self,
        secs: f64,
        stop: &AtomicBool,
        times: &mut Vec<f64>,
        on_frame: &mut dyn FnMut(),
    ) -> Result<(), GpuError> {
        if self.reset_in_frames {
            return Err(GpuError::Reset("DXGI_ERROR_DEVICE_REMOVED".into()));
        }
        let until = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(2));
            times.push(2.0);
            on_frame();
        }
        Ok(())
    }
}

fn busy(secs: f64, stop: &AtomicBool) {
    let until = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < until && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn fake_gpu(reset: bool) -> GpuFactory {
    Arc::new(move || {
        Ok(Box::new(FakeGpu {
            reset_in_frames: reset,
        }) as Box<dyn Gpu>)
    })
}

/// Fresh readings on every look, from a template the test can change.
struct FakeProbe {
    sample: Mutex<Sample>,
    frozen: Option<Instant>,
}

impl FakeProbe {
    fn new(s: Sample) -> Self {
        Self {
            sample: Mutex::new(s),
            frozen: None,
        }
    }
}

impl Probe for FakeProbe {
    fn sample(&self) -> Option<Sample> {
        let mut s = crate::lock(&self.sample).clone();
        s.at = Some(self.frozen.unwrap_or_else(Instant::now));
        Some(s)
    }
}

fn reading() -> Sample {
    Sample {
        cpu_temp: Some(70.0),
        cpu_power: Some(88.0),
        cpu_mhz: Some(5200.0),
        cpu_usage: Some(3.0),
        cpu_throttle: Some(false),
        gpus: vec![GpuReading {
            name: "Fake GPU".into(),
            integrated: Some(false),
            temp: Some(60.0),
            power: Some(150.0),
            mhz: Some(2800.0),
            throttle: Some(false),
        }],
        available_mb: Some(16_000.0),
        ..Sample::default()
    }
}

fn inputs(mode: BenchMode, dir: &std::path::Path, time: f64, gpu: Option<GpuFactory>) -> RunInputs {
    RunInputs {
        mode,
        setup: BenchSetup {
            vyro: "test".into(),
            ..BenchSetup::default()
        },
        temp_dir: dir.to_owned(),
        free_bytes: Some(100 << 30),
        threads: 2,
        gpu,
        gpu_name: Some("Fake GPU".into()),
        hardware_errors: Some(Box::new(|_| Some(0))),
        scale: Scale {
            time,
            mem_bytes: 1 << 20,
            file_bytes: 4 << 20,
        },
    }
}

#[test]
fn a_quick_run_scores_every_area_and_watches_it() {
    let dir = tempfile::tempdir().unwrap();
    let probe = FakeProbe::new(reading());
    let ctl = Control::new();
    let seen = Mutex::new(Vec::new());
    let run = runner::run(
        &inputs(BenchMode::Quick, dir.path(), 0.01, Some(fake_gpu(false))),
        &ctl,
        &probe,
        &|p| crate::lock(&seen).push(p.done),
    );
    assert_eq!(run.status, RunStatus::Completed, "{:?}", run.reason);
    assert_eq!(run.method, METHOD_VERSION);
    assert!(!run.busy);
    assert_eq!(
        run.errors,
        BenchErrors {
            hardware: Some(0),
            ..BenchErrors::default()
        }
    );
    let keys: Vec<&str> = run.metrics().map(|(_, m)| m.key.as_str()).collect();
    for k in [
        "cpu.single",
        "cpu.multi",
        "cpu.scaling",
        "mem.read",
        "mem.copy",
        "mem.latency",
        "disk.read",
        "disk.write",
        "disk.random",
        "gpu.compute",
        "gpu.bandwidth",
        "gfx.fps",
        "gfx.low",
        "eff.cpu",
        "eff.gfx",
        "stab.calc",
        "stab.resets",
    ] {
        assert!(keys.contains(&k), "{k} missing from {keys:?}");
    }
    assert!(
        run.area(AreaId::Sustained).is_none(),
        "Quick has no Sustained"
    );
    let multi = run.area(AreaId::CpuMulti).unwrap();
    let m = multi.metric("cpu.multi").unwrap();
    assert_eq!(m.passes.len(), PASSES);
    let w = multi.watch.as_ref().unwrap();
    assert_eq!((w.cpu_temp_max, w.cpu_power_avg), (Some(70.0), Some(88.0)));
    assert_eq!(w.throttled_secs, Some(0.0));
    let eff = run
        .area(AreaId::Efficiency)
        .unwrap()
        .metric("eff.cpu")
        .unwrap()
        .value;
    assert!((eff - m.value / 88.0).abs() < 1e-9);
    assert_eq!(
        run.area(AreaId::Graphics)
            .unwrap()
            .metric("gfx.fps")
            .unwrap()
            .passes
            .len(),
        3
    );
    let done = crate::lock(&seen).clone();
    assert!(
        done.windows(2).all(|w| w[1] >= w[0] - 1e-9),
        "progress only moves forward"
    );
    assert!(done.last().is_some_and(|d| *d > 0.99));
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "the file is gone"
    );
}

#[test]
fn a_full_run_adds_sustained_loops_with_stability() {
    let dir = tempfile::tempdir().unwrap();
    let probe = FakeProbe::new(reading());
    let run = runner::run(
        &inputs(BenchMode::Full, dir.path(), 0.004, Some(fake_gpu(false))),
        &Control::new(),
        &probe,
        &|_| {},
    );
    assert_eq!(run.status, RunStatus::Completed, "{:?}", run.reason);
    let s = run.area(AreaId::Sustained).unwrap();
    assert_eq!(s.loops.len(), SUSTAIN_LOOPS, "{:?}", s.missing);
    assert!(s.loops.iter().all(|l| l.cpu.is_some() && l.gpu.is_some()));
    let st = s.metric("sustain.stability").unwrap().value;
    assert!(st > 0.0 && st <= 100.0);
    assert_eq!(
        s.metric("sustain.cpuSag").unwrap().value,
        0.0,
        "steady clocks, no sag"
    );
}

#[test]
fn no_gpu_leaves_the_gpu_areas_missing() {
    let dir = tempfile::tempdir().unwrap();
    let mut inp = inputs(BenchMode::Quick, dir.path(), 0.01, None);
    inp.free_bytes = Some(1 << 30);
    let run = runner::run(&inp, &Control::new(), &FakeProbe::new(reading()), &|_| {});
    assert_eq!(run.status, RunStatus::Completed);
    for a in [AreaId::GpuCompute, AreaId::Graphics] {
        assert!(run
            .area(a)
            .unwrap()
            .missing
            .as_deref()
            .unwrap()
            .contains("No Direct3D 11 GPU"));
    }
    let storage = run.area(AreaId::Storage).unwrap();
    assert!(storage
        .missing
        .as_deref()
        .unwrap()
        .starts_with("Storage is skipped"));
    assert!(run
        .notes
        .iter()
        .any(|n| n.starts_with("Storage is skipped")));
    assert!(run
        .area(AreaId::Efficiency)
        .unwrap()
        .metric("eff.gfx")
        .is_none());
}

#[test]
fn a_gpu_reset_counts_towards_stability() {
    let dir = tempfile::tempdir().unwrap();
    let run = runner::run(
        &inputs(BenchMode::Quick, dir.path(), 0.01, Some(fake_gpu(true))),
        &Control::new(),
        &FakeProbe::new(reading()),
        &|_| {},
    );
    assert_eq!(run.errors.gpu_resets, 1);
    assert!(run
        .area(AreaId::Graphics)
        .unwrap()
        .missing
        .as_deref()
        .unwrap()
        .contains("reset"));
    assert!(run.area(AreaId::GpuCompute).unwrap().missing.is_none());
    let stab = run.area(AreaId::Stability).unwrap();
    assert!(stab.detail.as_deref().unwrap().starts_with("Errors found"));
}

#[test]
fn a_stop_ends_all_load_within_a_second() {
    let dir = tempfile::tempdir().unwrap();
    let probe = FakeProbe::new(reading());
    let ctl = Arc::new(Control::new());
    let stopper = Arc::clone(&ctl);
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        stopper.stop("Stopped by you.");
        Instant::now()
    });
    // Real durations: without the stop this would take minutes.
    let run = runner::run(
        &inputs(BenchMode::Full, dir.path(), 1.0, Some(fake_gpu(false))),
        &ctl,
        &probe,
        &|_| {},
    );
    let asked = t.join().unwrap();
    assert!(
        asked.elapsed() < Duration::from_secs(1),
        "took {:?}",
        asked.elapsed()
    );
    assert_eq!(run.status, RunStatus::Stopped);
    assert_eq!(run.reason.as_deref(), Some("Stopped by you."));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn heat_a_new_problem_or_lost_readings_stop_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let hot = Sample {
        cpu_temp: Some(99.4),
        ..reading()
    };
    let run = runner::run(
        &inputs(BenchMode::Quick, dir.path(), 0.05, None),
        &Control::new(),
        &FakeProbe::new(hot),
        &|_| {},
    );
    assert_eq!(run.status, RunStatus::Stopped);
    assert_eq!(
        run.reason.as_deref(),
        Some("The CPU reached 99 °C, Health's problem limit.")
    );

    // A problem already there at the start does not stop it; a new one does.
    let probe = FakeProbe::new(Sample {
        problems: vec!["Fan 2 stopped".into()],
        ..reading()
    });
    let ctl = Control::new();
    std::thread::scope(|sc| {
        sc.spawn(|| {
            std::thread::sleep(Duration::from_millis(150));
            crate::lock(&probe.sample)
                .problems
                .push("GPU fan stopped".into());
        });
        let run = runner::run(
            &inputs(BenchMode::Quick, dir.path(), 0.05, None),
            &ctl,
            &probe,
            &|_| {},
        );
        assert_eq!(
            run.reason.as_deref(),
            Some("Health reported a problem: GPU fan stopped")
        );
    });

    let frozen = FakeProbe {
        sample: Mutex::new(reading()),
        frozen: Some(Instant::now()),
    };
    let run = runner::run(
        &inputs(BenchMode::Quick, dir.path(), 0.05, None),
        &Control::new(),
        &frozen,
        &|_| {},
    );
    assert_eq!(run.status, RunStatus::Stopped);
    assert!(run.reason.unwrap().starts_with("No sensor reading"));
}

#[test]
fn a_busy_pc_and_a_missing_cpu_temperature_are_noted() {
    let dir = tempfile::tempdir().unwrap();
    let probe = FakeProbe::new(Sample {
        cpu_usage: Some(35.0),
        cpu_temp: None,
        ..reading()
    });
    let run = runner::run(
        &inputs(BenchMode::Quick, dir.path(), 0.01, None),
        &Control::new(),
        &probe,
        &|_| {},
    );
    assert!(run.busy);
    assert_eq!(run.setup.busy_pct, Some(35.0));
    assert!(run.notes.iter().any(|n| n.contains("busy")));
    assert!(run
        .notes
        .iter()
        .any(|n| n.contains("CPU temperature is not read")));
}

#[test]
fn preflight_blocks_a_game_heat_and_a_second_run() {
    let ok = preflight(
        Some(&reading()),
        false,
        Some("GPU".into()),
        None,
        Some(10 << 30),
    );
    assert!(ok.blockers.is_empty() && ok.notes.is_empty(), "{ok:?}");
    assert!(ok.full_secs > ok.quick_secs);
    let s = Sample {
        game: true,
        warnings: vec!["CPU at 91 °C".into()],
        cpu_temp: None,
        ..reading()
    };
    let p = preflight(Some(&s), true, None, None, Some(1 << 30));
    assert_eq!(p.blockers.len(), 3, "{:?}", p.blockers);
    assert_eq!(p.notes.len(), 3, "{:?}", p.notes);
    assert_eq!(preflight(None, false, None, None, None).blockers.len(), 1);
}

#[test]
fn samples_come_from_snapshots() {
    let mut snap = SensorSnapshot::default();
    snap.cpu.temp_package = Some(92.0);
    snap.cpu.power_pkg_w = Some(120.0);
    snap.gpu.push(crate::snapshot::GpuSnapshot {
        name: "AMD Radeon(TM) Graphics".into(),
        integrated: Some(true),
        ..Default::default()
    });
    snap.gpu.push(crate::snapshot::GpuSnapshot {
        name: "NVIDIA GeForce RTX 5060 Ti".into(),
        integrated: Some(false),
        temp_edge: Some(61.0),
        ..Default::default()
    });
    snap.health.subsystems.push(crate::info::SubsystemHealth {
        name: "Fans".into(),
        status: Status::Fail,
        messages: vec!["CPU fan stopped".into()],
        notes: Vec::new(),
    });
    let s = Sample::from_snapshot(&snap, Instant::now());
    assert_eq!(s.cpu_throttle, None, "no flags reported");
    assert_eq!(s.problems, ["CPU fan stopped"]);
    assert_eq!(s.warnings, ["CPU at 92 °C"]);
    assert_eq!(s.gpu(None).unwrap().temp, Some(61.0), "the discrete GPU");
    assert_eq!(
        s.gpu(Some("AMD Radeon(TM) Graphics")).unwrap().integrated,
        Some(true)
    );
    snap.cpu.throttle_thermal = Some(false);
    assert_eq!(
        Sample::from_snapshot(&snap, Instant::now()).cpu_throttle,
        Some(false)
    );
}

//! Runs a benchmark: settles, runs each area's workload on its own thread while this thread
//! watches the readings (peaks, power, clocks, throttling) and the safety limits, then works out
//! Efficiency and Stability. A stop (the owner's or a safety rule's) sets one flag every worker
//! checks; the runner waits for all of them, so nothing is left running when it returns.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::gpu::{self, GpuError, GpuFactory};
use super::{
    cpu, memory, storage, AreaId, AreaResult, BenchErrors, BenchMode, BenchProgress, BenchRun,
    BenchSetup, Better, Metric, Probe, RunStatus, Sample, SustainLoop, Watch, BUSY_PCT, CPU_STOP_C,
    FILE_BYTES, GPU_STOP_C, HOT_READINGS, METHOD_VERSION, MIN_AVAILABLE_MB, MIN_FREE_BYTES, PASSES,
    SETTLE_SECS, STALE_SECS, SUSTAIN_LOOPS, SUSTAIN_LOOP_SECS,
};
use crate::lock;

/// How often the runner looks at the readings and reports progress.
const TICK: Duration = Duration::from_millis(100);
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Durations and sizes; tests shrink them.
#[derive(Debug, Clone, Copy)]
pub struct Scale {
    pub time: f64,
    pub mem_bytes: usize,
    pub file_bytes: u64,
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            time: 1.0,
            mem_bytes: memory::BUFFER_BYTES,
            file_bytes: FILE_BYTES,
        }
    }
}

/// WHEA hardware errors logged since a Unix time; `None` when the log cannot be read.
pub type HardwareErrors = Box<dyn Fn(i64) -> Option<u32> + Send + Sync>;

pub struct RunInputs {
    pub mode: BenchMode,
    pub setup: BenchSetup,
    /// The folder for the storage file (the temporary folder).
    pub temp_dir: PathBuf,
    pub free_bytes: Option<u64>,
    /// Logical processors.
    pub threads: usize,
    pub gpu: Option<GpuFactory>,
    /// The GPU the GPU areas use, to watch its readings.
    pub gpu_name: Option<String>,
    pub hardware_errors: Option<HardwareErrors>,
    pub scale: Scale,
}

/// The stop flag and the first reason given.
#[derive(Default)]
pub struct Control {
    stop: AtomicBool,
    reason: Mutex<Option<String>>,
}

impl Control {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops the run; the first reason stands.
    pub fn stop(&self, reason: &str) {
        let mut r = lock(&self.reason);
        if r.is_none() {
            *r = Some(reason.to_owned());
        }
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub fn reason(&self) -> Option<String> {
        lock(&self.reason).clone()
    }
}

/// What an area's workload shares with the watching thread.
pub struct AreaCtx<'a> {
    pub stop: &'a AtomicBool,
    pub calc_errors: &'a AtomicU32,
    pub storage_errors: &'a AtomicU32,
    step: Mutex<String>,
    /// Progress within the area, 0–10 000.
    frac: AtomicU32,
    /// A measured pass is running (power and clock averages count only then).
    measuring: AtomicBool,
    /// Sustained: the loop running, from 1; 0 before the first.
    phase: AtomicUsize,
}

impl AreaCtx<'_> {
    fn at(&self, frac: f64, step: impl Into<String>) {
        *lock(&self.step) = step.into();
        self.frac
            .store((frac.clamp(0.0, 1.0) * 10_000.0) as u32, Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

enum Fail {
    Missing(String),
    Reset(String),
}

impl From<GpuError> for Fail {
    fn from(e: GpuError) -> Self {
        match e {
            GpuError::Reset(m) => Self::Reset(m),
            GpuError::Other(m) => Self::Missing(m),
        }
    }
}

#[derive(Default)]
struct AreaOut {
    metrics: Vec<Metric>,
    detail: Option<String>,
    loops: Vec<SustainLoop>,
}

const STOPPED: &str = "Stopped before this area finished.";

/// An instant at least [`PROGRESS_EVERY`] ago, so the first progress report goes out at once.
fn long_ago() -> Instant {
    let now = Instant::now();
    now.checked_sub(PROGRESS_EVERY).unwrap_or(now)
}

/// Runs `n` measured passes after one warm-up, reporting progress across `[0, 1]`.
fn passes(
    ctx: &AreaCtx,
    from: f64,
    to: f64,
    what: &str,
    mut one: impl FnMut(bool) -> Result<f64, Fail>,
) -> Result<Vec<f64>, Fail> {
    let span = (to - from) / (PASSES + 1) as f64;
    ctx.at(from, format!("{what}: warm-up"));
    one(true)?;
    let mut v = Vec::with_capacity(PASSES);
    for i in 0..PASSES {
        if ctx.stopped() {
            return Err(Fail::Missing(STOPPED.into()));
        }
        ctx.at(
            from + span * (i + 1) as f64,
            format!("{what}: pass {} of {PASSES}", i + 1),
        );
        ctx.measuring.store(true, Ordering::Relaxed);
        let r = one(false);
        ctx.measuring.store(false, Ordering::Relaxed);
        v.push(r?);
    }
    if ctx.stopped() {
        return Err(Fail::Missing(STOPPED.into()));
    }
    Ok(v)
}

/// The readings folded over an area (and, for Sustained, per loop).
#[derive(Default)]
struct Acc {
    secs: f64,
    cpu_temp_max: Option<f64>,
    cpu_power_max: Option<f64>,
    gpu_temp_max: Option<f64>,
    gpu_power_max: Option<f64>,
    cpu_power: Avg,
    cpu_mhz: Avg,
    gpu_power: Avg,
    gpu_mhz: Avg,
    flags: bool,
    throttled: f64,
    full_speed: f64,
}

#[derive(Default, Clone, Copy)]
struct Avg {
    sum: f64,
    n: u32,
}

impl Avg {
    fn add(&mut self, v: Option<f64>) {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            self.sum += v;
            self.n += 1;
        }
    }

    fn get(self) -> Option<f64> {
        (self.n > 0).then(|| self.sum / f64::from(self.n))
    }
}

fn max(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b.filter(|v| v.is_finite())) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

impl Acc {
    fn add(&mut self, s: &Sample, gpu_name: Option<&str>, dt: f64, measuring: bool) {
        let g = s.gpu(gpu_name);
        self.secs += dt;
        self.cpu_temp_max = max(self.cpu_temp_max, s.cpu_temp);
        self.cpu_power_max = max(self.cpu_power_max, s.cpu_power);
        self.gpu_temp_max = max(self.gpu_temp_max, g.and_then(|g| g.temp));
        self.gpu_power_max = max(self.gpu_power_max, g.and_then(|g| g.power));
        if measuring {
            self.cpu_power.add(s.cpu_power);
            self.cpu_mhz.add(s.cpu_mhz);
            self.gpu_power.add(g.and_then(|g| g.power));
            self.gpu_mhz.add(g.and_then(|g| g.mhz));
        }
        let gpu_flag = g.and_then(|g| g.throttle);
        if s.cpu_throttle.is_some() || gpu_flag.is_some() {
            self.flags = true;
            if s.cpu_throttle == Some(true) || gpu_flag == Some(true) {
                self.throttled += dt;
            }
        }
        if s.full_speed {
            self.full_speed += dt;
        }
    }

    fn watch(&self) -> Watch {
        Watch {
            secs: self.secs,
            cpu_temp_max: self.cpu_temp_max,
            cpu_power_max: self.cpu_power_max,
            cpu_power_avg: self.cpu_power.get(),
            cpu_mhz_avg: self.cpu_mhz.get(),
            gpu_temp_max: self.gpu_temp_max,
            gpu_power_max: self.gpu_power_max,
            gpu_power_avg: self.gpu_power.get(),
            gpu_mhz_avg: self.gpu_mhz.get(),
            throttled_secs: self.flags.then_some(self.throttled),
            full_speed_secs: self.full_speed,
        }
    }
}

/// Watches the readings for the whole run: folds them into the running area and stops the run
/// on a safety rule.
struct Monitor<'a> {
    ctl: &'a Control,
    probe: &'a dyn Probe,
    gpu_name: Option<String>,
    started: Instant,
    /// Readings had at the start: problems already there do not stop the run.
    initial_problems: Vec<String>,
    last_at: Option<Instant>,
    last_seen: Instant,
    hot_cpu: u32,
    hot_gpu: u32,
    planned: f64,
    /// No reading for this long stops the run, s ([`STALE_SECS`]; shorter in shrunk tests).
    stale: f64,
    /// CPU use while settling.
    usage: Avg,
    had_cpu_temp: bool,
}

impl Monitor<'_> {
    /// Looks at the newest reading; returns it when it is new, with the time since the last.
    fn poll(&mut self) -> Option<(Sample, f64)> {
        let now = Instant::now();
        if now.duration_since(self.started).as_secs_f64() > 2.0 * self.planned {
            self.ctl.stop("The run took twice its planned time.");
        }
        let s = self.probe.sample();
        let fresh = s
            .as_ref()
            .and_then(|s| s.at)
            .filter(|at| Some(*at) != self.last_at);
        let Some(at) = fresh else {
            if now.duration_since(self.last_seen).as_secs_f64() > self.stale {
                self.ctl.stop(&format!(
                    "No sensor reading for {STALE_SECS:.0} s, so the run could not watch the temperatures."
                ));
            }
            return None;
        };
        let s = s?;
        let dt = self
            .last_at
            .map_or(0.0, |l| at.duration_since(l).as_secs_f64().min(2.0));
        self.last_at = Some(at);
        self.last_seen = now;
        self.check(&s);
        Some((s, dt))
    }

    fn check(&mut self, s: &Sample) {
        self.had_cpu_temp |= s.cpu_temp.is_some();
        match s.cpu_temp.filter(|t| *t >= CPU_STOP_C) {
            Some(t) => {
                self.hot_cpu += 1;
                if self.hot_cpu >= HOT_READINGS {
                    self.ctl.stop(&format!(
                        "The CPU reached {t:.0} °C, Health's problem limit."
                    ));
                }
            }
            None => self.hot_cpu = 0,
        }
        let hot_gpu = s
            .gpus
            .iter()
            .filter_map(|g| g.temp.filter(|t| *t >= GPU_STOP_C).map(|t| (g, t)))
            .next();
        match hot_gpu {
            Some((g, t)) => {
                self.hot_gpu += 1;
                if self.hot_gpu >= HOT_READINGS {
                    self.ctl.stop(&format!(
                        "{} reached {t:.0} °C, Health's problem limit.",
                        g.name
                    ));
                }
            }
            None => self.hot_gpu = 0,
        }
        if let Some(p) = s
            .problems
            .iter()
            .find(|p| !self.initial_problems.contains(p))
        {
            self.ctl.stop(&format!("Health reported a problem: {p}"));
        }
        if s.game {
            self.ctl.stop("A game started.");
        }
    }
}

/// Everything shared by the areas of one run.
struct Run<'a> {
    inp: &'a RunInputs,
    ctl: &'a Control,
    calc: AtomicU32,
    storage_errs: AtomicU32,
    resets: AtomicU32,
    cpu_in: cpu::Inputs,
    cpu_ref: u64,
    planned: f64,
    done_planned: f64,
    finished: Vec<AreaResult>,
}

/// Runs a benchmark to the end or to a stop. `progress` is called about four times a second.
pub fn run(
    inp: &RunInputs,
    ctl: &Control,
    probe: &dyn Probe,
    progress: &dyn Fn(&BenchProgress),
) -> BenchRun {
    let ts = crate::unix_now();
    let t0 = Instant::now();
    let tscale = inp.scale.time;
    let plan = AreaId::plan(inp.mode);
    let planned = (SETTLE_SECS + plan.iter().map(|a| a.planned_secs()).sum::<f64>()) * tscale;
    super::full_speed_process(true);

    let initial = probe.sample();
    let mut mon = Monitor {
        ctl,
        probe,
        gpu_name: inp.gpu_name.clone(),
        started: t0,
        initial_problems: initial
            .as_ref()
            .map(|s| s.problems.clone())
            .unwrap_or_default(),
        last_at: None,
        last_seen: t0,
        hot_cpu: 0,
        hot_gpu: 0,
        planned,
        stale: STALE_SECS * tscale.min(1.0),
        usage: Avg::default(),
        had_cpu_temp: false,
    };
    let cpu_in = cpu::Inputs::new();
    let cpu_ref = cpu::reference(&cpu_in);
    let mut run = Run {
        inp,
        ctl,
        calc: AtomicU32::new(0),
        storage_errs: AtomicU32::new(0),
        resets: AtomicU32::new(0),
        cpu_in,
        cpu_ref,
        planned,
        done_planned: 0.0,
        finished: Vec::new(),
    };

    // Settle: watch the PC before any load, for background use.
    let settle_until = t0 + Duration::from_secs_f64(SETTLE_SECS * tscale);
    let mut last_progress = long_ago();
    while Instant::now() < settle_until && !ctl.stopped() {
        if let Some((s, _)) = mon.poll() {
            mon.usage.add(s.cpu_usage);
        }
        if last_progress.elapsed() >= PROGRESS_EVERY {
            last_progress = Instant::now();
            progress(&run.progress(None, "Settling", 0.0, t0));
        }
        std::thread::sleep(TICK.min(settle_until.saturating_duration_since(Instant::now())));
    }
    // A reading taken while settling counts too.
    if let Some((s, _)) = mon.poll() {
        mon.usage.add(s.cpu_usage);
    }
    run.done_planned = SETTLE_SECS * tscale;
    let busy_pct = mon.usage.get();
    let mut notes = Vec::new();
    if !mon.had_cpu_temp && initial.as_ref().is_none_or(|s| s.cpu_temp.is_none()) {
        notes.push(super::no_cpu_temp_note());
    }

    for area in plan {
        if ctl.stopped() {
            break;
        }
        let result = run.area(area, &mut mon, t0, progress);
        run.done_planned += area.planned_secs() * tscale;
        run.finished.push(result);
        progress(&run.progress(None, "", 0.0, t0));
    }
    super::full_speed_process(false);

    let mut areas = std::mem::take(&mut run.finished);
    if let Some(a) = areas
        .iter()
        .find(|a| a.area == AreaId::Storage)
        .and_then(|a| a.missing.as_ref())
        .filter(|m| m.starts_with("Storage is skipped"))
    {
        notes.push(a.clone());
    }
    derive_scaling(&mut areas);
    areas.push(efficiency(&areas));
    let errors = BenchErrors {
        calc: run.calc.load(Ordering::SeqCst),
        storage: run.storage_errs.load(Ordering::SeqCst),
        hardware: inp.hardware_errors.as_ref().and_then(|f| f(ts)),
        gpu_resets: run.resets.load(Ordering::SeqCst),
    };
    areas.push(stability(&errors));
    let busy = busy_pct.is_some_and(|b| b > BUSY_PCT);
    if busy {
        notes.push(format!(
            "The PC was busy before the start ({:.0} % CPU): the results are likely low.",
            busy_pct.unwrap_or(0.0)
        ));
    }
    let stopped = ctl.stopped();
    BenchRun {
        id: 0,
        ts,
        method: METHOD_VERSION,
        mode: inp.mode,
        status: if stopped {
            RunStatus::Stopped
        } else {
            RunStatus::Completed
        },
        reason: if stopped { ctl.reason() } else { None },
        secs: t0.elapsed().as_secs_f64(),
        busy,
        notes,
        setup: BenchSetup {
            busy_pct,
            ..inp.setup.clone()
        },
        areas,
        errors,
    }
}

impl Run<'_> {
    fn progress(&self, area: Option<AreaId>, step: &str, frac: f64, t0: Instant) -> BenchProgress {
        let cur = area.map_or(0.0, |a| a.planned_secs() * self.inp.scale.time * frac);
        BenchProgress {
            mode: self.inp.mode,
            area,
            step: step.into(),
            done: ((self.done_planned + cur) / self.planned.max(1e-9)).clamp(0.0, 1.0),
            elapsed: t0.elapsed().as_secs_f64(),
            planned: self.planned,
            finished: self.finished.clone(),
        }
    }

    /// Runs one area's workload on its own thread while this one watches.
    fn area(
        &self,
        area: AreaId,
        mon: &mut Monitor,
        t0: Instant,
        progress: &dyn Fn(&BenchProgress),
    ) -> AreaResult {
        let ctx = AreaCtx {
            stop: &self.ctl.stop,
            calc_errors: &self.calc,
            storage_errors: &self.storage_errs,
            step: Mutex::new(String::new()),
            frac: AtomicU32::new(0),
            measuring: AtomicBool::new(false),
            phase: AtomicUsize::new(0),
        };
        let mut acc = Acc::default();
        let mut loops: Vec<Acc> = Vec::new();
        let gpu_name = mon.gpu_name.clone();
        let available_mb = mon.probe.sample().and_then(|s| s.available_mb);
        let this = self;
        let out = std::thread::scope(|sc| {
            let work = std::thread::Builder::new()
                .name(format!("vyro-bench-{area:?}"))
                .spawn_scoped(sc, || this.work(area, &ctx, available_mb));
            let work = match work {
                Ok(w) => w,
                Err(e) => return Err(Fail::Missing(format!("Could not start: {e}"))),
            };
            let mut last_progress = long_ago();
            while !work.is_finished() {
                if let Some((s, dt)) = mon.poll() {
                    let measuring = ctx.measuring.load(Ordering::Relaxed);
                    acc.add(&s, gpu_name.as_deref(), dt, measuring);
                    let phase = ctx.phase.load(Ordering::Relaxed);
                    if phase > 0 {
                        if loops.len() < phase {
                            loops.resize_with(phase, Acc::default);
                        }
                        loops[phase - 1].add(&s, gpu_name.as_deref(), dt, true);
                    }
                }
                if last_progress.elapsed() >= PROGRESS_EVERY {
                    last_progress = Instant::now();
                    let frac = f64::from(ctx.frac.load(Ordering::Relaxed)) / 10_000.0;
                    let step = lock(&ctx.step).clone();
                    progress(&this.progress(Some(area), &step, frac, t0));
                }
                std::thread::sleep(TICK);
            }
            work.join()
                .unwrap_or_else(|_| Err(Fail::Missing("The workload failed.".into())))
        });
        let watch = (acc.secs > 0.0 || acc.cpu_temp_max.is_some()).then(|| acc.watch());
        match out {
            Ok(mut o) => {
                if area == AreaId::Sustained {
                    finish_sustained(&mut o, &loops);
                }
                AreaResult {
                    area,
                    metrics: o.metrics,
                    watch,
                    missing: None,
                    detail: o.detail,
                    loops: o.loops,
                }
            }
            Err(Fail::Missing(why)) => AreaResult {
                watch,
                ..AreaResult::missing(area, why)
            },
            Err(Fail::Reset(m)) => {
                self.resets.fetch_add(1, Ordering::SeqCst);
                AreaResult {
                    watch,
                    ..AreaResult::missing(
                        area,
                        format!("The GPU driver reset during this area ({m})."),
                    )
                }
            }
        }
    }

    fn work(
        &self,
        area: AreaId,
        ctx: &AreaCtx,
        available_mb: Option<f64>,
    ) -> Result<AreaOut, Fail> {
        match area {
            AreaId::CpuSingle => self.cpu(ctx, 1, "cpu.single"),
            AreaId::CpuMulti => self.cpu(ctx, self.inp.threads, "cpu.multi"),
            AreaId::Memory => self.memory(ctx, available_mb),
            AreaId::Storage => self.storage(ctx),
            AreaId::GpuCompute => self.gpu_compute(ctx),
            AreaId::Graphics => self.graphics(ctx),
            AreaId::Sustained => self.sustained(ctx),
            AreaId::Efficiency | AreaId::Stability => Ok(AreaOut::default()),
        }
    }

    fn secs(&self, s: f64) -> f64 {
        s * self.inp.scale.time
    }

    fn cpu(&self, ctx: &AreaCtx, threads: usize, key: &str) -> Result<AreaOut, Fail> {
        let v = passes(ctx, 0.0, 1.0, "Rounds", |warm| {
            let secs = self.secs(if warm { 2.0 } else { 4.0 });
            Ok(cpu::pass(
                &self.cpu_in,
                self.cpu_ref,
                threads,
                secs,
                ctx.stop,
                ctx.calc_errors,
            ))
        })?;
        Ok(AreaOut {
            metrics: vec![Metric::measured(key, "Points", "pts", Better::Higher, v)],
            detail: Some(format!(
                "{threads} thread{}",
                if threads == 1 { "" } else { "s" }
            )),
            loops: Vec::new(),
        })
    }

    fn memory(&self, ctx: &AreaCtx, available_mb: Option<f64>) -> Result<AreaOut, Fail> {
        if available_mb.is_some_and(|a| a < MIN_AVAILABLE_MB) {
            return Err(Fail::Missing(format!(
                "Skipped: less than {:.1} GiB of memory available.",
                MIN_AVAILABLE_MB / 1024.0
            )));
        }
        let threads = self.inp.threads;
        let pass = self.secs(1.0);
        let read;
        let copy;
        {
            ctx.at(0.0, "Setting aside memory");
            let mut buf = memory::Buffers::new(self.inp.scale.mem_bytes / 8)
                .ok_or_else(|| Fail::Missing("Could not set aside the memory.".into()))?;
            read = passes(ctx, 0.05, 0.35, "Read", |_| {
                Ok(memory::read(&buf, threads, pass, ctx.stop, ctx.calc_errors))
            })?;
            copy = passes(ctx, 0.35, 0.65, "Copy", |_| {
                Ok(memory::copy(
                    &mut buf,
                    threads,
                    pass,
                    ctx.stop,
                    ctx.calc_errors,
                ))
            })?;
        }
        ctx.at(0.65, "Latency: building the chain");
        let chain = memory::Chain::new(self.inp.scale.mem_bytes)
            .ok_or_else(|| Fail::Missing("Could not set aside the memory.".into()))?;
        let latency = passes(ctx, 0.7, 1.0, "Latency", |warm| {
            chain
                .latency(if warm { pass / 2.0 } else { pass }, ctx.stop)
                .ok_or_else(|| Fail::Missing(STOPPED.into()))
        })?;
        Ok(AreaOut {
            metrics: vec![
                Metric::measured("mem.read", "Read", "GB/s", Better::Higher, read),
                Metric::measured("mem.copy", "Copy", "GB/s", Better::Higher, copy),
                Metric::measured("mem.latency", "Latency", "ns", Better::Lower, latency),
            ],
            detail: None,
            loops: Vec::new(),
        })
    }

    fn storage(&self, ctx: &AreaCtx) -> Result<AreaOut, Fail> {
        let bytes = self.inp.scale.file_bytes;
        if let Some(free) = self.inp.free_bytes.filter(|f| *f < MIN_FREE_BYTES) {
            return Err(Fail::Missing(super::storage_skip_note(free)));
        }
        let io = |e: std::io::Error| Fail::Missing(format!("The temporary file failed: {e}"));
        ctx.at(0.0, "Writing the file");
        let file = storage::TempFile::create(&self.inp.temp_dir, bytes).map_err(io)?;
        let errs = ctx.storage_errors;
        // The warm-up writes the whole file, so every sector read back was written.
        storage::sequential(&file, true, None, ctx.stop, errs).map_err(io)?;
        if ctx.stopped() {
            return Err(Fail::Missing(STOPPED.into()));
        }
        let cap = Some(self.secs(storage::PASS_CAP_SECS));
        let write = passes(ctx, 0.1, 0.4, "Sequential write", |warm| {
            if warm {
                return Ok(0.0);
            }
            storage::sequential(&file, true, cap, ctx.stop, errs).map_err(io)
        })?;
        let read = passes(ctx, 0.4, 0.7, "Sequential read", |warm| {
            if warm {
                return Ok(0.0);
            }
            storage::sequential(&file, false, cap, ctx.stop, errs).map_err(io)
        })?;
        let mut seed = 1;
        let random = passes(ctx, 0.7, 1.0, "Random 4K read", |warm| {
            seed += 1;
            let secs = self.secs(if warm { 1.0 } else { storage::RANDOM_SECS });
            storage::random_read(&file, secs, seed, ctx.stop, errs).map_err(io)
        })?;
        drop(file);
        Ok(AreaOut {
            metrics: vec![
                Metric::measured("disk.read", "Sequential read", "MB/s", Better::Higher, read),
                Metric::measured(
                    "disk.write",
                    "Sequential write",
                    "MB/s",
                    Better::Higher,
                    write,
                ),
                Metric::measured(
                    "disk.random",
                    "Random 4K read",
                    "IOPS",
                    Better::Higher,
                    random,
                ),
            ],
            detail: Some(self.inp.temp_dir.display().to_string()),
            loops: Vec::new(),
        })
    }

    fn open_gpu(&self) -> Result<Box<dyn gpu::Gpu>, Fail> {
        let f = self
            .inp
            .gpu
            .as_ref()
            .ok_or_else(|| Fail::Missing("No Direct3D 11 GPU found.".into()))?;
        f().map_err(Fail::from)
    }

    fn gpu_compute(&self, ctx: &AreaCtx) -> Result<AreaOut, Fail> {
        ctx.at(0.0, "Preparing the GPU");
        let mut g = self.open_gpu()?;
        let pass = self.secs(2.0);
        let flops = passes(ctx, 0.05, 0.5, "FP32", |warm| {
            Ok(g.compute(if warm { pass / 2.0 } else { pass }, ctx.stop)?)
        })?;
        let bw = passes(ctx, 0.5, 0.95, "Bandwidth", |warm| {
            Ok(g.bandwidth(if warm { pass / 4.0 } else { pass }, ctx.stop)?)
        })?;
        ctx.at(0.95, "Checking results");
        let bad = g.check()?;
        ctx.calc_errors.fetch_add(bad, Ordering::Relaxed);
        Ok(AreaOut {
            metrics: vec![
                Metric::measured(
                    "gpu.compute",
                    "FP32 throughput",
                    "GFLOPS",
                    Better::Higher,
                    flops,
                ),
                Metric::measured(
                    "gpu.bandwidth",
                    "Memory bandwidth",
                    "GB/s",
                    Better::Higher,
                    bw,
                ),
            ],
            detail: Some(g.name()),
            loops: Vec::new(),
        })
    }

    fn graphics(&self, ctx: &AreaCtx) -> Result<AreaOut, Fail> {
        ctx.at(0.0, "Preparing the scene");
        let mut g = self.open_gpu()?;
        let mut lows = Vec::new();
        let fps = passes(ctx, 0.05, 1.0, "Scene", |warm| {
            let secs = self.secs(if warm { 2.0 } else { 8.0 });
            let mut times = Vec::new();
            let start = Instant::now();
            g.frames(secs, ctx.stop, &mut times, &mut || {})?;
            let took = start.elapsed().as_secs_f64();
            let (fps, low) = gpu::frame_stats(&times, took)
                .ok_or_else(|| Fail::Missing("No frames were rendered.".into()))?;
            if !warm {
                lows.push(low);
            }
            Ok(fps)
        })?;
        Ok(AreaOut {
            metrics: vec![
                Metric::measured("gfx.fps", "Frame rate", "fps", Better::Higher, fps),
                Metric::measured("gfx.low", "1 % low", "fps", Better::Higher, lows),
            ],
            detail: Some(format!(
                "{} · {}×{} offscreen",
                g.name(),
                gpu::SCENE_W,
                gpu::SCENE_H
            )),
            loops: Vec::new(),
        })
    }

    fn sustained(&self, ctx: &AreaCtx) -> Result<AreaOut, Fail> {
        let loop_secs = self.secs(SUSTAIN_LOOP_SECS);
        let total = loop_secs * SUSTAIN_LOOPS as f64;
        let start = Instant::now();
        let until = start + Duration::from_secs_f64(total);
        let rounds = AtomicU64::new(0);
        let frames = AtomicU64::new(0);
        let gpu_failed: Mutex<Option<Fail>> = Mutex::new(None);
        let gpu_on = AtomicBool::new(false);
        let cpu_threads = if self.inp.gpu.is_some() {
            self.inp.threads.saturating_sub(1).max(1)
        } else {
            self.inp.threads.max(1)
        };
        let mut loops = Vec::with_capacity(SUSTAIN_LOOPS);
        std::thread::scope(|sc| {
            for _ in 0..cpu_threads {
                sc.spawn(|| {
                    cpu::spin(
                        &self.cpu_in,
                        self.cpu_ref,
                        until,
                        ctx.stop,
                        ctx.calc_errors,
                        Some(&rounds),
                    )
                });
            }
            if self.inp.gpu.is_some() {
                sc.spawn(|| {
                    let r = self.open_gpu().and_then(|mut g| {
                        gpu_on.store(true, Ordering::SeqCst);
                        let mut times = Vec::new();
                        let left = until
                            .saturating_duration_since(Instant::now())
                            .as_secs_f64();
                        g.frames(left, ctx.stop, &mut times, &mut || {
                            frames.fetch_add(1, Ordering::Relaxed);
                        })
                        .map_err(Fail::from)
                    });
                    if let Err(e) = r {
                        *lock(&gpu_failed) = Some(e);
                    }
                });
            }
            // Count each loop at its boundary.
            let (mut r0, mut f0) = (0u64, 0u64);
            for i in 0..SUSTAIN_LOOPS {
                ctx.phase.store(i + 1, Ordering::Relaxed);
                ctx.measuring.store(true, Ordering::Relaxed);
                ctx.at(
                    i as f64 / SUSTAIN_LOOPS as f64,
                    format!("Loop {} of {SUSTAIN_LOOPS}", i + 1),
                );
                let end = start + Duration::from_secs_f64(loop_secs * (i + 1) as f64);
                while Instant::now() < end && !ctx.stopped() {
                    std::thread::sleep(
                        super::STOP_CHECK.min(end.saturating_duration_since(Instant::now())),
                    );
                }
                if ctx.stopped() {
                    break;
                }
                let (r1, f1) = (
                    rounds.load(Ordering::Relaxed),
                    frames.load(Ordering::Relaxed),
                );
                let gpu = gpu_on.load(Ordering::SeqCst) && lock(&gpu_failed).is_none();
                loops.push(SustainLoop {
                    cpu: Some((r1 - r0) as f64 / loop_secs),
                    gpu: gpu.then(|| (f1 - f0) as f64 / loop_secs),
                    ..SustainLoop::default()
                });
                (r0, f0) = (r1, f1);
            }
            ctx.measuring.store(false, Ordering::Relaxed);
        });
        if ctx.stopped() {
            return Err(Fail::Missing(STOPPED.into()));
        }
        if let Some(Fail::Reset(m)) = lock(&gpu_failed).take() {
            return Err(Fail::Reset(m));
        }
        Ok(AreaOut {
            metrics: Vec::new(),
            detail: Some(format!(
                "{cpu_threads} CPU thread{}{}",
                if cpu_threads == 1 { "" } else { "s" },
                if self.inp.gpu.is_some() {
                    " + the graphics scene"
                } else {
                    ""
                }
            )),
            loops,
        })
    }
}

/// Fills in each loop's readings and works out stability and clock sag.
fn finish_sustained(o: &mut AreaOut, accs: &[Acc]) {
    for (l, a) in o.loops.iter_mut().zip(accs) {
        l.cpu_mhz = a.cpu_mhz.get();
        l.gpu_mhz = a.gpu_mhz.get();
        l.cpu_temp_max = a.cpu_temp_max;
        l.gpu_temp_max = a.gpu_temp_max;
    }
    let ratio = |v: Vec<f64>| -> Option<f64> {
        let hi = v.iter().copied().fold(f64::MIN, f64::max);
        let lo = v.iter().copied().fold(f64::MAX, f64::min);
        (v.len() >= 2 && hi > 0.0).then(|| lo / hi * 100.0)
    };
    let cpu = ratio(o.loops.iter().filter_map(|l| l.cpu).collect());
    let gpu = ratio(o.loops.iter().filter_map(|l| l.gpu).collect());
    if let Some(s) = match (cpu, gpu) {
        (Some(c), Some(g)) => Some(c.min(g)),
        (a, b) => a.or(b),
    } {
        o.metrics.push(Metric::derived(
            "sustain.stability",
            "Stability",
            "%",
            Better::Higher,
            s,
        ));
    }
    if let Some(c) = cpu {
        o.metrics.push(Metric::derived(
            "sustain.cpu",
            "CPU loops, worst ÷ best",
            "%",
            Better::Shown,
            c,
        ));
    }
    if let Some(g) = gpu {
        o.metrics.push(Metric::derived(
            "sustain.gpu",
            "GPU loops, worst ÷ best",
            "%",
            Better::Shown,
            g,
        ));
    }
    let sag = |f: fn(&SustainLoop) -> Option<f64>| -> Option<f64> {
        let first = o.loops.first().and_then(f)?;
        let last = o.loops.last().and_then(f)?;
        (o.loops.len() >= 2 && first > 0.0).then(|| ((1.0 - last / first) * 100.0).max(0.0))
    };
    if let Some(s) = sag(|l| l.cpu_mhz) {
        o.metrics.push(Metric::derived(
            "sustain.cpuSag",
            "CPU clock sag",
            "%",
            Better::Lower,
            s,
        ));
    }
    if let Some(s) = sag(|l| l.gpu_mhz) {
        o.metrics.push(Metric::derived(
            "sustain.gpuSag",
            "GPU clock sag",
            "%",
            Better::Lower,
            s,
        ));
    }
}

/// Multi-thread scaling, shown with the multi-thread points.
fn derive_scaling(areas: &mut [AreaResult]) {
    let single = areas
        .iter()
        .find(|a| a.area == AreaId::CpuSingle)
        .and_then(|a| a.metric("cpu.single"))
        .map(|m| m.value);
    if let (Some(s), Some(multi)) = (
        single.filter(|s| *s > 0.0),
        areas.iter_mut().find(|a| a.area == AreaId::CpuMulti),
    ) {
        if let Some(m) = multi.metric("cpu.multi").map(|m| m.value) {
            multi.metrics.push(Metric::derived(
                "cpu.scaling",
                "Scaling (multi ÷ single)",
                "×",
                Better::Shown,
                m / s,
            ));
        }
    }
}

fn efficiency(areas: &[AreaResult]) -> AreaResult {
    let per = |area: AreaId, key: &str, power: fn(&Watch) -> Option<f64>| -> Option<f64> {
        let a = areas.iter().find(|a| a.area == area)?;
        let v = a.metric(key)?.value;
        let w = a.watch.as_ref().and_then(power).filter(|w| *w > 1.0)?;
        Some(v / w)
    };
    let mut metrics = Vec::new();
    if let Some(v) = per(AreaId::CpuMulti, "cpu.multi", |w| w.cpu_power_avg) {
        metrics.push(Metric::derived(
            "eff.cpu",
            "CPU points per watt",
            "pts/W",
            Better::Higher,
            v,
        ));
    }
    if let Some(v) = per(AreaId::Graphics, "gfx.fps", |w| w.gpu_power_avg) {
        metrics.push(Metric::derived(
            "eff.gfx",
            "Graphics frames per joule",
            "fps/W",
            Better::Higher,
            v,
        ));
    }
    if metrics.is_empty() {
        return AreaResult::missing(
            AreaId::Efficiency,
            "Needs the CPU package power or the GPU board power during the multi-thread or graphics passes.",
        );
    }
    AreaResult {
        metrics,
        missing: None,
        ..AreaResult::missing(AreaId::Efficiency, "")
    }
}

fn stability(e: &BenchErrors) -> AreaResult {
    let mut metrics = vec![
        Metric::derived(
            "stab.calc",
            "Calculation errors",
            "",
            Better::Shown,
            f64::from(e.calc),
        ),
        Metric::derived(
            "stab.storage",
            "Storage read-back errors",
            "",
            Better::Shown,
            f64::from(e.storage),
        ),
    ];
    if let Some(h) = e.hardware {
        metrics.push(Metric::derived(
            "stab.hardware",
            "Hardware errors (WHEA)",
            "",
            Better::Shown,
            f64::from(h),
        ));
    }
    metrics.push(Metric::derived(
        "stab.resets",
        "GPU driver resets",
        "",
        Better::Shown,
        f64::from(e.gpu_resets),
    ));
    AreaResult {
        metrics,
        missing: None,
        detail: Some(if e.total() == 0 {
            "No errors.".into()
        } else {
            "Errors found: usually an unstable overclock or undervolt, memory, or heat.".into()
        }),
        ..AreaResult::missing(AreaId::Stability, "")
    }
}

/// Shared handle the app keeps while a run goes on.
pub type SharedControl = Arc<Control>;

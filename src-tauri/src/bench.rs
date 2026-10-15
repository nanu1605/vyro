//! Benchmark mode (P20), app side. A run happens in this process, on its own thread, so
//! quitting Vyro always ends it; it watches the PC through the snapshots the engine sends, and
//! the result goes to the engine's history store. A run stops when the owner presses Stop,
//! closes the window (also when it hides to the tray) or quits Vyro, or on a safety rule (see
//! `vyro_core::bench::runner`). The Benchmark profile is in force while it runs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};
use vyro_core::bench::runner::{self, Control, RunInputs, Scale};
use vyro_core::bench::{
    self, BenchMode, BenchProgress, BenchRun, BenchSetup, Preflight, Probe, RunSummary, Sample,
};
use vyro_core::{lock, SensorSnapshot};

use crate::engine::Engine;

/// Emitted with a [`BenchProgress`] about four times a second while a run goes on.
pub const PROGRESS_EVENT: &str = "bench-progress";
/// Emitted with the finished [`BenchRun`] (its store id set once kept).
pub const DONE_EVENT: &str = "bench-done";

#[derive(Default)]
pub struct BenchState {
    latest: Arc<Latest>,
    run: Mutex<Option<Running>>,
    progress: Arc<Mutex<Option<BenchProgress>>>,
}

struct Running {
    ctl: Arc<Control>,
    thread: JoinHandle<()>,
}

/// The newest reading, from the snapshot sink.
#[derive(Default)]
struct Latest(Mutex<Option<Sample>>);

impl Probe for Latest {
    fn sample(&self) -> Option<Sample> {
        lock(&self.0).clone()
    }
}

/// From every snapshot: the reading a run watches (and the start checks look at).
pub fn note_snapshot(app: &AppHandle, snap: &SensorSnapshot) {
    if let Some(s) = app.try_state::<BenchState>() {
        *lock(&s.latest.0) = Some(Sample::from_snapshot(snap, Instant::now()));
    }
}

fn running(state: &BenchState) -> bool {
    lock(&state.run)
        .as_ref()
        .is_some_and(|r| !r.thread.is_finished())
}

/// Stops a run, if one goes on; the first reason given stands.
pub fn stop(app: &AppHandle, reason: &str) {
    if let Some(s) = app.try_state::<BenchState>() {
        if let Some(r) = lock(&s.run).as_ref() {
            r.ctl.stop(reason);
        }
    }
}

/// At exit: stops a run and waits for it (at most `wait`), so its file is deleted and the
/// stopped run is kept.
pub fn stop_and_wait(app: &AppHandle, reason: &str, wait: Duration) {
    stop(app, reason);
    let Some(s) = app.try_state::<BenchState>() else {
        return;
    };
    let start = Instant::now();
    while running(&s) && start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

/// "C: · Samsung SSD 990 PRO 2TB": the temporary folder's drive and, where known, its model.
fn drive_label(engine: &Engine, dir: &Path) -> Option<String> {
    let s = dir.to_string_lossy();
    let letter = s
        .get(..2)
        .filter(|l| l.ends_with(':'))?
        .to_ascii_uppercase();
    let model = engine.api.drives().ok().and_then(|drives| {
        drives
            .into_iter()
            .find(|d| {
                d.volumes
                    .iter()
                    .any(|v| v.letter.eq_ignore_ascii_case(&letter))
            })
            .map(|d| d.model)
    });
    Some(match model {
        Some(m) if !m.trim().is_empty() => format!("{letter} · {}", m.trim()),
        _ => letter,
    })
}

fn same_gpu(a: &str, b: &str) -> bool {
    let n = |s: &str| s.to_ascii_lowercase().replace(['(', ')'], "");
    let (a, b) = (n(a), n(b));
    a == b || a.contains(&b) || b.contains(&a)
}

/// The setup recorded with a run.
fn setup(
    app: &AppHandle,
    engine: &Engine,
    gpu: Option<String>,
    drive: Option<String>,
) -> BenchSetup {
    let hw = engine.inventory.hardware();
    let sys = engine.inventory.system();
    let gpu_id = gpu
        .as_deref()
        .and_then(|n| hw.gpus.iter().find(|g| same_gpu(&g.name, n)));
    let board = [
        hw.board.manufacturer.as_deref(),
        hw.board.product.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let windows = [
        sys.os.name.as_deref(),
        sys.os.display_version.as_deref(),
        sys.os.build.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let effective = crate::gaming::effective(app);
    BenchSetup {
        vyro: app.package_info().version.to_string(),
        cpu: hw.cpu.brand.clone(),
        cores: hw.cpu.cores,
        threads: hw.cpu.threads,
        gpu_driver: gpu_id.and_then(|g| {
            g.vendor_driver_version
                .clone()
                .or_else(|| g.driver_version.clone())
        }),
        gpu,
        ram_gb: hw
            .memory
            .total_mb
            .map(|m| (m / 1024.0 * 10.0).round() / 10.0),
        ram_mts: hw
            .memory
            .dimms
            .iter()
            .filter_map(|d| d.configured_speed_mts.or(d.speed_mts))
            .max(),
        board: (!board.is_empty()).then_some(board),
        bios: hw.board.bios_version.clone(),
        windows: (!windows.is_empty()).then_some(windows),
        power_plan: vyro_core::sysinfo::power_plan(),
        profile: Some(effective.profile.spec().name),
        fan_profile: Some(effective.fans.spec().name),
        drive,
        busy_pct: None,
    }
}

fn preflight_now(app: &AppHandle, state: &BenchState) -> Preflight {
    let gpu = bench::gpu::adapter_name();
    let dir = temp_dir();
    let drive = app
        .try_state::<Engine>()
        .and_then(|e| drive_label(&e, &dir));
    let sample = state.latest.sample();
    bench::preflight(
        sample.as_ref(),
        running(state),
        gpu,
        drive,
        bench::free_bytes(&dir),
    )
}

/// What a run would use, and what stands in its way now.
#[tauri::command]
pub async fn benchmark_preflight(app: AppHandle) -> Result<Preflight, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<BenchState>();
        preflight_now(&app, &state)
    })
    .await
    .map_err(|e| e.to_string())
}

/// Starts a run; refused while something in [`Preflight::blockers`] holds.
#[tauri::command]
pub async fn start_benchmark(app: AppHandle, mode: BenchMode) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start(&app, mode))
        .await
        .map_err(|e| e.to_string())?
}

fn start(app: &AppHandle, mode: BenchMode) -> Result<(), String> {
    let state = app.state::<BenchState>();
    let engine = app.state::<Engine>();
    let pre = preflight_now(app, &state);
    if let Some(b) = pre.blockers.first() {
        return Err(b.clone());
    }
    let mut slot = lock(&state.run);
    if slot.as_ref().is_some_and(|r| !r.thread.is_finished()) {
        return Err("A benchmark is already running.".into());
    }
    // Readings keep coming while the window is hidden, at the Benchmark profile's rate.
    engine.set_bench(true);
    crate::gaming::set_benchmark(app, true);
    let dir = temp_dir();
    let inputs = RunInputs {
        mode,
        setup: setup(app, &engine, pre.gpu.clone(), pre.drive.clone()),
        free_bytes: bench::free_bytes(&dir),
        temp_dir: dir,
        threads: std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        gpu: bench::gpu::factory(),
        gpu_name: pre.gpu,
        hardware_errors: Some(Box::new(|since| {
            vyro_core::sysinfo::stability_counts(since).map(|c| c.corrected + c.fatal)
        })),
        scale: Scale::default(),
    };
    let ctl = Arc::new(Control::new());
    let probe = Arc::clone(&state.latest);
    let progress = Arc::clone(&state.progress);
    let thread = {
        let app = app.clone();
        let ctl = Arc::clone(&ctl);
        std::thread::Builder::new()
            .name("vyro-bench".into())
            .spawn(move || {
                log::info!("benchmark: {mode:?} run started");
                let shown = |p: &BenchProgress| {
                    *lock(&progress) = Some(p.clone());
                    let visible = app.try_state::<Engine>().is_some_and(|e| e.visible());
                    if visible {
                        if let Err(e) = app.emit(PROGRESS_EVENT, p) {
                            log::debug!("emit {PROGRESS_EVENT}: {e}");
                        }
                    }
                };
                let mut run = runner::run(&inputs, &ctl, probe.as_ref(), &shown);
                *lock(&progress) = None;
                crate::gaming::set_benchmark(&app, false);
                if let Some(e) = app.try_state::<Engine>() {
                    e.set_bench(false);
                    match e.api.add_benchmark(run.clone()) {
                        Ok(id) => run.id = id,
                        Err(err) => log::warn!("benchmark: not kept: {err}"),
                    }
                }
                log::info!(
                    "benchmark: {:?} in {:.0} s{}",
                    run.status,
                    run.secs,
                    run.reason
                        .as_deref()
                        .map(|r| format!(": {r}"))
                        .unwrap_or_default()
                );
                if let Err(e) = app.emit(DONE_EVENT, &run) {
                    log::debug!("emit {DONE_EVENT}: {e}");
                }
            })
            .map_err(|e| e.to_string())
    };
    match thread {
        Ok(thread) => {
            *slot = Some(Running { ctl, thread });
            Ok(())
        }
        Err(e) => {
            crate::gaming::set_benchmark(app, false);
            engine.set_bench(false);
            Err(e)
        }
    }
}

#[tauri::command]
pub fn stop_benchmark(app: AppHandle) {
    stop(&app, "Stopped by you.");
}

/// Where the run is, while one goes on (a reopened view picks it up).
#[tauri::command]
pub fn get_benchmark_progress(state: State<'_, BenchState>) -> Option<BenchProgress> {
    if running(&state) {
        lock(&state.progress).clone()
    } else {
        None
    }
}

#[tauri::command]
pub async fn get_benchmark_runs(engine: State<'_, Engine>) -> Result<Vec<RunSummary>, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.benchmark_runs())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_benchmark_run(
    engine: State<'_, Engine>,
    id: i64,
) -> Result<Option<BenchRun>, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.benchmark_run(id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_benchmark(engine: State<'_, Engine>, id: i64) -> Result<bool, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.delete_benchmark(id))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpus_match_by_name() {
        assert!(same_gpu(
            "NVIDIA GeForce RTX 5060 Ti",
            "NVIDIA GeForce RTX 5060 Ti"
        ));
        assert!(same_gpu(
            "AMD Radeon(TM) Graphics",
            "AMD Radeon TM Graphics"
        ));
        assert!(!same_gpu(
            "NVIDIA GeForce RTX 5060 Ti",
            "AMD Radeon(TM) Graphics"
        ));
    }
}

//! GPU tuning (1.6), app side: the window's commands, and the stability test and "Find my best"
//! on a thread of their own. The service changes the card; this thread asks it to put settings
//! on trial and renews the trial while the checks run, so a crashed or closed app ends the
//! trial within half a minute. The test refuses to start for the same reasons a benchmark does
//! (a game, Health warnings, a benchmark running), and runs with the Benchmark profile.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use vyro_core::engine::EngineApi;
use vyro_core::gpu_tune::search::{Next, Search, CONFIRM_SECS};
use vyro_core::gpu_tune::{
    self, describe, find, stress, PresetKind, Settings, Trial, TuneConfig, TuneEvent, TuneStatus,
};
use vyro_core::lock;

use crate::engine::Engine;

/// Emitted with a [`TestProgress`] while a test runs.
pub const PROGRESS_EVENT: &str = "gpu-test-progress";
/// Emitted with a [`TestDone`] when it ends.
pub const DONE_EVENT: &str = "gpu-test-done";
/// The trial is renewed this often (the service ends it after 30 s without).
const RENEW: Duration = Duration::from_secs(10);
/// Longest wait for the service to put the settings on the card.
const LAND: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestKind {
    /// "Test this setting".
    Test,
    /// "Find my best".
    Best,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestProgress {
    pub kind: TestKind,
    pub step: String,
    /// 0–1.
    pub done: f64,
    pub settings: Settings,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestDone {
    pub kind: TestKind,
    pub passed: bool,
    /// The settings that passed (the best found, for "Find my best").
    pub settings: Option<Settings>,
    pub message: String,
}

#[derive(Default)]
pub struct TuneTest {
    run: Mutex<Option<Running>>,
    progress: Arc<Mutex<Option<TestProgress>>>,
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

/// Whether a stability test or "Find my best" goes on.
pub fn testing(app: &AppHandle) -> bool {
    app.try_state::<TuneTest>().is_some_and(|t| {
        lock(&t.run)
            .as_ref()
            .is_some_and(|r| !r.thread.is_finished())
    })
}

/// Stops a test, if one goes on.
pub fn stop(app: &AppHandle) {
    if let Some(t) = app.try_state::<TuneTest>() {
        if let Some(r) = lock(&t.run).as_ref() {
            r.stop.store(true, Ordering::SeqCst);
        }
    }
}

/// At exit: stops a test and waits for it (at most `wait`), so the trial ends now.
pub fn stop_and_wait(app: &AppHandle, wait: Duration) {
    stop(app);
    let start = Instant::now();
    while testing(app) && start.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(20));
    }
}

async fn with_api<T: Send + 'static>(
    engine: &Engine,
    f: impl FnOnce(&dyn EngineApi) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let api = Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || f(api.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_gpu_tune(engine: State<'_, Engine>) -> Result<TuneStatus, String> {
    with_api(&engine, |e| e.gpu_tune_status()).await
}

/// Saves what the owner set; with `apply`, the preset in force goes on the card.
#[tauri::command]
pub async fn set_gpu_tune(
    engine: State<'_, Engine>,
    config: TuneConfig,
    apply: bool,
) -> Result<(), String> {
    with_api(&engine, move |e| e.set_gpu_tune(config, apply)).await
}

/// "Return GPU to stock".
#[tauri::command]
pub async fn restore_gpu(engine: State<'_, Engine>) -> Result<(), String> {
    with_api(&engine, |e| e.restore_gpu()).await
}

#[tauri::command]
pub async fn get_gpu_tune_events(engine: State<'_, Engine>) -> Result<Vec<TuneEvent>, String> {
    with_api(&engine, |e| e.gpu_tune_events()).await
}

/// Where a test is, while one goes on (a reopened view picks it up).
#[tauri::command]
pub fn get_gpu_test_progress(app: AppHandle, state: State<'_, TuneTest>) -> Option<TestProgress> {
    if testing(&app) {
        lock(&state.progress).clone()
    } else {
        None
    }
}

#[tauri::command]
pub fn stop_gpu_test(app: AppHandle) {
    stop(&app);
}

/// Starts "Test this setting" for `settings`, or "Find my best".
#[tauri::command]
pub async fn start_gpu_test(
    app: AppHandle,
    kind: TestKind,
    settings: Option<Settings>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start(&app, kind, settings))
        .await
        .map_err(|e| e.to_string())?
}

pub(crate) fn start(
    app: &AppHandle,
    kind: TestKind,
    settings: Option<Settings>,
) -> Result<(), String> {
    let engine = app.state::<Engine>();
    let status = engine.api.gpu_tune_status()?;
    if !status.config.enabled {
        return Err("Turn GPU tuning on first.".into());
    }
    if let Some(why) = &status.held {
        return Err(format!("The card is held at stock: {why}"));
    }
    let bench = app.state::<crate::bench::BenchState>();
    if let Some(b) = crate::bench::preflight_now(app, &bench).blockers.first() {
        return Err(b.clone());
    }
    if vyro_core::bench::gpu::factory().is_none() {
        return Err("No Direct3D 11 GPU found to test with.".into());
    }
    let state = app.state::<TuneTest>();
    let mut slot = lock(&state.run);
    if slot.as_ref().is_some_and(|r| !r.thread.is_finished()) {
        return Err("A GPU test is already running.".into());
    }
    let target = match kind {
        TestKind::Test => settings.ok_or("Which settings?")?,
        TestKind::Best => find(&status.presets, gpu_tune::PERFORMANCE).unwrap_or(Settings::STOCK),
    };
    if !status.limits.allows(&target) {
        return Err("Those settings are outside what the driver allows.".into());
    }
    engine.set_bench(true);
    crate::gaming::set_benchmark(app, true);
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let app = app.clone();
        let stop = Arc::clone(&stop);
        let progress = Arc::clone(&state.progress);
        let limits = status.limits;
        let vendor = status.gpu.as_ref().map(|g| g.vendor).unwrap_or_default();
        std::thread::Builder::new()
            .name("vyro-gpu-test".into())
            .spawn(move || {
                let api = Arc::clone(&app.state::<Engine>().api);
                let report = |p: TestProgress| {
                    *lock(&progress) = Some(p.clone());
                    if let Err(e) = app.emit(PROGRESS_EVENT, &p) {
                        log::debug!("emit {PROGRESS_EVENT}: {e}");
                    }
                };
                let done = match kind {
                    TestKind::Test => test_one(api.as_ref(), target, &stop, &report),
                    TestKind::Best => {
                        find_best(api.as_ref(), target, limits, vendor, &stop, &report)
                    }
                };
                if let Err(e) = api.gpu_trial(None) {
                    log::warn!("gpu tuning: ending the trial: {e}");
                }
                *lock(&progress) = None;
                crate::gaming::set_benchmark(&app, false);
                if let Some(e) = app.try_state::<Engine>() {
                    e.set_bench(false);
                }
                log::info!("gpu tuning: {kind:?} ended: {}", done.message);
                if let Err(e) = app.emit(DONE_EVENT, &done) {
                    log::debug!("emit {DONE_EVENT}: {e}");
                }
            })
            .map_err(|e| e.to_string())
    };
    match thread {
        Ok(thread) => {
            *slot = Some(Running { stop, thread });
            Ok(())
        }
        Err(e) => {
            crate::gaming::set_benchmark(app, false);
            engine.set_bench(false);
            Err(e)
        }
    }
}

/// One test: these settings, this long, shown as `step` with progress from `from` to `to`.
struct Step<'a> {
    kind: TestKind,
    settings: Settings,
    secs: u32,
    step: &'a str,
    from: f64,
    to: f64,
}

/// Puts the step's settings on trial, waits until the card runs them, then runs the checks,
/// renewing the trial as it goes.
fn trial(
    api: &dyn EngineApi,
    s: &Step<'_>,
    stop: &AtomicBool,
    report: &dyn Fn(TestProgress),
) -> Result<stress::Outcome, String> {
    let Step {
        kind,
        settings,
        secs,
        step,
        from,
        to,
    } = *s;
    let ask = |done: f64| {
        api.gpu_trial(Some(Trial {
            settings,
            step: step.to_owned(),
            done,
        }))
    };
    ask(from)?;
    let start = Instant::now();
    loop {
        let st = api.gpu_tune_status()?;
        if st.current == Some(settings) && st.trial.is_some() {
            break;
        }
        if st.trial.is_none() {
            return Err(st
                .held
                .unwrap_or_else(|| "The service ended the trial.".into()));
        }
        if start.elapsed() > LAND {
            return Err("The card did not take the settings in time.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let factory = vyro_core::bench::gpu::factory().ok_or("No Direct3D 11 GPU found.")?;
    let mut gpu = factory().map_err(|e| e.to_string())?;
    let mut renewed = Instant::now();
    let mut renew_error = None;
    let outcome = stress::run(gpu.as_mut(), f64::from(secs), stop, &mut |p| {
        let done = from + (to - from) * p;
        report(TestProgress {
            kind,
            step: step.to_owned(),
            done,
            settings,
        });
        if renewed.elapsed() >= RENEW {
            renewed = Instant::now();
            if let Err(e) = ask(done) {
                renew_error.get_or_insert(e);
            }
        }
    })?;
    // The service ended the trial (heat, a crash it saw): the result does not count.
    if let Some(e) = renew_error {
        if outcome.passed {
            return Err(e);
        }
    }
    Ok(outcome)
}

fn test_one(
    api: &dyn EngineApi,
    settings: Settings,
    stop: &AtomicBool,
    report: &dyn Fn(TestProgress),
) -> TestDone {
    let step = format!("Testing {}", describe(&settings));
    let done = |passed, message: String| TestDone {
        kind: TestKind::Test,
        passed,
        settings: passed.then_some(settings),
        message,
    };
    let s = Step {
        kind: TestKind::Test,
        settings,
        secs: CONFIRM_SECS,
        step: &step,
        from: 0.0,
        to: 1.0,
    };
    match trial(api, &s, stop, report) {
        Ok(o) if o.stopped => done(false, "Stopped.".into()),
        Ok(o) => {
            let note = o.reason.clone().unwrap_or_default();
            if let Err(e) = api.gpu_tested(settings, o.passed, note.clone()) {
                log::warn!("gpu tuning: {e}");
            }
            if o.passed {
                done(
                    true,
                    format!("Passed: {} is stable on this card.", describe(&settings)),
                )
            } else {
                done(false, format!("Failed: {note}. Try smaller offsets."))
            }
        }
        Err(e) => done(false, format!("The test could not run: {e}")),
    }
}

fn find_best(
    api: &dyn EngineApi,
    start: Settings,
    limits: gpu_tune::Limits,
    vendor: gpu_tune::Vendor,
    stop: &AtomicBool,
    report: &dyn Fn(TestProgress),
) -> TestDone {
    let done = |passed, settings, message: String| TestDone {
        kind: TestKind::Best,
        passed,
        settings,
        message,
    };
    let mut search = Search::new(start, limits, vendor);
    let mut next = search.first();
    loop {
        match next {
            Next::Test {
                settings,
                secs,
                step,
            } => {
                let from = search.progress();
                let s = Step {
                    kind: TestKind::Best,
                    settings,
                    secs,
                    step: &step,
                    from,
                    to: (from + 0.04).min(1.0),
                };
                let outcome = trial(api, &s, stop, report);
                match outcome {
                    Ok(o) if o.stopped => return done(false, None, "Stopped.".into()),
                    Ok(o) => next = search.result(o.passed, o.bandwidth_gbs),
                    Err(e) => return done(false, None, format!("The search could not go on: {e}")),
                }
            }
            Next::Done(best) => {
                return match save_best(api, best) {
                    Ok(()) => done(
                        true,
                        Some(best),
                        format!(
                            "Best for this card: {}. Saved as a preset.",
                            describe(&best)
                        ),
                    ),
                    Err(e) => done(
                        true,
                        Some(best),
                        format!("Found {}, but saving it failed: {e}", describe(&best)),
                    ),
                };
            }
            Next::GaveUp(why) => {
                return done(false, None, format!("No better settings found: {why}."))
            }
        }
    }
}

/// Keeps the result as the "Best for this card" preset (replacing an older one), tested.
fn save_best(api: &dyn EngineApi, best: Settings) -> Result<(), String> {
    api.gpu_tested(best, true, String::new())?;
    let mut cfg = api.gpu_tune_status()?.config;
    cfg.presets.retain(|p| p.id != gpu_tune::BEST);
    cfg.presets.insert(
        0,
        gpu_tune::Preset {
            id: gpu_tune::BEST.into(),
            name: "Best for this card".into(),
            settings: best,
            kind: PresetKind::Custom,
            note: Some("Found and tested on this card by Find my best.".into()),
        },
    );
    if cfg.presets.len() > gpu_tune::MAX_PRESETS {
        cfg.presets.truncate(gpu_tune::MAX_PRESETS);
    }
    api.set_gpu_tune(cfg, false)
}

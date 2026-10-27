//! Gaming (P10, P13): once a second, looks for a game in the foreground and tells the engine to
//! capture it; while it runs, puts the game's profile, fan settings, overlay choice and update
//! rate in force, and the owner's own back when it closes. The engine (the service) runs
//! PresentMon; this app picks the process, since only it sees the signed-in user's desktop.
//! When a captured game ends, its summary is a notification for the phones that want one (2.0).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use vyro_core::alerts::AlertBrief;
use vyro_core::gaming::auto::{Effective, Switcher};
use vyro_core::gaming::GameContext;
use vyro_core::profiles::Profile;
#[cfg(windows)]
use vyro_core::remote::relay::push::Note;
use vyro_core::{lock, SensorSnapshot};

use crate::engine::Engine;
use crate::tray::SettingsStore;

/// Emitted with the [`Effective`] state whenever it changes.
pub const STATE_EVENT: &str = "profile-state";

/// The app's side of automatic game profiles.
#[derive(Default)]
pub struct GameState {
    switcher: Mutex<Switcher>,
    /// Last applied, to apply only changes.
    applied: Mutex<Option<Effective>>,
    /// GPU use per process from the latest snapshot (the load trigger).
    gpu: Mutex<HashMap<u32, f64>>,
    /// The followed game is the foreground app.
    in_front: AtomicBool,
    /// Last sent to the engine.
    context: Mutex<Option<GameContext>>,
}

/// From every snapshot: per-process GPU use, for the load trigger.
pub fn note_snapshot(app: &AppHandle, snap: &SensorSnapshot) {
    if let Some(state) = app.try_state::<GameState>() {
        *lock(&state.gpu) = snap
            .processes
            .iter()
            .filter_map(|p| p.gpu_pct.map(|g| (p.pid, g)))
            .collect();
    }
}

/// What applies now (the owner's profile when no game runs).
pub fn effective(app: &AppHandle) -> Effective {
    let settings = app
        .try_state::<SettingsStore>()
        .map(|s| s.get())
        .unwrap_or_default();
    match app.try_state::<GameState>() {
        Some(state) => lock(&state.switcher).effective(settings.profile, &settings.games),
        None => Switcher::default().effective(settings.profile, &settings.games),
    }
}

/// From every snapshot: the profile an alert asks for while it holds (P18); applied when it
/// changes.
pub fn note_alerts(app: &AppHandle, alerts: &AlertBrief) {
    let Some(state) = app.try_state::<GameState>() else {
        return;
    };
    let changed = lock(&state.switcher).set_alert(alerts.profile());
    if changed {
        apply_effective(app);
    }
}

/// A benchmark run starts or ends (P20): the Benchmark profile is in force while it runs.
pub fn set_benchmark(app: &AppHandle, on: bool) {
    let Some(state) = app.try_state::<GameState>() else {
        return;
    };
    let changed = lock(&state.switcher).set_benchmark(on);
    if changed {
        apply_effective(app);
    }
}

/// The owner picked a profile: while a game runs, its own settings stay out until it closes.
pub fn manual_pick(app: &AppHandle) {
    if let Some(state) = app.try_state::<GameState>() {
        lock(&state.switcher).manual();
    }
}

/// Whether the overlay should show for the game in front: the game's own choice, else the
/// overlay setting.
pub fn overlay_wanted(app: &AppHandle, setting: bool) -> bool {
    app.try_state::<GameState>()
        .and_then(|s| lock(&s.applied).as_ref().and_then(|e| e.overlay))
        .unwrap_or(setting)
}

/// The game's own overlay mode, if it has one.
pub fn overlay_mode(app: &AppHandle) -> Option<vyro_core::gaming::overlay::OverlayMode> {
    app.try_state::<GameState>()
        .and_then(|s| lock(&s.applied).as_ref().and_then(|e| e.overlay_mode))
}

/// Puts the effective profile, fan settings and update rate in force (what changed), moves the
/// tray check mark and tells the window.
pub fn apply_effective(app: &AppHandle) {
    let next = effective(app);
    let Some(state) = app.try_state::<GameState>() else {
        return;
    };
    let prev = lock(&state.applied).replace(next.clone());
    if prev.as_ref() == Some(&next) {
        return;
    }
    if let Some(engine) = app.try_state::<Engine>() {
        engine.set_poll_interval(next.poll_ms);
        engine.set_game(next.game.is_some());
        if prev.as_ref().map(|p| p.fans) != Some(next.fans) {
            // The service puts the profile's own fan settings and GPU preset in force, if it
            // has any.
            if let Err(e) = engine.api.set_fan_profile(next.fans) {
                log::warn!("fans: profile: {e}");
            }
            if let Err(e) = engine.api.set_gpu_tune_profile(next.fans) {
                log::warn!("gpu tuning: profile: {e}");
            }
        }
    }
    if prev.as_ref().map(|p| (p.profile, p.auto)) != Some((next.profile, next.auto)) {
        match (&next.alert, &next.game, next.auto) {
            _ if next.benchmark => log::info!("benchmark: {:?} profile", next.profile),
            (Some(rule), _, _) => log::info!("alerts: {:?} profile for “{rule}”", next.profile),
            (None, Some(game), true) => {
                log::info!("gaming: {:?} profile for {game}", next.profile);
            }
            _ => log::info!("gaming: {:?} profile", next.profile),
        }
    }
    crate::tray::check_profile(app, next.profile);
    if let Err(e) = app.emit(STATE_EVENT, &next) {
        log::debug!("emit {STATE_EVENT}: {e}");
    }
    sync_context(app);
}

/// Tells the engine about the game: in front, profile, overlay. Only changes are sent.
fn sync_context(app: &AppHandle) {
    let (Some(state), Some(engine)) = (app.try_state::<GameState>(), app.try_state::<Engine>())
    else {
        return;
    };
    let context = lock(&state.applied).as_ref().and_then(|e| {
        e.game.as_ref().map(|_| GameContext {
            in_front: state.in_front.load(Ordering::SeqCst),
            profile: e.profile,
            auto: e.auto,
            overlay: crate::overlay::shown(app),
        })
    });
    let changed = {
        let mut sent = lock(&state.context);
        let changed = *sent != context;
        *sent = context;
        changed
    };
    if changed {
        if let Err(e) = engine.api.set_game_context(context) {
            log::debug!("gaming: context: {e}");
        }
    }
}

/// Starts the watcher thread (Windows only; elsewhere there is nothing to capture).
pub fn watch(app: &AppHandle) {
    #[cfg(windows)]
    {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("vyro-game-watch".into())
            .spawn(move || run(&app));
        if let Err(e) = spawned {
            log::warn!("gaming: watcher not started: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn run(app: &AppHandle) {
    use vyro_core::gaming::auto::LoadWatch;
    use vyro_core::gaming::foreground::{
        alive, foreground, is_candidate, Foreground, Known, Watcher,
    };

    let own = std::process::id();
    let mut watcher = Watcher::default();
    let mut load = LoadWatch::default();
    // The process the engine captures (frame rates are measured only with the setting on).
    let mut captured: Option<u32> = None;
    // After a capture ends, its session is looked for among those the engine logged, for a
    // while (it is logged at the engine's next tick): from when it ended, until when.
    let mut summary: Option<(i64, std::time::Instant)> = None;
    let mut tick: u64 = 0;
    loop {
        tick += 1;
        std::thread::sleep(std::time::Duration::from_secs(1));
        let (Some(engine), Some(store), Some(state)) = (
            app.try_state::<Engine>(),
            app.try_state::<SettingsStore>(),
            app.try_state::<GameState>(),
        ) else {
            continue;
        };
        let settings = store.get();
        let games = &settings.games;
        let fg = foreground();
        let fg_pid = fg.as_ref().map(|f| f.pid);
        let gpu = fg_pid.and_then(|pid| lock(&state.gpu).get(&pid).copied());
        let busy = load.step(fg_pid, gpu) && games.load_trigger;
        let known = |f: &Foreground| match f.exe.as_deref() {
            Some(e) if games.ignored(e) => Known::NotGame,
            Some(e) if games.known(e) => Known::Game,
            _ => Known::Unknown,
        };
        let is_game =
            |f: &Foreground| is_candidate(f, own, known(f), busy && Some(f.pid) == fg_pid);
        let following = settings.game_capture || games.enabled;
        // A game just marked "Not a game" stops being followed.
        let dropped = watcher.current().is_some_and(|t| games.ignored(&t.name));
        let change = if following && !dropped {
            watcher.step(fg.as_ref(), is_game, alive)
        } else if watcher.current().is_some() {
            watcher.reset();
            Some(None)
        } else {
            None
        };
        // The overlay shows only while the followed game is in front, on its monitor.
        let front = fg
            .as_ref()
            .filter(|f| watcher.current().is_some_and(|t| t.pid == f.pid));
        state.in_front.store(front.is_some(), Ordering::SeqCst);
        crate::overlay::set_front(app, front.is_some(), front.map(|f| f.monitor));
        if let Some(target) = change {
            match &target {
                Some(t) => log::info!("gaming: {} ({}) is a game", t.name, t.pid),
                None => log::info!("gaming: game ended"),
            }
            let exe = target.as_ref().map(|t| t.name.to_ascii_lowercase());
            lock(&state.switcher).set_game(exe.as_deref());
            apply_effective(app);
        }
        let capture = watcher.current().filter(|_| settings.game_capture).cloned();
        if capture.as_ref().map(|t| t.pid) != captured {
            if captured.is_some() {
                summary = Some((
                    vyro_core::unix_now(),
                    std::time::Instant::now() + std::time::Duration::from_secs(30),
                ));
            }
            captured = capture.as_ref().map(|t| t.pid);
            if let Err(e) = engine.api.set_capture_target(capture) {
                log::warn!("gaming: {e}");
            }
        }
        if let Some((ended, until)) = summary {
            if std::time::Instant::now() > until {
                summary = None;
            } else if tick.is_multiple_of(3) {
                let logged = engine.api.game_sessions().ok().and_then(|s| {
                    s.into_iter()
                        .find(|g| g.ended.is_some_and(|e| e >= ended - 60))
                });
                if let Some(g) = logged {
                    summary = None;
                    let unit = store.unit();
                    if let Some(note) = Note::game(&g, unit, vyro_core::unix_now()) {
                        crate::remote::notify(app, note);
                    }
                }
            }
        }
        sync_context(app);
    }
}

/// Logged game sessions, newest first.
#[tauri::command]
pub async fn get_game_sessions(
    engine: tauri::State<'_, Engine>,
) -> Result<Vec<vyro_core::gaming::GameSession>, String> {
    let api = std::sync::Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.game_sessions())
        .await
        .map_err(|e| e.to_string())?
}

/// A logged session with its samples and markers, for the replay; `None` once it is gone.
#[tauri::command]
pub async fn get_game_timeline(
    engine: tauri::State<'_, Engine>,
    id: i64,
) -> Result<Option<vyro_core::gaming::replay::GameTimeline>, String> {
    let api = std::sync::Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.game_timeline(id))
        .await
        .map_err(|e| e.to_string())?
}

/// Sensor history around a moment of a replay.
#[tauri::command]
pub async fn get_history_window(
    engine: tauri::State<'_, Engine>,
    metrics: Vec<String>,
    from: i64,
    to: i64,
) -> Result<Vec<vyro_core::history::WindowSeries>, String> {
    let api = std::sync::Arc::clone(&engine.api);
    tauri::async_runtime::spawn_blocking(move || api.history_window(&metrics, from, to))
        .await
        .map_err(|e| e.to_string())?
}

/// The profile, fan settings and update rate in force, and why.
#[tauri::command]
pub fn get_game_state(app: AppHandle) -> Effective {
    effective(&app)
}

/// A profile picked in the window: it becomes the owner's profile and wins over the game's.
#[tauri::command]
pub fn pick_profile(app: AppHandle, profile: Profile) -> Result<(), String> {
    crate::tray::pick_profile(&app, profile)
}

/// "Keep <profile>": the game's own settings stay out until it closes.
#[tauri::command]
pub fn keep_profile(app: AppHandle) {
    manual_pick(&app);
    apply_effective(&app);
}

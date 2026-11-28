//! Ambient lighting in the app (2.6, `clokrix_core::lighting`): the colour glowing most on the
//! screen on the devices the owner picks, through OpenRGB. It runs here, as the signed-in user,
//! because only the user's session sees the screen; nothing goes to the service, and nothing of
//! the screen leaves the PC: each picture is made small on the GPU and kept only as one colour.
//!
//! While a phone's screen session has the same monitor (Windows lets the app duplicate it once),
//! lighting lets go of it and takes its colour from the session's own pictures instead. Each
//! device's mode is noted (`lighting.json`) before Clokrix puts it in Direct, and put back when
//! lighting stops, Pro ends, Clokrix closes, or at the next start after a crash.
//!
//! OpenRGB (2.6.1, `lighting::setup`): when it is not on this PC, the view offers Set up OpenRGB,
//! which downloads its pinned installer and runs it with one administrator prompt; its service
//! then answers. An OpenRGB installed without the service is started here, once, while lighting
//! is on and it does not answer.
//!
//! Another lighting program (2.6.2, `lighting::others`): while lighting is on, the names of the
//! services and processes running are read every 10 s, and the view names each lighting program
//! Clokrix recognises among them, with how to stop it. Clokrix never stops or touches it.
//!
//! A header's LED count (2.6.3): a board's header for fans or a strip has none until its size is
//! set, so its lights stay dark. The owner types the count in the view and presses Set; the
//! thread sends it to OpenRGB (within the zone's bounds), which keeps it, and reads the device
//! again. A size set in OpenRGB itself tells no client, so the devices are read again every 10 s.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clokrix_core::lighting::colour::{Picker, Rgb8, Target, Thumb};
use clokrix_core::lighting::openrgb::{Client, Controller, PORT};
use clokrix_core::lighting::others::OtherLighting;
use clokrix_core::lighting::setup::{self, Next};
use clokrix_core::lighting::{
    self, device_id, info, interval, restore_mode, saved_mode, LightLink, LightingSettings,
    LightingStatus, SavedMode, SetupStep, ZoneSize,
};
use clokrix_core::{files, lock};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

/// Emitted with a [`LightingStatus`] while the Lighting view is open.
pub const EVENT: &str = "lighting-status";
const FILE: &str = "lighting.json";
/// OpenRGB is looked for this often while it does not answer.
const RETRY: Duration = Duration::from_secs(5);
/// The screen is looked at this often at most.
const FRAME: Duration = Duration::from_millis(33);
/// The view hears of the colour this often at most.
const TELL: Duration = Duration::from_millis(250);
/// Other lighting programs are looked for this often.
const OTHERS: Duration = Duration::from_secs(10);
/// OpenRGB's devices are read again this often, for sizes set in OpenRGB.
const REREAD: Duration = Duration::from_secs(10);
/// The owner's Sets waiting for the thread, at most.
const SIZES: usize = 16;

/// What is kept: the settings, and the modes to put back.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    settings: LightingSettings,
    saved: Vec<SavedMode>,
}

pub struct LightingState {
    path: Option<PathBuf>,
    stored: Mutex<Stored>,
    status: Mutex<LightingStatus>,
    /// The thread runs.
    running: AtomicBool,
    /// The thread is to stop, putting every device back.
    stop: Arc<AtomicBool>,
    /// The monitor a screen session has; lighting takes the session's pictures for it.
    yielded: Mutex<Option<u32>>,
    /// The thread holds a capture of the monitor it follows.
    holding: AtomicBool,
    /// The latest picture from the screen session, made small.
    tapped: Mutex<Option<Thumb>>,
    /// The view is open until then.
    watched: Mutex<Instant>,
    /// OpenRGB's set-up runs: this step.
    setting_up: Mutex<Option<SetupStep>>,
    /// OpenRGB was just set up: the thread looks for it afresh.
    fresh: AtomicBool,
    /// The owner's Sets of a header's LED count, for the thread to send (2.6.3).
    sizes: Mutex<Vec<ZoneSize>>,
}

impl LightingState {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_config_dir().ok().map(|d| d.join(FILE));
        let mut stored: Stored = path
            .as_deref()
            .and_then(|p| files::read_json(p, |_: &Stored| Ok(())).ok().flatten())
            .unwrap_or_default();
        stored.settings = stored.settings.clamped();
        Self {
            path,
            stored: Mutex::new(stored),
            status: Mutex::new(LightingStatus::default()),
            running: AtomicBool::new(false),
            stop: Arc::new(AtomicBool::new(false)),
            yielded: Mutex::new(None),
            holding: AtomicBool::new(false),
            tapped: Mutex::new(None),
            watched: Mutex::new(Instant::now()),
            setting_up: Mutex::new(None),
            fresh: AtomicBool::new(false),
            sizes: Mutex::new(Vec::new()),
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let s = lock(&self.stored).clone();
        if let Err(e) = files::write_json(path, &s) {
            log::warn!("lighting: cannot write {}: {e}", path.display());
        }
    }

    fn settings(&self) -> LightingSettings {
        lock(&self.stored).settings.clone()
    }

    fn status(&self) -> LightingStatus {
        let mut s = lock(&self.status).clone();
        s.offer = cfg!(windows).then(|| setup::VERSION.to_owned());
        s.setup = *lock(&self.setting_up);
        if s.setup.is_some() && s.link != LightLink::Off {
            s.link = LightLink::SettingUp;
        }
        s
    }
}

/// What the view gets: the settings and how lighting is now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightingView {
    pub settings: LightingSettings,
    pub status: LightingStatus,
    /// Monitors to follow, the primary first.
    pub monitors: usize,
}

fn view(state: &LightingState) -> LightingView {
    #[cfg(windows)]
    let monitors = clokrix_core::screen::capture::monitors().len().max(1);
    #[cfg(not(windows))]
    let monitors = 1;
    LightingView {
        settings: state.settings(),
        status: state.status(),
        monitors,
    }
}

fn tell(app: &AppHandle, state: &LightingState) {
    if Instant::now() < *lock(&state.watched) {
        if let Err(e) = app.emit(EVENT, state.status()) {
            log::debug!("emit {EVENT}: {e}");
        }
    }
}

/// At start: puts back the modes a crash left noted, then starts lighting if it was on.
pub fn start(app: &AppHandle) {
    let Some(state) = app.try_state::<LightingState>() else {
        return;
    };
    if !lock(&state.stored).saved.is_empty() {
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("clokrix-lighting-restore".into())
            .spawn(move || {
                if let Some(state) = app.try_state::<LightingState>() {
                    if let Ok(mut c) = Client::connect(openrgb()) {
                        if let Ok(devices) = c.controllers() {
                            restore_all(&state, &mut c, &devices, None);
                        }
                    }
                }
                resume(&app);
            });
        if let Err(e) = spawned {
            log::warn!("lighting: {e}");
        }
        return;
    }
    resume(app);
}

/// Lighting on, if it is set on and Pro is open; off otherwise.
fn resume(app: &AppHandle) {
    let Some(state) = app.try_state::<LightingState>() else {
        return;
    };
    let wanted = state.settings().on && crate::edition::full(app);
    if !wanted {
        state.stop.store(true, Ordering::SeqCst);
        return;
    }
    if state.running.swap(true, Ordering::SeqCst) {
        state.stop.store(false, Ordering::SeqCst);
        return;
    }
    state.stop.store(false, Ordering::SeqCst);
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("clokrix-lighting".into())
        .spawn(move || {
            run(&app);
            if let Some(s) = app.try_state::<LightingState>() {
                s.running.store(false, Ordering::SeqCst);
                s.holding.store(false, Ordering::SeqCst);
                *lock(&s.status) = LightingStatus::default();
                tell(&app, &s);
                // Turned on again while it was stopping.
                if s.settings().on && !s.stop.load(Ordering::SeqCst) && crate::edition::full(&app) {
                    resume(&app);
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("lighting: not started: {e}");
        state.running.store(false, Ordering::SeqCst);
    }
}

/// Pro on or off (`edition::apply`): off stops lighting, putting the devices back; on brings it
/// back if it was on.
pub fn set_pro(app: &AppHandle, on: bool) {
    if on {
        resume(app);
    } else if let Some(state) = app.try_state::<LightingState>() {
        state.stop.store(true, Ordering::SeqCst);
    }
}

/// As Clokrix closes: lighting stops and the devices go back, waiting a moment for it.
pub fn wind_down(app: &AppHandle) {
    let Some(state) = app.try_state::<LightingState>() else {
        return;
    };
    state.stop.store(true, Ordering::SeqCst);
    let until = Instant::now() + Duration::from_secs(2);
    while state.running.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What hands a screen session's pictures to lighting.
pub type Tap = Box<dyn Fn(&clokrix_core::screen::producer::Picture) + Send>;

/// A screen session is about to capture `monitor`: lighting lets go of it (waiting a moment for
/// that) and returns what hands the session's pictures to it. None when lighting does not follow
/// that monitor.
pub fn yield_output(app: &AppHandle, monitor: usize) -> Option<Tap> {
    let state = app.try_state::<LightingState>()?;
    if !state.running.load(Ordering::SeqCst) || state.settings().monitor as usize != monitor {
        return None;
    }
    *lock(&state.yielded) = Some(monitor as u32);
    let until = Instant::now() + Duration::from_secs(1);
    while state.holding.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let app = app.clone();
    Some(Box::new(move |p| {
        if let Some(s) = app.try_state::<LightingState>() {
            // Every 4th pixel each way: the session paid for the picture already.
            *lock(&s.tapped) = Some(Thumb::from_bgra(&p.bgra, p.width, p.height, 4));
        }
    }))
}

/// The screen session ended: lighting takes the monitor again.
pub fn resume_output(app: &AppHandle) {
    if let Some(state) = app.try_state::<LightingState>() {
        *lock(&state.yielded) = None;
        *lock(&state.tapped) = None;
    }
}

fn openrgb() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, PORT))
}

/// Puts each noted device back in its mode (`only`: those devices only), and forgets the notes
/// of those put back or gone.
fn restore_all(
    state: &LightingState,
    c: &mut Client,
    devices: &[Controller],
    only: Option<&HashSet<String>>,
) {
    let saved = lock(&state.stored).saved.clone();
    let mut kept = Vec::new();
    for s in saved {
        if only.is_some_and(|o| !o.contains(&s.device)) {
            kept.push(s);
            continue;
        }
        let Some(d) = devices.iter().find(|d| device_id(d) == s.device) else {
            continue;
        };
        match restore_mode(d, &s, c.version) {
            Some((mode, raw)) => {
                if let Err(e) = c.set_mode(d.index, mode, &raw) {
                    log::info!("lighting: {} not put back: {e}", d.name);
                    kept.push(s);
                } else {
                    log::info!("lighting: {} back in {}", d.name, s.name);
                }
            }
            None => log::info!("lighting: {}'s mode {} is gone", d.name, s.name),
        }
    }
    lock(&state.stored).saved = kept;
    state.save();
}

/// OpenRGB does not answer: what the view says, and OpenRGB started if it is installed without
/// its service (once a run of the thread).
fn silent(
    state: &LightingState,
    now: Instant,
    silent_since: &mut Option<Instant>,
    started_at: &mut Option<Instant>,
) -> LightLink {
    if lock(&state.setting_up).is_some() {
        return LightLink::SettingUp;
    }
    let since = *silent_since.get_or_insert(now);
    let found = setup::find();
    match setup::next(
        &found,
        now - since,
        started_at.map(|at| now.saturating_duration_since(at)),
    ) {
        Next::Missing => LightLink::Missing,
        Next::Wait => LightLink::Starting,
        Next::NotRunning => LightLink::NotRunning,
        Next::Start(exe) => {
            *started_at = Some(now);
            #[cfg(windows)]
            {
                match setup::start(&exe) {
                    Ok(()) => {
                        log::info!("lighting: started {}", exe.display());
                        LightLink::Starting
                    }
                    Err(e) => {
                        log::warn!("lighting: {e}");
                        LightLink::NotRunning
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let _ = exe;
                LightLink::NotRunning
            }
        }
    }
}

/// The lighting programs of `others::KNOWN` running now, from the names of the services and
/// processes running (read only).
fn other_lighting() -> Vec<OtherLighting> {
    #[cfg(windows)]
    return clokrix_core::lighting::others::found(
        &clokrix_core::ipc::transport::running_services(),
        &clokrix_core::live::names(),
    );
    #[cfg(not(windows))]
    Vec::new()
}

/// Each program as the log names it: with its services running, or its processes.
fn others_named(found: &[OtherLighting]) -> Vec<String> {
    found
        .iter()
        .map(|o| {
            let what = if o.services.is_empty() {
                &o.processes
            } else {
                &o.services
            };
            format!("{} ({})", o.name, what.join(", "))
        })
        .collect()
}

/// Whether a game is in front (for the Auto speed).
fn game_in_front(app: &AppHandle) -> bool {
    crate::gaming::game_in_front(app)
}

/// The lighting thread: until stopped, the screen's colour to the devices picked.
fn run(app: &AppHandle) {
    let Some(state) = app.try_state::<LightingState>() else {
        return;
    };
    log::info!("lighting: on");
    // Sets from an earlier run are not this one's.
    lock(&state.sizes).clear();
    let mut client: Option<Client> = None;
    let mut devices: Vec<Controller> = Vec::new();
    let mut look_at = Instant::now();
    let mut reread_at = Instant::now() + REREAD;
    // OpenRGB not answering since then; started here then (once a run).
    let mut silent_since: Option<Instant> = None;
    let mut started_at: Option<Instant> = None;
    // What OpenRGB last failed with, logged once until it changes.
    let mut failed: Option<String> = None;
    // Other lighting programs: looked for then; those last logged.
    let mut others_at = Instant::now();
    let mut others_logged: Vec<String> = Vec::new();
    // Devices in Direct now, and what each was last sent.
    let mut direct: HashSet<String> = HashSet::new();
    let mut sent: HashMap<String, (Rgb8, Instant)> = HashMap::new();
    let mut picker = Picker::new();
    let mut target: Option<Target> = None;
    let mut stepped = Instant::now();
    let mut told = Instant::now();
    #[cfg(windows)]
    let mut capture: Option<clokrix_core::screen::capture::Capture> = None;
    #[cfg(windows)]
    let mut open_at = Instant::now();
    while !state.stop.load(Ordering::SeqCst) {
        let tick = Instant::now();
        let settings = state.settings();

        // Other lighting programs, which take the lights back: named, never touched.
        if tick >= others_at {
            others_at = tick + OTHERS;
            let found = other_lighting();
            let named = others_named(&found);
            if named != others_logged {
                if named.is_empty() {
                    log::info!("lighting: no other lighting program runs now");
                } else {
                    log::info!(
                        "lighting: {} runs: it takes the lights back",
                        named.join("; ")
                    );
                }
                others_logged = named;
            }
            lock(&state.status).others = found;
        }

        // OpenRGB.
        if state.fresh.swap(false, Ordering::SeqCst) {
            silent_since = None;
            started_at = None;
            look_at = tick;
        }
        if client.is_none() && tick >= look_at {
            look_at = tick + RETRY;
            match Client::connect(openrgb()).and_then(|mut c| c.controllers().map(|d| (c, d))) {
                Ok((c, d)) => {
                    log::info!(
                        "lighting: OpenRGB (protocol {}) has {} devices",
                        c.version,
                        d.len()
                    );
                    client = Some(c);
                    devices = d;
                    direct.clear();
                    sent.clear();
                    silent_since = None;
                    failed = None;
                    reread_at = tick + REREAD;
                }
                Err(e) => {
                    let said = e.to_string();
                    if failed.as_deref() != Some(said.as_str()) {
                        log::info!("lighting: OpenRGB does not answer: {said}");
                        failed = Some(said);
                    }
                    let link = silent(&state, tick, &mut silent_since, &mut started_at);
                    lock(&state.status).link = link;
                }
            }
        }
        if let Some(c) = &mut client {
            match c.devices_changed() {
                Ok(false) => {}
                Ok(true) => match c.controllers() {
                    Ok(d) => {
                        devices = d;
                        direct.clear();
                        sent.clear();
                    }
                    Err(e) => {
                        log::info!("lighting: OpenRGB: {e}");
                        client = None;
                    }
                },
                Err(e) => {
                    log::info!("lighting: OpenRGB went: {e}");
                    client = None;
                }
            }
        }

        // A size set in OpenRGB (its Edit Zone) tells no client: the devices read again.
        if tick >= reread_at {
            reread_at = tick + REREAD;
            if let Some(c) = &mut client {
                match c.controllers() {
                    Ok(now) => {
                        let ids = |l: &[Controller]| l.iter().map(device_id).collect::<Vec<_>>();
                        if ids(&now) != ids(&devices) {
                            // Another list: as when OpenRGB says its devices changed.
                            direct.clear();
                            sent.clear();
                        } else {
                            for (was, d) in devices.iter().zip(&now) {
                                if was.leds != d.leds {
                                    log::info!("lighting: {} now has {} LEDs", d.name, d.leds);
                                    sent.remove(&device_id(d));
                                }
                            }
                        }
                        devices = now;
                    }
                    Err(e) => {
                        log::info!("lighting: OpenRGB: {e}");
                        client = None;
                    }
                }
            }
        }

        // The owner's Set: a header's LED count, then the device read again for its new count.
        if let Some(c) = client.as_mut().filter(|_| !lock(&state.sizes).is_empty()) {
            let asked = std::mem::take(&mut *lock(&state.sizes));
            for z in asked {
                let Some(i) = devices.iter().position(|d| device_id(d) == z.device) else {
                    log::info!("lighting: no device for {}'s LEDs now", z.zone);
                    continue;
                };
                let (index, name) = (devices[i].index, devices[i].name.clone());
                let Some((zone, leds)) = devices[i].zone_size(&z.zone, z.leds) else {
                    log::info!("lighting: {name} has no {} whose LEDs can be set", z.zone);
                    continue;
                };
                match c
                    .resize_zone(index, zone, leds)
                    .and_then(|()| c.controller(index))
                {
                    Ok(again) => {
                        let has = again.zones.get(zone as usize).map_or(0, |z| z.count);
                        if has == leds {
                            log::info!("lighting: {name}: {} set to {leds} LEDs", z.zone);
                        } else {
                            log::info!(
                                "lighting: {name}: {} asked for {leds} LEDs, OpenRGB has {has}",
                                z.zone
                            );
                        }
                        sent.remove(&z.device);
                        devices[i] = again;
                    }
                    Err(e) => {
                        log::info!("lighting: OpenRGB: {e}");
                        client = None;
                        break;
                    }
                }
            }
        }

        // The devices picked, in Direct (their mode noted first); those unpicked, put back.
        if let Some(c) = &mut client {
            let picked: HashSet<String> = settings.devices.iter().cloned().collect();
            let unpicked: HashSet<String> = direct.difference(&picked).cloned().collect();
            if !unpicked.is_empty() {
                restore_all(&state, c, &devices, Some(&unpicked));
                for d in &unpicked {
                    direct.remove(d);
                    sent.remove(d);
                }
            }
            for d in &devices {
                let id = device_id(d);
                if !picked.contains(&id) || direct.contains(&id) {
                    continue;
                }
                let Some(m) = d.direct_mode() else { continue };
                {
                    let mut stored = lock(&state.stored);
                    if !stored.saved.iter().any(|s| s.device == id) {
                        if let Some(s) = saved_mode(d, c.version) {
                            stored.saved.push(s);
                        }
                    }
                }
                state.save();
                match c.set_mode(d.index, m as i32, &d.modes[m].raw) {
                    Ok(()) => {
                        let empty = d.empty_zones();
                        if empty.is_empty() {
                            log::info!("lighting: {} in Direct, {} LEDs", d.name, d.leds);
                        } else {
                            log::info!(
                                "lighting: {} in Direct, {} LEDs; no LEDs set in OpenRGB for {}",
                                d.name,
                                d.leds,
                                empty.join(", ")
                            );
                        }
                        direct.insert(id);
                    }
                    Err(e) => {
                        log::info!("lighting: OpenRGB: {e}");
                        client = None;
                        break;
                    }
                }
            }
            let mut status = lock(&state.status);
            status.link = if client.is_some() {
                LightLink::Connected
            } else {
                LightLink::NotRunning
            };
            status.devices = devices.iter().map(|d| info(d, &settings.devices)).collect();
        }

        // The screen: looked at only while a device takes its colour, or the view shows it.
        let mut thumb: Option<Thumb> = None;
        let mut held: Option<String> = None;
        let wanted = !direct.is_empty() || Instant::now() < *lock(&state.watched);
        let yielded = *lock(&state.yielded) == Some(settings.monitor);
        if !wanted {
            #[cfg(windows)]
            {
                capture = None;
            }
            state.holding.store(false, Ordering::SeqCst);
            lock(&state.status).capture = None;
            std::thread::sleep(Duration::from_millis(250));
        } else if yielded {
            #[cfg(windows)]
            {
                capture = None;
            }
            state.holding.store(false, Ordering::SeqCst);
            thumb = lock(&state.tapped).take();
            lock(&state.status).capture = Some("with the phone's screen session".into());
            std::thread::sleep(FRAME);
        } else {
            #[cfg(windows)]
            {
                use clokrix_core::screen::capture::{Capture, CaptureError};
                if capture
                    .as_ref()
                    .is_some_and(|c| c.monitor.index != settings.monitor as usize)
                {
                    capture = None;
                }
                if capture.is_none() && tick >= open_at {
                    match Capture::open(settings.monitor as usize) {
                        Ok(c) => {
                            capture = Some(c);
                            state.holding.store(true, Ordering::SeqCst);
                        }
                        Err(e) => {
                            open_at = tick + Duration::from_millis(500);
                            held = Some(e.to_string());
                        }
                    }
                }
                state.holding.store(capture.is_some(), Ordering::SeqCst);
                match capture
                    .as_mut()
                    .map(|c| c.next_thumb(FRAME.as_millis() as u32))
                {
                    Some(Ok(t)) => thumb = t,
                    Some(Err(CaptureError::Lost)) => {
                        std::thread::sleep(Duration::from_millis(100));
                        if let Some(c) = &mut capture {
                            let _ = c.reopen();
                        }
                    }
                    Some(Err(CaptureError::Secure)) => {
                        held = Some(CaptureError::Secure.to_string());
                        std::thread::sleep(Duration::from_millis(500));
                        if let Some(c) = &mut capture {
                            let _ = c.reopen();
                        }
                    }
                    Some(Err(e)) => {
                        log::info!("lighting: the screen: {e}");
                        held = Some(e.to_string());
                        capture = None;
                        state.holding.store(false, Ordering::SeqCst);
                        open_at = Instant::now() + Duration::from_secs(2);
                    }
                    None => std::thread::sleep(FRAME),
                }
                lock(&state.status).capture = capture.as_ref().map(|c| {
                    if c.method() == "GDI" {
                        "GDI".to_owned()
                    } else {
                        "Desktop Duplication".to_owned()
                    }
                });
            }
            #[cfg(not(windows))]
            std::thread::sleep(FRAME);
        }

        // The colour, eased, to each device picked as often as it takes one.
        let accent = lighting::colour::PickSettings::default().accent;
        let pick = lighting::pick_settings(&settings, game_in_front(app), accent);
        if let Some(t) = &thumb {
            target = Some(picker.target(t, &pick));
        }
        let dt = stepped.elapsed().as_secs_f32();
        stepped = Instant::now();
        let colour = target.as_ref().map(|t| picker.step(t, dt, &pick));
        if let (Some(colour), Some(c)) = (colour, &mut client) {
            for d in &devices {
                let id = device_id(d);
                if !direct.contains(&id) {
                    continue;
                }
                let due = sent
                    .get(&id)
                    .is_none_or(|(was, at)| *was != colour && at.elapsed() >= interval(d));
                if !due {
                    continue;
                }
                if let Err(e) = c.set_colour(d.index, d.leds, colour) {
                    log::info!("lighting: OpenRGB: {e}");
                    client = None;
                    break;
                }
                sent.insert(id, (colour, Instant::now()));
            }
        }
        {
            let mut status = lock(&state.status);
            status.colour = colour;
            status.held = held;
        }
        if told.elapsed() >= TELL {
            told = Instant::now();
            tell(app, &state);
        }
    }
    // Every device back as it was.
    if let Some(c) = &mut client {
        restore_all(&state, c, &devices, None);
    }
    state.holding.store(false, Ordering::SeqCst);
    log::info!("lighting: off");
}

#[tauri::command]
pub fn get_lighting(state: tauri::State<'_, LightingState>) -> LightingView {
    view(&state)
}

/// Lighting set from the view: turned on or off, devices picked, speed and the rest.
#[tauri::command]
pub fn set_lighting(
    app: AppHandle,
    state: tauri::State<'_, LightingState>,
    settings: LightingSettings,
) -> Result<LightingView, String> {
    if settings.on && !crate::edition::full(&app) {
        return Err("Ambient lighting is part of Clokrix Pro.".into());
    }
    lock(&state.stored).settings = settings.clamped();
    state.save();
    resume(&app);
    Ok(view(&state))
}

/// The owner's Set (2.6.3): a header's LED count, sent to OpenRGB by the lighting thread, within
/// the zone's bounds. The view sees the new count in its status.
#[tauri::command]
pub fn set_light_zone(
    app: AppHandle,
    state: tauri::State<'_, LightingState>,
    size: ZoneSize,
) -> Result<(), String> {
    if !crate::edition::full(&app) {
        return Err("Ambient lighting is part of Clokrix Pro.".into());
    }
    if !state.settings().on {
        return Err("Turn ambient lighting on first.".into());
    }
    let mut sizes = lock(&state.sizes);
    // The newest for a zone wins.
    sizes.retain(|z| (&z.device, &z.zone) != (&size.device, &size.zone));
    if sizes.len() >= SIZES {
        return Err("Too many at once: wait a moment.".into());
    }
    sizes.push(size);
    Ok(())
}

/// The view is open: it hears the colour for the next few seconds.
#[tauri::command]
pub fn watch_lighting(state: tauri::State<'_, LightingState>) -> LightingStatus {
    *lock(&state.watched) = Instant::now() + Duration::from_secs(5);
    state.status()
}

/// "Set up OpenRGB": its pinned installer downloaded and checked, then run after an
/// administrator prompt on this PC. Says what came of it.
#[tauri::command]
pub async fn set_up_openrgb(app: AppHandle) -> Result<String, String> {
    let state = app
        .try_state::<LightingState>()
        .ok_or("Clokrix is starting.")?;
    if !crate::edition::full(&app) {
        return Err("Ambient lighting is part of Clokrix Pro.".into());
    }
    if !cfg!(windows) {
        return Err("Clokrix sets up OpenRGB on Windows only.".into());
    }
    if setup::find().exe.is_some() {
        return Ok("OpenRGB is on this PC already.".into());
    }
    {
        let mut step = lock(&state.setting_up);
        if step.is_some() {
            return Err("OpenRGB's set-up is running already.".into());
        }
        *step = Some(SetupStep::Downloading);
    }
    tell(&app, &state);
    let dir = app
        .path()
        .app_local_data_dir()
        .ok()
        .map(|d| d.join("openrgb"));
    let worker = app.clone();
    let done = tauri::async_runtime::spawn_blocking(move || set_up(&worker, dir))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    *lock(&state.setting_up) = None;
    state.fresh.store(true, Ordering::SeqCst);
    tell(&app, &state);
    match done {
        Ok(setup::Installed::Done) => {
            log::info!("lighting: OpenRGB {} set up", setup::VERSION);
            Ok(format!(
                "OpenRGB {} is set up: Clokrix finds your devices in a moment.",
                setup::VERSION
            ))
        }
        Ok(setup::Installed::Restart) => {
            Ok("Almost: restart this PC to finish setting up OpenRGB.".into())
        }
        Err(e) => {
            log::warn!("lighting: OpenRGB not set up: {e}");
            Err(e)
        }
    }
}

/// The download, then the install.
fn set_up(app: &AppHandle, dir: Option<PathBuf>) -> Result<setup::Installed, String> {
    let bytes = setup::download()?;
    if let Some(state) = app.try_state::<LightingState>() {
        *lock(&state.setting_up) = Some(SetupStep::Installing);
        tell(app, &state);
    }
    #[cfg(windows)]
    {
        let dir = dir.ok_or("Clokrix has no folder for the download.")?;
        let package = setup::Package::write(&bytes, &dir)?;
        setup::install(&package)
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, dir);
        Err("Clokrix sets up OpenRGB on Windows only.".into())
    }
}

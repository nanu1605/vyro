//! The PC's screen on a phone (1.7). A phone the owner allowed to (Settings → Remote, per phone)
//! sends its WebRTC offer through the relay, sealed; the session runs here in the app, as the
//! signed-in user: the picture goes straight to the phone and its taps and typing come back as
//! Windows input (`vyro_core::screen`). Where no direct path exists the phone asks for one
//! picture at a time through the relay instead (the slow view, 20 minutes a day). One phone at a
//! time; while it lasts the PC shows a banner and a tray item, both with Stop. Nothing captures
//! unless a session is on.

// Elsewhere only the commands and the ends are built (and never reached).
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vyro_core::remote::relay::Request;
use vyro_core::screen::share::{self, ScreenShare, ScreenStatus, SlowAllowance};
use vyro_core::{files, lock, unix_now};

/// Emitted with the [`ScreenStatus`] when a session starts or ends, or the settings change.
pub const SCREEN_EVENT: &str = "screen-share";
const FILE: &str = "screen.json";
/// A slow view whose phone stopped asking for pictures this long has ended.
#[cfg(windows)]
const SLOW_GONE: Duration = Duration::from_secs(60);
/// The longest offer taken (a browser's is a few kB).
#[cfg(windows)]
const MAX_OFFER: usize = 32 * 1024;
/// The most inputs one slow-view message carries.
#[cfg(windows)]
const MAX_INPUTS: usize = 20;
/// The slow view's JPEG quality.
#[cfg(windows)]
const SLOW_QUALITY: f32 = 0.55;
/// What today's slow view used is saved this often.
const SAVE_EVERY: Duration = Duration::from_secs(30);
#[cfg(not(windows))]
const ONLY_WINDOWS: &str = "Only Vyro on Windows shares its screen.";

/// `screen.json`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    lock_on_end: bool,
    slow: SlowAllowance,
}

/// Who ended a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The owner, at the PC: no lock.
    Pc,
    /// The phone, the network, the idle rule.
    Phone,
    /// The same phone started again.
    Replaced,
}

pub struct ScreenState {
    path: Option<PathBuf>,
    stored: Mutex<Stored>,
    saved: Mutex<Instant>,
    live: Mutex<Option<Live>>,
    serial: AtomicU64,
}

struct Live {
    serial: u64,
    share: ScreenShare,
    started: Instant,
    stop: Arc<AtomicBool>,
    /// When the phone last showed it is there: input, its "here", a picture asked for.
    used: Arc<Mutex<Instant>>,
    #[cfg(windows)]
    kind: Kind,
}

#[cfg(windows)]
enum Kind {
    Direct(Arc<vyro_core::screen::rtc::Session>),
    Slow(Box<Slow>),
}

/// The slow view: the monitor, and the picture last taken.
#[cfg(windows)]
struct Slow {
    capture: vyro_core::screen::capture::Capture,
    last: Option<vyro_core::screen::capture::Picture>,
    /// A newer picture than the phone has.
    fresh: bool,
    asked: Option<Instant>,
    /// Milliseconds used and not yet counted against today's allowance.
    pending_ms: u64,
}

impl ScreenState {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_local_data_dir().ok().map(|d| d.join(FILE));
        let stored = path
            .as_deref()
            .and_then(|p| files::read_json::<Stored>(p, |_| Ok(())).ok())
            .flatten()
            .unwrap_or_default();
        Self {
            path,
            stored: Mutex::new(stored),
            saved: Mutex::new(Instant::now()),
            live: Mutex::new(None),
            serial: AtomicU64::new(0),
        }
    }

    fn save(&self) {
        *lock(&self.saved) = Instant::now();
        let stored = *lock(&self.stored);
        if let Some(p) = &self.path {
            if let Err(e) = files::write_json(p, &stored) {
                log::warn!("screen: {e}");
            }
        }
    }

    pub fn status(&self) -> ScreenStatus {
        ScreenStatus {
            share: lock(&self.live).as_ref().map(|l| l.share.clone()),
            lock_on_end: lock(&self.stored).lock_on_end,
            slow_left: lock(&self.stored).slow.left(unix_now()),
        }
    }
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, ScreenState>> {
    app.try_state::<ScreenState>()
}

/// Tells the window and the tray.
fn announce(app: &AppHandle) {
    let Some(state) = state(app) else {
        return;
    };
    let status = state.status();
    crate::tray::set_screen_item(app, status.share.as_ref().map(|s| s.phone.as_str()));
    if let Err(e) = app.emit(SCREEN_EVENT, &status) {
        log::debug!("emit {SCREEN_EVENT}: {e}");
    }
}

/// "12 s", "4 min".
fn span(d: Duration) -> String {
    match d.as_secs() {
        s if s < 60 => format!("{s} s"),
        s => format!("{} min", (s + 30) / 60),
    }
}

/// A phone's request for the screen: the relay connection has checked its permission.
pub fn handle(app: &AppHandle, r: &Request) -> Result<Value, String> {
    #[cfg(windows)]
    return win::handle(app, r);
    #[cfg(not(windows))]
    {
        let _ = (app, r);
        Err(ONLY_WINDOWS.into())
    }
}

/// Refuses a second phone; ends the same phone's earlier session (it started again).
#[cfg(windows)]
fn claim(app: &AppHandle, r: &Request) -> Result<(), String> {
    let Some(state) = state(app) else {
        return Err("Vyro is starting.".into());
    };
    let current = lock(&state.live)
        .as_ref()
        .map(|l| (l.serial, l.share.id.clone(), l.share.phone.clone()));
    match current {
        Some((_, id, name)) if id != r.phone => {
            Err(format!("The screen is already shared with {name}."))
        }
        Some((serial, ..)) => {
            end(app, serial, "the phone started again", By::Replaced);
            Ok(())
        }
        None => Ok(()),
    }
}

/// Keeps a session that just started, tells the owner, and watches it.
#[cfg(windows)]
fn begin(app: &AppHandle, live: Live) -> Result<(), String> {
    let state = state(app).ok_or("Vyro is starting.")?;
    let (serial, phone, slow) = (live.serial, live.share.phone.clone(), live.share.slow);
    let (stop, used) = (Arc::clone(&live.stop), Arc::clone(&live.used));
    {
        let mut current = lock(&state.live);
        if let Some(other) = current.as_ref() {
            live.stop.store(true, Ordering::SeqCst);
            let name = other.share.phone.clone();
            drop(current);
            std::thread::spawn(move || drop(live));
            return Err(format!("The screen is already shared with {name}."));
        }
        *current = Some(live);
    }
    log::info!(
        "screen: shared with a phone{}",
        if slow { " (slow view)" } else { "" }
    );
    crate::remote_api::log_action(
        app,
        &phone,
        if slow {
            "Started the slow view of the screen".into()
        } else {
            "Started sharing the screen".into()
        },
    );
    if let Err(e) = app
        .notification()
        .builder()
        .title("Your screen is shared")
        .body(format!(
            "{phone} sees this screen and can use it. Stop it from Vyro's window or its tray icon."
        ))
        .show()
    {
        log::warn!("screen: notification failed: {e}");
    }
    announce(app);
    watch(app, serial, stop, used, slow);
    Ok(())
}

/// Ends the session when the phone has left it.
#[cfg(windows)]
fn watch(
    app: &AppHandle,
    serial: u64,
    stop: Arc<AtomicBool>,
    used: Arc<Mutex<Instant>>,
    slow: bool,
) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vyro-screen-idle".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let since = lock(&used).elapsed();
            let gone = if slow {
                (since >= SLOW_GONE).then_some("the phone stopped asking for pictures")
            } else {
                share::idle(since).then_some("nobody used it for 10 minutes")
            };
            if let Some(why) = gone {
                end(&app, serial, why, By::Phone);
                return;
            }
        });
    if let Err(e) = spawned {
        log::warn!("screen: no idle watch: {e}");
    }
}

/// Ends session `serial` (a later one is left alone).
fn end(app: &AppHandle, serial: u64, why: &str, by: By) {
    let Some(state) = state(app) else {
        return;
    };
    let live = {
        let mut current = lock(&state.live);
        if current.as_ref().is_some_and(|l| l.serial == serial) {
            current.take()
        } else {
            None
        }
    };
    let Some(live) = live else {
        return;
    };
    live.stop.store(true, Ordering::SeqCst);
    #[cfg(windows)]
    if let Kind::Direct(s) = &live.kind {
        s.tell(share::Note::End { why: why.into() }.json());
        s.end();
    }
    let lasted = live.started.elapsed();
    log::info!("screen: ended after {} s: {why}", lasted.as_secs());
    crate::remote_api::log_action(
        app,
        &live.share.phone,
        format!("Stopped sharing the screen after {}: {why}", span(lasted)),
    );
    state.save();
    announce(app);
    // Dropping a session waits for its thread, which may be the one calling here.
    let _ = std::thread::Builder::new()
        .name("vyro-screen-end".into())
        .spawn(move || drop(live));
    if by == By::Phone && lock(&state.stored).lock_on_end {
        let lock_pc = vyro_core::actions::power::PowerRequest {
            action: vyro_core::actions::power::PowerAction::Lock,
            wait: 0,
            force: false,
        };
        if let Err(e) = vyro_core::actions::power::act(&lock_pc, "the end of a screen session") {
            log::warn!("screen: lock: {e}");
        }
    }
}

/// Ends the session of phone `id` (hex), if it has one: signed out, or no longer allowed.
pub fn end_phone(app: &AppHandle, id: &str, why: &str) {
    end_of(app, id, why, By::Pc);
}

fn end_of(app: &AppHandle, id: &str, why: &str, by: By) {
    let serial = state(app).and_then(|s| {
        lock(&s.live)
            .as_ref()
            .filter(|l| l.share.id == id)
            .map(|l| l.serial)
    });
    if let Some(serial) = serial {
        end(app, serial, why, by);
    }
}

/// Ends any session (the relay turned off, Vyro closing, Stop at the PC).
pub fn end_all(app: &AppHandle, why: &str) {
    let serial = state(app).and_then(|s| lock(&s.live).as_ref().map(|l| l.serial));
    if let Some(serial) = serial {
        end(app, serial, why, By::Pc);
    }
}

#[cfg(windows)]
mod win {
    use std::net::IpAddr;

    use serde_json::json;
    use vyro_core::remote::relay::proto::b64url;
    use vyro_core::screen::capture::{self, Capture, CaptureError};
    use vyro_core::screen::rtc::{self, SessionEvent};
    use vyro_core::screen::share::{MonitorChoice, Note, SLOW_INTERVAL, SLOW_MAX, SLOW_SPENT};
    use vyro_core::screen::{frames, inject, input, jpeg, producer};

    use super::*;

    pub fn handle(app: &AppHandle, r: &Request) -> Result<Value, String> {
        match r.method.as_str() {
            "screen.monitors" => monitors(),
            "screen.start" => start(app, r),
            "screen.stop" => {
                end_of(app, &r.phone, "stopped on the phone", By::Phone);
                Ok(json!({ "done": true }))
            }
            "screen.slow" => slow(app, r),
            "screen.input" => slow_input(app, r),
            _ => Err(vyro_core::remote::relay::api::UNKNOWN.into()),
        }
    }

    fn monitor_param(r: &Request) -> usize {
        r.params
            .get("monitor")
            .and_then(Value::as_u64)
            .and_then(|m| usize::try_from(m).ok())
            .unwrap_or(0)
    }

    fn monitors() -> Result<Value, String> {
        let list: Vec<MonitorChoice> = capture::monitors()
            .into_iter()
            .map(|m| MonitorChoice {
                index: u32::try_from(m.index).unwrap_or(0),
                name: m.name,
                width: u32::try_from(m.rect.width).unwrap_or(0),
                height: u32::try_from(m.rect.height).unwrap_or(0),
                primary: m.rect.left == 0 && m.rect.top == 0,
            })
            .collect();
        serde_json::to_value(list).map_err(|e| e.to_string())
    }

    fn next_serial(app: &AppHandle) -> u64 {
        state(app).map_or(0, |s| s.serial.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn share(r: &Request, slow: bool) -> ScreenShare {
        ScreenShare {
            phone: r.name.clone(),
            id: r.phone.clone(),
            since: unix_now(),
            slow,
        }
    }

    /// A direct session: the phone's offer in, the PC's answer out.
    fn start(app: &AppHandle, r: &Request) -> Result<Value, String> {
        let offer = r
            .params
            .get("offer")
            .and_then(Value::as_str)
            .ok_or("The request needs \"offer\".")?
            .to_owned();
        if offer.len() > MAX_OFFER {
            return Err("The phone's offer is too long.".into());
        }
        let monitor = monitor_param(r);
        if monitor >= capture::monitors().len() {
            return Err("No such screen.".into());
        }
        claim(app, r)?;
        let hosts: Vec<IpAddr> = crate::remote_api::wake_sources()
            .into_iter()
            .map(|(ip, _)| IpAddr::V4(ip))
            .collect();
        let opts = rtc::Options {
            hosts,
            ..rtc::Options::default()
        };
        let serial = next_serial(app);
        let used = Arc::new(Mutex::new(Instant::now()));
        let rect = Arc::new(Mutex::new(None));
        let sink: rtc::EventSink = {
            let (app, used, rect) = (app.clone(), Arc::clone(&used), Arc::clone(&rect));
            Box::new(move |e| match e {
                SessionEvent::Connected => log::info!("screen: the phone is connected"),
                SessionEvent::Input(i) => {
                    *lock(&used) = Instant::now();
                    if let Some(on) = *lock(&rect) {
                        if let Err(e) = inject::apply(&i, on) {
                            log::debug!("screen: {e}");
                        }
                    }
                }
                SessionEvent::Closed(why) => end(&app, serial, &why, By::Phone),
            })
        };
        let (session, answer) = rtc::start(&offer, &opts, sink)?;
        let session = Arc::new(session);
        let stop = Arc::new(AtomicBool::new(false));
        begin(
            app,
            Live {
                serial,
                share: share(r, false),
                started: Instant::now(),
                stop: Arc::clone(&stop),
                used,
                kind: Kind::Direct(Arc::clone(&session)),
            },
        )?;
        let owner = app.clone();
        let spawned = std::thread::Builder::new()
            .name("vyro-screen".into())
            .spawn(move || {
                let settings = producer::Settings {
                    monitor,
                    ..producer::Settings::default()
                };
                let status = |s: producer::Status| match s {
                    producer::Status::Sending {
                        width,
                        height,
                        rect: on,
                    } => {
                        *lock(&rect) = Some(on);
                        session.tell(
                            Note::Sending {
                                w: width,
                                h: height,
                            }
                            .json(),
                        );
                    }
                    producer::Status::Secure => session.tell(Note::Secure.json()),
                };
                if let Err(e) = producer::run(&session, &settings, &stop, &status) {
                    let why = format!("the screen could not be sent: {e}");
                    end(&owner, serial, &why, By::Phone);
                }
            });
        if let Err(e) = spawned {
            end(app, serial, "it could not start", By::Phone);
            return Err(format!("cannot start the screen: {e}"));
        }
        Ok(json!({ "answer": answer }))
    }

    /// The slow view's next picture, through the relay: `{jpeg, w, h, left}` when the screen
    /// changed, `{same: true, left}` when it did not, `{secure: true, left}` when Windows shows
    /// its secure desktop. The first ask starts the session.
    fn slow(app: &AppHandle, r: &Request) -> Result<Value, String> {
        let state = state(app).ok_or("Vyro is starting.")?;
        let now = unix_now();
        if lock(&state.stored).slow.left(now) == 0 {
            return Err(SLOW_SPENT.into());
        }
        let ours = lock(&state.live)
            .as_ref()
            .is_some_and(|l| l.share.id == r.phone && matches!(l.kind, Kind::Slow(_)));
        if !ours {
            claim(app, r)?;
            let capture = Capture::open(monitor_param(r)).map_err(|e| e.to_string())?;
            begin(
                app,
                Live {
                    serial: next_serial(app),
                    share: share(r, true),
                    started: Instant::now(),
                    stop: Arc::new(AtomicBool::new(false)),
                    used: Arc::new(Mutex::new(Instant::now())),
                    kind: Kind::Slow(Box::new(Slow {
                        capture,
                        last: None,
                        fresh: false,
                        asked: None,
                        pending_ms: 0,
                    })),
                },
            )?;
        }
        let mut current = lock(&state.live);
        let live = current
            .as_mut()
            .filter(|l| l.share.id == r.phone)
            .ok_or("The session ended.")?;
        let serial = live.serial;
        let Kind::Slow(view) = &mut live.kind else {
            return Err("The session ended.".into());
        };
        *lock(&live.used) = Instant::now();
        // The time since the last picture counts against today's allowance (a pause of more
        // than a few seconds does not).
        let since = view.asked.map(|a| a.elapsed());
        view.asked = Some(Instant::now());
        view.pending_ms += since.map_or(0, |d| d.as_millis().min(5_000) as u64);
        let secs = u32::try_from(view.pending_ms / 1000).unwrap_or(u32::MAX);
        view.pending_ms %= 1000;
        let left = {
            let mut stored = lock(&state.stored);
            if secs > 0 && !stored.slow.spend(now, secs) {
                0
            } else {
                stored.slow.left(now)
            }
        };
        if lock(&state.saved).elapsed() >= SAVE_EVERY {
            state.save();
        }
        if left == 0 {
            drop(current);
            end(
                app,
                serial,
                "today's 20 minutes of the slow view are used up",
                By::Phone,
            );
            return Err(SLOW_SPENT.into());
        }
        if since.is_some_and(|d| d < SLOW_INTERVAL) {
            return Ok(json!({ "same": true, "left": left }));
        }
        match view.capture.next(200) {
            Ok(Some(p)) => {
                view.last = Some(p);
                view.fresh = true;
            }
            Ok(None) => {}
            Err(CaptureError::Lost) => {
                let _ = view.capture.reopen();
            }
            Err(CaptureError::Secure) => {
                let _ = view.capture.reopen();
                return Ok(json!({ "secure": true, "left": left }));
            }
            Err(CaptureError::Other(e)) => return Err(e),
        }
        let Some(p) = view.last.as_ref().filter(|_| view.fresh) else {
            return Ok(json!({ "same": true, "left": left }));
        };
        let (w, h) = frames::fit(p.width, p.height, SLOW_MAX.0, SLOW_MAX.1);
        let bgr = frames::bgra_to_bgr(&p.bgra, p.width, p.height, p.width as usize * 4, w, h);
        let jpeg = jpeg::encode(&bgr, w, h, SLOW_QUALITY)?;
        view.fresh = false;
        Ok(json!({ "jpeg": b64url(&jpeg), "w": w, "h": h, "left": left }))
    }

    /// The slow view's input: `{inputs: [...]}`, each as the direct session's data channel
    /// carries it.
    fn slow_input(app: &AppHandle, r: &Request) -> Result<Value, String> {
        let list = r
            .params
            .get("inputs")
            .and_then(Value::as_array)
            .ok_or("The request needs \"inputs\".")?;
        if list.len() > MAX_INPUTS {
            return Err("Too much input at once.".into());
        }
        let inputs = list
            .iter()
            .map(|v| input::parse(&v.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let state = state(app).ok_or("Vyro is starting.")?;
        let on = {
            let current = lock(&state.live);
            let live = current
                .as_ref()
                .filter(|l| l.share.id == r.phone)
                .ok_or("The session ended.")?;
            let Kind::Slow(view) = &live.kind else {
                return Err("A direct session takes input on its own connection.".into());
            };
            *lock(&live.used) = Instant::now();
            view.capture.monitor.rect
        };
        for i in &inputs {
            inject::apply(i, on)?;
        }
        Ok(json!({ "done": true }))
    }
}

/// Settings → Remote and the banner.
#[tauri::command]
pub fn get_screen_status(state: tauri::State<'_, ScreenState>) -> ScreenStatus {
    state.status()
}

/// Stop, at the PC.
#[tauri::command]
pub fn stop_screen_share(app: AppHandle) -> ScreenStatus {
    end_all(&app, "stopped on the PC");
    state(&app).map(|s| s.status()).unwrap_or_default()
}

/// "Lock the PC when a session ends".
#[tauri::command]
pub fn set_screen_lock(app: AppHandle, on: bool) -> ScreenStatus {
    let Some(state) = state(&app) else {
        return ScreenStatus::default();
    };
    lock(&state.stored).lock_on_end = on;
    state.save();
    announce(&app);
    state.status()
}

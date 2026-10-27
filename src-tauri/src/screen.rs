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
/// What today's slow view used is saved this often.
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// The phone also stops a session to start the next at once (another monitor, or the slow view
/// when no direct path came): one of its sessions begun this soon after it stopped the last goes
/// on from it. After the phone's Stop, "Lock this PC when a phone's session ends" waits this
/// long (a PC locked meanwhile would show the phone only the secure screen), and the owner is
/// not told again that the screen is shared.
const NEXT_WITHIN: Duration = Duration::from_secs(10);
#[cfg(not(windows))]
const ONLY_WINDOWS: &str = "Only Vyro on Windows shares its screen.";

/// `screen.json`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Stored {
    lock_on_end: bool,
    slow: SlowAllowance,
    /// While the phone's microphone plays, the cable is Windows' default microphone (1.7).
    mic_switch: bool,
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            lock_on_end: false,
            slow: SlowAllowance::default(),
            mic_switch: true,
        }
    }
}

/// Who ended a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The owner, at the PC: no lock.
    Pc,
    /// The network, the idle rule, the phone gone: the lock at once.
    Phone,
    /// The phone's Stop: the lock waits [`NEXT_WITHIN`], and is left out if a session began
    /// meanwhile.
    Stopped,
    /// The same phone started again.
    Replaced,
}

pub struct ScreenState {
    path: Option<PathBuf>,
    stored: Mutex<Stored>,
    saved: Mutex<Instant>,
    live: Mutex<Option<Live>>,
    serial: AtomicU64,
    /// Sessions begun so far: a lock left waiting by the phone's Stop is not due once another
    /// has begun.
    begun: AtomicU64,
    /// The phone (its id) that last ended its own session (Stop, or started again), and when: its
    /// next one within [`NEXT_WITHIN`] goes on from it.
    moved_on: Mutex<Option<(String, Instant)>>,
    /// The phone's voice clips and microphone (1.7).
    pub(crate) voice: crate::voice::Voice,
}

struct Live {
    serial: u64,
    share: ScreenShare,
    started: Instant,
    stop: Arc<AtomicBool>,
    /// When the phone last showed it is there: input, its "here", a picture asked for.
    used: Arc<Mutex<Instant>>,
    /// The keys and buttons its input holds down, let go when it ends.
    held: Arc<Mutex<vyro_core::screen::input::Held>>,
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
    /// HD or SD, as the last picture went: one asked for the other is sent again, though the
    /// screen did not change.
    sent: Option<vyro_core::screen::producer::Quality>,
    asked: Option<Instant>,
    /// Milliseconds used and not yet counted against today's allowance.
    pending_ms: u64,
}

impl ScreenState {
    pub fn load(app: &AppHandle) -> Self {
        let dir = app.path().app_local_data_dir().ok();
        let path = dir.as_ref().map(|d| d.join(FILE));
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
            begun: AtomicU64::new(0),
            moved_on: Mutex::new(None),
            voice: crate::voice::Voice::new(dir),
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

    /// Whether the lock the phone's Stop left waiting is still due: the owner still wants it,
    /// and no session began since (`begun`: the count of sessions begun then).
    fn lock_due(&self, begun: u64) -> bool {
        lock(&self.stored).lock_on_end && self.begun.load(Ordering::SeqCst) == begun
    }

    /// Whether a session of phone `id` beginning now goes on from the one it ended itself just
    /// before (another monitor, the slow view): the owner was told of that one.
    fn goes_on(&self, id: &str) -> bool {
        lock(&self.moved_on)
            .take()
            .is_some_and(|(by, at)| by == id && at.elapsed() < NEXT_WITHIN)
    }

    pub fn status(&self) -> ScreenStatus {
        // One lock at a time: a guard made inside the struct expression would live to its end.
        let share = lock(&self.live).as_ref().map(|l| l.share.clone());
        let (lock_on_end, slow_left, mic_switch) = {
            let mut stored = lock(&self.stored);
            (
                stored.lock_on_end,
                stored.slow.left(unix_now()),
                stored.mic_switch,
            )
        };
        ScreenStatus {
            share,
            lock_on_end,
            slow_left,
            mic: self.voice.on(),
            mic_switch,
        }
    }
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, ScreenState>> {
    app.try_state::<ScreenState>()
}

/// Tells the window and the tray.
pub(crate) fn announce(app: &AppHandle) {
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
pub(crate) fn span(d: Duration) -> String {
    match d.as_secs() {
        s if s < 60 => format!("{s} s"),
        s => format!("{} min", (s + 30) / 60),
    }
}

/// Tells the phone of a direct session (over its data channel), if there is one.
pub(crate) fn tell(app: &AppHandle, note: &share::Note) {
    #[cfg(windows)]
    if let Some(state) = state(app) {
        if let Some(Kind::Direct(s)) = lock(&state.live).as_ref().map(|l| &l.kind) {
            s.tell(note);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, note);
}

/// "Make the cable Windows' default microphone while the phone's microphone is on".
pub(crate) fn mic_switch(app: &AppHandle) -> bool {
    state(app).is_some_and(|s| lock(&s.stored).mic_switch)
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
    let goes_on = state.goes_on(&live.share.id);
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
        state.begun.fetch_add(1, Ordering::SeqCst);
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
    // Once for the phone's use: not again for a session going on from its last.
    if !goes_on {
        if let Err(e) = app
            .notification()
            .builder()
            .title("Your screen is shared")
            .body(format!(
                "{phone} sees this screen and can use it. Stop it from Vyro's window or its tray \
                 icon."
            ))
            .show()
        {
            log::warn!("screen: notification failed: {e}");
        }
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
    if matches!(by, By::Stopped | By::Replaced) {
        *lock(&state.moved_on) = Some((live.share.id.clone(), Instant::now()));
    }
    live.stop.store(true, Ordering::SeqCst);
    #[cfg(windows)]
    vyro_core::screen::inject::let_go(&lock(&live.held).end());
    crate::voice::session_ended(app, why);
    #[cfg(windows)]
    if let Kind::Direct(s) = &live.kind {
        s.tell(&share::Note::End { why: why.into() });
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
    match by {
        By::Phone if lock(&state.stored).lock_on_end => lock_pc(),
        By::Stopped if lock(&state.stored).lock_on_end => {
            let (app, begun) = (app.clone(), state.begun.load(Ordering::SeqCst));
            let spawned = std::thread::Builder::new()
                .name("vyro-screen-lock".into())
                .spawn(move || {
                    std::thread::sleep(NEXT_WITHIN);
                    match app.try_state::<ScreenState>() {
                        Some(s) if s.lock_due(begun) => lock_pc(),
                        Some(_) => log::info!("screen: not locked after the phone's Stop"),
                        None => {}
                    }
                });
            if spawned.is_err() {
                lock_pc();
            }
        }
        _ => {}
    }
}

/// "Lock this PC when a phone's session ends".
fn lock_pc() {
    let lock_pc = vyro_core::actions::power::PowerRequest {
        action: vyro_core::actions::power::PowerAction::Lock,
        wait: 0,
        force: false,
    };
    if let Err(e) = vyro_core::actions::power::act(&lock_pc, "the end of a screen session") {
        log::warn!("screen: lock: {e}");
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
    use vyro_core::screen::share::{MonitorChoice, Note, SLOW_INTERVAL, SLOW_SPENT};
    use vyro_core::screen::{frames, inject, input, jpeg, producer};

    use super::*;

    pub fn handle(app: &AppHandle, r: &Request) -> Result<Value, String> {
        match r.method.as_str() {
            "screen.monitors" => monitors(),
            "screen.start" => start(app, r),
            "screen.stop" => {
                // Also before the next session (another monitor, the slow view): see NEXT_WITHIN.
                end_of(app, &r.phone, "stopped on the phone", By::Stopped);
                Ok(json!({ "done": true }))
            }
            "screen.slow" => slow(app, r),
            "screen.input" => slow_input(app, r),
            "screen.clip" => clip(app, r),
            "screen.audio" => Ok(json!({
                "cable": crate::voice::cable_name()?,
                "switchDefault": mic_switch(app),
            })),
            _ => Err(vyro_core::remote::relay::api::UNKNOWN.into()),
        }
    }

    /// HD or SD (1.7); HD when the phone does not say (a phone app before 1.7).
    fn quality_param(r: &Request) -> producer::Quality {
        r.params
            .get("quality")
            .cloned()
            .and_then(|q| serde_json::from_value(q).ok())
            .unwrap_or_default()
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
        let held = Arc::new(Mutex::new(input::Held::default()));
        let rect = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let sink: rtc::EventSink = {
            let (app, used, rect) = (app.clone(), Arc::clone(&used), Arc::clone(&rect));
            let (held, ended) = (Arc::clone(&held), Arc::clone(&stop));
            let phone = r.name.clone();
            Box::new(move |e| match e {
                SessionEvent::Connected => log::info!("screen: the phone is connected"),
                SessionEvent::Input(i) => {
                    *lock(&used) = Instant::now();
                    if let Some(on) = *lock(&rect) {
                        let mut held = lock(&held);
                        if held.note(&i) {
                            if let Err(e) = inject::apply(&i, on) {
                                log::debug!("screen: {e}");
                            }
                        }
                    }
                }
                SessionEvent::Clip(wav, reply) => {
                    *lock(&used) = Instant::now();
                    let (app, phone) = (app.clone(), phone.clone());
                    // Encoding takes a moment; the session's own thread goes on.
                    let spawned = std::thread::Builder::new()
                        .name("vyro-phone-clip".into())
                        .spawn(move || {
                            let saved = crate::voice::save_clip(&app, &phone, &wav);
                            if let Err(e) = &saved {
                                log::warn!("phone clip: {e}");
                            }
                            reply.send(saved.as_deref().map_err(String::as_str));
                        });
                    if let Err(e) = spawned {
                        log::warn!("phone clip: {e}");
                    }
                }
                SessionEvent::Mic(frame) => {
                    *lock(&used) = Instant::now();
                    crate::voice::mic_frame(&app, &phone, &ended, &frame);
                }
                // One the connection still delivers after the end would turn off the next
                // session's.
                SessionEvent::MicEnd if !ended.load(Ordering::SeqCst) => {
                    crate::voice::mic_off(&app, "turned off on the phone");
                }
                SessionEvent::MicEnd => {}
                SessionEvent::Closed(why) => end(&app, serial, &why, By::Phone),
            })
        };
        let (session, answer) = rtc::start(&offer, &opts, sink)?;
        session.set_quality(quality_param(r));
        let session = Arc::new(session);
        begin(
            app,
            Live {
                serial,
                share: share(r, false),
                started: Instant::now(),
                stop: Arc::clone(&stop),
                used,
                held,
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
                let reports = std::sync::atomic::AtomicU32::new(0);
                let status = |s: producer::Status| match s {
                    producer::Status::Sending {
                        width,
                        height,
                        rect: on,
                    } => {
                        *lock(&rect) = Some(on);
                        session.tell(&Note::Sending {
                            w: width,
                            h: height,
                        });
                    }
                    producer::Status::Secure => session.tell(&Note::Secure),
                    producer::Status::Report(r) => {
                        session.tell(&Note::report(&r));
                        // The first few, then one every half minute.
                        let n = reports.fetch_add(1, Ordering::SeqCst);
                        if n < 3 || n.is_multiple_of(15) {
                            log::info!("screen: {r}");
                        }
                    }
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
                    held: Arc::default(),
                    kind: Kind::Slow(Box::new(Slow {
                        capture,
                        last: None,
                        fresh: false,
                        sent: None,
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
        // Asked again too soon (the phone paces itself; the relay's delay varies): nothing new.
        if since.is_some_and(|d| d < SLOW_INTERVAL / 2) {
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
        let asked = quality_param(r);
        let Some(p) = view
            .last
            .as_ref()
            .filter(|_| view.fresh || view.sent != Some(asked))
        else {
            return Ok(json!({ "same": true, "left": left }));
        };
        let ((max_w, max_h), quality) = asked.slow();
        let (w, h) = frames::fit(p.width, p.height, max_w, max_h);
        let bgr = frames::bgra_to_bgr(&p.bgra, p.width, p.height, p.width as usize * 4, w, h);
        let jpeg = jpeg::encode(&bgr, w, h, quality)?;
        view.fresh = false;
        view.sent = Some(asked);
        Ok(json!({ "jpeg": b64url(&jpeg), "w": w, "h": h, "left": left }))
    }

    /// A voice clip through the relay (1.7), a piece a request: `{id, i, n, d}` with `d` the
    /// piece in base64url. `{got: i}` until the last, then `{saved: file name}` once it is on
    /// the clipboard. Only during this phone's session.
    fn clip(app: &AppHandle, r: &Request) -> Result<Value, String> {
        let text = |k: &str| r.params.get(k).and_then(Value::as_str);
        let number = |k: &str| {
            r.params
                .get(k)
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
        };
        let id = text("id").ok_or("The request needs \"id\".")?;
        let (i, n) = number("i")
            .zip(number("n"))
            .ok_or("The request needs \"i\" and \"n\".")?;
        let piece = text("d")
            .and_then(vyro_core::remote::relay::proto::unb64url)
            .ok_or("The request needs \"d\", in base64url.")?;
        {
            let state = state(app).ok_or("Vyro is starting.")?;
            let current = lock(&state.live);
            let live = current
                .as_ref()
                .filter(|l| l.share.id == r.phone)
                .ok_or("The session ended.")?;
            *lock(&live.used) = Instant::now();
        }
        match crate::voice::clip_piece(app, (&r.phone, &r.name), id, (i, n), piece)? {
            Some(name) => Ok(json!({ "saved": name })),
            None => Ok(json!({ "got": i })),
        }
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
        // One this PC does not know (a newer phone app's) is left out, not the rest with it, as
        // on a direct session.
        let inputs: Vec<input::Input> = list
            .iter()
            .filter_map(|v| {
                input::parse(&v.to_string())
                    .inspect_err(|e| log::debug!("screen: input refused: {e}"))
                    .ok()
            })
            .collect();
        let state = state(app).ok_or("Vyro is starting.")?;
        let (on, held) = {
            let current = lock(&state.live);
            let live = current
                .as_ref()
                .filter(|l| l.share.id == r.phone)
                .ok_or("The session ended.")?;
            let Kind::Slow(view) = &live.kind else {
                return Err("A direct session takes input on its own connection.".into());
            };
            *lock(&live.used) = Instant::now();
            (view.capture.monitor.rect, Arc::clone(&live.held))
        };
        let mut held = lock(&held);
        for i in &inputs {
            if !held.note(i) {
                return Err("The session ended.".into());
            }
            // One Windows refused goes, not the rest with it: a key let go after it would stay
            // held down.
            if let Err(e) = inject::apply(i, on) {
                log::debug!("screen: {e}");
            }
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

/// "Make the cable Windows' default microphone while the phone's microphone is on" (1.7).
#[tauri::command]
pub fn set_screen_mic(app: AppHandle, on: bool) -> ScreenStatus {
    let Some(state) = state(&app) else {
        return ScreenStatus::default();
    };
    lock(&state.stored).mic_switch = on;
    state.save();
    announce(&app);
    state.status()
}

/// The microphone a virtual cable offers, if one is installed (Settings → Remote).
#[tauri::command]
pub async fn find_phone_cable() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(crate::voice::cable_name)
        .await
        .map_err(|e| e.to_string())?
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> ScreenState {
        ScreenState {
            path: None,
            stored: Mutex::new(Stored::default()),
            saved: Mutex::new(Instant::now()),
            live: Mutex::new(None),
            serial: AtomicU64::new(0),
            begun: AtomicU64::new(0),
            moved_on: Mutex::new(None),
            voice: crate::voice::Voice::new(None),
        }
    }

    /// The status takes each lock once: the window asks for it at start, on the main thread.
    #[test]
    fn the_status_answers() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(state().status());
        });
        let status = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the status never answered (a lock taken twice)");
        assert_eq!(
            status,
            ScreenStatus {
                share: None,
                lock_on_end: false,
                slow_left: share::SLOW_DAILY_SECS,
                mic: false,
                mic_switch: true,
            }
        );
    }

    /// The phone stops a session to start the next (another monitor, the slow view): the lock
    /// its Stop left waiting is not due once that one has begun.
    #[test]
    fn a_session_begun_after_the_phone_stopped_one_leaves_the_pc_unlocked() {
        let s = state();
        lock(&s.stored).lock_on_end = true;
        let begun = s.begun.load(Ordering::SeqCst);
        assert!(s.lock_due(begun));
        s.begun.fetch_add(1, Ordering::SeqCst);
        assert!(!s.lock_due(begun));
        let begun = s.begun.load(Ordering::SeqCst);
        lock(&s.stored).lock_on_end = false;
        assert!(!s.lock_due(begun));
    }

    /// Another monitor, or the slow view after no direct path: the owner was told of the phone's
    /// last session, and is not told again.
    #[test]
    fn a_session_going_on_from_the_last_is_told_once() {
        let s = state();
        assert!(!s.goes_on("phone"), "the first is told");
        *lock(&s.moved_on) = Some(("phone".into(), Instant::now()));
        assert!(s.goes_on("phone"));
        assert!(!s.goes_on("phone"), "once");
        *lock(&s.moved_on) = Some(("phone".into(), Instant::now()));
        assert!(!s.goes_on("another phone"));
        assert!(!s.goes_on("phone"), "another phone's session came between");
        let long_ago = Instant::now().checked_sub(NEXT_WITHIN + Duration::from_secs(1));
        if let Some(at) = long_ago {
            *lock(&s.moved_on) = Some(("phone".into(), at));
            assert!(!s.goes_on("phone"), "a while later");
        }
    }

    #[test]
    fn a_session_lasts_in_words() {
        assert_eq!(span(Duration::from_secs(12)), "12 s");
        assert_eq!(span(Duration::from_secs(4 * 60 + 20)), "4 min");
        assert_eq!(span(Duration::from_secs(4 * 60 + 40)), "5 min");
    }
}

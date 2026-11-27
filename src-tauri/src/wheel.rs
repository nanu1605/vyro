//! The phone wheel in the app (2.6, `clokrix_core::wheel`): a phone the owner allowed to steer
//! games starts a direct session (its WebRTC offer through the relay, as a screen session's),
//! whose only input is a game controller's. Its packets become a virtual Xbox controller's
//! reports (ViGEmBus); the game's rumble goes back to the phone. Nothing else of the phone's
//! input is used: a wheel session never types or clicks.
//!
//! One phone at a time; while it lasts the PC shows a banner with Stop. ViGEmBus is set up only
//! from the PC (Settings → Remote, "Set up the virtual controller"), with one administrator
//! prompt; a phone never starts the install.

// Elsewhere only the commands and the ends are built (and never reached).
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clokrix_core::remote::relay::Request;
use clokrix_core::wheel::WheelOptions;
use clokrix_core::{files, lock, unix_now};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

/// Emitted with the [`WheelStatus`] when a session starts or ends, or the settings change.
pub const EVENT: &str = "wheel-status";
const FILE: &str = "wheel.json";

/// The virtual controller's driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Driver {
    Installed,
    Missing,
    /// Its installer runs now.
    Installing,
    /// Not on this system (not Windows).
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

/// A phone steering now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelSession {
    pub phone: String,
    pub id: String,
    /// Unix seconds.
    pub since: i64,
}

/// What the PC shows of the wheel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WheelStatus {
    pub driver: Driver,
    pub session: Option<WheelSession>,
    pub options: WheelOptions,
}

struct Live {
    serial: u64,
    session: WheelSession,
    stop: Arc<AtomicBool>,
    #[cfg(windows)]
    rtc: Arc<clokrix_core::screen::rtc::Session>,
}

pub struct WheelState {
    path: Option<PathBuf>,
    options: Mutex<WheelOptions>,
    live: Mutex<Option<Live>>,
    serial: AtomicU64,
    installing: AtomicBool,
}

impl WheelState {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_config_dir().ok().map(|d| d.join(FILE));
        let options = path
            .as_deref()
            .and_then(|p| files::read_json::<WheelOptions>(p, |_| Ok(())).ok())
            .flatten()
            .unwrap_or_default()
            .clamped();
        Self {
            path,
            options: Mutex::new(options),
            live: Mutex::new(None),
            serial: AtomicU64::new(0),
            installing: AtomicBool::new(false),
        }
    }

    fn status(&self) -> WheelStatus {
        WheelStatus {
            driver: driver(self),
            session: lock(&self.live).as_ref().map(|l| l.session.clone()),
            options: *lock(&self.options),
        }
    }
}

fn driver(state: &WheelState) -> Driver {
    #[cfg(windows)]
    {
        if state.installing.load(Ordering::SeqCst) {
            Driver::Installing
        } else if clokrix_core::wheel::vigem::installed() {
            Driver::Installed
        } else {
            Driver::Missing
        }
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        Driver::Unsupported
    }
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, WheelState>> {
    app.try_state::<WheelState>()
}

/// Tells the window.
fn announce(app: &AppHandle) {
    if let Some(s) = state(app) {
        if let Err(e) = app.emit(EVENT, s.status()) {
            log::debug!("emit {EVENT}: {e}");
        }
    }
}

/// A phone's request for the wheel: the relay connection has checked its permission.
pub fn handle(app: &AppHandle, r: &Request) -> Result<Value, String> {
    #[cfg(windows)]
    return win::handle(app, r);
    #[cfg(not(windows))]
    {
        let _ = (app, r);
        Err("Only Clokrix on Windows plays as a game controller.".into())
    }
}

/// Ends the session numbered `serial`, if it is the one running.
fn end(app: &AppHandle, serial: u64, why: &str) {
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
    {
        live.rtc
            .tell(&clokrix_core::screen::share::Note::End { why: why.into() });
        live.rtc.end();
    }
    log::info!("wheel: ended: {why}");
    crate::remote_api::log_action(app, &live.session.phone, format!("Stopped steering: {why}"));
    announce(app);
}

/// Ends the phone `id`'s session (signed out, no longer allowed).
pub fn end_phone(app: &AppHandle, id: &str, why: &str) {
    let serial = state(app).and_then(|s| {
        lock(&s.live)
            .as_ref()
            .filter(|l| l.session.id == id)
            .map(|l| l.serial)
    });
    if let Some(serial) = serial {
        end(app, serial, why);
    }
}

/// Ends a session whose phone is no longer paired.
pub fn end_unpaired(app: &AppHandle, paired: &[String], why: &str) {
    let serial = state(app).and_then(|s| {
        lock(&s.live)
            .as_ref()
            .filter(|l| !paired.contains(&l.session.id))
            .map(|l| l.serial)
    });
    if let Some(serial) = serial {
        end(app, serial, why);
    }
}

/// Ends any session (Pro ended, Clokrix closing, the relay changed).
pub fn end_all(app: &AppHandle, why: &str) {
    let serial = state(app).and_then(|s| lock(&s.live).as_ref().map(|l| l.serial));
    if let Some(serial) = serial {
        end(app, serial, why);
    }
}

#[cfg(windows)]
mod win {
    use std::net::IpAddr;

    use clokrix_core::screen::input::Input;
    use clokrix_core::screen::rtc::{self, SessionEvent, WHEEL_ECHO, WHEEL_RUMBLE};
    use clokrix_core::screen::share::Note;
    use clokrix_core::wheel::packet;
    use clokrix_core::wheel::vigem::{self, Pad};
    use clokrix_core::wheel::Wheel;
    use serde_json::json;
    use tauri_plugin_notification::NotificationExt;

    use super::*;

    /// The longest offer taken (a browser's is a few kB).
    const MAX_OFFER: usize = 32 * 1024;
    /// The phone hears its newest packet back this often, to show the delay.
    const ECHO_EVERY: Duration = Duration::from_millis(100);
    /// The controller is looked at this often while nothing comes (the dead man's easing).
    const LOOK: Duration = Duration::from_millis(8);
    /// A session nobody steers in (no packet, no "here") ends after this long.
    const IDLE: Duration = Duration::from_secs(10 * 60);

    pub fn handle(app: &AppHandle, r: &Request) -> Result<Value, String> {
        match r.method.as_str() {
            "wheel.start" => start(app, r),
            "wheel.stop" => {
                end_phone(app, &r.phone, "stopped on the phone");
                Ok(json!({ "done": true }))
            }
            _ => Err(clokrix_core::remote::relay::api::UNKNOWN.into()),
        }
    }

    fn start(app: &AppHandle, r: &Request) -> Result<Value, String> {
        let state = state(app).ok_or("Clokrix is starting.")?;
        if !crate::edition::full(app) {
            return Err("The phone wheel is part of Clokrix Pro.".into());
        }
        if !vigem::installed() {
            return Err(
                "Set up the wheel on the PC first: Clokrix → Settings → Remote → Set up the virtual \
                 controller."
                    .into(),
            );
        }
        let offer = r
            .params
            .get("offer")
            .and_then(Value::as_str)
            .ok_or("The request needs \"offer\".")?
            .to_owned();
        if offer.len() > MAX_OFFER {
            return Err("The phone's offer is too long.".into());
        }
        // One phone at a time; the same phone starting again replaces its session.
        let current = lock(&state.live)
            .as_ref()
            .map(|l| (l.serial, l.session.id.clone(), l.session.phone.clone()));
        match current {
            Some((_, id, name)) if id != r.phone => {
                return Err(format!("{name} is steering already."));
            }
            Some((serial, ..)) => end(app, serial, "the phone started again"),
            None => {}
        }
        let pad = Arc::new(Pad::plug()?);
        let hosts: Vec<IpAddr> = crate::remote_api::wake_sources()
            .into_iter()
            .map(|(ip, _)| IpAddr::V4(ip))
            .collect();
        let opts = rtc::Options {
            hosts,
            ..rtc::Options::default()
        };
        let serial = state.serial.fetch_add(1, Ordering::SeqCst) + 1;
        let stop = Arc::new(AtomicBool::new(false));
        let wheel = Arc::new(Mutex::new(Wheel::new(*lock(&state.options))));
        let used = Arc::new(Mutex::new(Instant::now()));
        // Woken on each packet, so a report goes to the game at once.
        let (wake, woken) = std::sync::mpsc::sync_channel::<()>(1);
        let sink: rtc::EventSink = {
            let (app, wheel, used) = (app.clone(), Arc::clone(&wheel), Arc::clone(&used));
            Box::new(move |e| match e {
                SessionEvent::Wheel(bytes) => match packet::parse(&bytes) {
                    Ok(p) => {
                        *lock(&used) = Instant::now();
                        if lock(&wheel).take(&p, Instant::now()) {
                            let _ = wake.try_send(());
                        }
                    }
                    Err(e) => log::debug!("wheel: refused: {e}"),
                },
                SessionEvent::WheelEnd => {
                    lock(&wheel).let_go(Instant::now());
                    let _ = wake.try_send(());
                }
                // Only that the phone is there: a wheel session never types or clicks.
                SessionEvent::Input(Input::Here) => *lock(&used) = Instant::now(),
                SessionEvent::Closed(why) => end(&app, serial, &why),
                _ => {}
            })
        };
        let (session, answer) = rtc::start(&offer, &opts, sink)?;
        let session = Arc::new(session);
        session.tell(&Note::Pad {
            ok: true,
            why: None,
        });
        *lock(&state.live) = Some(Live {
            serial,
            session: WheelSession {
                phone: r.name.clone(),
                id: r.phone.clone(),
                since: unix_now(),
            },
            stop: Arc::clone(&stop),
            rtc: Arc::clone(&session),
        });
        log::info!("wheel: a phone steers");
        crate::remote_api::log_action(app, &r.name, "Started steering games".into());
        if let Err(e) = app
            .notification()
            .builder()
            .title("A phone is steering games")
            .body(format!(
                "{} plays as a game controller on this PC. Stop it from Clokrix's window.",
                r.name
            ))
            .show()
        {
            log::warn!("wheel: notification failed: {e}");
        }
        announce(app);

        // The controller: a report on each packet, and as the dead man eases it to centre.
        {
            let (owner, pad, session, stop) = (
                app.clone(),
                Arc::clone(&pad),
                Arc::clone(&session),
                Arc::clone(&stop),
            );
            let spawned = std::thread::Builder::new()
                .name("clokrix-wheel".into())
                .spawn(move || {
                    let mut last = None;
                    let mut echoed = Instant::now();
                    while !stop.load(Ordering::SeqCst) && !session.closed() {
                        let _ = woken.recv_timeout(LOOK);
                        let now = Instant::now();
                        let (report, newest) = {
                            let mut w = lock(&wheel);
                            (w.report(now), w.newest())
                        };
                        if last != Some(report) {
                            if let Err(e) = pad.submit(&report) {
                                log::warn!("wheel: {e}");
                                session.tell(&Note::Pad {
                                    ok: false,
                                    why: Some(e),
                                });
                                end(&owner, serial, "the virtual controller stopped");
                                break;
                            }
                            last = Some(report);
                        }
                        if echoed.elapsed() >= ECHO_EVERY {
                            echoed = now;
                            if let Some((seq, t)) = newest {
                                session.wheel_send(WHEEL_ECHO, packet::echo(seq, t).to_vec());
                            }
                        }
                        if lock(&used).elapsed() > IDLE {
                            end(&owner, serial, "nobody steered for 10 minutes");
                            break;
                        }
                    }
                    // Let go of everything before the controller goes.
                    let _ = pad.submit(&clokrix_core::wheel::Report::default());
                    pad.wake();
                });
            if let Err(e) = spawned {
                end(app, serial, "it could not start");
                return Err(format!("cannot start the wheel: {e}"));
            }
        }
        // The game's rumble, to the phone.
        {
            let (pad, session, stop) = (pad, session, stop);
            let _ = std::thread::Builder::new()
                .name("clokrix-wheel-rumble".into())
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        let Some((large, small)) = pad.rumble() else {
                            break;
                        };
                        session.wheel_send(WHEEL_RUMBLE, packet::rumble(large, small).to_vec());
                    }
                    // The last owner of the controller unplugs it here, or in the thread above.
                });
        }
        Ok(json!({ "answer": answer }))
    }

    /// Runs ViGEmBus's own installer, elevated (one administrator prompt on this PC), and waits
    /// for it. Ok(true): installed; Ok(false): a restart is needed first.
    pub fn install() -> Result<bool, String> {
        use clokrix_core::elevated::{self, NotRun};

        let setup = std::env::current_exe()
            .ok()
            .and_then(|e| {
                e.parent()
                    .map(|d| d.join("vigembus").join("ViGEmBus_setup.exe"))
            })
            .filter(|p| p.is_file())
            .ok_or("ViGEmBus's installer is missing: install Clokrix again.")?;
        let code = elevated::run(&setup, "").map_err(|e| match e {
            NotRun::Declined => {
                "The virtual controller was not set up: the administrator prompt was declined."
                    .to_owned()
            }
            NotRun::Failed(e) => format!("ViGEmBus's installer did not start: {e}"),
        })?;
        match code {
            0 => {}
            3010 | 1641 => return Ok(false),
            1602 | 1223 => return Err("ViGEmBus's installer was cancelled.".into()),
            c => return Err(format!("ViGEmBus's installer ended with code {c}.")),
        }
        // The bus comes up a moment after its installer ends.
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            if vigem::installed() {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        Ok(false)
    }
}

#[tauri::command]
pub fn get_wheel_status(state: tauri::State<'_, WheelState>) -> WheelStatus {
    state.status()
}

/// Stop, at the PC.
#[tauri::command]
pub fn stop_wheel(app: AppHandle) -> Option<WheelStatus> {
    end_all(&app, "stopped on the PC");
    state(&app).map(|s| s.status())
}

#[tauri::command]
pub fn set_wheel_options(
    app: AppHandle,
    state: tauri::State<'_, WheelState>,
    options: WheelOptions,
) -> WheelStatus {
    let options = options.clamped();
    *lock(&state.options) = options;
    if let Some(p) = &state.path {
        if let Err(e) = files::write_json(p, &options) {
            log::warn!("wheel: cannot write {}: {e}", p.display());
        }
    }
    announce(&app);
    state.status()
}

/// "Set up the virtual controller": ViGEmBus's installer, after an administrator prompt on this
/// PC. Says whether a restart is needed first.
#[tauri::command]
pub async fn install_wheel_driver(app: AppHandle) -> Result<String, String> {
    let s = state(&app).ok_or("Clokrix is starting.")?;
    if matches!(driver(&s), Driver::Installed) {
        return Ok("The virtual controller is set up.".into());
    }
    if s.installing.swap(true, Ordering::SeqCst) {
        return Err("Its installer is running already.".into());
    }
    announce(&app);
    #[cfg(windows)]
    let done = tauri::async_runtime::spawn_blocking(win::install)
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    #[cfg(not(windows))]
    let done: Result<bool, String> =
        Err("Only Clokrix on Windows plays as a game controller.".into());
    s.installing.store(false, Ordering::SeqCst);
    announce(&app);
    match done {
        Ok(true) => {
            log::info!("wheel: ViGEmBus installed");
            Ok("The virtual controller is set up: a phone allowed to steer can start now.".into())
        }
        Ok(false) => {
            Ok("Almost: restart this PC to finish setting up the virtual controller.".into())
        }
        Err(e) => Err(e),
    }
}

//! The phone's voice on this PC (1.7), during a screen session. A clip the phone recorded is
//! kept in "Phone clips" (AAC, or WAV where Windows has no AAC encoder) and put on the clipboard
//! as Explorer's Copy puts a file, so Paste in a chat attaches it. The phone's live microphone
//! plays into a virtual cable (VB-CABLE and the like: Windows lets only a signed driver add a
//! microphone, and Vyro ships none); while it plays, the cable's recording side is Windows'
//! default microphone if the owner allows it, and the defaults it replaced are put back after,
//! even after a crash (`mic-restore.json`). What needs no app is in `vyro_core::screen::voice`.

// Elsewhere only the pure parts are reached.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use vyro_core::screen::share::Note;
use vyro_core::screen::voice::{self, Pieces};
use vyro_core::{files, lock};

/// The clips' folder, in the app's local data folder.
const CLIPS: &str = "Phone clips";
/// The default microphones the cable replaced.
const RESTORE: &str = "mic-restore.json";
/// Clips older than this are deleted.
const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// The microphone stops when no frame came for this long and the phone did not turn it off (it
/// went to the background, or the link is gone). A stall shorter than this plays silence and
/// leaves the default microphone alone, so a voice note being recorded keeps the cable.
const QUIET: Duration = Duration::from_secs(10);
/// After the cable failed to start, it is not tried again for this long.
const RETRY: Duration = Duration::from_secs(10);
const NO_CABLE: &str = "No virtual cable is installed on the PC. Install VB-CABLE (free, \
                        vb-audio.com), then try again.";

/// `mic-restore.json`: the default microphones before the cable took over.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Defaults {
    console: Option<String>,
    calls: Option<String>,
    /// The cable's recording side, made the default.
    cable: String,
}

/// What to put back: a role whose default is still the cable gets its earlier device; a role
/// the owner changed since is left alone.
fn put_back(
    was: &Defaults,
    console: Option<&str>,
    calls: Option<&str>,
) -> (Option<String>, Option<String>) {
    let back = |now: Option<&str>, before: &Option<String>| {
        (now == Some(was.cable.as_str()))
            .then(|| before.clone())
            .flatten()
    };
    (back(console, &was.console), back(calls, &was.calls))
}

/// Whether a file in the clips' folder is a clip Vyro made (only those are deleted).
fn is_clip(name: &str) -> bool {
    name.starts_with("Clip ") && (name.ends_with(".m4a") || name.ends_with(".wav"))
}

/// A clip's path that no file has yet ("Clip … (2).m4a" after the first).
fn free_path(dir: &Path, t: [u16; 6], ext: &str) -> PathBuf {
    let first = voice::clip_name(t, ext);
    let stem = first.trim_end_matches(&format!(".{ext}")).to_owned();
    let taken = |p: &Path| p.exists() || p.with_extension("wav").exists();
    let mut path = dir.join(&first);
    let mut n = 2;
    while taken(&path) {
        path = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Deletes clips older than a week.
fn prune(dir: &Path) {
    let Ok(list) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in list.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > KEEP);
        if old && entry.file_name().to_str().is_some_and(is_clip) {
            if let Err(e) = std::fs::remove_file(entry.path()) {
                log::debug!("phone clips: {e}");
            }
        }
    }
}

/// The voice part of a screen session's state.
pub struct Voice {
    /// The app's local data folder.
    dir: Option<PathBuf>,
    /// A clip coming through the relay, and the phone (id) sending it.
    pieces: Mutex<Option<(String, Pieces)>>,
    mic: Mutex<Mic>,
}

#[derive(Default)]
enum Mic {
    #[default]
    Off,
    /// Starting on its own thread, for the phone named; frames meanwhile are dropped. `off`: it
    /// was turned off meanwhile, and stops once started, unless that phone turns it on again
    /// first (one start at a time: two would each switch, and put back, the default microphone).
    Starting { phone: String, off: bool },
    /// It could not start; not tried again before [`RETRY`].
    Failed(Instant),
    #[cfg(windows)]
    On(Box<On>),
}

impl Mic {
    /// A frame from `phone` while the microphone does not play: whether it starts it.
    fn frame(&mut self, phone: &str) -> bool {
        match self {
            Mic::Starting { phone: p, off } => {
                if p.as_str() == phone {
                    *off = false;
                }
                false
            }
            Mic::Failed(at) if at.elapsed() < RETRY => false,
            #[cfg(windows)]
            Mic::On(_) => false,
            Mic::Off | Mic::Failed(_) => {
                *self = Mic::Starting {
                    phone: phone.to_owned(),
                    off: false,
                };
                true
            }
        }
    }

    /// Turned off: what played, to stop. One starting stops once started.
    fn turn_off(&mut self) -> Mic {
        if let Mic::Starting { off, .. } = self {
            *off = true;
            return Mic::Off;
        }
        std::mem::take(self)
    }

    /// A start finished: whether it plays now (else it was turned off as it started).
    fn started(&self) -> bool {
        matches!(self, Mic::Starting { off: false, .. })
    }
}

#[cfg(windows)]
struct On {
    /// The phone's name.
    phone: String,
    player: voice::win::CablePlayer,
    last: Instant,
    /// The defaults were switched to the cable, to be put back.
    switched: bool,
}

impl Voice {
    pub fn new(dir: Option<PathBuf>) -> Self {
        // Only a crash leaves the file: no microphone plays yet.
        #[cfg(windows)]
        if let Some(path) = dir.as_ref().map(|d| d.join(RESTORE)).filter(|p| p.exists()) {
            let _ = std::thread::Builder::new()
                .name("vyro-mic-restore".into())
                .spawn(move || {
                    // At sign-in Windows' sound may not answer yet: up to a minute for it.
                    for _ in 0..60 {
                        let read = voice::win::default_microphones();
                        if read.is_ok_and(|(console, calls)| console.is_some() || calls.is_some()) {
                            break;
                        }
                        std::thread::sleep(Duration::from_secs(1));
                    }
                    restore(&path);
                });
        }
        Self {
            dir,
            pieces: Mutex::new(None),
            mic: Mutex::new(Mic::Off),
        }
    }

    /// The phone's microphone plays now.
    pub fn on(&self) -> bool {
        #[cfg(windows)]
        return matches!(*lock(&self.mic), Mic::On(_));
        #[cfg(not(windows))]
        false
    }

    fn restore_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(RESTORE))
    }
}

fn state(app: &AppHandle) -> Option<tauri::State<'_, crate::screen::ScreenState>> {
    use tauri::Manager;
    app.try_state::<crate::screen::ScreenState>()
}

/// A voice clip (a WAV from the phone named `phone`): kept, and put on the clipboard. Its file
/// name.
#[cfg(windows)]
pub fn save_clip(app: &AppHandle, phone: &str, wav: &[u8]) -> Result<String, String> {
    use voice::win;
    let samples = voice::samples(voice::read_wav(wav)?);
    let state = state(app).ok_or("Vyro is starting.")?;
    let dir = state
        .voice
        .dir
        .as_ref()
        .ok_or("Vyro has no folder for clips.")?
        .join(CLIPS);
    std::fs::create_dir_all(&dir).map_err(|e| format!("the clips' folder: {e}"))?;
    prune(&dir);
    let path = free_path(&dir, win::local_now(), "m4a");
    let path = match win::encode_m4a(&path, &samples) {
        Ok(()) => path,
        Err(e) => {
            // Windows "N" editions have no AAC encoder: the phone's WAV as it came.
            log::warn!("phone clip: {e}; kept as WAV");
            let _ = std::fs::remove_file(&path);
            let path = path.with_extension("wav");
            std::fs::write(&path, wav).map_err(|e| format!("the clip: {e}"))?;
            path
        }
    };
    win::files_to_clipboard(std::slice::from_ref(&path))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned();
    let length = crate::screen::span(Duration::from_millis(
        samples.len() as u64 * 1000 / u64::from(voice::RATE),
    ));
    log::info!("phone clip: {length}, on the clipboard");
    crate::remote_api::log_action(
        app,
        phone,
        format!("Put a voice clip ({length}) on the clipboard"),
    );
    if let Err(e) = app
        .notification()
        .builder()
        .title("A voice clip is on the clipboard")
        .body(format!("From {phone}. Paste it into a chat."))
        .show()
    {
        log::debug!("phone clip: notification failed: {e}");
    }
    Ok(name)
}

/// Piece `i` of `n` of clip `id` through the relay, from phone `phone_id` (named `phone`): the
/// clip's file name once the last piece came.
#[cfg(windows)]
pub fn clip_piece(
    app: &AppHandle,
    (phone_id, phone): (&str, &str),
    id: &str,
    (i, n): (u32, u32),
    piece: Vec<u8>,
) -> Result<Option<String>, String> {
    let whole = {
        let state = state(app).ok_or("Vyro is starting.")?;
        let mut pieces = lock(&state.voice.pieces);
        let same = pieces
            .as_ref()
            .is_some_and(|(p, clip)| p == phone_id && clip.id == id);
        if !same {
            // A new clip (or another phone's) replaces one that never finished.
            *pieces = Some((phone_id.to_owned(), Pieces::new(id, n)?));
        }
        let Some((_, clip)) = pieces.as_mut() else {
            return Err("The clip was lost.".into());
        };
        match clip.add(i, n, piece) {
            Ok(None) => return Ok(None),
            Ok(Some(whole)) => {
                *pieces = None;
                whole
            }
            Err(e) => {
                *pieces = None;
                return Err(e);
            }
        }
    };
    save_clip(app, phone, &whole).map(Some)
}

/// What `screen.audio` and Settings show: the microphone the cable offers, if one is installed.
pub fn cable_name() -> Result<Option<String>, String> {
    #[cfg(windows)]
    return Ok(voice::win::cable()?.map(|c| c.capture.name));
    #[cfg(not(windows))]
    Ok(None)
}

/// A frame of the phone's microphone, from the phone named `phone`, in the session `ended` marks
/// as ended.
#[cfg(windows)]
pub fn mic_frame(app: &AppHandle, phone: &str, ended: &AtomicBool, frame: &[u8]) {
    let (seq, samples) = match voice::read_frame(frame) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("phone microphone: {e}");
            return;
        }
    };
    let Some(state) = state(app) else {
        return;
    };
    let mut mic = lock(&state.voice.mic);
    // A frame the connection still delivered after the session ended starts nothing: the end
    // marks the session ended before it takes this lock to turn the microphone off.
    if ended.load(Ordering::SeqCst) {
        return;
    }
    if let Mic::On(on) = &mut *mic {
        on.last = Instant::now();
        lock(&on.player.frames).push(seq, samples);
        return;
    }
    if !mic.frame(phone) {
        return;
    }
    drop(mic);
    let (app, phone) = (app.clone(), phone.to_owned());
    let spawned = std::thread::Builder::new()
        .name("vyro-mic-start".into())
        .spawn(move || start(&app, phone));
    if let Err(e) = spawned {
        log::warn!("phone microphone: {e}");
        *lock(&state.voice.mic) = Mic::Failed(Instant::now());
    }
}

/// Tells the phone (a direct session) about its microphone.
#[cfg(windows)]
fn tell(app: &AppHandle, on: bool, into: Option<String>, default: bool, why: Option<String>) {
    crate::screen::tell(
        app,
        &Note::Mic {
            on,
            into,
            default,
            why,
        },
    );
}

#[cfg(windows)]
fn start(app: &AppHandle, phone: String) {
    use voice::win;
    let Some(state) = state(app) else {
        return;
    };
    let started = win::cable()
        .and_then(|c| c.ok_or_else(|| NO_CABLE.to_owned()))
        .and_then(|cable| Ok((win::CablePlayer::start(&cable.render.id)?, cable)));
    let (player, cable) = match started {
        Ok(s) => s,
        Err(why) => {
            log::warn!("phone microphone: {why}");
            *lock(&state.voice.mic) = Mic::Failed(Instant::now());
            tell(app, false, None, false, Some(why));
            return;
        }
    };
    let switched = crate::screen::mic_switch(app)
        && match make_default(state.voice.restore_path().as_deref(), &cable.capture.id) {
            Ok(switched) => switched,
            Err(e) => {
                log::warn!("phone microphone: {e}");
                false
            }
        };
    let default = win::default_microphones()
        .ok()
        .and_then(|(console, _)| console)
        .is_some_and(|id| id == cable.capture.id);
    let on = On {
        phone: phone.clone(),
        player,
        last: Instant::now(),
        switched,
    };
    {
        let mut mic = lock(&state.voice.mic);
        if !mic.started() {
            // Turned off, or the session ended, meanwhile.
            drop(mic);
            stop(app, on, "turned off as it started");
            return;
        }
        *mic = Mic::On(Box::new(on));
    }
    log::info!(
        "phone microphone: on, into {}{}",
        cable.capture.name,
        if default {
            " (Windows' default microphone)"
        } else {
            ""
        }
    );
    crate::remote_api::log_action(
        app,
        &phone,
        format!(
            "Turned on the phone's microphone (the PC hears it as {})",
            cable.capture.name
        ),
    );
    crate::screen::announce(app);
    tell(app, true, Some(cable.capture.name), default, None);
    watch(app);
}

/// Stops the microphone once the phone stops sending.
#[cfg(windows)]
fn watch(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vyro-mic-quiet".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            let Some(state) = state(&app) else {
                return;
            };
            let mut mic = lock(&state.voice.mic);
            match &*mic {
                Mic::On(on) if on.last.elapsed() >= QUIET => {}
                Mic::On(_) => continue,
                _ => return,
            }
            let Mic::On(on) = std::mem::take(&mut *mic) else {
                return;
            };
            drop(mic);
            stop(&app, *on, "the phone stopped sending");
            return;
        });
    if let Err(e) = spawned {
        log::warn!("phone microphone: no watch: {e}");
    }
}

/// The session ended: the microphone stops, and a clip half sent through the relay goes.
pub fn session_ended(app: &AppHandle, why: &str) {
    if let Some(state) = state(app) {
        *lock(&state.voice.pieces) = None;
    }
    mic_off(app, why);
}

/// Ends the microphone, if it plays (or stops it starting).
pub fn mic_off(app: &AppHandle, why: &str) {
    #[cfg(windows)]
    {
        let Some(state) = state(app) else {
            return;
        };
        // Starting: `start` sees it and stops. Failed: the phone's next try starts at once.
        let was = lock(&state.voice.mic).turn_off();
        if let Mic::On(on) = was {
            stop(app, *on, why);
        }
    }
    #[cfg(not(windows))]
    let _ = (app, why);
}

#[cfg(windows)]
fn stop(app: &AppHandle, on: On, why: &str) {
    let On {
        phone,
        player,
        switched,
        ..
    } = on;
    drop(player);
    if switched {
        if let Some(path) = state(app).and_then(|s| s.voice.restore_path()) {
            restore(&path);
        }
    }
    log::info!("phone microphone: off: {why}");
    crate::remote_api::log_action(
        app,
        &phone,
        format!("Turned off the phone's microphone: {why}"),
    );
    crate::screen::announce(app);
    tell(app, false, None, false, None);
}

/// Makes the cable (`cable`, its recording side's id) Windows' default microphone, keeping the
/// defaults it replaces at `path`; whether there is anything to put back.
#[cfg(windows)]
fn make_default(path: Option<&Path>, cable: &str) -> Result<bool, String> {
    use voice::win;
    let path = path.ok_or("Vyro has no folder for its settings.")?;
    let (console, calls) = win::default_microphones()?;
    if console.as_deref() == Some(cable) && calls.as_deref() == Some(cable) {
        return Ok(false);
    }
    // A file a crash left holds the defaults from before that: it stays.
    let wrote = !path.exists();
    if wrote {
        let was = Defaults {
            console,
            calls,
            cable: cable.to_owned(),
        };
        files::write_json(path, &was).map_err(|e| e.to_string())?;
    }
    if let Err(e) = win::set_default_microphone(Some(cable), Some(cable)) {
        if wrote {
            let _ = std::fs::remove_file(path);
        }
        return Err(e);
    }
    Ok(true)
}

/// Puts back the default microphones kept at `path`, then forgets them.
#[cfg(windows)]
fn restore(path: &Path) {
    use voice::win;
    let was = match files::read_json::<Defaults>(path, |_| Ok(())) {
        Ok(Some(was)) => was,
        Ok(None) => return,
        Err(e) => {
            log::warn!("phone microphone: {e}");
            let _ = std::fs::remove_file(path);
            return;
        }
    };
    let (console, calls) = match win::default_microphones() {
        Ok(now) => now,
        // Not known which role the cable still has: kept for the next start.
        Err(e) => {
            log::warn!("phone microphone: {e}");
            return;
        }
    };
    let (console, calls) = put_back(&was, console.as_deref(), calls.as_deref());
    match win::set_default_microphone(console.as_deref(), calls.as_deref()) {
        Ok(()) => {
            log::info!("phone microphone: the default microphone is back");
            let _ = std::fs::remove_file(path);
        }
        // Kept for the next start.
        Err(e) => log::warn!("phone microphone: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_back_only_what_the_cable_still_has() {
        let was = Defaults {
            console: Some("mic".into()),
            calls: Some("headset".into()),
            cable: "cable".into(),
        };
        assert_eq!(
            put_back(&was, Some("cable"), Some("cable")),
            (Some("mic".into()), Some("headset".into()))
        );
        // The owner chose another microphone for calls meanwhile: it stays.
        assert_eq!(
            put_back(&was, Some("cable"), Some("webcam")),
            (Some("mic".into()), None)
        );
        // No default before: nothing to put back.
        let none = Defaults {
            console: None,
            ..was
        };
        assert_eq!(put_back(&none, Some("cable"), None), (None, None));
    }

    #[test]
    fn off_and_on_again_while_it_starts_is_one_start() {
        let mut mic = Mic::Off;
        assert!(mic.frame("phone"), "the first frame starts it");
        assert!(!mic.frame("phone"), "one start at a time");
        assert!(matches!(mic.turn_off(), Mic::Off), "nothing plays yet");
        // On again before the start finished: no second start, and the first one plays.
        assert!(!mic.frame("phone"));
        assert!(mic.started());

        // Off while it starts, and not on again: it stops once started.
        let mut mic = Mic::Off;
        mic.frame("phone");
        mic.turn_off();
        assert!(!mic.started());
        // Another phone's frames (a session after) wait for that start to end.
        assert!(!mic.frame("other phone"));
        assert!(!mic.started());
    }

    #[test]
    fn deletes_only_its_own_clips() {
        assert!(is_clip("Clip 2026-10-21 14-03-22.m4a"));
        assert!(is_clip("Clip 2026-10-21 14-03-22 (2).wav"));
        assert!(!is_clip("notes.m4a"));
        assert!(!is_clip("Clip 2026-10-21 14-03-22.m4a.lnk"));
    }

    #[test]
    fn a_second_clip_in_the_same_second_gets_its_own_name() {
        let dir = std::env::temp_dir().join(format!("vyro-clips-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let t = [2026, 10, 21, 14, 3, 22];
        let first = free_path(&dir, t, "m4a");
        assert_eq!(first.file_name().unwrap(), "Clip 2026-10-21 14-03-22.m4a");
        // A WAV of that second (no AAC encoder) takes the name too.
        std::fs::write(first.with_extension("wav"), b"").unwrap();
        assert_eq!(
            free_path(&dir, t, "m4a").file_name().unwrap(),
            "Clip 2026-10-21 14-03-22 (2).m4a"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Clokrix Free and Pro in the app (2.5, `clokrix_core::edition`, docs/pro.md): the trial clock,
//! buying in the browser and collecting the key, a key entered by hand, moving and freeing it, and
//! what switches off when the PC turns Free.
//!
//! Free keeps the Dashboard and Settings' own sections. Everything else stops: fans go back to
//! the BIOS and the GPU to stock (their settings kept), desktop widgets and the game overlay go,
//! game watching stops, the phone link closes, and alerts stop notifying. History keeps
//! recording, so it is all there once Pro comes back.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use clokrix_core::edition::checkout::{Checkout, Refusal, Standing};
use clokrix_core::edition::{self, Claim, Edition, Stored, Tier, CHECK_EVERY, DAY};
use clokrix_core::{files, lock, unix_now};
use tauri::{AppHandle, Emitter, Manager};

/// Emitted with an [`Edition`] when it changes.
pub const EDITION_EVENT: &str = "edition-changed";
const FILE: &str = "edition.json";
/// A purchase opened in the browser is waited for this long.
const BUY_WINDOW: i64 = 30 * 60;
/// How often a purchase is asked after, while waited for.
const CLAIM_EVERY: Duration = Duration::from_secs(5);

/// The checkout and its public key, when this build carries them (the Release workflow sets
/// both; a build without them is Pro).
fn checkout() -> Option<Checkout> {
    option_env!("CLOKRIX_LICENCE_URL")
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(Checkout::new)
}

fn public_key() -> Option<[u8; 32]> {
    edition::public_key(option_env!("CLOKRIX_LICENCE_PUBKEY"))
}

pub struct EditionState {
    path: Option<PathBuf>,
    pc: String,
    key: Option<[u8; 32]>,
    stored: Mutex<Stored>,
    /// The tier last told, to act once on a change.
    tier: Mutex<Tier>,
}

impl EditionState {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_config_dir().ok().map(|d| d.join(FILE));
        let mut stored: Stored = path
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut changed = stored.start(unix_now());
        let pc = match edition::machine_guid() {
            Some(g) => edition::pc_id(&g),
            // No id to be found: one made for this install, kept with the rest.
            None => {
                let own = stored
                    .own_pc
                    .clone()
                    .unwrap_or_else(|| edition::pc_id(&edition::new_claim()));
                changed |= stored.own_pc.as_deref() != Some(own.as_str());
                stored.own_pc = Some(own.clone());
                own
            }
        };
        let key = public_key().filter(|_| checkout().is_some());
        let tier = edition::tier(&stored, unix_now(), &pc, key.as_ref());
        log::info!(
            "edition: {tier:?}{}",
            if key.is_none() {
                " (a build without the checkout)"
            } else {
                ""
            }
        );
        let this = Self {
            path,
            pc,
            key,
            stored: Mutex::new(stored),
            tier: Mutex::new(tier),
        };
        if changed {
            this.save();
        }
        this
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let s = lock(&self.stored).clone();
        if let Err(e) = files::write_json(path, &s) {
            log::warn!("edition: cannot write {}: {e}", path.display());
        }
    }

    pub fn edition(&self) -> Edition {
        edition::edition(
            &lock(&self.stored),
            unix_now(),
            &self.pc,
            self.key.as_ref(),
            checkout().is_some(),
        )
    }

    pub fn tier(&self) -> Tier {
        edition::tier(&lock(&self.stored), unix_now(), &self.pc, self.key.as_ref())
    }

    fn change(&self, f: impl FnOnce(&mut Stored)) {
        f(&mut lock(&self.stored));
        self.save();
    }
}

/// Everything is open: Pro, or the trial.
pub fn full(app: &AppHandle) -> bool {
    app.try_state::<EditionState>()
        .is_none_or(|e| e.tier().full())
}

/// Tells the views, and acts on a change of tier.
fn changed(app: &AppHandle) {
    let Some(state) = app.try_state::<EditionState>() else {
        return;
    };
    let now = state.tier();
    let before = std::mem::replace(&mut *lock(&state.tier), now);
    if before != now {
        log::info!("edition: {before:?} → {now:?}");
    }
    // The trial and Pro open the same: only a change to or from Free does anything.
    if before.full() != now.full() {
        apply(app, now.full());
    }
    if let Err(e) = app.emit(EDITION_EVENT, state.edition()) {
        log::debug!("emit {EDITION_EVENT}: {e}");
    }
}

/// What Pro opens, on or off. Off: fans to the BIOS, the GPU to stock (with tuning, and applying
/// it at start, off, so the service does not tune the card at the next start either); widgets,
/// the overlay, game watching and the phone link go. On: what was on comes back.
fn apply(app: &AppHandle, on: bool) {
    crate::tray::set_pro(app, on);
    crate::widgets::sync(app);
    if let Some(r) = app.try_state::<crate::remote::Remote>() {
        if on {
            r.resume(app);
        } else {
            r.stop();
            r.stop_relay();
        }
    }
    if !on {
        let app = app.clone();
        std::thread::spawn(move || stand_down(&app));
    }
}

/// Fans to the BIOS and the GPU to stock, their control turned off but kept as set.
fn stand_down(app: &AppHandle) {
    let Some(engine) = app.try_state::<crate::engine::Engine>() else {
        return;
    };
    let api = &engine.api;
    if let Ok(status) = api.fan_status() {
        if status.config.enabled {
            let _ = api.restore_fans();
            let mut c = status.config;
            c.enabled = false;
            if let Err(e) = api.set_fan_config(c) {
                log::warn!("edition: fan control not turned off: {e}");
            }
        }
    }
    if let Ok(status) = api.gpu_tune_status() {
        if status.config.enabled || status.config.apply_at_start {
            let _ = api.restore_gpu();
            let mut c = status.config;
            c.enabled = false;
            c.apply_at_start = false;
            if let Err(e) = api.set_gpu_tune(c, false) {
                log::warn!("edition: GPU tuning not turned off: {e}");
            }
        }
    }
}

/// At start, after the rest is set up: Free switches off what a trial left on (the service may
/// have tuned the GPU before the app started), and the watch begins.
pub fn start(app: &AppHandle) {
    if !full(app) {
        apply(app, false);
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("clokrix-edition".into())
        .spawn(move || watch(&app));
    if let Err(e) = spawned {
        log::warn!("edition: watch not started: {e}");
    }
}

/// Once a minute: the trial's end, and the weekly look at the activation. Every 5 s while a
/// purchase is waited for: its key.
fn watch(app: &AppHandle) {
    let mut last_minute = 0;
    loop {
        std::thread::sleep(CLAIM_EVERY);
        let Some(state) = app.try_state::<EditionState>() else {
            continue;
        };
        let now = unix_now();
        let claim = lock(&state.stored).claim.clone().filter(|c| c.until > now);
        if let Some(c) = claim {
            collect(app, &state, &c);
        }
        if now - last_minute < 60 {
            continue;
        }
        last_minute = now;
        recheck(app, &state, now);
        if *lock(&state.tier) != state.tier() {
            changed(app);
        }
    }
}

/// Asks after the purchase `c`; once paid, its key turns Pro on here.
fn collect(app: &AppHandle, state: &EditionState, c: &Claim) {
    let Some(checkout) = checkout() else { return };
    match checkout.claim(&c.code) {
        Ok(None) => {}
        Ok(Some(key)) => {
            state.change(|s| s.claim = None);
            log::info!("edition: purchase paid; turning Pro on");
            if let Err(e) = activate(app, &key, false) {
                state.change(|s| {
                    s.key = Some(key.clone());
                    s.note = Some(format!("Paid, but Pro did not turn on: {e}"));
                });
                changed(app);
            }
        }
        // Not reached, or asked too often: asked again in 5 s.
        Err(Refusal::Unreachable(e)) => log::debug!("edition: claim: {e}"),
        Err(Refusal::Busy) => log::debug!("edition: claim: busy"),
        Err(e) => {
            log::info!("edition: claim: {e}");
            state.change(|s| {
                s.claim = None;
                s.note = Some(e.to_string());
            });
            changed(app);
        }
    }
}

/// At most weekly, when online: is the key still Pro on this PC? Moved, freed or refunded: Free,
/// with a note. Unreachable: nothing changes.
fn recheck(app: &AppHandle, state: &EditionState, now: i64) {
    let (key, due) = {
        let s = lock(&state.stored);
        (
            s.key.clone().filter(|_| s.activation.is_some()),
            s.checked
                .is_none_or(|t| now - t >= CHECK_EVERY || t > now + DAY),
        )
    };
    let (Some(key), true, Some(checkout)) = (key, due, checkout()) else {
        return;
    };
    let note = match checkout.check(&key, &state.pc) {
        Ok(Standing::Here) => None,
        Ok(Standing::Moved) => Some("Pro moved to another PC with this key."),
        Ok(Standing::Released) => Some("Pro was freed on this PC."),
        Ok(Standing::Revoked) => Some("This key's payment was refunded."),
        Ok(Standing::NoKey) => Some("The checkout no longer knows this key."),
        Err(e) => {
            log::debug!("edition: check: {e}");
            return;
        }
    };
    state.change(|s| {
        s.checked = Some(now);
        if let Some(n) = note {
            s.activation = None;
            s.note = Some(n.into());
        }
    });
    if note.is_some() {
        changed(app);
    }
}

/// `key` Pro on this PC (`move_here`: also from another PC).
fn activate(app: &AppHandle, key: &str, move_here: bool) -> Result<(), String> {
    let state = app.state::<EditionState>();
    let checkout = checkout().ok_or("This build of Clokrix cannot be bought from.")?;
    let token = checkout
        .activate(key, &state.pc, move_here)
        .map_err(|e| match e {
            Refusal::MovedRecently { until: Some(t) } => format!(
                "Pro moved to another PC less than 30 days ago; it can move again in {} days.",
                ((t - unix_now()) / DAY).max(1)
            ),
            e => e.to_string(),
        })?;
    let key_bytes = state
        .key
        .ok_or("This build of Clokrix cannot be bought from.")?;
    if edition::verify(&token, &key_bytes, &state.pc).is_none() {
        return Err(
            "The checkout's answer did not check out. Try again, or update Clokrix.".into(),
        );
    }
    state.change(|s| {
        s.key = Some(key.to_owned());
        s.activation = Some(token);
        s.checked = Some(unix_now());
        s.note = None;
        s.claim = None;
    });
    changed(app);
    Ok(())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_edition(state: tauri::State<'_, EditionState>) -> Edition {
    state.edition()
}

/// Buy Pro: opens the checkout in the browser with a new purchase's code, then waits for its key.
#[tauri::command]
pub fn buy_pro(app: AppHandle, state: tauri::State<'_, EditionState>) -> Result<Edition, String> {
    let checkout = checkout().ok_or("This build of Clokrix cannot be bought from.")?;
    let code = edition::new_claim();
    open_in_browser(&checkout.buy_url(&code))?;
    state.change(|s| {
        s.claim = Some(Claim {
            code,
            until: unix_now() + BUY_WINDOW,
        });
        s.note = None;
    });
    changed(&app);
    Ok(state.edition())
}

/// A key typed in Settings → Clokrix Pro; `move_here` takes it from another PC.
#[tauri::command]
pub async fn enter_key(app: AppHandle, key: String, move_here: bool) -> Result<Edition, String> {
    blocking(move || {
        activate(&app, key.trim(), move_here)?;
        Ok(app.state::<EditionState>().edition())
    })
    .await
}

/// Free this PC: the key goes off it, for another PC to take; this PC turns Free.
#[tauri::command]
pub async fn release_pro(app: AppHandle) -> Result<Edition, String> {
    blocking(move || {
        let state = app.state::<EditionState>();
        let key = lock(&state.stored)
            .key
            .clone()
            .ok_or("No key is in use here.")?;
        let checkout = checkout().ok_or("This build of Clokrix cannot be bought from.")?;
        match checkout.release(&key, &state.pc) {
            Ok(()) | Err(Refusal::NotHere) => {}
            Err(e) => return Err(e.to_string()),
        }
        state.change(|s| {
            s.activation = None;
            s.note = Some("Pro was freed on this PC; its key can be used on another.".into());
        });
        changed(&app);
        Ok(state.edition())
    })
    .await
}

/// The views have said the trial ended (once).
#[tauri::command]
pub fn edition_seen(app: AppHandle, state: tauri::State<'_, EditionState>) -> Edition {
    state.change(|s| {
        s.ended_seen = true;
        s.note = None;
    });
    changed(&app);
    state.edition()
}

#[cfg(windows)]
fn open_in_browser(url: &str) -> Result<(), String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // SAFETY: plain strings in; ShellExecuteW opens the address with the default browser.
    let h = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // Above 32 is success (the documented convention).
    if h.0 as isize > 32 {
        Ok(())
    } else {
        Err("The browser could not be opened.".into())
    }
}

#[cfg(not(windows))]
fn open_in_browser(url: &str) -> Result<(), String> {
    log::info!("edition: open {url}");
    Ok(())
}

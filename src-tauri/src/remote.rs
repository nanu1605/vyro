//! Remote companion (P26) in the app: the owner's choices and the paired devices in
//! `remote.json` (local app data, never roamed), the server's life, and the Settings commands.
//! The server itself is `vyro_core::remote`; it runs here, as the signed-in user, never in the
//! LocalSystem service.
//!
//! Reach from anywhere (1.4): the relay connection (`vyro_core::remote::relay`) runs here too,
//! with the phones paired through it kept in the same file (their secrets sealed with DPAPI).

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vyro_core::actions::wake::WakeReadiness;
use vyro_core::remote::auth::{hex, Auth, Stored, STORED_VERSION};
use vyro_core::remote::relay::proto::{pairing_link, relay_base};
use vyro_core::remote::relay::{
    Phones, RelayClient, RelayHooks, RelayPairing, RelayState, RelayStatus, StoredRelay,
};
use vyro_core::remote::{
    firewall, net, qr, random, FirewallState, Hooks, RemoteBrief, RemoteInfo, RemoteServer,
    RemoteSettings, RemoteStatus, ServerConfig, DAY_CHOICES, FILE, MIN_PORT,
};
use vyro_core::{files, lock, unix_now, SensorSnapshot};

use crate::tray::SettingsStore;

/// Emitted with a [`RemoteBrief`] when the companion starts or stops, a live view opens or
/// closes, or the paired devices change.
pub const REMOTE_EVENT: &str = "remote-status";

/// The relay built into this build (`VYRO_RELAY_URL` when it was built), if any.
fn built_in_relay() -> Option<String> {
    option_env!("VYRO_RELAY_URL")
        .filter(|u| !u.trim().is_empty())
        .and_then(|u| relay_base(u).ok())
}

/// The owner's relay choices, kept with the phones in `remote.json`.
#[derive(Debug, Clone, Default)]
struct RelayChoice {
    enabled: bool,
    /// The owner's own relay; none: the built-in one.
    url: Option<String>,
    /// "Wake from phone" (1.5): phones are told this PC's wired address.
    wake: bool,
}

pub struct Remote {
    path: Option<PathBuf>,
    settings: Mutex<RemoteSettings>,
    auth: Arc<Mutex<Auth>>,
    server: Mutex<Option<RemoteServer>>,
    /// Why it is not running although turned on (the port is taken, ...).
    error: Mutex<Option<String>>,
    /// Live views on the local network.
    watching: AtomicU32,
    relay: Mutex<RelayChoice>,
    /// This PC's room and key on the relay, and its phones; made the first time it is turned on.
    phones: Mutex<Option<Arc<Mutex<Phones>>>>,
    client: Mutex<Option<RelayClient>>,
    /// Why the relay is not used although turned on (no address, ...).
    relay_error: Mutex<Option<String>>,
    /// Phones with the app open, through the relay.
    relay_watching: AtomicU32,
}

/// Reads `remote.json`; one that does not parse is set aside as `remote.bad.json` (see
/// `vyro_core::files`) and the companion stays off.
fn read(path: &Path) -> Stored {
    match files::read_json::<Stored>(path, |_| Ok(())) {
        Ok(s) => s.unwrap_or_default(),
        Err(e) => {
            log::warn!(
                "remote: {} does not load ({e}); the companion stays off",
                path.display()
            );
            Stored::default()
        }
    }
}

/// Writes through a temporary file, so a crash mid-write never leaves half a file.
fn write(path: &Path, stored: &Stored) -> Result<(), String> {
    files::write_json(path, stored).map_err(|e| format!("{}: {e}", path.display()))
}

/// A settings enum's name ("midnight").
fn name_of(v: &impl Serialize) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// A file of the page from the bundled build. Never Tauri's fallback to `index.html` for a
/// file that is not there.
fn asset(app: &AppHandle, path: &str) -> Option<Vec<u8>> {
    let resolver = app.asset_resolver();
    let key = format!("/{path}");
    let embedded = resolver.iter().next().is_some();
    if embedded && !resolver.iter().any(|(k, _)| k == key.as_str()) {
        return None;
    }
    resolver.get(path.to_owned()).map(|a| a.bytes)
}

fn emit(app: &AppHandle) {
    if let Some(r) = app.try_state::<Remote>() {
        if let Err(e) = app.emit(REMOTE_EVENT, r.brief()) {
            log::debug!("emit {REMOTE_EVENT}: {e}");
        }
    }
}

/// PC name, version, theme, accent and unit, as the companion and the phones show them.
fn info(app: &AppHandle) -> RemoteInfo {
    let s = app
        .try_state::<SettingsStore>()
        .map(|s| s.get())
        .unwrap_or_default();
    RemoteInfo {
        pc: net::pc_name(),
        version: app.package_info().version.to_string(),
        theme: name_of(&s.theme),
        accent: name_of(&s.accent),
        unit: s.temperature_unit,
        ..RemoteInfo::default()
    }
}

/// Saves the paired devices and tells the window.
fn saved(app: &AppHandle) {
    if let Some(r) = app.try_state::<Remote>() {
        r.save();
    }
    emit(app);
}

/// A phone watching, here or through the relay, keeps the readings at the window's rate.
fn watching_changed(app: &AppHandle) {
    if let (Some(r), Some(engine)) = (
        app.try_state::<Remote>(),
        app.try_state::<crate::engine::Engine>(),
    ) {
        engine.set_remote(r.brief().watching > 0);
    }
    emit(app);
}

fn hooks(app: &AppHandle) -> Hooks {
    let (files, about, save, watched) = (app.clone(), app.clone(), app.clone(), app.clone());
    Hooks {
        asset: Box::new(move |path| asset(&files, path)),
        info: Box::new(move || info(&about)),
        saved: Box::new(move || saved(&save)),
        watching: Box::new(move |n| {
            if let Some(r) = watched.try_state::<Remote>() {
                r.watching.store(n, Ordering::SeqCst);
            }
            watching_changed(&watched);
        }),
    }
}

/// The wired adapter another PC would wake this one through, while "Wake from phone" is on.
/// Read at most every few minutes: a phone's `info` asks for it while the relay connection
/// holds its phones.
fn wake_target(app: &AppHandle) -> Option<vyro_core::actions::wake::WakeTarget> {
    use vyro_core::actions::wake::{mac_text, parse_mac, WakeTarget};
    type Cached = Option<(std::time::Instant, Option<WakeTarget>)>;
    static CACHE: Mutex<Cached> = Mutex::new(None);
    const FRESH: Duration = Duration::from_secs(300);
    let on = app
        .try_state::<Remote>()
        .is_some_and(|r| lock(&r.relay).wake);
    if !on {
        return None;
    }
    if let Some((at, target)) = lock(&CACHE).as_ref() {
        if at.elapsed() < FRESH {
            return target.clone();
        }
    }
    let target = wired_adapter().and_then(|a| {
        let mac = parse_mac(&a.mac?.0)?;
        Some(WakeTarget {
            mac: mac_text(&mac),
        })
    });
    *lock(&CACHE) = Some((std::time::Instant::now(), target.clone()));
    target
}

/// This PC's wired, physical adapter, a connected one first.
fn wired_adapter() -> Option<vyro_core::info::NetAdapterInfo> {
    let mut wired: Vec<_> = crate::commands::collect_network()
        .into_iter()
        .filter(|a| {
            a.kind == vyro_core::snapshot::NetKind::Ethernet
                && !a.virtual_adapter
                && a.mac.is_some()
        })
        .collect();
    wired.sort_by_key(|a| (!a.connected, !a.primary));
    wired.into_iter().next()
}

fn relay_hooks(app: &AppHandle) -> RelayHooks {
    let (about, save, watched, state) = (app.clone(), app.clone(), app.clone(), app.clone());
    let asked = app.clone();
    RelayHooks {
        // Only phones through the relay, sealed, hear how to wake this PC; never the LAN page.
        info: Box::new(move || RemoteInfo {
            wake: wake_target(&about),
            ..info(&about)
        }),
        saved: Box::new(move || saved(&save)),
        watching: Box::new(move |n| {
            if let Some(r) = watched.try_state::<Remote>() {
                r.relay_watching.store(n, Ordering::SeqCst);
            }
            watching_changed(&watched);
        }),
        state: Box::new(move |_| emit(&state)),
        request: Box::new(move |r, reply| crate::remote_api::handle(&asked, r, reply)),
    }
}

/// A fresh place on the relay: a random room and key.
fn fresh_phones() -> Result<Phones, String> {
    let (room, key) = random::<16>()
        .zip(random::<32>())
        .ok_or("No randomness available.")?;
    Ok(Phones::fresh(room, key))
}

impl Remote {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_local_data_dir().ok().map(|d| d.join(FILE));
        let stored = path.as_deref().map(read).unwrap_or_default();
        let now = unix_now();
        let relay = stored.relay.unwrap_or_default();
        let phones = Phones::load(&relay, now).map(|p| Arc::new(Mutex::new(p)));
        if phones.is_none() && !relay.room.is_empty() {
            log::warn!("remote: the relay keys do not open on this account; they start afresh");
        }
        Self {
            path,
            settings: Mutex::new(stored.settings.sanitized()),
            auth: Arc::new(Mutex::new(Auth::new(stored.devices, now))),
            server: Mutex::new(None),
            error: Mutex::new(None),
            watching: AtomicU32::new(0),
            relay: Mutex::new(RelayChoice {
                enabled: relay.enabled,
                url: relay.url.and_then(|u| relay_base(&u).ok()),
                wake: relay.wake,
            }),
            phones: Mutex::new(phones),
            client: Mutex::new(None),
            relay_error: Mutex::new(None),
            relay_watching: AtomicU32::new(0),
        }
    }

    /// At launch: serves again, and reconnects to the relay, if the owner left them on.
    pub fn resume(&self, app: &AppHandle) {
        if lock(&self.settings).enabled {
            self.start(app);
        }
        if lock(&self.relay).enabled {
            self.start_relay(app);
        }
    }

    pub fn running(&self) -> bool {
        lock(&self.server).is_some()
    }

    fn relay_state(&self) -> (RelayState, Option<String>) {
        match lock(&self.client).as_ref() {
            Some(c) => c.status(),
            None => (RelayState::Off, lock(&self.relay_error).clone()),
        }
    }

    pub fn brief(&self) -> RemoteBrief {
        let relay = self.relay_state().0;
        RemoteBrief {
            running: self.running() || relay != RelayState::Off,
            watching: self.watching.load(Ordering::SeqCst)
                + self.relay_watching.load(Ordering::SeqCst),
            relay,
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let choice = lock(&self.relay).clone();
        let relay = match lock(&self.phones).as_ref() {
            Some(p) => Some(StoredRelay {
                wake: choice.wake,
                ..lock(p).stored(choice.enabled, choice.url)
            }),
            None if choice.enabled || choice.url.is_some() || choice.wake => Some(StoredRelay {
                enabled: choice.enabled,
                url: choice.url,
                wake: choice.wake,
                ..StoredRelay::default()
            }),
            None => None,
        };
        let stored = Stored {
            version: STORED_VERSION,
            settings: *lock(&self.settings),
            devices: lock(&self.auth).stored().to_vec(),
            relay,
        };
        if let Err(e) = write(path, &stored) {
            log::warn!("remote: {e}");
        }
    }

    /// Starts serving; a failure (port taken) is kept for Settings to show.
    fn start(&self, app: &AppHandle) {
        let mut server = lock(&self.server);
        if server.is_some() {
            return;
        }
        let settings = *lock(&self.settings);
        let config = ServerConfig {
            port: settings.port,
            bind: Ipv4Addr::UNSPECIFIED,
            names: vec![net::pc_name()],
            session_days: settings.session_days,
        };
        match RemoteServer::start(config, Arc::clone(&self.auth), hooks(app)) {
            Ok(s) => {
                *server = Some(s);
                *lock(&self.error) = None;
            }
            Err(e) => {
                log::warn!("remote: {e}");
                *lock(&self.error) = Some(e);
            }
        }
    }

    /// Stops serving and closes every connection.
    pub fn stop(&self) {
        let server = lock(&self.server).take();
        if let Some(mut s) = server {
            s.stop();
        }
        lock(&self.auth).cancel_pairing();
        self.watching.store(0, Ordering::SeqCst);
    }

    fn set_enabled(&self, app: &AppHandle, on: bool) {
        lock(&self.settings).enabled = on;
        if on {
            self.start(app);
        } else {
            self.stop();
            *lock(&self.error) = None;
            if let Some(engine) = app.try_state::<crate::engine::Engine>() {
                engine.set_remote(false);
            }
        }
        self.save();
        log::info!("remote: turned {}", if on { "on" } else { "off" });
    }

    fn set_options(&self, app: &AppHandle, port: u16, session_days: u32) -> Result<(), String> {
        if port < MIN_PORT {
            return Err(format!("Pick a port from {MIN_PORT} to 65535."));
        }
        if !DAY_CHOICES.contains(&session_days) {
            return Err("Pick 1, 7, 30 or 90 days.".into());
        }
        let moved = {
            let mut s = lock(&self.settings);
            let moved = s.port != port;
            s.port = port;
            s.session_days = session_days;
            moved
        };
        if let Some(s) = lock(&self.server).as_ref() {
            s.set_session_days(session_days);
        }
        if let Some(c) = lock(&self.client).as_ref() {
            c.set_session_days(session_days);
        }
        if moved && self.running() {
            self.stop();
            self.start(app);
        }
        self.save();
        Ok(())
    }

    fn start_pairing(&self) -> Result<(), String> {
        if !self.running() {
            return Err("Turn the companion on first.".into());
        }
        let code = vyro_core::remote::random().ok_or("No randomness available.")?;
        lock(&self.auth).start_pairing(unix_now(), code);
        Ok(())
    }

    /// Signs one device out, or all of them; their live views close at once.
    fn sign_out(&self, id: Option<&str>) {
        {
            let mut auth = lock(&self.auth);
            match id {
                Some(id) => {
                    auth.sign_out(id);
                }
                None => auth.sign_out_all(),
            }
        }
        if let Some(s) = lock(&self.server).as_ref() {
            s.close_device(id);
        }
        if let Some(p) = lock(&self.phones).as_ref() {
            let mut p = lock(p);
            match id {
                Some(id) => {
                    p.sign_out(id);
                }
                None => p.sign_out_all(),
            }
        }
        if let Some(c) = lock(&self.client).as_ref() {
            c.nudge();
        }
        self.save();
    }

    /// The relay to use: the owner's, else the built-in one.
    fn relay_url(&self) -> Option<String> {
        lock(&self.relay).url.clone().or_else(built_in_relay)
    }

    /// Connects to the relay; a failure (no address) is kept for Settings to show.
    fn start_relay(&self, app: &AppHandle) {
        let mut client = lock(&self.client);
        if client.is_some() {
            return;
        }
        let Some(base) = self.relay_url() else {
            *lock(&self.relay_error) = Some(
                "This build of Vyro has no relay. Enter your relay's address under Options.".into(),
            );
            return;
        };
        let phones = {
            let mut slot = lock(&self.phones);
            match slot.as_ref() {
                Some(p) => Arc::clone(p),
                None => match fresh_phones() {
                    Ok(p) => {
                        let p = Arc::new(Mutex::new(p));
                        *slot = Some(Arc::clone(&p));
                        p
                    }
                    Err(e) => {
                        *lock(&self.relay_error) = Some(e);
                        return;
                    }
                },
            }
        };
        let days = lock(&self.settings).session_days;
        match RelayClient::start(base, phones, days, relay_hooks(app)) {
            Ok(c) => {
                *client = Some(c);
                *lock(&self.relay_error) = None;
            }
            Err(e) => {
                log::warn!("remote: {e}");
                *lock(&self.relay_error) = Some(e);
            }
        }
    }

    pub fn stop_relay(&self) {
        let client = lock(&self.client).take();
        if let Some(mut c) = client {
            c.stop();
        }
        if let Some(p) = lock(&self.phones).as_ref() {
            lock(p).cancel_pairing();
        }
        self.relay_watching.store(0, Ordering::SeqCst);
    }

    fn set_relay_enabled(&self, app: &AppHandle, on: bool) {
        lock(&self.relay).enabled = on;
        if on {
            self.start_relay(app);
        } else {
            self.stop_relay();
            *lock(&self.relay_error) = None;
        }
        self.save();
        watching_changed(app);
        log::info!(
            "remote: reach from anywhere turned {}",
            if on { "on" } else { "off" }
        );
    }

    /// The owner's own relay (`None`: the built-in one). Phones paired through the old one
    /// cannot reach the new one, so they are signed out.
    fn set_relay_url(&self, app: &AppHandle, url: Option<&str>) -> Result<(), String> {
        let url = match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(u) => Some(relay_base(u)?),
            None => None,
        };
        let before = self.relay_url();
        lock(&self.relay).url = url;
        if self.relay_url() != before {
            let on = lock(&self.relay).enabled;
            self.stop_relay();
            if let Some(p) = lock(&self.phones).as_ref() {
                lock(p).sign_out_all();
            }
            *lock(&self.relay_error) = None;
            if on {
                self.start_relay(app);
            }
        }
        self.save();
        Ok(())
    }

    /// Starts over on the relay: a new room and key, every phone signed out.
    fn reset_relay(&self, app: &AppHandle) -> Result<(), String> {
        let fresh = fresh_phones()?;
        let on = lock(&self.relay).enabled;
        self.stop_relay();
        *lock(&self.phones) = Some(Arc::new(Mutex::new(fresh)));
        *lock(&self.relay_error) = None;
        if on {
            self.start_relay(app);
        }
        self.save();
        Ok(())
    }

    /// Lets a phone control the PC, or only look (1.5). The phone hears at once.
    fn set_phone_control(&self, id: &str, control: bool) -> Result<(), String> {
        let phones = lock(&self.phones).clone().ok_or("No phone is paired.")?;
        if !lock(&phones).set_control(id, control) {
            return Err("That phone is no longer paired.".into());
        }
        log::info!(
            "remote: a phone may {}",
            if control {
                "control this PC"
            } else {
                "only look"
            }
        );
        self.save();
        Ok(())
    }

    /// Lets a phone see and use the screen, or not (1.7). The phone hears at once.
    fn set_phone_screen(&self, id: &str, on: bool) -> Result<(), String> {
        let phones = lock(&self.phones).clone().ok_or("No phone is paired.")?;
        if !lock(&phones).set_screen(id, on) {
            return Err("That phone is no longer paired.".into());
        }
        log::info!(
            "remote: a phone may {}see the screen",
            if on { "" } else { "no longer " }
        );
        self.save();
        Ok(())
    }

    fn set_wake(&self, on: bool) {
        lock(&self.relay).wake = on;
        self.save();
    }

    fn start_relay_pairing(&self, control: bool) -> Result<(), String> {
        if lock(&self.client).is_none() {
            return Err("Turn on reach from anywhere first.".into());
        }
        let phones = lock(&self.phones)
            .clone()
            .ok_or("Turn on reach from anywhere first.")?;
        let secret = random::<32>().ok_or("No randomness available.")?;
        let lan: Vec<String> = lock(&self.auth)
            .stored()
            .iter()
            .map(|d| d.id.clone())
            .collect();
        let generation = {
            let mut p = lock(&phones);
            // A phone id is 32 random bits; draw again in the unlikely case it is taken.
            let id = (0..8)
                .filter_map(|_| random::<4>())
                .find(|id| p.id_free(id) && !lan.contains(&hex(id)))
                .ok_or("No randomness available.")?;
            p.start_pairing(unix_now(), secret, id)?;
            p.set_control(&hex(&id), control);
            p.generation()
        };
        if let Some(c) = lock(&self.client).as_ref() {
            c.nudge();
            // The link works once the relay has the phone's key: show it then.
            c.wait_listed(generation, Duration::from_secs(3));
        }
        Ok(())
    }

    fn cancel_relay_pairing(&self) {
        if let Some(p) = lock(&self.phones).as_ref() {
            lock(p).cancel_pairing();
        }
        if let Some(c) = lock(&self.client).as_ref() {
            c.nudge();
        }
    }

    fn relay_status(&self, now: i64) -> (RelayStatus, Vec<vyro_core::remote::RemoteDevice>) {
        let choice = lock(&self.relay).clone();
        let url = self.relay_url().unwrap_or_default();
        let (state, error) = self.relay_state();
        let watching = lock(&self.client)
            .as_ref()
            .map(RelayClient::watching)
            .unwrap_or_default();
        let (pairing, devices, pruned) = match lock(&self.phones).as_ref() {
            Some(p) => {
                let mut p = lock(p);
                let pruned = p.prune(now);
                let pairing =
                    p.pairing(now)
                        .filter(|_| !url.is_empty())
                        .map(|(id, secret, expires)| {
                            let link = pairing_link(&url, p.room(), &id, &secret);
                            let (qr, qr_size) =
                                qr::path(&link).map_or((None, 0), |(q, n)| (Some(q), n));
                            RelayPairing {
                                url: link,
                                expires,
                                qr,
                                qr_size,
                            }
                        });
                (pairing, p.devices(&watching), pruned)
            }
            None => (None, Vec::new(), false),
        };
        if pruned {
            if let Some(c) = lock(&self.client).as_ref() {
                c.nudge();
            }
            self.save();
        }
        let status = RelayStatus {
            enabled: choice.enabled,
            url,
            custom: choice.url.is_some(),
            state,
            error,
            pairing,
            watching: u32::try_from(watching.len()).unwrap_or(u32::MAX),
            wake: choice.wake,
        };
        (status, devices)
    }

    /// Settings → Remote. Reads the firewall's rules (a moment's work), so off the UI thread.
    fn status(&self) -> RemoteStatus {
        let settings = *lock(&self.settings);
        let (running, port, watching) = {
            let server = lock(&self.server);
            (
                server.is_some(),
                server.as_ref().map_or(settings.port, |s| s.addr().port()),
                server
                    .as_ref()
                    .map(RemoteServer::watching)
                    .unwrap_or_default(),
            )
        };
        let urls: Vec<String> = net::lan_addresses(&net::local_addresses())
            .into_iter()
            .map(|ip| net::url(ip, port))
            .collect();
        let now = unix_now();
        let (devices, pairing, pruned) = {
            let mut auth = lock(&self.auth);
            let pruned = auth.prune(now);
            (auth.devices(&watching), auth.pairing(now), pruned)
        };
        if pruned {
            self.save();
        }
        let pairing = pairing.map(|mut p| {
            if let Some(base) = urls.first() {
                let url = format!("{base}#pair={}", p.code.replace('-', ""));
                if let Some((path, size)) = qr::path(&url) {
                    p.qr = Some(path);
                    p.qr_size = size;
                }
                p.url = Some(url);
            }
            p
        });
        let firewall = match std::env::current_exe() {
            Ok(exe) if running => firewall::state(&exe),
            _ => FirewallState::Unknown,
        };
        let (relay, phones) = self.relay_status(now);
        let mut devices = devices;
        devices.extend(phones);
        RemoteStatus {
            settings,
            running,
            error: lock(&self.error).clone(),
            urls,
            devices,
            watching: self.watching.load(Ordering::SeqCst),
            pairing,
            firewall,
            relay,
        }
    }
}

/// Feeds a reading to the live views and the relay, while they are on.
pub fn publish(app: &AppHandle, snap: &SensorSnapshot) {
    let Some(r) = app.try_state::<Remote>() else {
        return;
    };
    let server = lock(&r.server);
    let client = lock(&r.client);
    if server.is_none() && client.is_none() {
        return;
    }
    let unit = app
        .try_state::<SettingsStore>()
        .map(|s| s.unit())
        .unwrap_or_default();
    if let Some(s) = server.as_ref() {
        s.publish(snap, unit);
    }
    if let Some(c) = client.as_ref() {
        c.publish(snap, unit);
    }
}

/// Runs a change off the UI thread, then answers with the new status.
async fn change(
    app: AppHandle,
    f: impl FnOnce(&AppHandle, &Remote) -> Result<(), String> + Send + 'static,
) -> Result<RemoteStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let r = app.state::<Remote>();
        f(&app, &r)?;
        let status = r.status();
        emit(&app);
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_remote_status(app: AppHandle) -> Result<RemoteStatus, String> {
    change(app, |_, _| Ok(())).await
}

#[tauri::command]
pub async fn set_remote_enabled(app: AppHandle, enabled: bool) -> Result<RemoteStatus, String> {
    change(app, move |app, r| {
        r.set_enabled(app, enabled);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_remote_options(
    app: AppHandle,
    port: u16,
    session_days: u32,
) -> Result<RemoteStatus, String> {
    change(app, move |app, r| r.set_options(app, port, session_days)).await
}

#[tauri::command]
pub async fn start_remote_pairing(app: AppHandle) -> Result<RemoteStatus, String> {
    change(app, |_, r| r.start_pairing()).await
}

#[tauri::command]
pub async fn cancel_remote_pairing(app: AppHandle) -> Result<RemoteStatus, String> {
    change(app, |_, r| {
        lock(&r.auth).cancel_pairing();
        Ok(())
    })
    .await
}

/// `id`: one device; none: every device.
#[tauri::command]
pub async fn sign_out_remote(app: AppHandle, id: Option<String>) -> Result<RemoteStatus, String> {
    change(app, move |app, r| {
        match id.as_deref() {
            Some(id) => crate::screen::end_phone(app, id, "the phone was signed out"),
            None => crate::screen::end_all(app, "every phone was signed out"),
        }
        r.sign_out(id.as_deref());
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn get_remote_brief(remote: tauri::State<'_, Remote>) -> RemoteBrief {
    remote.brief()
}

#[tauri::command]
pub async fn set_relay_enabled(app: AppHandle, enabled: bool) -> Result<RemoteStatus, String> {
    change(app, move |app, r| {
        if !enabled {
            crate::screen::end_all(app, "reach from anywhere was turned off");
        }
        r.set_relay_enabled(app, enabled);
        Ok(())
    })
    .await
}

/// `url`: the owner's own relay; none or empty: the one built into Vyro.
#[tauri::command]
pub async fn set_relay_url(app: AppHandle, url: Option<String>) -> Result<RemoteStatus, String> {
    change(app, move |app, r| r.set_relay_url(app, url.as_deref())).await
}

/// `control`: the phone may control the PC (1.5); otherwise it only looks.
#[tauri::command]
pub async fn start_relay_pairing(app: AppHandle, control: bool) -> Result<RemoteStatus, String> {
    change(app, move |_, r| r.start_relay_pairing(control)).await
}

/// Lets a phone paired through the relay control the PC, or only look (1.5).
#[tauri::command]
pub async fn set_phone_control(
    app: AppHandle,
    id: String,
    control: bool,
) -> Result<RemoteStatus, String> {
    change(app, move |_, r| r.set_phone_control(&id, control)).await
}

/// Lets a phone paired through the relay see and use the screen, or not (1.7); taking it back
/// ends the phone's session at once.
#[tauri::command]
pub async fn set_phone_screen(
    app: AppHandle,
    id: String,
    on: bool,
) -> Result<RemoteStatus, String> {
    change(app, move |app, r| {
        if !on {
            crate::screen::end_phone(app, &id, "the screen is no longer allowed");
        }
        r.set_phone_screen(&id, on)
    })
    .await
}

/// "Wake from phone" (1.5).
#[tauri::command]
pub async fn set_wake_from_phone(app: AppHandle, on: bool) -> Result<RemoteStatus, String> {
    change(app, move |_, r| {
        r.set_wake(on);
        Ok(())
    })
    .await
}

/// How ready this PC is to be woken: its wired adapter and Windows' settings for it.
#[tauri::command]
pub async fn get_wake_readiness(app: AppHandle) -> Result<WakeReadiness, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let adapter = wired_adapter();
        let laptop = app.try_state::<crate::engine::Engine>().is_some_and(|e| {
            e.inventory.hardware().board.form_factor == vyro_core::info::FormFactor::Laptop
        });
        WakeReadiness {
            magic_packet: adapter
                .as_ref()
                .and_then(|a| vyro_core::actions::wake::magic_packet_setting(&a.description)),
            adapter: adapter.map(|a| a.name),
            fast_startup: vyro_core::actions::wake::fast_startup(),
            laptop,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_relay_pairing(app: AppHandle) -> Result<RemoteStatus, String> {
    change(app, |_, r| {
        r.cancel_relay_pairing();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn reset_relay(app: AppHandle) -> Result<RemoteStatus, String> {
    change(app, |app, r| {
        crate::screen::end_all(app, "the relay was reset");
        r.reset_relay(app)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_file_turns_the_companion_off_and_is_kept() {
        let dir = std::env::temp_dir().join(format!("vyro-remote-{}", std::process::id()));
        let path = dir.join(FILE);
        let stored = Stored {
            version: STORED_VERSION,
            settings: RemoteSettings {
                enabled: true,
                port: 9000,
                session_days: 7,
            },
            devices: Vec::new(),
            relay: None,
        };
        write(&path, &stored).unwrap();
        assert_eq!(read(&path), stored);
        assert!(!path.with_extension("json.tmp").exists());

        std::fs::write(&path, "{ not json").unwrap();
        let back = read(&path);
        assert!(!back.settings.enabled);
        assert!(!path.exists());
        assert!(dir.join("remote.bad.json").exists());
        assert_eq!(read(&path), Stored::default(), "no file: off");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enum_names_are_their_settings_names() {
        assert_eq!(name_of(&crate::look::Theme::Daylight), "daylight");
        assert_eq!(name_of(&crate::look::Accent::Theme), "theme");
    }
}

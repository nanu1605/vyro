//! Remote companion (P26) in the app: the owner's choices and the paired devices in
//! `remote.json` (local app data, never roamed), the server's life, and the Settings commands.
//! The server itself is `vyro_core::remote`; it runs here, as the signed-in user, never in the
//! LocalSystem service.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use vyro_core::remote::auth::{Auth, Stored, STORED_VERSION};
use vyro_core::remote::{
    firewall, net, qr, FirewallState, Hooks, RemoteBrief, RemoteInfo, RemoteServer, RemoteSettings,
    RemoteStatus, ServerConfig, DAY_CHOICES, FILE, MIN_PORT,
};
use vyro_core::{files, lock, unix_now, SensorSnapshot};

use crate::tray::SettingsStore;

/// Emitted with a [`RemoteBrief`] when the companion starts or stops, a live view opens or
/// closes, or the paired devices change.
pub const REMOTE_EVENT: &str = "remote-status";

pub struct Remote {
    path: Option<PathBuf>,
    settings: Mutex<RemoteSettings>,
    auth: Arc<Mutex<Auth>>,
    server: Mutex<Option<RemoteServer>>,
    /// Why it is not running although turned on (the port is taken, ...).
    error: Mutex<Option<String>>,
    watching: AtomicU32,
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

fn hooks(app: &AppHandle) -> Hooks {
    let (files, about, saved, watched) = (app.clone(), app.clone(), app.clone(), app.clone());
    Hooks {
        asset: Box::new(move |path| asset(&files, path)),
        info: Box::new(move || {
            let s = about
                .try_state::<SettingsStore>()
                .map(|s| s.get())
                .unwrap_or_default();
            RemoteInfo {
                pc: net::pc_name(),
                version: about.package_info().version.to_string(),
                theme: name_of(&s.theme),
                accent: name_of(&s.accent),
                unit: s.temperature_unit,
                ..RemoteInfo::default()
            }
        }),
        saved: Box::new(move || {
            if let Some(r) = saved.try_state::<Remote>() {
                r.save();
            }
            emit(&saved);
        }),
        watching: Box::new(move |n| {
            if let Some(r) = watched.try_state::<Remote>() {
                r.watching.store(n, Ordering::SeqCst);
            }
            // A phone watching keeps the readings at the window's rate.
            if let Some(engine) = watched.try_state::<crate::engine::Engine>() {
                engine.set_remote(n > 0);
            }
            emit(&watched);
        }),
    }
}

impl Remote {
    pub fn load(app: &AppHandle) -> Self {
        let path = app.path().app_local_data_dir().ok().map(|d| d.join(FILE));
        let stored = path.as_deref().map(read).unwrap_or_default();
        Self {
            path,
            settings: Mutex::new(stored.settings.sanitized()),
            auth: Arc::new(Mutex::new(Auth::new(stored.devices, unix_now()))),
            server: Mutex::new(None),
            error: Mutex::new(None),
            watching: AtomicU32::new(0),
        }
    }

    /// At launch: serves again if the owner left it on.
    pub fn resume(&self, app: &AppHandle) {
        if lock(&self.settings).enabled {
            self.start(app);
        }
    }

    pub fn running(&self) -> bool {
        lock(&self.server).is_some()
    }

    pub fn brief(&self) -> RemoteBrief {
        RemoteBrief {
            running: self.running(),
            watching: self.watching.load(Ordering::SeqCst),
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let stored = Stored {
            version: STORED_VERSION,
            settings: *lock(&self.settings),
            devices: lock(&self.auth).stored().to_vec(),
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
        self.save();
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
        RemoteStatus {
            settings,
            running,
            error: lock(&self.error).clone(),
            urls,
            devices,
            watching: self.watching.load(Ordering::SeqCst),
            pairing,
            firewall,
        }
    }
}

/// Feeds a reading to the live views, while the companion is on.
pub fn publish(app: &AppHandle, snap: &SensorSnapshot) {
    let Some(r) = app.try_state::<Remote>() else {
        return;
    };
    let server = lock(&r.server);
    if let Some(s) = server.as_ref() {
        let unit = app
            .try_state::<SettingsStore>()
            .map(|s| s.unit())
            .unwrap_or_default();
        s.publish(snap, unit);
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
    change(app, move |_, r| {
        r.sign_out(id.as_deref());
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn get_remote_brief(remote: tauri::State<'_, Remote>) -> RemoteBrief {
    remote.brief()
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

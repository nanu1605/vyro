//! Updates (P11): release builds carry the Tauri updater's public key and look for a newer
//! signed MSI in the repository's latest GitHub release (`latest.json`). Other builds (CI
//! artifacts, local builds) have no updater configured and never check. A check fetches only
//! that file; installing runs the MSI (`msiexec /passive`, one UAC prompt), which replaces app
//! and service and starts Vyro again.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use vyro_core::profiles::NotifyPolicy;
use vyro_core::{lock, unix_now};

use crate::tray::SettingsStore;

/// Carries the new [`UpdateStatus`] after every check.
pub const EVENT: &str = "update-status";
/// First automatic check after start (the network may still be coming up after sign-in).
const FIRST_CHECK: Duration = Duration::from_secs(60);
/// Between automatic checks, in seconds by the clock.
const DAY: i64 = 24 * 3600;
/// How often the checker looks at the clock (2.0.3). It waited 24 hours at a time, and Windows
/// does not count the time it sleeps in a wait: a PC asleep at night looked every few days. A
/// check that failed (no network yet) waited a day too.
const TICK: Duration = Duration::from_secs(3600);
/// How long the update server has to answer a check (2.0.3; not the download, see below). A
/// request left hanging (one under way as the PC goes to sleep can) would hold every later check
/// until Vyro restarted, and "Check now" with it.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the installer's download may go without a byte (2.0.4). It has no overall limit (a
/// slow connection takes its time), and one that stalled, the network gone or the PC asleep
/// mid-download, never ended: Settings stayed "installing", Install and "Check now" held, until
/// Vyro restarted. Now it fails, and says so.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Whether an automatic check is due: none has succeeded yet, the last one was a day ago, or the
/// clock has gone back past it.
fn due(last_check: Option<i64>, now: i64) -> bool {
    last_check.is_none_or(|t| !(0..DAY).contains(&(now - t)))
}

/// Whether this build has an updater: the release workflow adds `plugins.updater` with the
/// public key to the configuration.
pub fn configured(config: &tauri::Config) -> bool {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: Option<String>,
    /// Unix seconds.
    pub date: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// This build can update itself (a release build).
    pub configured: bool,
    pub current: String,
    pub checking: bool,
    pub available: Option<AvailableUpdate>,
    /// Unix seconds of the last completed check.
    pub last_check: Option<i64>,
    pub error: Option<String>,
    pub installing: bool,
}

#[derive(Default)]
pub struct Updates {
    status: Mutex<UpdateStatus>,
    pending: Mutex<Option<Update>>,
    /// Version a notification was shown for (once per version and run).
    notified: Mutex<Option<String>>,
}

impl Updates {
    pub fn new(configured: bool, current: String) -> Self {
        Self {
            status: Mutex::new(UpdateStatus {
                configured,
                current,
                ..UpdateStatus::default()
            }),
            ..Self::default()
        }
    }

    pub(crate) fn status(&self) -> UpdateStatus {
        lock(&self.status).clone()
    }

    fn change(&self, app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) -> UpdateStatus {
        let status = {
            let mut s = lock(&self.status);
            f(&mut s);
            s.clone()
        };
        if let Err(e) = app.emit(EVENT, &status) {
            log::debug!("emit {EVENT}: {e}");
        }
        status
    }
}

/// Looks for an update; `auto` checks notify when one is found.
pub(crate) async fn check(app: &AppHandle, auto: bool) -> UpdateStatus {
    let state = app.state::<Updates>();
    if !state.status().configured {
        return state.status();
    }
    state.change(app, |s| {
        s.checking = true;
        s.error = None;
    });
    // The installer ends this process at once (Windows), without the app's own exit: what
    // would outlast it ends first, then the plugin's own cleanup, which this hook replaces.
    let handle = app.clone();
    let updater = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        // Each read, the download's too: the `Update` a check finds keeps this client.
        .configure_client(|c| c.read_timeout(STALL_TIMEOUT))
        .on_before_exit(move || {
            crate::wind_down(&handle, "Vyro is installing an update");
            handle.cleanup_before_exit();
        })
        .build();
    let result = match updater {
        Ok(u) => u.check().await.map_err(|e| describe(&e)),
        Err(e) => Err(describe(&e)),
    };
    let available = match &result {
        Ok(Some(u)) => Some(AvailableUpdate {
            version: u.version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
            date: u.date.map(|d| d.unix_timestamp()),
        }),
        _ => None,
    };
    let error = result.as_ref().err().cloned();
    if let Some(e) = &error {
        log::warn!("update check: {e}");
    }
    if let Ok(found) = result {
        *lock(&state.pending) = found;
    }
    let status = state.change(app, |s| {
        s.checking = false;
        s.error = error;
        // A failed check says nothing about whether Vyro is up to date.
        if s.error.is_none() {
            s.last_check = Some(unix_now());
            s.available = available.clone();
        }
    });
    if let (true, Some(a)) = (auto, &available) {
        notify(app, &state, &a.version);
    }
    status
}

/// The updater's errors in words a user can act on.
fn describe(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match e {
        // The release file answered with an error status: no release there yet, or a private
        // repository (its downloads need a sign-in).
        Error::ReleaseNotFound => "no release information found on the update server".into(),
        Error::Reqwest(_) | Error::Network(_) => {
            "could not reach GitHub; check the internet connection".into()
        }
        e => e.to_string(),
    }
}

fn notify(app: &AppHandle, state: &Updates, version: &str) {
    {
        let mut notified = lock(&state.notified);
        if notified.as_deref() == Some(version) {
            return;
        }
        *notified = Some(version.to_owned());
    }
    let policy = app
        .try_state::<SettingsStore>()
        .map(|s| s.get().profile.spec().notifications)
        .unwrap_or(NotifyPolicy::All);
    if !matches!(policy, NotifyPolicy::All | NotifyPolicy::Quiet) {
        return;
    }
    let shown = app
        .notification()
        .builder()
        .title(format!("Vyro {version} is available"))
        .body("Open Vyro → Settings → Updates to install it.")
        .show();
    if let Err(e) = shown {
        log::warn!("update notification failed: {e}");
    }
}

/// Checks after start and then daily by the clock, while the setting allows it.
pub fn watch(app: &AppHandle) {
    if !app.state::<Updates>().status().configured {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("vyro-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            loop {
                let enabled = app
                    .try_state::<SettingsStore>()
                    .is_some_and(|s| s.get().check_updates);
                let last = app.state::<Updates>().status().last_check;
                if enabled && due(last, unix_now()) {
                    tauri::async_runtime::block_on(check(&app, true));
                }
                std::thread::sleep(TICK);
            }
        });
    if let Err(e) = spawned {
        log::warn!("update checks: {e}");
    }
}

#[tauri::command]
pub fn get_update_status(state: State<'_, Updates>) -> UpdateStatus {
    state.status()
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateStatus, String> {
    Ok(check(&app, false).await)
}

/// Downloads, verifies and runs the installer. On success the installer takes over and Vyro
/// exits (the MSI starts it again).
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Updates>();
    let update = lock(&state.pending)
        .clone()
        .ok_or("no update to install; check again")?;
    state.change(&app, |s| {
        s.installing = true;
        s.error = None;
    });
    log::info!("installing Vyro {}", update.version);
    let result = update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| describe(&e));
    // Only reached when it failed (a started installer ends this process).
    if let Err(e) = &result {
        log::warn!("update install: {e}");
        state.change(&app, |s| {
            s.installing = false;
            s.error = Some(e.clone());
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(plugins: serde_json::Value) -> tauri::Config {
        let mut c: tauri::Config = serde_json::from_value(serde_json::json!({
            "identifier": "app.vyro.test"
        }))
        .unwrap();
        c.plugins = serde_json::from_value(plugins).unwrap();
        c
    }

    /// Daily by the clock however long Windows slept, and a failed check (none succeeded since)
    /// again within the hour.
    #[test]
    fn checks_are_due_daily_by_the_clock() {
        let now = 1_800_000_000;
        assert!(due(None, now));
        assert!(!due(Some(now - 3600), now));
        assert!(!due(Some(now - DAY + 1), now));
        assert!(due(Some(now - DAY), now));
        assert!(due(Some(now - 3 * DAY), now));
        // The clock set back since the last check: not waited out.
        assert!(due(Some(now + 3600), now));
    }

    #[test]
    fn only_builds_with_a_public_key_update() {
        assert!(!configured(&config(serde_json::json!({}))));
        assert!(!configured(&config(
            serde_json::json!({"updater": {"pubkey": " "}})
        )));
        assert!(configured(&config(serde_json::json!({
            "updater": {"pubkey": "dW50cnVzdGVk", "endpoints": ["https://example.com/latest.json"]}
        }))));
    }
}

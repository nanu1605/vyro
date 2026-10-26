//! What's new (1.5): on the first start after Vyro was updated, the window shows what the new
//! version brings (the notes are the page's, `src/whatsnew.ts`). The version last shown is kept
//! in `version.json` beside the settings, apart from them: the window saves its settings whole
//! and would write over it.
//!
//! Vyro before 1.5 kept no version, so an update from it is told from a new install by what an
//! earlier Vyro leaves behind: its settings files, or a WebView or log folder from before this
//! start.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;
use vyro_core::profiles::NotifyPolicy;
use vyro_core::{files, lock};

use crate::tray::SettingsStore;

const FILE: &str = "version.json";
/// Made before this start: older than this (the WebView and the log are made as Vyro starts).
const EARLIER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    /// The version whose notes were last shown (or that was installed new).
    seen: String,
}

/// What to do at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Show the notes of every version after `from`; `None`: updated from a Vyro before 1.5,
    /// whose version is not known, so every version's notes.
    Show { from: Option<String> },
    /// Keep this version without showing anything (a new install, or an older version put back).
    Record,
    /// Shown already.
    Nothing,
}

/// `major.minor.patch`, with any pre-release or build part left out.
pub fn parse(v: &str) -> Option<(u32, u32, u32)> {
    let core = v.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

/// `seen`: the stored version, if any; `earlier`: an earlier Vyro was on this PC.
pub fn decide(seen: Option<&str>, current: &str, earlier: bool) -> Decision {
    let now = parse(current);
    match seen.and_then(|s| parse(s).map(|v| (s, v))) {
        Some((s, v)) => match now {
            Some(c) if v < c => Decision::Show {
                from: Some(s.to_owned()),
            },
            Some(c) if v == c => Decision::Nothing,
            _ => Decision::Record,
        },
        None if earlier => Decision::Show { from: None },
        None => Decision::Record,
    }
}

/// Whether any of `paths`, or anything in `dirs`, was made at least [`EARLIER`] before `now`.
fn made_before(paths: &[PathBuf], dirs: &[PathBuf], now: SystemTime) -> bool {
    let old = |p: &Path| {
        std::fs::metadata(p)
            .and_then(|m| m.created().or_else(|_| m.modified()))
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age >= EARLIER))
    };
    paths.iter().any(|p| old(p))
        || dirs.iter().any(|d| {
            std::fs::read_dir(d).is_ok_and(|entries| entries.flatten().any(|e| old(&e.path())))
        })
}

/// An earlier Vyro was here: run before anything of this start writes its files.
fn earlier_vyro(app: &AppHandle) -> bool {
    let p = app.path();
    // Settings the window or the Remote card saved: there before this start whatever their age.
    let saved: Vec<PathBuf> = [
        p.app_config_dir().map(|d| {
            vec![
                d.join("settings.json"),
                d.join("dashboard.json"),
                d.join("widgets.json"),
            ]
        }),
        p.app_local_data_dir()
            .map(|d| vec![d.join(vyro_core::remote::FILE)]),
        p.app_data_dir()
            .map(|d| vec![d.join(vyro_core::engine::ALERTS_FILE)]),
    ]
    .into_iter()
    .flatten()
    .flatten()
    .collect();
    if saved.iter().any(|f| f.exists()) {
        return true;
    }
    // Made by every start, this one too: only older ones count.
    let webview: Vec<PathBuf> = p
        .app_local_data_dir()
        .map(|d| d.join("EBWebView"))
        .into_iter()
        .collect();
    let logs: Vec<PathBuf> = p.app_log_dir().into_iter().collect();
    made_before(&webview, &logs, SystemTime::now())
}

/// The answer to the window: the notes to show are those after `from` up to `current`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNewAnswer {
    pub current: String,
    pub from: Option<String>,
}

pub struct WhatsNew {
    path: Option<PathBuf>,
    current: String,
    /// `Some(from)` while the notes wait to be shown.
    pending: Mutex<Option<Option<String>>>,
}

impl WhatsNew {
    /// Decides, first thing at start; a new install's version is kept at once.
    pub fn load(app: &AppHandle, current: &str) -> Self {
        let path = app.path().app_config_dir().ok().map(|d| d.join(FILE));
        let seen = path
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Stored>(&b).ok())
            .map(|s| s.seen)
            .filter(|s| !s.is_empty());
        // Only when nothing says which version came before: the folders' look is not needed.
        let earlier = seen.is_none() && earlier_vyro(app);
        let decision = decide(seen.as_deref(), current, earlier);
        log::info!(
            "what's new: {decision:?} (seen {}, this {current})",
            seen.as_deref().unwrap_or("none")
        );
        let this = Self {
            path,
            current: current.to_owned(),
            pending: Mutex::new(None),
        };
        match decision {
            Decision::Show { from } => *lock(&this.pending) = Some(from),
            Decision::Record => this.record(),
            Decision::Nothing => {}
        }
        this
    }

    fn record(&self) {
        let Some(path) = &self.path else { return };
        let stored = Stored {
            seen: self.current.clone(),
        };
        if let Err(e) = files::write_json(path, &stored) {
            log::warn!("what's new: cannot write {}: {e}", path.display());
        }
    }

    pub fn pending(&self) -> Option<WhatsNewAnswer> {
        lock(&self.pending).clone().map(|from| WhatsNewAnswer {
            current: self.current.clone(),
            from,
        })
    }

    pub fn seen(&self) {
        *lock(&self.pending) = None;
        self.record();
    }
}

/// Started in the tray after an update: a notification says the window has something to show.
pub fn announce(app: &AppHandle, minimized: bool) {
    let Some(state) = app.try_state::<WhatsNew>() else {
        return;
    };
    if !minimized || state.pending().is_none() {
        return;
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
        .title(format!("Vyro was updated to {}", state.current))
        .body("Open Vyro to see what's new.")
        .show();
    if let Err(e) = shown {
        log::warn!("what's new notification failed: {e}");
    }
}

/// The notes waiting to be shown, if any.
#[tauri::command]
pub fn get_whats_new(state: State<'_, WhatsNew>) -> Option<WhatsNewAnswer> {
    state.pending()
}

/// The notes were shown: not again for this version.
#[tauri::command]
pub fn whats_new_seen(state: State<'_, WhatsNew>) {
    state.seen();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_versions() {
        assert_eq!(parse("1.5.0"), Some((1, 5, 0)));
        assert_eq!(parse(" 1.10.2 "), Some((1, 10, 2)));
        assert_eq!(parse("1.5"), Some((1, 5, 0)));
        assert_eq!(parse("1.5.0-beta.1"), Some((1, 5, 0)));
        assert_eq!(parse(""), None);
        assert_eq!(parse("one"), None);
        assert_eq!(parse("1.5.0.1"), None);
    }

    #[test]
    fn shows_once_after_an_update() {
        let show = |from: &str| Decision::Show {
            from: Some(from.into()),
        };
        assert_eq!(decide(Some("1.5.0"), "1.6.0", false), show("1.5.0"));
        // Several versions at once: the window shows each one after `from`.
        assert_eq!(decide(Some("1.5.0"), "1.7.0", true), show("1.5.0"));
        assert_eq!(decide(Some("1.5.0"), "1.5.1", false), show("1.5.0"));
        assert_eq!(decide(Some("1.5.0"), "1.5.0", true), Decision::Nothing);
        // An older version put back: kept, so the next update shows from there.
        assert_eq!(decide(Some("1.7.0"), "1.6.0", true), Decision::Record);
    }

    #[test]
    fn tells_an_update_from_before_1_5_from_a_new_install() {
        assert_eq!(decide(None, "1.5.0", true), Decision::Show { from: None });
        assert_eq!(decide(None, "1.5.0", false), Decision::Record);
        // A damaged file says nothing; the folders decide.
        assert_eq!(
            decide(Some("?"), "1.5.0", true),
            Decision::Show { from: None }
        );
        assert_eq!(decide(Some("?"), "1.5.0", false), Decision::Record);
    }

    #[test]
    fn only_folders_from_before_this_start_count() {
        let dir = std::env::temp_dir().join(format!("vyro-whatsnew-{}", std::process::id()));
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("Vyro.log"), "started").unwrap();
        let webview = dir.join("EBWebView");
        std::fs::create_dir_all(&webview).unwrap();
        let now = SystemTime::now();
        // Made just now (this start): not an earlier Vyro.
        let (webview, logs) = (vec![webview], vec![logs]);
        assert!(!made_before(&webview, &logs, now));
        // Seen from an hour later, the same folders are from before.
        let later = now + Duration::from_secs(3600);
        assert!(made_before(&webview, &[], later));
        assert!(made_before(&[], &logs, later));
        // Missing ones never count.
        assert!(!made_before(
            &[dir.join("none")],
            &[dir.join("nowhere")],
            later
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

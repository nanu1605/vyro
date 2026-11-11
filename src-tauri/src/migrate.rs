//! What Vyro kept, carried over to Clokrix (2.2; see `clokrix_core::rename`): once Vyro is gone,
//! its folders, "Start with Windows" and notification registration move to the new names. Each
//! step stands alone: a failure is noted for the log and the start goes on (worst case, that one
//! thing starts fresh).

use std::path::Path;

use clokrix_core::rename::{self, Moved};
use tauri::{AppHandle, Manager};

/// First thing at start, before Tauri opens the window (WebView2 keeps its data, the page's saved
/// layout and looks among it, in the local folder): the app's folders under `%APPDATA%` and
/// `%LOCALAPPDATA%` (settings, layouts, widgets, alert rules, paired devices, logs), "Start with
/// Windows" and Vyro's notification registration. What happened, for the log.
pub fn before_start(identifier: &str) -> Vec<String> {
    let mut notes = Vec::new();
    #[cfg(windows)]
    {
        if rename::vyro_installed() {
            return notes;
        }
        for var in ["APPDATA", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var).map(std::path::PathBuf::from) {
                carry(
                    &mut notes,
                    &base.join(rename::OLD_IDENTIFIER),
                    &base.join(identifier),
                );
            }
        }
        match clokrix_core::autostart::carry_over() {
            Ok(true) => notes.push("\"Start with Windows\" carried over from Vyro".into()),
            Ok(false) => {}
            Err(e) => notes.push(format!("\"Start with Windows\" not carried over: {e}")),
        }
        match clokrix_core::alerts::unregister_toast_app(rename::OLD_IDENTIFIER) {
            Ok(true) => notes.push("removed Vyro's notification registration".into()),
            Ok(false) => {}
            Err(e) => notes.push(format!("Vyro's notification registration stays: {e}")),
        }
    }
    #[cfg(not(windows))]
    let _ = identifier;
    notes
}

/// Once Tauri knows the user's folders: share cards (Pictures) and reports (Documents).
pub fn after_start(app: &AppHandle) -> Vec<String> {
    let mut notes = Vec::new();
    if rename::vyro_installed() {
        return notes;
    }
    let paths = app.path();
    if let Ok(pictures) = paths.picture_dir() {
        let (old, new) = (crate::share::LEGACY_FOLDER, crate::share::FOLDER);
        carry(&mut notes, &pictures.join(old), &pictures.join(new));
    }
    if let Ok(documents) = paths.document_dir() {
        let (old, new) = (crate::diag::LEGACY_FOLDER, crate::diag::FOLDER);
        carry(&mut notes, &documents.join(old), &documents.join(new));
    }
    notes
}

fn carry(notes: &mut Vec<String>, old: &Path, new: &Path) {
    let (from, to) = (old.display(), new.display());
    match rename::move_folder(old, new) {
        Ok(Moved::Nothing) => {}
        Ok(Moved::Whole) => notes.push(format!("moved {from} to {to}")),
        Ok(Moved::Entries { left }) if left.is_empty() => {
            notes.push(format!("moved {from} to {to}, entry by entry"));
        }
        Ok(Moved::Entries { left }) => notes.push(format!(
            "moved {from} to {to}, entry by entry; in use and left behind: {}",
            left.join(", ")
        )),
        Err(e) => notes.push(format!("cannot move {from}: {e}")),
    }
}

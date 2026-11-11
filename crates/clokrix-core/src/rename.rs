//! What an installed Vyro kept, carried over after the rename to Clokrix (2.2). The Clokrix MSI
//! replaces Vyro as an ordinary upgrade (the same upgrade code), but Vyro's folders, "Start with
//! Windows" entry and notification registration keep their old names until the new service and
//! app move them: once, and only when Vyro itself is gone (no longer in Windows' list of
//! installed programs), so a bare `clokrix.exe` run beside an installed Vyro takes nothing from it.

use std::io;
use std::path::Path;
use std::time::Duration;

/// The product's name before 2.2, as its folders under `%ProgramData%`, Pictures and Documents,
/// and its "Start with Windows" entry, were named.
pub const OLD_NAME: &str = "Vyro";
/// The app's identifier before 2.2: its folders under `%APPDATA%` and `%LOCALAPPDATA%` (with
/// WebView2's data) and its notification registration.
pub const OLD_IDENTIFIER: &str = "app.vyro.monitor";

/// A folder in use (a file still open by a process that is exiting) is tried this many more
/// times, this far apart.
const RETRIES: u32 = 4;
const RETRY_GAP: Duration = Duration::from_millis(250);

/// Where Windows lists installed programs (the MSI's entry, by product code).
#[cfg(windows)]
const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

/// Whether Vyro is still installed: Windows lists a program named and published "Vyro". Nothing is
/// carried over then. The upgrade removes that entry before Clokrix first starts, whereas Vyro's
/// service can linger for a while, marked for deletion.
pub fn vyro_installed() -> bool {
    #[cfg(windows)]
    {
        use crate::winutil::{Key, HKEY_LOCAL_MACHINE};
        let Some(list) = Key::open(HKEY_LOCAL_MACHINE, UNINSTALL) else {
            return false;
        };
        list.subkeys()
            .iter()
            .filter_map(|k| list.subkey(k))
            .any(|k| {
                k.string("DisplayName").as_deref() == Some(OLD_NAME)
                    && k.string("Publisher").as_deref() == Some(OLD_NAME)
            })
    }
    #[cfg(not(windows))]
    false
}

/// What [`move_folder`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moved {
    /// Nothing to move: no old folder (a link is never followed), or the new one exists.
    Nothing,
    /// The folder was renamed in one step.
    Whole,
    /// The folder itself could not be renamed, so its entries were moved one by one; these
    /// stayed behind (in use).
    Entries { left: Vec<String> },
}

/// Renames `old` to `new` in one step when `new` does not exist and `old` is a real folder (not
/// a link). `Ok(false)`: nothing to move. A folder in use is tried again for about a second.
pub fn rename_folder(old: &Path, new: &Path) -> io::Result<bool> {
    rename_with(old, new, &|a, b| std::fs::rename(a, b), RETRY_GAP)
}

/// [`rename_folder`], else entry by entry: each file or folder in `old` that can move goes into
/// `new`, and what cannot stays, named in the result. Each entry moves whole, so a folder inside
/// (WebView2's data) is never split.
pub fn move_folder(old: &Path, new: &Path) -> io::Result<Moved> {
    move_with(old, new, &|a, b| std::fs::rename(a, b), RETRY_GAP)
}

type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

/// Whether `old` can move to `new`: `old` is a folder itself (not a link to one) and nothing is
/// at `new`.
fn movable(old: &Path, new: &Path) -> io::Result<bool> {
    if std::fs::symlink_metadata(new).is_ok() {
        return Ok(false);
    }
    match std::fs::symlink_metadata(old) {
        Ok(meta) => Ok(meta.is_dir() && !is_link(&meta)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// A link or any other reparse point (a junction, a mount point): never followed.
fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn rename_with(old: &Path, new: &Path, rename: Rename, gap: Duration) -> io::Result<bool> {
    if !movable(old, new)? {
        return Ok(false);
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tries = 0;
    loop {
        match rename(old, new) {
            Ok(()) => return Ok(true),
            Err(e) if tries < RETRIES && e.kind() != io::ErrorKind::NotFound => {
                tries += 1;
                std::thread::sleep(gap);
            }
            Err(e) => return Err(e),
        }
    }
}

fn move_with(old: &Path, new: &Path, rename: Rename, gap: Duration) -> io::Result<Moved> {
    match rename_with(old, new, rename, gap) {
        Ok(true) => return Ok(Moved::Whole),
        Ok(false) => return Ok(Moved::Nothing),
        Err(_) => {}
    }
    std::fs::create_dir(new)?;
    let mut left = Vec::new();
    for entry in std::fs::read_dir(old)? {
        let entry = entry?;
        let name = entry.file_name();
        if rename(&entry.path(), &new.join(&name)).is_err() {
            left.push(name.to_string_lossy().into_owned());
        }
    }
    left.sort();
    if left.is_empty() {
        let _ = std::fs::remove_dir(old);
    }
    Ok(Moved::Entries { left })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("clokrix-rename-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Vyro's folder as an install leaves it: settings, and WebView2's data in a folder.
    fn old_folder(base: &Path) -> PathBuf {
        let old = base.join(OLD_IDENTIFIER);
        std::fs::create_dir_all(old.join("EBWebView/Default")).unwrap();
        std::fs::write(old.join("settings.json"), "{}").unwrap();
        std::fs::write(old.join("EBWebView/Default/Preferences"), "x").unwrap();
        old
    }

    #[test]
    fn renames_the_folder_when_the_new_one_is_missing() {
        let base = temp("whole");
        let old = old_folder(&base);
        let new = base.join("app.clokrix.monitor");
        assert_eq!(move_folder(&old, &new).unwrap(), Moved::Whole);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(new.join("settings.json")).unwrap(),
            "{}"
        );
        assert!(new.join("EBWebView/Default/Preferences").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn leaves_both_alone_when_the_new_one_exists() {
        let base = temp("both");
        let old = old_folder(&base);
        let new = base.join("app.clokrix.monitor");
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(move_folder(&old, &new).unwrap(), Moved::Nothing);
        assert!(old.join("settings.json").exists());
        assert!(!new.join("settings.json").exists());
        assert!(!rename_folder(&old, &new).unwrap());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn nothing_to_move_without_an_old_folder() {
        let base = temp("none");
        let new = base.join("Clokrix");
        assert_eq!(
            move_folder(&base.join(OLD_NAME), &new).unwrap(),
            Moved::Nothing
        );
        assert!(!new.exists());
        // A file of the old name is not a folder.
        std::fs::write(base.join(OLD_NAME), "x").unwrap();
        assert!(!rename_folder(&base.join(OLD_NAME), &new).unwrap());
        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_followed() {
        let base = temp("link");
        let target = base.join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("file"), "x").unwrap();
        std::os::unix::fs::symlink(&target, base.join(OLD_NAME)).unwrap();
        let new = base.join("Clokrix");
        assert_eq!(
            move_folder(&base.join(OLD_NAME), &new).unwrap(),
            Moved::Nothing
        );
        assert!(!new.exists());
        assert!(target.join("file").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn moves_entry_by_entry_when_the_folder_is_in_use() {
        let base = temp("entries");
        let old = old_folder(&base);
        std::fs::write(old.join("in-use.log"), "x").unwrap();
        let new = base.join("app.clokrix.monitor");
        // The folder itself and one file inside are held open; everything else moves.
        let busy = |a: &Path, b: &Path| {
            if a == old || a.ends_with("in-use.log") {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                std::fs::rename(a, b)
            }
        };
        let moved = move_with(&old, &new, &busy, Duration::ZERO).unwrap();
        assert_eq!(
            moved,
            Moved::Entries {
                left: vec!["in-use.log".into()]
            }
        );
        assert!(new.join("settings.json").exists());
        assert!(new.join("EBWebView/Default/Preferences").exists());
        assert!(old.join("in-use.log").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn everything_moved_entry_by_entry_removes_the_old_folder() {
        let base = temp("entries-all");
        let old = old_folder(&base);
        let new = base.join("app.clokrix.monitor");
        let whole_busy = |a: &Path, b: &Path| {
            if a == old {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                std::fs::rename(a, b)
            }
        };
        let moved = move_with(&old, &new, &whole_busy, Duration::ZERO).unwrap();
        assert_eq!(moved, Moved::Entries { left: vec![] });
        assert!(!old.exists());
        assert!(new.join("settings.json").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_folder_still_in_use_is_reported_and_left_in_place() {
        let base = temp("busy");
        let old = old_folder(&base);
        let new = base.join("Clokrix");
        let tries = std::cell::Cell::new(0);
        let busy = |_: &Path, _: &Path| {
            tries.set(tries.get() + 1);
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        };
        assert!(rename_with(&old, &new, &busy, Duration::ZERO).is_err());
        assert_eq!(tries.get(), RETRIES + 1);
        assert!(old.join("settings.json").exists());
        assert!(!new.exists());
        let _ = std::fs::remove_dir_all(base);
    }
}

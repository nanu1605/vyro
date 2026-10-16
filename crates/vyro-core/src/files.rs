//! The small state files Vyro keeps (settings, alert rules, fan settings, layouts, paired
//! devices, the BIOS fan state): one rule for all of them (P27).
//!
//! - [`write_atomic`] writes a temporary file, flushes it to disk and renames it over the old
//!   one, so a crash or power cut leaves the old file or the new one, never half of one.
//! - [`read_json`] reads one back. A file that is there but does not parse is set aside as
//!   `<name>.bad.<ext>` (only the newest is kept) and reported, so the caller's defaults never
//!   overwrite what the user had and the file can still be looked at.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// `path` with `.tmp` added: where [`write_atomic`] writes first.
fn tmp_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".tmp");
    PathBuf::from(name)
}

/// Where a damaged `path` goes: `settings.json` → `settings.bad.json`, `history.sqlite` →
/// `history.bad.sqlite`.
pub fn bad_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = match path.extension() {
        Some(ext) => format!("{stem}.bad.{}", ext.to_string_lossy()),
        None => format!("{stem}.bad"),
    };
    path.with_file_name(name)
}

/// Writes `bytes` to `path` through a temporary file that is flushed to disk before it
/// replaces `path`. Creates the folder if needed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = tmp_path(path);
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// [`write_atomic`] of `value` as pretty JSON.
pub fn write_json(path: &Path, value: &impl serde::Serialize) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_atomic(path, &json)
}

/// Moves a file that could not be read to [`bad_path`], replacing an older one. Returns where
/// it went, `None` if it could not be moved (it is then left where it was).
pub fn set_aside(path: &Path) -> Option<PathBuf> {
    let bad = bad_path(path);
    let _ = std::fs::remove_file(&bad);
    match std::fs::rename(path, &bad) {
        Ok(()) => Some(bad),
        Err(e) => {
            log::warn!("cannot set {} aside: {e}", path.display());
            None
        }
    }
}

/// Why a state file was not used.
#[derive(Debug)]
pub enum ReadError {
    /// It could not be read (it is left as it is: it may be fine, e.g. locked for a moment).
    Io(io::Error),
    /// It is damaged or from a version that wrote something else; it was set aside.
    Invalid {
        error: String,
        aside: Option<PathBuf>,
    },
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Invalid {
                error,
                aside: Some(p),
            } => write!(f, "{error} (kept as {})", p.display()),
            Self::Invalid { error, aside: None } => write!(f, "{error}"),
        }
    }
}

/// Reads `path` as JSON. `Ok(None)`: there is no such file (first run). A file that does not
/// parse, or that `valid` refuses, is set aside.
pub fn read_json<T: DeserializeOwned>(
    path: &Path,
    valid: impl FnOnce(&T) -> Result<(), String>,
) -> Result<Option<T>, ReadError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ReadError::Io(e)),
    };
    let parsed = serde_json::from_slice::<T>(&bytes)
        .map_err(|e| e.to_string())
        .and_then(|v| valid(&v).map(|()| v));
    match parsed {
        Ok(v) => Ok(Some(v)),
        Err(error) => Err(ReadError::Invalid {
            error,
            aside: set_aside(path),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_bad_copy_after_the_file() {
        let p = Path::new("/data/settings.json");
        assert_eq!(bad_path(p), Path::new("/data/settings.bad.json"));
        assert_eq!(
            bad_path(Path::new("history.sqlite")),
            Path::new("history.bad.sqlite")
        );
        assert_eq!(bad_path(Path::new("/x/notes")), Path::new("/x/notes.bad"));
        assert_eq!(tmp_path(p), Path::new("/data/settings.json.tmp"));
    }

    #[test]
    fn writes_whole_files_and_sets_damaged_ones_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("rules.json");
        let none: Result<Option<Vec<u32>>, _> = read_json(&path, |_| Ok(()));
        assert!(matches!(none, Ok(None)));

        write_json(&path, &vec![1u32, 2, 3]).unwrap();
        assert!(!tmp_path(&path).exists());
        let back: Vec<u32> = read_json(&path, |_| Ok(())).unwrap().unwrap();
        assert_eq!(back, [1, 2, 3]);
        write_atomic(&path, b"[4]").unwrap();
        assert_eq!(
            read_json::<Vec<u32>>(&path, |_| Ok(())).unwrap(),
            Some(vec![4])
        );

        // Half a file (as a crash in an older version could leave): set aside, never reused.
        std::fs::write(&path, b"[1, 2").unwrap();
        let err = read_json::<Vec<u32>>(&path, |_| Ok(())).unwrap_err();
        assert!(
            matches!(&err, ReadError::Invalid { aside: Some(_), .. }),
            "{err}"
        );
        assert!(!path.exists());
        let bad = dir.path().join("sub").join("rules.bad.json");
        assert_eq!(std::fs::read(&bad).unwrap(), b"[1, 2");
        assert!(matches!(read_json::<Vec<u32>>(&path, |_| Ok(())), Ok(None)));

        // Parses but is refused: set aside too, replacing the older bad copy.
        write_json(&path, &vec![9u32]).unwrap();
        let err = read_json::<Vec<u32>>(&path, |v| {
            if v.len() > 1 {
                Ok(())
            } else {
                Err("too short".into())
            }
        })
        .unwrap_err();
        assert!(err.to_string().starts_with("too short (kept as "), "{err}");
        assert_eq!(std::fs::read(&bad).unwrap(), b"[\n  9\n]");
    }

    #[test]
    fn a_failed_write_leaves_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        write_atomic(&path, b"old").unwrap();
        // The temporary file's name is taken by a folder: the write fails before the rename.
        std::fs::create_dir(tmp_path(&path)).unwrap();
        assert!(write_atomic(&path, b"new").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
    }
}

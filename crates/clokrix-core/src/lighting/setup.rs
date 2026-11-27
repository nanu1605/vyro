//! OpenRGB set up by Clokrix (2.6.1; the owner, 2026-11-27: "install OpenRGB  with the
//! installer", choosing to download it at set-up). When the owner presses Set up OpenRGB, the
//! app downloads OpenRGB's own official installer from its project's release page, checks it
//! against the SHA-256 pinned here, and runs it with one administrator prompt. Nothing of OpenRGB
//! ships with Clokrix.
//!
//! The installer (WiX; its tables read with msitools when it was pinned) puts OpenRGB in
//! `Program Files\OpenRGB` and registers the "OpenRGB" service: LocalSystem, automatic start,
//! started at install. As a service OpenRGB runs its SDK server on 127.0.0.1:6742 only, with its
//! settings in the install folder, which only an administrator can change. Clokrix's MSI already
//! installs PawnIO, which OpenRGB uses for memory and boards. Uninstalling Clokrix leaves
//! OpenRGB: it is its own program, in Windows' Apps.
//!
//! Without the service (an OpenRGB the owner installed some other way, or the service stopped),
//! the app starts OpenRGB itself, once, as the signed-in user: [`START_ARGS`].

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub use crate::ipc::transport::ServiceState;

/// The release set up: OpenRGB 1.1 (2026-10-16), GPL-2.0, from
/// <https://codeberg.org/OpenRGB/OpenRGB/releases/tag/release_1.1>.
pub const VERSION: &str = "1.1";
pub const URL: &str = "https://codeberg.org/OpenRGB/OpenRGB/releases/download/release_1.1/OpenRGB_1.1_Windows_64_8e16de2.msi";
pub const SHA256: &str = "a4ce48a5e1532750ad4f5248424d42f7f2c710775cee88db070ccdb63c549caa";
/// Its size in bytes.
pub const SIZE: usize = 23_345_664;
pub const FILE: &str = "OpenRGB_1.1_Windows_64_8e16de2.msi";
/// The service the installer registers.
pub const SERVICE: &str = "OpenRGB";
/// msiexec's options after the package: a progress bar only, no restart, and both of the
/// installer's features, the application and its service.
pub const MSI_OPTIONS: &str =
    "/passive /norestart ADDLOCAL=OpenRGBApplication,OpenRGBRegisterService";
/// How the app starts OpenRGB when there is no service: its SDK server on this PC only, in the
/// tray.
pub const START_ARGS: [&str; 4] = ["--server", "--server-host", "127.0.0.1", "--startminimized"];
/// The service finds every device before its server answers: up to a minute or two.
pub const SERVICE_GRACE: Duration = Duration::from_secs(120);
/// OpenRGB started by the app answers within this, or it is not running.
pub const START_GRACE: Duration = Duration::from_secs(45);

/// The download is the pinned installer: its size and SHA-256.
pub fn verify(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != SIZE {
        return Err(format!(
            "OpenRGB's installer is not the one Clokrix knows ({} bytes, not {SIZE}): nothing was \
             installed.",
            bytes.len()
        ));
    }
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if hex != SHA256 {
        return Err(
            "OpenRGB's installer is not the one Clokrix knows (its SHA-256 differs): nothing was \
             installed."
                .into(),
        );
    }
    Ok(())
}

/// OpenRGB's installer, downloaded and checked.
pub fn download() -> Result<Vec<u8>, String> {
    let bytes = crate::edition::https::fetch(URL, SIZE)
        .map_err(|e| format!("OpenRGB's installer did not download: {e}"))?;
    verify(&bytes)?;
    Ok(bytes)
}

/// msiexec's arguments for the package at `msi`.
pub fn msiexec_params(msi: &Path) -> String {
    format!("/i \"{}\" {MSI_OPTIONS}", msi.display())
}

/// How a finished install went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    Done,
    /// Windows wants a restart to finish.
    Restart,
}

/// What msiexec's exit `code` means.
pub fn outcome(code: u32) -> Result<Installed, String> {
    match code {
        0 => Ok(Installed::Done),
        3010 | 1641 => Ok(Installed::Restart),
        1602 => Err("OpenRGB's installer was cancelled.".into()),
        1618 => Err("Another installation is running: try again when it ends.".into()),
        c => Err(format!("OpenRGB's installer ended with code {c}.")),
    }
}

/// OpenRGB on this PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// `OpenRGB.exe` in `Program Files\OpenRGB`.
    pub exe: Option<PathBuf>,
    pub service: ServiceState,
    /// An `OpenRGB.exe` runs (its window, or the service).
    pub open: bool,
}

impl Default for Found {
    fn default() -> Self {
        Self {
            exe: None,
            service: ServiceState::NotInstalled,
            open: false,
        }
    }
}

/// What to do while OpenRGB does not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Not on this PC: the owner can set it up.
    Missing,
    /// Starting: wait for it.
    Wait,
    /// Start this, as [`START_ARGS`] say.
    Start(PathBuf),
    /// On this PC but not answering: the owner's to look at.
    NotRunning,
}

/// What to do when OpenRGB has not answered for `silent_for`, `since_start` after the app
/// started it (if it did; it does so once).
pub fn next(found: &Found, silent_for: Duration, since_start: Option<Duration>) -> Next {
    match found.service {
        ServiceState::Running | ServiceState::Starting if silent_for < SERVICE_GRACE => {
            return Next::Wait;
        }
        ServiceState::Running | ServiceState::Starting => return Next::NotRunning,
        _ => {}
    }
    match (&found.exe, since_start) {
        (_, Some(d)) if d < START_GRACE => Next::Wait,
        (_, Some(_)) => Next::NotRunning,
        // Open without its server: starting another would put a second one in the tray.
        (Some(_), None) if found.open => Next::NotRunning,
        (Some(exe), None) => Next::Start(exe.clone()),
        (None, None) if found.service == ServiceState::NotInstalled => Next::Missing,
        (None, None) => Next::NotRunning,
    }
}

/// OpenRGB on this PC now.
pub fn find() -> Found {
    #[cfg(windows)]
    return win::find();
    #[cfg(not(windows))]
    Found::default()
}

#[cfg(windows)]
pub use win::{install, msiexec, start, Package};

#[cfg(windows)]
mod win {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::process::CommandExt;

    use super::*;
    use crate::elevated::{self, NotRun};

    /// FILE_SHARE_READ: others may read the package while it is held, never change it.
    const SHARE_READ: u32 = 1;
    /// CREATE_NO_WINDOW.
    const NO_WINDOW: u32 = 0x0800_0000;

    pub fn find() -> Found {
        let exe = std::env::var_os("ProgramW6432")
            .or_else(|| std::env::var_os("ProgramFiles"))
            .map(|p| PathBuf::from(p).join("OpenRGB").join("OpenRGB.exe"))
            .filter(|p| p.is_file());
        Found {
            exe,
            service: crate::ipc::transport::state_of(SERVICE),
            open: crate::live::running("OpenRGB.exe"),
        }
    }

    /// `msiexec.exe` in System32 (never one found on the path).
    pub fn msiexec() -> PathBuf {
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        PathBuf::from(windows).join("System32").join("msiexec.exe")
    }

    /// The checked installer in a folder of the app's, held open so that no other program can
    /// change it between its check and the install; removed when dropped (Windows Installer keeps
    /// its own copy).
    pub struct Package {
        pub path: PathBuf,
        file: Option<File>,
    }

    impl Package {
        /// Writes `bytes` (checked with [`verify`]) into `dir`, then opens the file again,
        /// sharing it for reading only, and checks it once more through that handle.
        pub fn write(bytes: &[u8], dir: &Path) -> Result<Self, String> {
            verify(bytes)?;
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
            let path = dir.join(FILE);
            let mut out =
                File::create(&path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            out.write_all(bytes)
                .and_then(|()| out.sync_all())
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            drop(out);
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(SHARE_READ)
                .open(&path)
                .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
            let mut again = Vec::with_capacity(SIZE);
            file.read_to_end(&mut again)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let package = Self {
                path,
                file: Some(file),
            };
            verify(&again)?;
            Ok(package)
        }

        pub fn params(&self) -> String {
            msiexec_params(&self.path)
        }
    }

    impl Drop for Package {
        fn drop(&mut self) {
            drop(self.file.take());
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Runs the package's installer as administrator (one prompt) and waits for it.
    pub fn install(package: &Package) -> Result<Installed, String> {
        let code = elevated::run(&msiexec(), &package.params()).map_err(|e| match e {
            NotRun::Declined => {
                "OpenRGB was not set up: the administrator prompt was declined.".to_owned()
            }
            NotRun::Failed(e) => format!("OpenRGB's installer did not start: {e}"),
        })?;
        outcome(code)
    }

    /// Starts OpenRGB as the signed-in user, its SDK server on this PC only.
    pub fn start(exe: &Path) -> Result<(), String> {
        let mut cmd = std::process::Command::new(exe);
        cmd.args(START_ARGS).creation_flags(NO_WINDOW);
        if let Some(dir) = exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.spawn()
            .map(|_| ())
            .map_err(|e| format!("cannot start {}: {e}", exe.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn takes_only_the_pinned_installer() {
        assert_eq!(SHA256.len(), 64);
        assert!(SHA256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(URL.starts_with("https://codeberg.org/OpenRGB/OpenRGB/releases/download/"));
        assert!(URL.ends_with(FILE) && URL.contains(&format!("release_{VERSION}/")));
        // The right size with other bytes, and the wrong size: both refused.
        let e = verify(&vec![0u8; SIZE]).unwrap_err();
        assert!(e.contains("SHA-256"), "{e}");
        let e = verify(b"MZ").unwrap_err();
        assert!(e.contains("2 bytes"), "{e}");
    }

    #[test]
    fn installs_both_features_without_a_restart() {
        let p = msiexec_params(Path::new(r"C:\Users\a\AppData\Local\x\OpenRGB.msi"));
        assert_eq!(
            p,
            r#"/i "C:\Users\a\AppData\Local\x\OpenRGB.msi" /passive /norestart ADDLOCAL=OpenRGBApplication,OpenRGBRegisterService"#
        );
        assert_eq!(outcome(0), Ok(Installed::Done));
        assert_eq!(outcome(3010), Ok(Installed::Restart));
        assert_eq!(outcome(1641), Ok(Installed::Restart));
        assert!(outcome(1602).unwrap_err().contains("cancelled"));
        assert!(outcome(1618).unwrap_err().contains("Another installation"));
        assert!(outcome(1603).unwrap_err().contains("1603"));
        // Its server on this PC only, when the app starts it.
        assert_eq!(&START_ARGS[..3], ["--server", "--server-host", "127.0.0.1"]);
    }

    #[test]
    fn waits_for_starts_or_offers_openrgb_as_it_finds_it() {
        let exe = PathBuf::from(r"C:\Program Files\OpenRGB\OpenRGB.exe");
        let installed = Found {
            exe: Some(exe.clone()),
            service: ServiceState::Running,
            open: true,
        };
        // The service finds its devices first.
        assert_eq!(next(&installed, 30 * SECOND, None), Next::Wait);
        assert_eq!(next(&installed, 121 * SECOND, None), Next::NotRunning);
        let starting = Found {
            service: ServiceState::Starting,
            ..installed.clone()
        };
        assert_eq!(next(&starting, 5 * SECOND, None), Next::Wait);

        // No service: the app starts it once, then waits, then leaves it to the owner.
        let bare = Found {
            exe: Some(exe.clone()),
            service: ServiceState::NotInstalled,
            open: false,
        };
        assert_eq!(next(&bare, 5 * SECOND, None), Next::Start(exe.clone()));
        assert_eq!(next(&bare, 5 * SECOND, Some(10 * SECOND)), Next::Wait);
        assert_eq!(next(&bare, 5 * SECOND, Some(46 * SECOND)), Next::NotRunning);
        let stopped = Found {
            service: ServiceState::Stopped,
            ..bare.clone()
        };
        assert_eq!(next(&stopped, 5 * SECOND, None), Next::Start(exe));
        // Open without its server: never a second copy.
        let open = Found { open: true, ..bare };
        assert_eq!(next(&open, 5 * SECOND, None), Next::NotRunning);

        // Nowhere: offered; a service without the program: the owner's to look at.
        assert_eq!(next(&Found::default(), 5 * SECOND, None), Next::Missing);
        let odd = Found {
            service: ServiceState::Stopped,
            ..Found::default()
        };
        assert_eq!(next(&odd, 5 * SECOND, None), Next::NotRunning);
    }
}

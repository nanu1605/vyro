//! PawnIO client (Phase 6). PawnIO is a signed, general-purpose kernel driver that runs small
//! verified "modules"; Vyro loads the signed modules it needs (vendored in `pawnio/`, LGPL-2.1)
//! and calls their functions through two IOCTLs. Everything here is pure (buffer layout,
//! versions, module identity); the device calls are in `win`.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{on_cpu, status, urgent, BusLock, Handle};

use crate::info::{PawnIoState, PawnIoStatus};

/// The driver's device, opened with `CreateFileW`.
pub const DEVICE_PATH: &str = r"\\?\GLOBALROOT\Device\PawnIO";
/// `CTL_CODE(0xA1B2, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS)`: load a module blob.
pub const IOCTL_LOAD_BINARY: u32 = (0xA1B2 << 16) | (0x821 << 2);
/// `CTL_CODE(0xA1B2, 0x841, METHOD_BUFFERED, FILE_ANY_ACCESS)`: call a module function.
pub const IOCTL_EXECUTE: u32 = (0xA1B2 << 16) | (0x841 << 2);
/// Function names occupy a fixed, NUL-padded field at the start of the input buffer.
pub const FN_NAME_LENGTH: usize = 32;
/// Uninstall entry written by the PawnIO setup (`DisplayVersion`).
pub const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\PawnIO";
/// Oldest driver the vendored modules are known to work with.
pub const MIN_VERSION: [u32; 3] = [2, 0, 0];
/// Coexistence mutexes shared with other monitoring tools (LibreHardwareMonitor, HWiNFO, …).
pub const MUTEX_PCI: &str = r"Global\Access_PCI";
pub const MUTEX_ISABUS: &str = r"Global\Access_ISABUS.HTP.Method";
pub const MUTEX_SMBUS: &str = r"Global\Access_SMBUS.HTP.Method";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Module {
    AmdFamily17,
    LpcIo,
    SmbusPiix4,
    RyzenSmu,
    Nvidia,
}

impl Module {
    pub const ALL: [Self; 5] = [
        Self::AmdFamily17,
        Self::LpcIo,
        Self::SmbusPiix4,
        Self::RyzenSmu,
        Self::Nvidia,
    ];

    pub const fn file(self) -> &'static str {
        match self {
            Self::AmdFamily17 => "AMDFamily17.bin",
            Self::LpcIo => "LpcIO.bin",
            Self::SmbusPiix4 => "SmbusPIIX4.bin",
            Self::RyzenSmu => "RyzenSMU.bin",
            Self::Nvidia => "Nvidia.bin",
        }
    }

    /// The signed module image.
    #[cfg(windows)]
    pub const fn blob(self) -> &'static [u8] {
        match self {
            Self::AmdFamily17 => include_bytes!("../../pawnio/AMDFamily17.bin"),
            Self::LpcIo => include_bytes!("../../pawnio/LpcIO.bin"),
            Self::SmbusPiix4 => include_bytes!("../../pawnio/SmbusPIIX4.bin"),
            Self::RyzenSmu => include_bytes!("../../pawnio/RyzenSMU.bin"),
            Self::Nvidia => include_bytes!("../../pawnio/Nvidia.bin"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PawnIoError {
    #[error("PawnIO is not installed")]
    NotInstalled,
    #[error("PawnIO device is not running")]
    NotRunning,
    #[error("PawnIO requires administrator rights")]
    NoAccess,
    #[error("module {0} rejected: {1}")]
    Load(&'static str, String),
    #[error("{0} failed: {1}")]
    Call(String, String),
    #[error("function name {0:?} is too long")]
    Name(String),
}

/// Input buffer for `IOCTL_EXECUTE`: the NUL-padded function name, then the `u64` arguments.
pub fn pack_call(name: &str, args: &[u64]) -> Result<Vec<u8>, PawnIoError> {
    if name.len() >= FN_NAME_LENGTH || !name.is_ascii() {
        return Err(PawnIoError::Name(name.to_owned()));
    }
    let mut buf = vec![0u8; FN_NAME_LENGTH + args.len() * 8];
    buf[..name.len()].copy_from_slice(name.as_bytes());
    for (chunk, a) in buf[FN_NAME_LENGTH..]
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(args)
    {
        *chunk = a.to_le_bytes();
    }
    Ok(buf)
}

/// Output words from the bytes the driver returned (a trailing partial word is dropped).
pub fn unpack(out: &[u8]) -> Vec<u64> {
    out.as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_le_bytes(*c))
        .collect()
}

/// Parses "2.0.1" / "2.0.1.0" into its first three components.
pub fn parse_version(s: &str) -> Option<[u32; 3]> {
    let mut parts = s.trim().split('.').map(|p| p.trim().parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some([major, minor, patch])
}

/// Classifies an installation from its registry version and whether the device opened.
pub fn classify(version: Option<&str>, open: Result<(), PawnIoError>) -> PawnIoStatus {
    let version_s = version.map(str::to_owned);
    let outdated = version
        .and_then(parse_version)
        .is_some_and(|v| v < MIN_VERSION);
    let (state, detail) = match open {
        Ok(()) if outdated => (PawnIoState::Outdated, None),
        Ok(()) => (PawnIoState::Ready, None),
        Err(PawnIoError::NotInstalled | PawnIoError::NotRunning) if version.is_none() => {
            (PawnIoState::NotInstalled, None)
        }
        Err(PawnIoError::NotInstalled | PawnIoError::NotRunning) => (PawnIoState::NotRunning, None),
        Err(PawnIoError::NoAccess) => (PawnIoState::NoAccess, None),
        Err(e) => (PawnIoState::Error, Some(e.to_string())),
    };
    PawnIoStatus {
        state,
        version: version_s,
        detail,
    }
}

/// Status on platforms without PawnIO.
#[cfg(not(windows))]
pub fn status() -> PawnIoStatus {
    PawnIoStatus {
        state: PawnIoState::Unsupported,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_codes_match_the_driver() {
        assert_eq!(IOCTL_LOAD_BINARY, 0xA1B2_2084);
        assert_eq!(IOCTL_EXECUTE, 0xA1B2_2104);
    }

    #[test]
    fn packs_name_then_little_endian_arguments() {
        let b = pack_call("ioctl_read_smn", &[0x5_9800, u64::MAX]).expect("pack");
        assert_eq!(b.len(), 32 + 16);
        assert_eq!(&b[..14], b"ioctl_read_smn");
        assert!(b[14..32].iter().all(|&c| c == 0));
        assert_eq!(&b[32..40], &0x5_9800u64.to_le_bytes());
        assert_eq!(&b[40..48], &[0xFF; 8]);
        assert!(pack_call(&"x".repeat(32), &[]).is_err());
        assert_eq!(pack_call(&"x".repeat(31), &[]).expect("31").len(), 32);
    }

    #[test]
    fn unpacks_whole_words() {
        let mut out = 7u64.to_le_bytes().to_vec();
        out.extend(0xDEAD_BEEF_0000_0001u64.to_le_bytes());
        out.push(9);
        assert_eq!(unpack(&out), vec![7, 0xDEAD_BEEF_0000_0001]);
    }

    #[test]
    fn versions_and_states() {
        assert_eq!(parse_version("2.2.0"), Some([2, 2, 0]));
        assert_eq!(parse_version(" 2.0.1.0 "), Some([2, 0, 1]));
        assert_eq!(parse_version("3"), Some([3, 0, 0]));
        assert_eq!(parse_version("beta"), None);

        let s = classify(Some("2.2.0"), Ok(()));
        assert_eq!(
            (s.state, s.version.as_deref()),
            (PawnIoState::Ready, Some("2.2.0"))
        );
        assert_eq!(classify(Some("1.9"), Ok(())).state, PawnIoState::Outdated);
        assert_eq!(classify(None, Ok(())).state, PawnIoState::Ready);
        assert_eq!(
            classify(None, Err(PawnIoError::NotInstalled)).state,
            PawnIoState::NotInstalled
        );
        assert_eq!(
            classify(Some("2.2.0"), Err(PawnIoError::NotInstalled)).state,
            PawnIoState::NotRunning
        );
        assert_eq!(
            classify(Some("2.2.0"), Err(PawnIoError::NoAccess)).state,
            PawnIoState::NoAccess
        );
        let e = classify(Some("2.2.0"), Err(PawnIoError::Load("x", "bad".into())));
        assert_eq!(e.state, PawnIoState::Error);
        assert!(e.detail.is_some());
    }

    #[test]
    fn vendored_modules_match_pinned_hashes() {
        use sha2::{Digest, Sha256};
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("pawnio");
        let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).expect("SHA256SUMS");
        for m in Module::ALL {
            let line = sums
                .lines()
                .find(|l| l.ends_with(m.file()))
                .unwrap_or_else(|| panic!("{} not pinned", m.file()));
            let want = line.split_whitespace().next().expect("hash");
            let bytes = std::fs::read(dir.join(m.file())).expect("module file");
            let got: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(got, want, "{}", m.file());
        }
    }
}

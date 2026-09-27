//! "Start with Windows": a per-user `Run` entry that starts the app in the tray at sign-in. The
//! app runs unelevated (the sensor service does the privileged reads), so neither a scheduled
//! task nor a UAC prompt is involved. Versions before the service used an elevated scheduled
//! task named [`LEGACY_TASK_NAME`]; the MSI removes it.

/// `HKCU` key holding per-user startup commands.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Value name under [`RUN_KEY`].
pub const VALUE_NAME: &str = "Vyro";
/// Command-line flag: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";
/// The elevated logon task of earlier versions.
pub const LEGACY_TASK_NAME: &str = "Vyro";

/// The startup command for `exe`.
pub fn command_line(exe: &str) -> String {
    format!("\"{exe}\" {MINIMIZED_ARG}")
}

#[cfg(windows)]
mod win {
    use super::{command_line, RUN_KEY, VALUE_NAME};
    use crate::winutil::{delete_value, set_string, Key, HKEY_CURRENT_USER};

    pub fn is_enabled() -> bool {
        Key::open(HKEY_CURRENT_USER, RUN_KEY)
            .and_then(|k| k.string(VALUE_NAME))
            .is_some()
    }

    pub fn enable() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        set_string(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            &command_line(&exe.to_string_lossy()),
        )
        .map_err(|e| e.to_string())
    }

    pub fn disable() -> Result<(), String> {
        if !is_enabled() {
            return Ok(());
        }
        delete_value(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
pub use win::{disable, enable, is_enabled};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_the_exe_quoted_and_minimized() {
        assert_eq!(
            command_line(r"C:\Program Files\Vyro\vyro.exe"),
            r#""C:\Program Files\Vyro\vyro.exe" --minimized"#
        );
    }
}

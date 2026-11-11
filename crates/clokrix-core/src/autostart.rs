//! "Start with Windows": a per-user `Run` entry that starts the app in the tray at sign-in. The
//! app runs unelevated (the sensor service does the privileged reads), so neither a scheduled
//! task nor a UAC prompt is involved. Versions before the service used an elevated scheduled
//! task named [`LEGACY_TASK_NAME`]; the MSI removes it.

/// `HKCU` key holding per-user startup commands.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Value name under [`RUN_KEY`].
pub const VALUE_NAME: &str = "Clokrix";
/// The value's name before the rename to Clokrix (2.2): carried over by [`carry_over`].
pub const LEGACY_VALUE_NAME: &str = "Vyro";
/// Command-line flag: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";
/// The elevated logon task of earlier versions (named before the rename to Clokrix, so still
/// "Vyro").
pub const LEGACY_TASK_NAME: &str = "Vyro";

/// The startup command for `exe`.
pub fn command_line(exe: &str) -> String {
    format!("\"{exe}\" {MINIMIZED_ARG}")
}

#[cfg(windows)]
mod win {
    use super::{command_line, LEGACY_VALUE_NAME, RUN_KEY, VALUE_NAME};
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

    /// "Start with Windows" as Vyro left it (2.2): its entry, which names Vyro's removed exe,
    /// becomes this app's (unless that is there already) and goes. `Ok(true)`: it was on.
    pub fn carry_over() -> Result<bool, String> {
        let on = Key::open(HKEY_CURRENT_USER, RUN_KEY)
            .and_then(|k| k.string(LEGACY_VALUE_NAME))
            .is_some();
        if !on {
            return Ok(false);
        }
        if !is_enabled() {
            enable()?;
        }
        delete_value(HKEY_CURRENT_USER, RUN_KEY, LEGACY_VALUE_NAME).map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[cfg(windows)]
pub use win::{carry_over, disable, enable, is_enabled};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_the_exe_quoted_and_minimized() {
        assert_eq!(
            command_line(r"C:\Program Files\Clokrix\clokrix.exe"),
            r#""C:\Program Files\Clokrix\clokrix.exe" --minimized"#
        );
    }
}

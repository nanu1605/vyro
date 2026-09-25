//! Startup-entry helpers.

/// Explorer's `StartupApproved` values: first byte even → enabled, odd → disabled. A missing
/// value means the entry was never toggled, i.e. enabled.
pub fn approved_enabled(value: Option<&[u8]>) -> bool {
    value.and_then(<[u8]>::first).is_none_or(|b| b & 1 == 0)
}

/// Display name for a Startup-folder entry: file name without `.lnk` / `.url`.
pub fn entry_name(file_name: &str) -> String {
    let lower = file_name.to_ascii_lowercase();
    for ext in [".lnk", ".url"] {
        if lower.ends_with(ext) {
            return file_name[..file_name.len() - ext.len()].to_owned();
        }
    }
    file_name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_flags() {
        assert!(approved_enabled(None));
        assert!(approved_enabled(Some(&[0x02, 0, 0, 0])));
        assert!(approved_enabled(Some(&[0x06, 0, 0, 0])));
        assert!(!approved_enabled(Some(&[0x03, 0, 0, 0, 0x10, 0x20])));
        assert!(!approved_enabled(Some(&[0x07])));
    }

    #[test]
    fn names() {
        assert_eq!(entry_name("OneDrive.lnk"), "OneDrive");
        assert_eq!(entry_name("Notes.URL"), "Notes");
        assert_eq!(entry_name("script.cmd"), "script.cmd");
    }
}

//! Ending a process (1.5), from the Processes view or a phone. Windows' own processes and
//! Vyro's are never ended, and the process is named again just before: a number Windows has
//! given to a new process since the list was read is left alone.

/// Processes that are never ended: Windows needs them (ending some stops the PC at once), the
/// desktop itself, and Vyro.
const PROTECTED: &[&str] = &[
    "system",
    "registry",
    "memory compression",
    "secure system",
    "idle",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "fontdrvhost.exe",
    "dwm.exe",
    "explorer.exe",
    "vyro.exe",
    "vyro-service.exe",
];

/// Why a process may not be ended, or `None`.
pub fn refused(pid: u32, name: &str) -> Option<&'static str> {
    let name = name.trim().to_ascii_lowercase();
    if pid <= 4 || name.is_empty() || PROTECTED.contains(&name.as_str()) {
        return Some("Windows or Vyro needs this process, so Vyro does not end it.");
    }
    if pid == std::process::id() {
        return Some("That is Vyro itself.");
    }
    None
}

/// The file name in a full image path (`C:\…\chrome.exe` → `chrome.exe`).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Ends process `pid` if it is still the one named `name`.
#[cfg(windows)]
pub fn end(pid: u32, name: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    if let Some(why) = refused(pid, name) {
        return Err(why.into());
    }
    // SAFETY: a plain open; the handle is closed below.
    let h = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|e| {
        if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
            "It runs as administrator or as another user, so Vyro cannot end it.".to_owned()
        } else {
            "That process has already ended.".to_owned()
        }
    })?;
    let mut buf = [0u16; 1024];
    let mut len = u32::try_from(buf.len()).unwrap_or(0);
    // SAFETY: the buffer and its length in characters.
    let named = unsafe {
        QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
    }
    .ok()
    .map(|()| String::from_utf16_lossy(&buf[..len as usize]));
    let same = named
        .as_deref()
        .is_some_and(|p| file_name(p).eq_ignore_ascii_case(name.trim()));
    let result = if same {
        // SAFETY: a handle opened with PROCESS_TERMINATE.
        unsafe { TerminateProcess(h, 1) }
            .map_err(|e| format!("Windows did not end it ({})", e.message()))
    } else {
        Err("That process has already ended.".into())
    };
    // SAFETY: opened above, closed once.
    let _ = unsafe { CloseHandle(h) };
    if result.is_ok() {
        log::info!("actions: ended {name} ({pid})");
    }
    result
}

#[cfg(not(windows))]
pub fn end(pid: u32, name: &str) -> Result<(), String> {
    match refused(pid, name) {
        Some(why) => Err(why.into()),
        None => Err("Ending a process works on Windows only.".into()),
    }
}

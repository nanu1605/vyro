//! A program run as administrator, after Windows' own prompt on this PC, and waited for: the
//! installers Clokrix runs only when the owner asks (ViGEmBus for the phone wheel, 2.6; OpenRGB
//! for ambient lighting, 2.6.1).

use std::path::Path;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Why it did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRun {
    /// The owner said no at the administrator prompt.
    Declined,
    Failed(String),
}

/// Runs `file` with `params` as administrator (one prompt) and waits for it: its exit code.
pub fn run(file: &Path, params: &str) -> Result<u32, NotRun> {
    let file = HSTRING::from(file.as_os_str());
    let params = HSTRING::from(params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: if params.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(params.as_ptr())
        },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: a filled-in structure whose strings outlive the call; the process handle it
    // returns is closed below.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        // 1223: ERROR_CANCELLED, the prompt declined.
        return Err(if e.code().0 as u32 & 0xFFFF == 1223 {
            NotRun::Declined
        } else {
            NotRun::Failed(e.to_string())
        });
    }
    let mut code = 0u32;
    // SAFETY: the process handle ShellExecuteExW gave, waited for and closed once.
    unsafe {
        if WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
    }
    Ok(code)
}

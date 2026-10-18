//! Shut down, restart, sleep, hibernate, lock and sign out (1.5), for a phone allowed to
//! control the PC. The app does them as the signed-in user; Windows lets a user shut down their
//! own PC. Shutting down and restarting wait a while (30 s unless asked for "now"), so they can
//! be cancelled at the PC or from the phone, and are full ones (not Fast Startup's), which a
//! wake packet can wake from.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PowerAction {
    ShutDown,
    Restart,
    Sleep,
    Hibernate,
    Lock,
    SignOut,
}

/// A power action as a phone asks for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PowerRequest {
    pub action: PowerAction,
    /// Seconds before shutting down or restarting; 0 is now.
    #[serde(default = "default_wait")]
    pub wait: u32,
    /// Close apps even with unsaved work (otherwise one can hold the shutdown up).
    #[serde(default)]
    pub force: bool,
}

/// The default wait before shutting down or restarting, and the longest.
pub const WAIT: u32 = 30;
pub const MAX_WAIT: u32 = 600;

fn default_wait() -> u32 {
    WAIT
}

impl PowerAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::ShutDown => "Shut down",
            Self::Restart => "Restart",
            Self::Sleep => "Sleep",
            Self::Hibernate => "Hibernate",
            Self::Lock => "Lock",
            Self::SignOut => "Sign out",
        }
    }

    /// Shutting down or restarting: waits, can be cancelled.
    pub fn delayed(self) -> bool {
        matches!(self, Self::ShutDown | Self::Restart)
    }
}

impl PowerRequest {
    /// The wait, within its limits (only shutting down and restarting wait).
    pub fn wait(&self) -> u32 {
        if self.action.delayed() {
            self.wait.min(MAX_WAIT)
        } else {
            0
        }
    }
}

/// A shutdown or restart under way, for the PC's banner and the phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PendingPower {
    pub action: PowerAction,
    /// Unix seconds when it happens.
    pub at: i64,
    /// Who asked: a phone's name.
    pub by: String,
}

/// What the PC can do now (sleep and hibernate depend on Windows' settings).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PowerState {
    pub pending: Option<PendingPower>,
    pub can_sleep: bool,
    pub can_hibernate: bool,
}

/// The message Windows shows while it waits to shut down or restart.
pub fn message(action: PowerAction, by: &str) -> String {
    format!(
        "Vyro: {} asked from {by}. Cancel it from Vyro's window or its tray icon.",
        action.label()
    )
}

#[cfg(windows)]
mod win {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Power::{
        IsPwrHibernateAllowed, IsPwrSuspendAllowed, SetSuspendState,
    };
    use windows::Win32::System::Shutdown::{
        AbortSystemShutdownW, ExitWindowsEx, InitiateShutdownW, LockWorkStation, EWX_LOGOFF,
        SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER, SHTDN_REASON_MINOR_OTHER,
        SHUTDOWN_FLAGS, SHUTDOWN_FORCE_OTHERS, SHUTDOWN_POWEROFF, SHUTDOWN_REASON,
        SHUTDOWN_RESTART,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use super::{PowerAction, PowerRequest};

    /// Shutting down needs the shutdown privilege enabled on the app's token (every user has it).
    fn shutdown_privilege() -> Result<(), String> {
        let mut token = HANDLE::default();
        // SAFETY: the current process's token, closed below.
        unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
        }
        .map_err(|e| e.message())?;
        let mut luid = LUID::default();
        // SAFETY: a known privilege name and an out LUID.
        let result = unsafe {
            LookupPrivilegeValueW(
                PCWSTR::null(),
                &HSTRING::from("SeShutdownPrivilege"),
                &mut luid,
            )
        }
        .and_then(|()| {
            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            // SAFETY: one privilege, nothing back.
            unsafe { AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None) }
        })
        .map_err(|e| e.message());
        // SAFETY: opened above.
        let _ = unsafe { CloseHandle(token) };
        result
    }

    fn reason() -> SHUTDOWN_REASON {
        SHUTDOWN_REASON(
            SHTDN_REASON_MAJOR_OTHER.0 | SHTDN_REASON_MINOR_OTHER.0 | SHTDN_REASON_FLAG_PLANNED.0,
        )
    }

    pub fn act(req: &PowerRequest, message: &str) -> Result<(), String> {
        match req.action {
            PowerAction::ShutDown | PowerAction::Restart => {
                shutdown_privilege()?;
                let mut flags = if req.action == PowerAction::Restart {
                    SHUTDOWN_RESTART.0
                } else {
                    SHUTDOWN_POWEROFF.0
                };
                if req.force {
                    flags |= SHUTDOWN_FORCE_OTHERS.0;
                }
                // SAFETY: this PC, a message and plain flags.
                let code = unsafe {
                    InitiateShutdownW(
                        PCWSTR::null(),
                        &HSTRING::from(message),
                        req.wait(),
                        SHUTDOWN_FLAGS(flags),
                        reason(),
                    )
                };
                if code == 0 {
                    Ok(())
                } else {
                    Err(windows::core::HRESULT::from_win32(code).message())
                }
            }
            PowerAction::Sleep | PowerAction::Hibernate => {
                shutdown_privilege()?;
                let hibernate = req.action == PowerAction::Hibernate;
                // SAFETY: plain flags; returns when the PC wakes.
                if unsafe { SetSuspendState(hibernate, false, false) } {
                    Ok(())
                } else {
                    Err(windows::core::Error::from_thread().message())
                }
            }
            // SAFETY: the signed-in user's own desktop.
            PowerAction::Lock => unsafe { LockWorkStation() }.map_err(|e| e.message()),
            PowerAction::SignOut => {
                // SAFETY: signs this user out.
                unsafe { ExitWindowsEx(EWX_LOGOFF, reason()) }.map_err(|e| e.message())
            }
        }
    }

    pub fn cancel() -> Result<(), String> {
        shutdown_privilege()?;
        // SAFETY: this PC.
        unsafe { AbortSystemShutdownW(PCWSTR::null()) }.map_err(|e| e.message())
    }

    pub fn abilities() -> (bool, bool) {
        // SAFETY: plain queries.
        unsafe { (IsPwrSuspendAllowed(), IsPwrHibernateAllowed()) }
    }
}

/// Does the action; shutting down and restarting start their wait.
pub fn act(req: &PowerRequest, by: &str) -> Result<(), String> {
    log::info!("actions: {} asked from {by}", req.action.label());
    #[cfg(windows)]
    return win::act(req, &message(req.action, by));
    #[cfg(not(windows))]
    Err("Power actions work on Windows only.".into())
}

/// Cancels a shutdown or restart that is still waiting.
pub fn cancel() -> Result<(), String> {
    #[cfg(windows)]
    return win::cancel();
    #[cfg(not(windows))]
    Err("Power actions work on Windows only.".into())
}

/// Whether this PC can sleep and hibernate.
pub fn abilities() -> (bool, bool) {
    #[cfg(windows)]
    return win::abilities();
    #[cfg(not(windows))]
    (false, false)
}

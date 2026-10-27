//! Told as the PC goes to sleep (2.0), so the relay hears that it is leaving rather than taking
//! its silence for a crash and telling the phones it stopped answering. Windows calls back on a
//! thread of its own as it suspends; no window is needed. Shutting down, restarting and signing
//! out end the app, which says so itself.

/// Calls the hook on each suspend until dropped.
pub struct SuspendWatch {
    #[cfg(windows)]
    handle: *mut core::ffi::c_void,
    /// What the callback reaches through its context; lives as long as the registration.
    #[cfg(windows)]
    _hook: Box<Hook>,
}

#[cfg(windows)]
type Hook = Box<dyn Fn() + Send + Sync>;

// The handle is only given back to Windows, from any thread.
#[cfg(windows)]
unsafe impl Send for SuspendWatch {}
#[cfg(windows)]
unsafe impl Sync for SuspendWatch {}

#[cfg(windows)]
unsafe extern "system" fn on_power(
    context: *const core::ffi::c_void,
    kind: u32,
    _: *const core::ffi::c_void,
) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::PBT_APMSUSPEND;
    if kind == PBT_APMSUSPEND && !context.is_null() {
        // SAFETY: `context` is the `Hook` the registration holds, alive until it is undone.
        let hook = unsafe { &*context.cast::<Hook>() };
        hook();
    }
    0
}

/// Starts calling `hook` as the PC goes to sleep; `None` where that is not possible.
pub fn on_suspend(hook: impl Fn() + Send + Sync + 'static) -> Option<SuspendWatch> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE};
        use windows::Win32::System::Power::{
            PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
        };
        use windows::Win32::UI::WindowsAndMessaging::DEVICE_NOTIFY_CALLBACK;
        let hook: Box<Hook> = Box::new(Box::new(hook));
        let mut params = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_power),
            Context: std::ptr::from_ref::<Hook>(&hook).cast_mut().cast(),
        };
        let mut handle = std::ptr::null_mut();
        // SAFETY: `params` lives through the call; the context it names lives in the watch.
        let r = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(std::ptr::from_mut(&mut params).cast()),
                &mut handle,
            )
        };
        if r != ERROR_SUCCESS {
            log::info!("relay: no word of sleep from Windows: {r:?}");
            return None;
        }
        Some(SuspendWatch {
            handle,
            _hook: hook,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = hook;
        None
    }
}

#[cfg(windows)]
impl Drop for SuspendWatch {
    fn drop(&mut self) {
        use windows::Win32::System::Power::{
            PowerUnregisterSuspendResumeNotification, HPOWERNOTIFY,
        };
        // SAFETY: the handle came from the registration and is undone once.
        unsafe {
            let _ = PowerUnregisterSuspendResumeNotification(HPOWERNOTIFY(self.handle as isize));
        }
    }
}

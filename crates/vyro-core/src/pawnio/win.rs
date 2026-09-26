//! PawnIO device calls: open, load a module, execute its functions; plus the coexistence
//! mutexes and per-CPU affinity that MSR reads need.

use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ,
    GENERIC_WRITE, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::SystemInformation::GROUP_AFFINITY;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentThread, OpenMutexW, ReleaseMutex, SetThreadGroupAffinity,
    WaitForSingleObject, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::System::IO::DeviceIoControl;

use super::{
    classify, pack_call, unpack, Module, PawnIoError, DEVICE_PATH, IOCTL_EXECUTE,
    IOCTL_LOAD_BINARY, UNINSTALL_KEY,
};
use crate::info::PawnIoStatus;
use crate::winutil::{Key, HKEY_LOCAL_MACHINE};

struct Device(HANDLE);

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open_device() -> Result<Device, PawnIoError> {
    // SAFETY: plain CreateFileW on the driver's device path; the handle is owned by `Device`.
    unsafe {
        CreateFileW(
            &HSTRING::from(DEVICE_PATH),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(Device)
    .map_err(|e| {
        let code = e.code();
        if code == ERROR_ACCESS_DENIED.to_hresult() {
            PawnIoError::NoAccess
        } else if code == ERROR_FILE_NOT_FOUND.to_hresult()
            || code == ERROR_PATH_NOT_FOUND.to_hresult()
        {
            PawnIoError::NotInstalled
        } else {
            PawnIoError::Call("open".into(), e.message())
        }
    })
}

fn installed_version() -> Option<String> {
    Key::open(HKEY_LOCAL_MACHINE, UNINSTALL_KEY)?
        .value("DisplayVersion")?
        .as_string()
        .filter(|v| !v.is_empty())
}

/// Installation state, for the UI and diagnostics. Opens (and closes) the device.
pub fn status() -> PawnIoStatus {
    classify(installed_version().as_deref(), open_device().map(drop))
}

/// A device handle with one module loaded. Calls on one handle are serialized by the driver.
pub struct Handle {
    dev: Device,
    module: Module,
}

// SAFETY: a device handle may be used from any thread.
unsafe impl Send for Handle {}

impl Handle {
    pub fn open(module: Module) -> Result<Self, PawnIoError> {
        let dev = open_device()?;
        let blob = module.blob();
        // SAFETY: the blob is a valid buffer of the given length; no output buffer.
        unsafe {
            DeviceIoControl(
                dev.0,
                IOCTL_LOAD_BINARY,
                Some(blob.as_ptr().cast()),
                u32::try_from(blob.len()).unwrap_or(u32::MAX),
                None,
                0,
                None,
                None,
            )
        }
        .map_err(|e| PawnIoError::Load(module.file(), e.message()))?;
        Ok(Self { dev, module })
    }

    pub const fn module(&self) -> Module {
        self.module
    }

    /// Calls a module function with `u64` arguments; returns up to `out_words` results.
    pub fn exec(
        &self,
        name: &str,
        args: &[u64],
        out_words: usize,
    ) -> Result<Vec<u64>, PawnIoError> {
        let input = pack_call(name, args)?;
        let mut out = vec![0u8; out_words * 8];
        let mut returned = 0u32;
        // SAFETY: both buffers are valid for their stated lengths for the whole call.
        unsafe {
            DeviceIoControl(
                self.dev.0,
                IOCTL_EXECUTE,
                Some(input.as_ptr().cast()),
                u32::try_from(input.len()).unwrap_or(u32::MAX),
                (!out.is_empty()).then_some(out.as_mut_ptr().cast()),
                u32::try_from(out.len()).unwrap_or(u32::MAX),
                Some(&mut returned),
                None,
            )
        }
        .map_err(|e| PawnIoError::Call(name.to_owned(), e.message()))?;
        out.truncate(returned as usize);
        Ok(unpack(&out))
    }

    /// Single-result call.
    pub fn exec1(&self, name: &str, args: &[u64]) -> Result<u64, PawnIoError> {
        self.exec(name, args, 1)?
            .first()
            .copied()
            .ok_or_else(|| PawnIoError::Call(name.to_owned(), "no result".into()))
    }
}

/// A named system-wide mutex shared with other monitoring tools, so two programs never
/// interleave index/data register sequences on the same bus. Opened once, held per access.
pub struct BusLock(Option<HANDLE>);

// SAFETY: a mutex handle may be used from any thread (ownership is per acquiring thread,
// and `hold` releases on the thread that acquired).
unsafe impl Send for BusLock {}

impl BusLock {
    pub fn new(name: &str) -> Self {
        let name = HSTRING::from(name);
        // SAFETY: plain create-or-open of a named mutex; the handle is owned by `self`.
        let h = unsafe { CreateMutexW(None, false, &name) }
            .or_else(|_| unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) })
            .ok();
        Self(h)
    }

    /// Runs `f` while holding the mutex. `None` when another tool keeps the bus for longer
    /// than `timeout_ms`; without a mutex at all (it could not be created) `f` runs unguarded.
    pub fn hold<R>(&self, timeout_ms: u32, f: impl FnOnce() -> R) -> Option<R> {
        let Some(h) = self.0 else {
            return Some(f());
        };
        // SAFETY: `h` is a live mutex handle.
        let w = unsafe { WaitForSingleObject(h, timeout_ms) };
        if w != WAIT_OBJECT_0 && w != WAIT_ABANDONED {
            return None;
        }
        let r = f();
        // SAFETY: this thread owns the mutex after the successful wait above.
        unsafe {
            let _ = ReleaseMutex(h);
        }
        Some(r)
    }
}

impl Drop for BusLock {
    fn drop(&mut self) {
        if let Some(h) = self.0 {
            // SAFETY: owned handle, closed once.
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

/// Runs `f` on logical processor `cpu` (group × 64 + index), restoring the thread's previous
/// affinity afterwards. `None` if the processor cannot be selected.
pub fn on_cpu<R>(cpu: u32, f: impl FnOnce() -> R) -> Option<R> {
    let target = GROUP_AFFINITY {
        Mask: 1usize << (cpu % 64),
        Group: u16::try_from(cpu / 64).ok()?,
        Reserved: [0; 3],
    };
    let mut previous = GROUP_AFFINITY::default();
    // SAFETY: both structs are valid for the call; the pseudo-handle needs no closing.
    let set = unsafe { SetThreadGroupAffinity(GetCurrentThread(), &target, Some(&mut previous)) };
    if !set.as_bool() {
        return None;
    }
    let r = f();
    // SAFETY: as above; restores what the call returned.
    unsafe {
        let _ = SetThreadGroupAffinity(GetCurrentThread(), &previous, None);
    }
    Some(r)
}

//! Loading vendor DLLs from System32 (never the app directory or PATH, so a planted DLL can't
//! be picked up). Libraries stay loaded for the life of the process: vendor runtimes start
//! threads and do not all tolerate unloading.

use std::ffi::CStr;

use windows::core::{HSTRING, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

#[derive(Clone, Copy)]
pub struct Library(HMODULE);

// SAFETY: a module handle is a process-wide value, valid on any thread while loaded (forever).
unsafe impl Send for Library {}

impl Library {
    pub fn system32(name: &str) -> Option<Self> {
        // SAFETY: plain load with a restricted search path.
        unsafe { LoadLibraryExW(&HSTRING::from(name), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .ok()
            .map(Self)
    }

    /// Looks up an export.
    ///
    /// # Safety
    /// `T` must be the `extern` function pointer type matching the export's real signature.
    pub unsafe fn get<T: Copy>(&self, name: &CStr) -> Option<T> {
        const { assert!(size_of::<T>() == size_of::<usize>()) };
        // SAFETY: valid module handle and NUL-terminated name.
        let f = unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr().cast())) }?;
        // SAFETY: caller guarantees `T` is the export's function pointer type.
        Some(unsafe { std::mem::transmute_copy(&f) })
    }

    /// Looks up an export that has a number and no name.
    ///
    /// # Safety
    /// As [`Library::get`].
    pub unsafe fn ordinal<T: Copy>(&self, n: u16) -> Option<T> {
        const { assert!(size_of::<T>() == size_of::<usize>()) };
        // SAFETY: GetProcAddress takes an ordinal in place of a name: a "pointer" whose high
        // bits are zero (MAKEINTRESOURCE).
        let f =
            unsafe { GetProcAddress(self.0, PCSTR(std::ptr::without_provenance(usize::from(n)))) }?;
        // SAFETY: caller guarantees `T` is the export's function pointer type.
        Some(unsafe { std::mem::transmute_copy(&f) })
    }
}

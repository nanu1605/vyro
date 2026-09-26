//! AMD Display Library (ADL) performance log: clocks, edge / hotspot / memory temperature,
//! fan, core voltage and power for Radeon GPUs and APUs.

use std::ffi::c_void;

use windows::Win32::System::Com::CoTaskMemAlloc;

use super::dll::Library;
use crate::usage::vendor::{
    decode_pmlog, parse_adl_adapters, pmlog, AdlAdapter, VendorReadings, ADL_ADAPTER_INFO_SIZE,
};

type Context = *mut c_void;
type MallocCallback = unsafe extern "system" fn(i32) -> *mut c_void;
type Create = unsafe extern "system" fn(MallocCallback, i32, *mut Context) -> i32;
type Destroy = unsafe extern "system" fn(Context) -> i32;
type NumberOfAdapters = unsafe extern "system" fn(Context, *mut i32) -> i32;
type AdapterInfoGet = unsafe extern "system" fn(Context, *mut u8, i32) -> i32;
type PmLogGet = unsafe extern "system" fn(Context, i32, *mut PmLogOutput) -> i32;

#[repr(C)]
struct PmLogOutput {
    size: i32,
    sensors: [[i32; 2]; pmlog::MAX_SENSORS],
}

/// ADL allocates its result buffers through this; they are never freed by ADL itself.
unsafe extern "system" fn adl_malloc(size: i32) -> *mut c_void {
    // SAFETY: plain allocation; a null result is handled by ADL.
    unsafe { CoTaskMemAlloc(usize::try_from(size).unwrap_or(0)) }
}

pub struct Adl {
    ctx: Context,
    destroy: Destroy,
    pmlog: PmLogGet,
    adapters: Vec<AdlAdapter>,
}

// SAFETY: the ADL2 context is used from one thread at a time (the poller).
unsafe impl Send for Adl {}

impl Adl {
    pub fn load() -> Option<Self> {
        let lib = Library::system32("atiadlxx.dll")?;
        // SAFETY (all lookups): signatures from the ADL SDK headers.
        let create: Create = unsafe { lib.get(c"ADL2_Main_Control_Create") }?;
        let destroy: Destroy = unsafe { lib.get(c"ADL2_Main_Control_Destroy") }?;
        let count: NumberOfAdapters = unsafe { lib.get(c"ADL2_Adapter_NumberOfAdapters_Get") }?;
        let info: AdapterInfoGet = unsafe { lib.get(c"ADL2_Adapter_AdapterInfo_Get") }?;
        let pmlog: PmLogGet = unsafe { lib.get(c"ADL2_New_QueryPMLogData_Get") }?;

        let mut ctx: Context = std::ptr::null_mut();
        // SAFETY: valid callback and out-pointer; 1 = connected adapters only.
        let rc = unsafe { create(adl_malloc, 1, &mut ctx) };
        if rc < 0 || ctx.is_null() {
            return None;
        }
        let mut adl = Self {
            ctx,
            destroy,
            pmlog,
            adapters: Vec::new(),
        };
        let mut n = 0i32;
        // SAFETY: valid context and out-pointer.
        if unsafe { count(ctx, &mut n) } != 0 || n <= 0 {
            return None;
        }
        let mut buf = vec![0u8; ADL_ADAPTER_INFO_SIZE * n as usize];
        let len = i32::try_from(buf.len()).ok()?;
        // SAFETY: buffer of `len` bytes for `n` AdapterInfo records.
        if unsafe { info(ctx, buf.as_mut_ptr(), len) } != 0 {
            return None;
        }
        adl.adapters = parse_adl_adapters(&buf);
        Some(adl)
    }

    /// Index into ADL's adapters for the GPU on `bus`.
    pub fn find(&self, bus: u32) -> Option<usize> {
        self.adapters.iter().position(|a| a.bus == bus)
    }

    pub fn read(&self, index: usize) -> Option<VendorReadings> {
        let a = self.adapters.get(index)?;
        let mut out = PmLogOutput {
            size: 0,
            sensors: [[0; 2]; pmlog::MAX_SENSORS],
        };
        // SAFETY: valid context and output struct.
        (unsafe { (self.pmlog)(self.ctx, a.index, &mut out) } == 0)
            .then(|| decode_pmlog(&out.sensors))
    }
}

impl Drop for Adl {
    fn drop(&mut self) {
        // SAFETY: context from ADL2_Main_Control_Create, destroyed once.
        unsafe {
            (self.destroy)(self.ctx);
        }
    }
}

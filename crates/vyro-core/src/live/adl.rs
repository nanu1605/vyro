//! AMD Display Library (ADL) performance log: clocks, edge / hotspot / memory temperature,
//! fan, core voltage and power for Radeon GPUs and APUs.

use std::ffi::c_void;

use windows::Win32::System::Com::CoTaskMemAlloc;

use super::dll::Library;
use crate::usage::vendor::{
    adl_adapters, decode_pmlog, parse_adl_records, pmlog, AdlAdapter, VendorReadings,
    ADL_ADAPTER_INFO_SIZE,
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
    pub fn load() -> Result<Self, String> {
        let lib = Library::system32("atiadlxx.dll").ok_or("atiadlxx.dll not found")?;
        // SAFETY (all lookups): signatures from the ADL SDK headers.
        let missing = |name: &str| format!("atiadlxx.dll lacks {name}");
        let create: Create =
            unsafe { lib.get(c"ADL2_Main_Control_Create") }.ok_or_else(|| missing("Create"))?;
        let destroy: Destroy =
            unsafe { lib.get(c"ADL2_Main_Control_Destroy") }.ok_or_else(|| missing("Destroy"))?;
        let count: NumberOfAdapters = unsafe { lib.get(c"ADL2_Adapter_NumberOfAdapters_Get") }
            .ok_or_else(|| missing("NumberOfAdapters_Get"))?;
        let info: AdapterInfoGet = unsafe { lib.get(c"ADL2_Adapter_AdapterInfo_Get") }
            .ok_or_else(|| missing("AdapterInfo_Get"))?;
        let pmlog: PmLogGet = unsafe { lib.get(c"ADL2_New_QueryPMLogData_Get") }
            .ok_or_else(|| missing("New_QueryPMLogData_Get"))?;

        let mut ctx: Context = std::ptr::null_mut();
        // SAFETY: valid callback and out-pointer. 0 = all adapters: a headless iGPU (no
        // monitor attached) is not "connected" but still has sensors.
        let rc = unsafe { create(adl_malloc, 0, &mut ctx) };
        if rc < 0 || ctx.is_null() {
            return Err(format!("ADL2_Main_Control_Create failed ({rc})"));
        }
        let mut adl = Self {
            ctx,
            destroy,
            pmlog,
            adapters: Vec::new(),
        };
        let mut n = 0i32;
        // SAFETY: valid context and out-pointer.
        let rc = unsafe { count(ctx, &mut n) };
        if rc != 0 || n <= 0 {
            return Err(format!("no ADL adapters (rc {rc}, count {n})"));
        }
        log::info!("gpu: ADL reports {n} logical adapters");
        let mut buf = vec![0u8; ADL_ADAPTER_INFO_SIZE * n as usize];
        let len = i32::try_from(buf.len()).map_err(|e| e.to_string())?;
        // SAFETY: buffer of `len` bytes for `n` AdapterInfo records.
        let rc = unsafe { info(ctx, buf.as_mut_ptr(), len) };
        if rc != 0 {
            return Err(format!("ADL2_Adapter_AdapterInfo_Get failed ({rc})"));
        }
        let records = parse_adl_records(&buf);
        for r in &records {
            log::info!(
                "gpu: ADL record index={} bus={} vendor={:#x} present={} name={:?}",
                r.index,
                r.bus,
                r.vendor,
                r.present,
                r.name
            );
        }
        adl.adapters = adl_adapters(&records);
        log::info!("gpu: ADL adapters: {:?}", adl.adapters);
        Ok(adl)
    }

    /// Index into ADL's adapters for the GPU on `bus`.
    pub fn find(&self, bus: u32) -> Option<usize> {
        self.adapters.iter().position(|a| a.bus == bus)
    }

    pub fn read(&self, index: usize) -> Result<VendorReadings, String> {
        let a = self.adapters.get(index).ok_or("no such ADL adapter")?;
        let mut out = PmLogOutput {
            size: 0,
            sensors: [[0; 2]; pmlog::MAX_SENSORS],
        };
        // SAFETY: valid context and output struct.
        let rc = unsafe { (self.pmlog)(self.ctx, a.index, &mut out) };
        if rc != 0 {
            return Err(format!(
                "ADL2_New_QueryPMLogData_Get({}) failed ({rc})",
                a.index
            ));
        }
        Ok(decode_pmlog(&out.sensors))
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

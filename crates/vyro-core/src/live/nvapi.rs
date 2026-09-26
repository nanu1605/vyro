//! NVIDIA NVAPI extras NVML lacks: hotspot / memory-junction temperature, core voltage and fan
//! tachometer. These entry points are not in the public SDK; they are reached through
//! `nvapi_QueryInterface` by id, and any failure simply leaves the reading empty.

use std::ffi::c_void;

use super::dll::Library;
use crate::usage::vendor::{nvapi_core_volts, nvapi_thermal, nvapi_version, VendorReadings};

type QueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type Handle = *mut c_void;
type Initialize = unsafe extern "C" fn() -> i32;
type EnumPhysicalGpus = unsafe extern "C" fn(*mut [Handle; 64], *mut u32) -> i32;
type HandleU32 = unsafe extern "C" fn(Handle, *mut u32) -> i32;
type ThermalSensors = unsafe extern "C" fn(Handle, *mut NvThermalSensors) -> i32;
type VoltRails = unsafe extern "C" fn(Handle, *mut NvVoltRails) -> i32;

const ID_INITIALIZE: u32 = 0x0150_E828;
const ID_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC_921F;
const ID_GET_BUS_ID: u32 = 0x1BE0_B8E5;
const ID_GET_THERMAL_SENSORS: u32 = 0x65FE_3AAD;
const ID_VOLT_RAILS_GET_STATUS: u32 = 0x465F_9BCF;
const ID_GET_TACH_READING: u32 = 0x5F60_8315;

#[repr(C)]
struct NvThermalSensors {
    version: u32,
    mask: u32,
    reserved: [i32; 8],
    temperatures: [i32; 32],
}
const _: () = assert!(size_of::<NvThermalSensors>() == 168);

#[repr(C)]
struct NvVoltRails {
    words: [u32; 19],
}
const _: () = assert!(size_of::<NvVoltRails>() == 0x4C);

struct Gpu {
    handle: Handle,
    bus: u32,
    /// Channel mask accepted by the thermal-sensor call; 0 when unsupported.
    thermal_mask: u32,
}

pub struct NvApi {
    gpus: Vec<Gpu>,
    thermal: Option<ThermalSensors>,
    volt: Option<VoltRails>,
    tach: Option<HandleU32>,
}

// SAFETY: GPU handles are opaque driver tokens valid for the process lifetime; NvApi is used
// from one thread at a time (the poller).
unsafe impl Send for NvApi {}

impl NvApi {
    pub fn load() -> Option<Self> {
        let lib = Library::system32("nvapi64.dll")?;
        // SAFETY: documented export signature.
        let qi: QueryInterface = unsafe { lib.get(c"nvapi_QueryInterface") }?;
        let get = |id: u32| {
            // SAFETY: plain lookup by id; null when the driver doesn't know it.
            let p = unsafe { qi(id) };
            (!p.is_null()).then_some(p)
        };
        // SAFETY (all transmutes below): each id maps to the function type declared for it.
        let init: Initialize = unsafe { std::mem::transmute(get(ID_INITIALIZE)?) };
        if unsafe { init() } != 0 {
            return None;
        }
        let enum_gpus: EnumPhysicalGpus =
            unsafe { std::mem::transmute(get(ID_ENUM_PHYSICAL_GPUS)?) };
        let bus_id: HandleU32 = unsafe { std::mem::transmute(get(ID_GET_BUS_ID)?) };
        let thermal: Option<ThermalSensors> =
            get(ID_GET_THERMAL_SENSORS).map(|p| unsafe { std::mem::transmute(p) });
        let volt: Option<VoltRails> =
            get(ID_VOLT_RAILS_GET_STATUS).map(|p| unsafe { std::mem::transmute(p) });
        let tach: Option<HandleU32> =
            get(ID_GET_TACH_READING).map(|p| unsafe { std::mem::transmute(p) });

        let mut handles = [std::ptr::null_mut(); 64];
        let mut count = 0u32;
        // SAFETY: 64-entry array and count out-pointer as the API requires.
        if unsafe { enum_gpus(&mut handles, &mut count) } != 0 {
            return None;
        }
        let mut gpus = Vec::new();
        for &handle in handles.iter().take(count.min(64) as usize) {
            let mut bus = 0u32;
            // SAFETY: valid handle and out-pointer.
            if unsafe { bus_id(handle, &mut bus) } != 0 {
                continue;
            }
            let thermal_mask = thermal.map_or(0, |f| probe_thermal_mask(f, handle));
            gpus.push(Gpu {
                handle,
                bus,
                thermal_mask,
            });
        }
        Some(Self {
            gpus,
            thermal,
            volt,
            tach,
        })
    }

    /// Index of the GPU on `bus`, if NVAPI knows it.
    pub fn find(&self, bus: u32) -> Option<usize> {
        self.gpus.iter().position(|g| g.bus == bus)
    }

    pub fn read(&self, index: usize, name: &str) -> VendorReadings {
        let mut r = VendorReadings::default();
        let Some(g) = self.gpus.get(index) else {
            return r;
        };
        if let (Some(f), true) = (self.thermal, g.thermal_mask != 0) {
            if let Some(s) = read_thermal(f, g.handle, g.thermal_mask) {
                (r.temp_hotspot, r.temp_mem) = nvapi_thermal(name, &s.temperatures);
            }
        }
        if let Some(f) = self.volt {
            let mut v = NvVoltRails { words: [0; 19] };
            v.words[0] = nvapi_version(size_of::<NvVoltRails>(), 1);
            // SAFETY: versioned struct of the declared size.
            if unsafe { f(g.handle, &mut v) } == 0 {
                let bytes: Vec<u8> = v.words.iter().flat_map(|w| w.to_le_bytes()).collect();
                r.voltage_v = nvapi_core_volts(&bytes);
            }
        }
        if let Some(f) = self.tach {
            let mut rpm = 0u32;
            // SAFETY: valid handle and out-pointer.
            if unsafe { f(g.handle, &mut rpm) } == 0 {
                r.fan_rpm = Some(f64::from(rpm));
            }
        }
        r
    }
}

fn read_thermal(f: ThermalSensors, handle: Handle, mask: u32) -> Option<NvThermalSensors> {
    let mut s = NvThermalSensors {
        version: nvapi_version(size_of::<NvThermalSensors>(), 2),
        mask,
        reserved: [0; 8],
        temperatures: [0; 32],
    };
    // SAFETY: versioned struct of the declared size.
    (unsafe { f(handle, &mut s) } == 0).then_some(s)
}

/// Widest channel mask the driver accepts (it rejects bits for channels that don't exist).
fn probe_thermal_mask(f: ThermalSensors, handle: Handle) -> u32 {
    for bit in 0..32 {
        if read_thermal(f, handle, 1u32 << bit).is_none() {
            return (1u32 << bit) - 1;
        }
    }
    u32::MAX
}

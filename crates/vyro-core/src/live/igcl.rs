//! Intel Graphics Control Library (IGCL): clocks, temperatures, voltage, power, fan and
//! limiter flags for Arc and recent integrated GPUs. Devices are matched to DXGI by LUID.

use std::ffi::c_void;

use windows::Win32::Foundation::LUID;

use super::dll::Library;
use crate::usage::vendor::{
    ctl_power, decode_ctl_telemetry, CtlSample, VendorReadings, CTL_DEVICE_PROPS_DEVICE_ID,
    CTL_DEVICE_PROPS_DEVICE_ID_SIZE, CTL_DEVICE_PROPS_SIZE, CTL_DEVICE_PROPS_VENDOR,
    CTL_INIT_ARGS_SIZE, CTL_TELEMETRY_SIZE,
};

type ApiHandle = *mut c_void;
type Device = *mut c_void;
type Init = unsafe extern "C" fn(*mut InitArgs, *mut ApiHandle) -> u32;
type Close = unsafe extern "C" fn(ApiHandle) -> u32;
type Enumerate = unsafe extern "C" fn(ApiHandle, *mut u32, *mut Device) -> u32;
type Properties = unsafe extern "C" fn(Device, *mut u64) -> u32;
type Telemetry = unsafe extern "C" fn(Device, *mut u64) -> u32;

#[repr(C)]
struct InitArgs {
    size: u32,
    version: u8,
    app_version: u32,
    flags: u32,
    supported_version: u32,
    application_uid: [u8; 16],
}
const _: () = assert!(size_of::<InitArgs>() == CTL_INIT_ARGS_SIZE);

/// `CTL_MAKE_VERSION(1, 1)`.
const CTL_IMPL_VERSION: u32 = (1 << 16) | 1;
const CTL_INIT_FLAG_USE_LEVEL_ZERO: u32 = 1;

struct Gpu {
    device: Device,
    luid: (i32, u32),
    prev: Option<CtlSample>,
}

pub struct Igcl {
    api: ApiHandle,
    close: Close,
    telemetry: Telemetry,
    gpus: Vec<Gpu>,
}

// SAFETY: IGCL handles are used from one thread at a time (the poller).
unsafe impl Send for Igcl {}

fn write_u32(buf: &mut [u64], off: usize, v: u32) {
    // SAFETY: `off + 4` lies inside the u64 buffer (checked by the callers' constants).
    let bytes =
        unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len() * 8) };
    bytes[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn as_bytes(buf: &[u64]) -> &[u8] {
    // SAFETY: reinterpreting initialised u64 storage as bytes.
    unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), buf.len() * 8) }
}

impl Igcl {
    pub fn load() -> Option<Self> {
        let lib = Library::system32("ControlLib.dll")?;
        // SAFETY (all lookups): signatures from igcl_api.h.
        let init: Init = unsafe { lib.get(c"ctlInit") }?;
        let close: Close = unsafe { lib.get(c"ctlClose") }?;
        let enumerate: Enumerate = unsafe { lib.get(c"ctlEnumerateDevices") }?;
        let properties: Properties = unsafe { lib.get(c"ctlGetDeviceProperties") }?;
        let telemetry: Telemetry = unsafe { lib.get(c"ctlPowerTelemetryGet") }?;

        let mut args = InitArgs {
            size: CTL_INIT_ARGS_SIZE as u32,
            version: 0,
            app_version: CTL_IMPL_VERSION,
            flags: CTL_INIT_FLAG_USE_LEVEL_ZERO,
            supported_version: 0,
            application_uid: [0; 16],
        };
        let mut api: ApiHandle = std::ptr::null_mut();
        // SAFETY: valid in/out args.
        if unsafe { init(&mut args, &mut api) } != 0 || api.is_null() {
            return None;
        }
        let mut me = Self {
            api,
            close,
            telemetry,
            gpus: Vec::new(),
        };
        let mut count = 0u32;
        // SAFETY: count query with no output array.
        if unsafe { enumerate(api, &mut count, std::ptr::null_mut()) } != 0 || count == 0 {
            return Some(me);
        }
        let mut devices = vec![std::ptr::null_mut(); count as usize];
        // SAFETY: array of `count` handles.
        if unsafe { enumerate(api, &mut count, devices.as_mut_ptr()) } != 0 {
            return Some(me);
        }
        for &device in devices.iter().take(count as usize) {
            let mut luid = LUID::default();
            let mut props = vec![0u64; CTL_DEVICE_PROPS_SIZE / 8];
            write_u32(&mut props, 0, CTL_DEVICE_PROPS_SIZE as u32);
            // Version (u8 at offset 4) = 2: the layout with PCI subsystem and BDF fields.
            write_u32(&mut props, 4, 2);
            props[CTL_DEVICE_PROPS_DEVICE_ID / 8] = (&raw mut luid) as u64;
            write_u32(
                &mut props,
                CTL_DEVICE_PROPS_DEVICE_ID_SIZE,
                size_of::<LUID>() as u32,
            );
            // SAFETY: properties buffer of the declared size; pDeviceID points at `luid`.
            if unsafe { properties(device, props.as_mut_ptr()) } != 0 {
                continue;
            }
            let b = as_bytes(&props);
            let vendor = u32::from_le_bytes(
                b[CTL_DEVICE_PROPS_VENDOR..CTL_DEVICE_PROPS_VENDOR + 4]
                    .try_into()
                    .unwrap_or_default(),
            );
            if vendor == 0x8086 {
                me.gpus.push(Gpu {
                    device,
                    luid: (luid.HighPart, luid.LowPart),
                    prev: None,
                });
            }
        }
        Some(me)
    }

    pub fn find(&self, luid: LUID) -> Option<usize> {
        self.gpus
            .iter()
            .position(|g| g.luid == (luid.HighPart, luid.LowPart))
    }

    pub fn read(&mut self, index: usize) -> Option<VendorReadings> {
        let g = self.gpus.get_mut(index)?;
        let mut buf = vec![0u64; CTL_TELEMETRY_SIZE / 8];
        write_u32(&mut buf, 0, CTL_TELEMETRY_SIZE as u32);
        write_u32(&mut buf, 4, 1); // Version 1: VR temps and effective clock included
                                   // SAFETY: telemetry buffer of the declared size.
        if unsafe { (self.telemetry)(g.device, buf.as_mut_ptr()) } != 0 {
            return None;
        }
        let sample = decode_ctl_telemetry(as_bytes(&buf));
        let mut r = sample.readings.clone();
        r.power_w = g.prev.as_ref().and_then(|p| ctl_power(p, &sample));
        g.prev = Some(sample);
        Some(r)
    }
}

impl Drop for Igcl {
    fn drop(&mut self) {
        // SAFETY: API handle from ctlInit, closed once.
        unsafe {
            (self.close)(self.api);
        }
    }
}

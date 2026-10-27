//! Intel Arc cards through the Intel Graphics Control Library (2.0), the interface of Intel's own
//! driver, as Intel's tools use it: `ControlLib.dll` from System32. Arc A- and B-series cards,
//! desktop and laptop; Arc graphics built into a processor are not tuned.
//!
//! The core takes a clock offset in MHz. The power limit is watts on the A-series and a percentage
//! on the B-series; Vyro counts both as a percentage of the card's own (its default, which the
//! library gives). The memory is left alone: the library offers no memory clock on these cards.
//! Intel asks tools that change clocks to set its warranty waiver first, which the owner accepted
//! when turning tuning on. Stock is the library's own reset.

use std::ffi::c_void;

use super::controller::TuneDevice;
use super::ffi::igcl::{
    self, core_range, power_range, DeviceProps, InitArgs, OcProperties, PowerScale,
};
use super::{is_laptop, Clocks, Family, GpuInfo, Limits, Settings, TuneCapability, Vendor};
use crate::live::dll::Library;

type Api = *mut c_void;
type Device = *mut c_void;
type Init = unsafe extern "C" fn(*mut InitArgs, *mut Api) -> u32;
type Enumerate = unsafe extern "C" fn(Api, *mut u32, *mut Device) -> u32;
type Properties = unsafe extern "C" fn(Device, *mut DeviceProps) -> u32;
type OcGetProperties = unsafe extern "C" fn(Device, *mut OcProperties) -> u32;
type GetValue = unsafe extern "C" fn(Device, *mut f64) -> u32;
type SetValue = unsafe extern "C" fn(Device, f64) -> u32;
type Plain = unsafe extern "C" fn(Device) -> u32;

/// The library's functions Vyro calls.
#[derive(Clone, Copy)]
struct Fns {
    waiver: Plain,
    reset: Plain,
    freq_get: GetValue,
    freq_set: SetValue,
    power_get: GetValue,
    power_set: SetValue,
}

/// The library, started once for the life of the process (its devices belong to it).
struct Started {
    api: Api,
    fns: Fns,
    enumerate: Enumerate,
    properties: Properties,
    oc_properties: OcGetProperties,
}
// SAFETY: the library's handles may be used from any thread; Vyro uses each card from one.
unsafe impl Send for Started {}
unsafe impl Sync for Started {}

fn started() -> Result<&'static Started, String> {
    static LIB: std::sync::OnceLock<Result<Started, String>> = std::sync::OnceLock::new();
    LIB.get_or_init(|| {
        let lib =
            Library::system32(igcl::DLL).ok_or("no Intel graphics driver (ControlLib) found")?;
        let missing = || "Intel's driver is too old for GPU tuning: update it".to_owned();
        // SAFETY (all lookups): the signatures in igcl_api.h.
        let init: Init = unsafe { lib.get(c"ctlInit") }.ok_or_else(missing)?;
        let s = Started {
            api: std::ptr::null_mut(),
            enumerate: unsafe { lib.get(c"ctlEnumerateDevices") }.ok_or_else(missing)?,
            properties: unsafe { lib.get(c"ctlGetDeviceProperties") }.ok_or_else(missing)?,
            oc_properties: unsafe { lib.get(c"ctlOverclockGetProperties") }.ok_or_else(missing)?,
            fns: Fns {
                waiver: unsafe { lib.get(c"ctlOverclockWaiverSet") }.ok_or_else(missing)?,
                reset: unsafe { lib.get(c"ctlOverclockResetToDefault") }.ok_or_else(missing)?,
                freq_get: unsafe { lib.get(c"ctlOverclockGpuFrequencyOffsetGetV2") }
                    .ok_or_else(missing)?,
                freq_set: unsafe { lib.get(c"ctlOverclockGpuFrequencyOffsetSetV2") }
                    .ok_or_else(missing)?,
                power_get: unsafe { lib.get(c"ctlOverclockPowerLimitGetV2") }
                    .ok_or_else(missing)?,
                power_set: unsafe { lib.get(c"ctlOverclockPowerLimitSetV2") }
                    .ok_or_else(missing)?,
            },
        };
        let mut args = InitArgs {
            size: size_of::<InitArgs>() as u32,
            version: 0,
            app_version: igcl::IMPL_VERSION,
            flags: igcl::INIT_FLAG_USE_LEVEL_ZERO,
            supported_version: 0,
            application_uid: [0; 16],
        };
        let mut api = std::ptr::null_mut();
        // SAFETY: valid in/out arguments.
        let r = unsafe { init(&mut args, &mut api) };
        if r != igcl::SUCCESS || api.is_null() {
            return Err(format!(
                "Intel's driver interface did not start (code {r:#x})"
            ));
        }
        Ok(Started { api, ..s })
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn ok(r: u32, what: &str) -> Result<(), String> {
    if r == igcl::SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "{what}: Intel's driver interface said no (code {r:#x})"
        ))
    }
}

struct IntelCard {
    device: Device,
    fns: Fns,
    /// The core offset the library calls stock (0 on every card so far).
    freq_default: f64,
    power: Option<PowerScale>,
    info: GpuInfo,
    limits: Limits,
}

// SAFETY: as `Started`.
unsafe impl Send for IntelCard {}

/// The first Intel Arc card Vyro can tune.
pub fn open_device() -> Result<Box<dyn TuneDevice>, TuneCapability> {
    let unavailable = |reason: String| TuneCapability::Unavailable { reason };
    let lib = started().map_err(unavailable)?;
    let mut count = 0u32;
    // SAFETY: a count query without an array.
    if unsafe { (lib.enumerate)(lib.api, &mut count, std::ptr::null_mut()) } != igcl::SUCCESS
        || count == 0
    {
        return Err(unavailable("no Intel GPU found".into()));
    }
    let mut devices = vec![std::ptr::null_mut(); count as usize];
    // SAFETY: an array of `count` handles.
    if unsafe { (lib.enumerate)(lib.api, &mut count, devices.as_mut_ptr()) } != igcl::SUCCESS {
        return Err(unavailable("Intel's driver listed no GPU".into()));
    }
    let mut seen = None;
    for &device in devices.iter().take(count as usize).filter(|d| !d.is_null()) {
        let mut luid = [0u8; 8];
        let mut props = DeviceProps::new(luid.as_mut_ptr().cast(), 8);
        // SAFETY: a properties structure of its declared size; its id buffer is `luid`.
        if unsafe { (lib.properties)(device, &mut props) } != igcl::SUCCESS
            || props.device_type != igcl::DEVICE_TYPE_GRAPHICS
            || props.pci_vendor_id != 0x8086
        {
            continue;
        }
        let name = props.name();
        let integrated = props.graphics_adapter_properties & igcl::ADAPTER_INTEGRATED != 0;
        let family = Family::from_arc(&name).filter(|_| !integrated);
        if family.is_none() {
            seen.get_or_insert(name);
            continue;
        }
        let mut oc = OcProperties::asked();
        // SAFETY: a version 1 structure of its declared size.
        if unsafe { (lib.oc_properties)(device, &mut oc) } != igcl::SUCCESS || oc.supported == 0 {
            seen.get_or_insert(name);
            continue;
        }
        let power = power_range(&oc);
        let limits = Limits {
            core: core_range(&oc),
            mem: None,
            power: power.map(|p| p.0),
            power_default_w: power.and_then(|(_, s)| match s.units {
                igcl::UNITS_WATTS => Some(s.default),
                igcl::UNITS_MILLIWATTS => Some(s.default / 1000.0),
                _ => None,
            }),
        };
        let [bus, dev, func] = props.bdf;
        let info = GpuInfo {
            vendor: Vendor::Intel,
            laptop: is_laptop(&name),
            uuid: format!(
                "INTEL-{:04x}-{bus:02x}:{dev:02x}.{func}",
                props.pci_device_id
            ),
            name,
            family,
            driver: props.driver(),
        };
        log::info!(
            "gpu tuning: {} ({:?}, driver {}): core {:?}, power {:?} %",
            info.name,
            info.family,
            info.driver,
            limits.core,
            limits.power
        );
        return Ok(Box::new(IntelCard {
            device,
            fns: lib.fns,
            freq_default: oc.gpu_frequency_offset.default,
            power: power.map(|p| p.1),
            info,
            limits,
        }));
    }
    Err(match seen {
        Some(name) => TuneCapability::ReadOnly {
            reason: format!("Vyro tunes Intel Arc A- and B-series cards; the {name} is not one"),
        },
        None => unavailable("no Intel Arc card found".into()),
    })
}

impl IntelCard {
    fn get(&self, f: GetValue, what: &str) -> Result<f64, String> {
        let mut v = 0.0;
        // SAFETY: a live device handle and an out pointer.
        ok(unsafe { f(self.device, &mut v) }, what)?;
        Ok(v)
    }
}

impl TuneDevice for IntelCard {
    fn info(&self) -> GpuInfo {
        self.info.clone()
    }

    fn limits(&self) -> Limits {
        self.limits
    }

    fn read(&mut self) -> Result<Settings, String> {
        let core_mhz = if self.limits.core.is_some() {
            (self.get(self.fns.freq_get, "core offset")? - self.freq_default).round() as i32
        } else {
            0
        };
        let power_pct = match self.power.filter(|_| self.limits.power.is_some()) {
            Some(p) => {
                (self.get(self.fns.power_get, "power limit")? / p.default * 100.0).round() as i32
            }
            None => 100,
        };
        Ok(Settings {
            core_mhz,
            mem_mhz: 0,
            power_pct,
        })
    }

    fn clocks(&mut self) -> Clocks {
        Clocks {
            core_max_mhz: None,
            mem_max_mhz: None,
            power_limit_w: self.limits.power_default_w.and_then(|w| {
                let pct = self.read().ok()?.power_pct;
                Some(w * f64::from(pct) / 100.0)
            }),
        }
    }

    /// Stock is Intel's own reset; anything else first sets the warranty waiver Intel asks for.
    fn apply(&mut self, s: &Settings) -> Result<(), String> {
        // SAFETY (all calls): a live device handle.
        if s.is_stock() && ok(unsafe { (self.fns.reset)(self.device) }, "reset").is_ok() {
            return Ok(());
        }
        ok(unsafe { (self.fns.waiver)(self.device) }, "warranty waiver")?;
        let now = self.read().unwrap_or_default();
        let power = |pct: i32| -> Result<(), String> {
            let Some(p) = self.power else {
                return Ok(());
            };
            ok(
                unsafe { (self.fns.power_set)(self.device, p.default * f64::from(pct) / 100.0) },
                "power limit",
            )
        };
        let power_first = s.power_pct >= now.power_pct;
        if power_first && self.limits.power.is_some() && s.power_pct != now.power_pct {
            power(s.power_pct)?;
        }
        if self.limits.core.is_some() && s.core_mhz != now.core_mhz {
            let v = self.freq_default + f64::from(s.core_mhz);
            ok(
                unsafe { (self.fns.freq_set)(self.device, v) },
                "core offset",
            )?;
        }
        if !power_first && self.limits.power.is_some() && s.power_pct != now.power_pct {
            power(s.power_pct)?;
        }
        Ok(())
    }
}

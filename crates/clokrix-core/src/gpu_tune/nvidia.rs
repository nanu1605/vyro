//! NVIDIA cards through NVML (1.6). The settings go through the driver's own interface, as the
//! vendor's tools do; NVML needs administrator rights to change them, so this runs only in the
//! service. Ranges come from the driver: `nvmlDeviceGetClockOffsets` (P0) for the offsets and
//! the power-limit constraints for the power limit. A setting without a range is left alone.
//!
//! From 2.3, NVAPI's P-state offsets ([`Nvapi`]) where NVML gives an offset no range, and where
//! NVML refuses to set one that NVAPI allows over the same range (GeForce cards on some drivers):
//! the interface MSI Afterburner uses. The power limit stays NVML's.

use std::ffi::c_void;

use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::{Device, Nvml};

use super::controller::TuneDevice;
use super::ffi::nvapi as api;
use super::{is_laptop, Clocks, Family, GpuInfo, Limits, Range, Settings, TuneCapability, Vendor};
use crate::live::dll::Library;

type QueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type Initialize = unsafe extern "C" fn() -> i32;
type EnumPhysicalGpus = unsafe extern "C" fn(*mut [*mut c_void; 64], *mut u32) -> i32;
type GetBusId = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
type Pstates20 = unsafe extern "C" fn(*mut c_void, *mut api::Pstates20) -> i32;

/// One card's P-state offsets through NVAPI.
struct Nvapi {
    gpu: *mut c_void,
    get: Pstates20,
    set: Pstates20,
    /// The structure version the driver took.
    version: u32,
}

// SAFETY: the GPU handle is an opaque driver token valid for the life of the process, and the
// card is used from one thread at a time (the tuning worker).
unsafe impl Send for Nvapi {}

impl Nvapi {
    /// The card on PCI bus `bus`, if NVAPI loads, knows the P-state calls and reads its table.
    fn open(bus: u32) -> Option<Self> {
        let lib = Library::system32(api::DLL)?;
        // SAFETY: the export's documented signature.
        let qi: QueryInterface = unsafe { lib.get(c"nvapi_QueryInterface") }?;
        let get = |id: u32| {
            // SAFETY: a lookup by id; null when the driver does not know it.
            let p = unsafe { qi(id) };
            (!p.is_null()).then_some(p)
        };
        // SAFETY (the transmutes): each id is the function of the type it is given here.
        let init: Initialize = unsafe { std::mem::transmute(get(api::ID_INITIALIZE)?) };
        // SAFETY: no arguments; NVAPI counts its initializations.
        if unsafe { init() } != api::OK {
            return None;
        }
        let enum_gpus: EnumPhysicalGpus =
            unsafe { std::mem::transmute(get(api::ID_ENUM_PHYSICAL_GPUS)?) };
        let bus_id: GetBusId = unsafe { std::mem::transmute(get(api::ID_GET_BUS_ID)?) };
        let read: Pstates20 = unsafe { std::mem::transmute(get(api::ID_GET_PSTATES20)?) };
        let write: Pstates20 = unsafe { std::mem::transmute(get(api::ID_SET_PSTATES20)?) };
        let mut gpus = [std::ptr::null_mut(); 64];
        let mut count = 0u32;
        // SAFETY: a 64-entry array and a count, as the call takes them.
        if unsafe { enum_gpus(&mut gpus, &mut count) } != api::OK {
            return None;
        }
        let gpu = gpus.into_iter().take(count.min(64) as usize).find(|&g| {
            let mut b = 0u32;
            // SAFETY: a handle from the enumeration and an out-pointer.
            let status = unsafe { bus_id(g, &mut b) };
            status == api::OK && b == bus
        })?;
        let version = api::VERSIONS.into_iter().find(|&version| {
            let mut info = api::Pstates20 {
                version,
                ..api::Pstates20::default()
            };
            // SAFETY: a structure as large as every version the call takes.
            let status = unsafe { read(gpu, &mut info) };
            status == api::OK
        })?;
        Some(Self {
            gpu,
            get: read,
            set: write,
            version,
        })
    }

    fn table(&self) -> Result<api::Pstates20, String> {
        let mut info = api::Pstates20 {
            version: self.version,
            ..api::Pstates20::default()
        };
        // SAFETY: as in `open`.
        match unsafe { (self.get)(self.gpu, &mut info) } {
            api::OK => Ok(info),
            e => Err(format!("NVAPI P-state read failed ({e})")),
        }
    }

    /// The clock's P0 offset now and its range, MHz.
    fn offset(&self, domain: u32) -> Option<(i32, Range)> {
        let (now, min, max) = api::offset(&self.table().ok()?, domain)?;
        Some((now, Range { min, max }))
    }

    fn set_offset(&self, domain: u32, mhz: i32) -> Result<(), String> {
        let mut info = api::request(self.version, domain, mhz);
        // SAFETY: a structure of the version the driver took for reading.
        match unsafe { (self.set)(self.gpu, &mut info) } {
            api::OK => Ok(()),
            e => Err(format!("NVAPI offset failed ({e})")),
        }
    }
}

const fn domain(clock: Clock) -> u32 {
    match clock {
        Clock::Memory => api::CLOCK_MEMORY,
        _ => api::CLOCK_GRAPHICS,
    }
}

/// NVML from System32 (the DCH driver's copy) or NVIDIA's own folder, never the search path:
/// the service runs as LocalSystem. The sensor readings load it the same way.
pub(crate) fn load_nvml() -> Result<Nvml, String> {
    let system = crate::winutil::system_dir().map(|d| d.join("nvml.dll"));
    let nvsmi = std::path::PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll");
    let mut last = "nvml.dll not found".to_owned();
    for p in system.into_iter().chain([nvsmi]).filter(|p| p.is_file()) {
        match Nvml::builder().lib_path(p.as_os_str()).init() {
            Ok(n) => return Ok(n),
            Err(e) => last = format!("{}: {e}", p.display()),
        }
    }
    Err(last)
}

/// NVML for tuning, loaded once for the life of the process (devices borrow it).
fn nvml() -> Option<&'static Nvml> {
    static NVML: std::sync::OnceLock<Option<&'static Nvml>> = std::sync::OnceLock::new();
    *NVML.get_or_init(|| {
        load_nvml()
            .inspect_err(|e| log::info!("gpu tuning: NVML unavailable: {e}"))
            .ok()
            .map(|n| &*Box::leak(Box::new(n)))
    })
}

struct NvidiaCard {
    device: Device<'static>,
    info: GpuInfo,
    limits: Limits,
    /// The driver's default power limit, mW.
    default_mw: Option<u32>,
    nvapi: Option<Nvapi>,
    /// The core and memory offsets go through NVAPI rather than NVML.
    via_nvapi: [bool; 2],
}

/// The first NVIDIA card Clokrix can tune (a laptop's or a desktop's discrete card).
pub fn open_device() -> Result<Box<dyn TuneDevice>, TuneCapability> {
    let nvml = nvml().ok_or_else(|| TuneCapability::Unavailable {
        reason: "no NVIDIA driver (NVML) found".into(),
    })?;
    let count = nvml.device_count().unwrap_or(0);
    let mut seen = None;
    for i in 0..count {
        let Ok(device) = nvml.device_by_index(i) else {
            continue;
        };
        let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".into());
        let family = device
            .architecture()
            .ok()
            .and_then(|a| Family::from_arch(&format!("{a:?}")));
        if family.is_none() {
            seen.get_or_insert(name);
            continue;
        }
        let info = GpuInfo {
            vendor: Vendor::Nvidia,
            laptop: is_laptop(&name),
            uuid: device.uuid().unwrap_or_else(|_| format!("index-{i}")),
            name,
            family,
            driver: nvml.sys_driver_version().unwrap_or_default(),
        };
        // Offsets through NVML's per-P-state calls (R555 and later), which give their range, or
        // else through NVAPI's P-state table.
        let nvml_range = |clock| -> Option<Range> {
            let o = device.clock_offset(clock, PerformanceState::Zero).ok()?;
            (o.max_clock_offset_mhz > o.min_clock_offset_mhz).then_some(Range {
                min: o.min_clock_offset_mhz,
                max: o.max_clock_offset_mhz,
            })
        };
        let nvapi = device.pci_info().ok().and_then(|p| Nvapi::open(p.bus));
        let mut via_nvapi = [false; 2];
        let mut range = |clock: Clock| {
            nvml_range(clock).or_else(|| {
                let r = nvapi.as_ref()?.offset(domain(clock))?.1;
                via_nvapi[usize::from(clock == Clock::Memory)] = true;
                Some(r)
            })
        };
        let default_mw = device.power_management_limit_default().ok();
        let power = default_mw.filter(|d| *d > 0).and_then(|d| {
            let c = device.power_management_limit_constraints().ok()?;
            let min = (u64::from(c.min_limit) * 100).div_ceil(u64::from(d));
            let max = u64::from(c.max_limit) * 100 / u64::from(d);
            (max > min).then(|| Range {
                min: min.min(100) as i32,
                max: max.max(100) as i32,
            })
        });
        let limits = Limits {
            core: range(Clock::Graphics),
            mem: range(Clock::Memory),
            power,
            power_default_w: default_mw.map(|mw| f64::from(mw) / 1000.0),
        };
        log::info!(
            "gpu tuning: {} ({:?}, driver {}): core {:?}, memory {:?}, power {:?} %; offsets \
             through {}{}",
            info.name,
            info.family,
            info.driver,
            limits.core,
            limits.mem,
            limits.power,
            match via_nvapi {
                [false, false] => "NVML",
                [true, true] => "NVAPI",
                [true, false] => "NVAPI (core) and NVML (memory)",
                [false, true] => "NVML (core) and NVAPI (memory)",
            },
            if nvapi.is_some() {
                ", NVAPI's at hand"
            } else {
                ""
            },
        );
        return Ok(Box::new(NvidiaCard {
            device,
            info,
            limits,
            default_mw,
            nvapi,
            via_nvapi,
        }));
    }
    Err(match seen {
        Some(name) => TuneCapability::ReadOnly {
            reason: format!("Clokrix tunes NVIDIA RTX 20 to RTX 50 cards; the {name} is not one"),
        },
        None => TuneCapability::Unavailable {
            reason: "no NVIDIA card found".into(),
        },
    })
}

impl NvidiaCard {
    fn slot(clock: Clock) -> usize {
        usize::from(clock == Clock::Memory)
    }

    fn offset(&self, clock: Clock) -> Result<i32, String> {
        if self.via_nvapi[Self::slot(clock)] {
            return self
                .nvapi
                .as_ref()
                .and_then(|n| n.offset(domain(clock)))
                .map(|(now, _)| now)
                .ok_or_else(|| format!("{clock:?} offset: NVAPI read failed"));
        }
        self.device
            .clock_offset(clock, PerformanceState::Zero)
            .map(|o| o.clock_offset_mhz)
            .map_err(|e| e.to_string())
    }

    fn range(&self, clock: Clock) -> Option<Range> {
        match clock {
            Clock::Memory => self.limits.mem,
            _ => self.limits.core,
        }
    }

    fn set_offset(&mut self, clock: Clock, mhz: i32) -> Result<(), String> {
        let slot = Self::slot(clock);
        if self.via_nvapi[slot] {
            let n = self.nvapi.as_ref().ok_or("NVAPI is not loaded")?;
            return n
                .set_offset(domain(clock), mhz)
                .map_err(|e| format!("{clock:?} offset: {e}"));
        }
        match self
            .device
            .set_clock_offset(clock, PerformanceState::Zero, mhz)
        {
            Ok(()) => Ok(()),
            // NVML refuses offsets on this card: NVAPI takes over the clock, but only where it
            // allows the same range, so the presets made for this card still mean the same.
            Err(e @ (NvmlError::NotSupported | NvmlError::NoPermission)) => {
                let same = self
                    .nvapi
                    .as_ref()
                    .and_then(|n| n.offset(domain(clock)))
                    .is_some_and(|(_, r)| Some(r) == self.range(clock));
                let n = self
                    .nvapi
                    .as_ref()
                    .filter(|_| same)
                    .ok_or_else(|| format!("{clock:?} offset: {e}"))?;
                n.set_offset(domain(clock), mhz)
                    .map_err(|e| format!("{clock:?} offset: {e}"))?;
                self.via_nvapi[slot] = true;
                log::info!("gpu tuning: NVML refused the {clock:?} offset ({e}); NVAPI sets it");
                Ok(())
            }
            Err(e) => Err(format!("{clock:?} offset: {e}")),
        }
    }

    fn set_power(&mut self, pct: i32) -> Result<(), String> {
        let d = self.default_mw.ok_or("no default power limit")?;
        let mw = (u64::from(d) * u64::try_from(pct).unwrap_or(100) / 100) as u32;
        self.device
            .set_power_management_limit(mw)
            .map_err(|e| format!("power limit: {e}"))
    }
}

impl TuneDevice for NvidiaCard {
    fn info(&self) -> GpuInfo {
        self.info.clone()
    }

    fn limits(&self) -> Limits {
        self.limits
    }

    fn read(&mut self) -> Result<Settings, String> {
        let power_pct = match (self.limits.power, self.default_mw) {
            (Some(_), Some(d)) => {
                let now = self
                    .device
                    .power_management_limit()
                    .map_err(|e| e.to_string())?;
                ((f64::from(now) * 100.0 / f64::from(d)).round()) as i32
            }
            _ => 100,
        };
        Ok(Settings {
            core_mhz: if self.limits.core.is_some() {
                self.offset(Clock::Graphics)?
            } else {
                0
            },
            mem_mhz: if self.limits.mem.is_some() {
                self.offset(Clock::Memory)?
            } else {
                0
            },
            power_pct,
        })
    }

    fn clocks(&mut self) -> Clocks {
        Clocks {
            core_max_mhz: self
                .device
                .max_clock_info(Clock::Graphics)
                .ok()
                .map(f64::from),
            mem_max_mhz: self
                .device
                .max_clock_info(Clock::Memory)
                .ok()
                .map(f64::from),
            power_limit_w: self
                .device
                .enforced_power_limit()
                .ok()
                .map(|mw| f64::from(mw) / 1000.0),
        }
    }

    /// Raising the power limit: it goes first, then the offsets; lowering it: the offsets first.
    fn apply(&mut self, s: &Settings) -> Result<(), String> {
        let now = self.read().unwrap_or_default();
        let power_first = s.power_pct >= now.power_pct;
        if power_first && self.limits.power.is_some() && s.power_pct != now.power_pct {
            self.set_power(s.power_pct)?;
        }
        if self.limits.core.is_some() && s.core_mhz != now.core_mhz {
            self.set_offset(Clock::Graphics, s.core_mhz)?;
        }
        if self.limits.mem.is_some() && s.mem_mhz != now.mem_mhz {
            self.set_offset(Clock::Memory, s.mem_mhz)?;
        }
        if !power_first && self.limits.power.is_some() && s.power_pct != now.power_pct {
            self.set_power(s.power_pct)?;
        }
        Ok(())
    }
}

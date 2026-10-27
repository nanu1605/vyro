//! NVIDIA cards through NVML (1.6). The settings go through the driver's own interface, as the
//! vendor's tools do; NVML needs administrator rights to change them, so this runs only in the
//! service. Ranges come from the driver: `nvmlDeviceGetClockOffsets` (P0) for the offsets and
//! the power-limit constraints for the power limit. A setting without a range is left alone.

use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};
use nvml_wrapper::{Device, Nvml};

use super::controller::TuneDevice;
use super::{is_laptop, Clocks, Family, GpuInfo, Limits, Range, Settings, TuneCapability, Vendor};

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
}

/// The first NVIDIA card Vyro can tune (a laptop's or a desktop's discrete card).
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
        // Offsets only through the per-P-state calls (R555 and later), which give their range.
        let range = |clock| -> Option<Range> {
            let o = device.clock_offset(clock, PerformanceState::Zero).ok()?;
            (o.max_clock_offset_mhz > o.min_clock_offset_mhz).then_some(Range {
                min: o.min_clock_offset_mhz,
                max: o.max_clock_offset_mhz,
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
            "gpu tuning: {} ({:?}, driver {}): core {:?}, memory {:?}, power {:?} %",
            info.name,
            info.family,
            info.driver,
            limits.core,
            limits.mem,
            limits.power
        );
        return Ok(Box::new(NvidiaCard {
            device,
            info,
            limits,
            default_mw,
        }));
    }
    Err(match seen {
        Some(name) => TuneCapability::ReadOnly {
            reason: format!("Vyro tunes NVIDIA RTX 20 to RTX 50 cards; the {name} is not one"),
        },
        None => TuneCapability::Unavailable {
            reason: "no NVIDIA card found".into(),
        },
    })
}

impl NvidiaCard {
    fn offset(&self, clock: Clock) -> Result<i32, String> {
        self.device
            .clock_offset(clock, PerformanceState::Zero)
            .map(|o| o.clock_offset_mhz)
            .map_err(|e| e.to_string())
    }

    fn set_offset(&mut self, clock: Clock, mhz: i32) -> Result<(), String> {
        self.device
            .set_clock_offset(clock, PerformanceState::Zero, mhz)
            .map_err(|e| format!("{clock:?} offset: {e}"))
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

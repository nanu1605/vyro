//! GPU telemetry. Every adapter gets Task Manager's engine utilization and VRAM usage from PDH;
//! NVIDIA cards add NVML (clocks, temperature, power, fan, PCIe link, throttle reasons, the
//! same source as `nvidia-smi`); other vendors fall back to the WDDM performance data that
//! Task Manager's GPU temperature comes from. Vendor libraries add the rest: NVAPI (hotspot /
//! memory temperature, voltage, fan RPM), AMD ADL and Intel IGCL.

use std::collections::HashMap;
use std::ffi::c_void;

use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;
use windows::Wdk::Graphics::Direct3D::{
    D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo, D3DKMT_ADAPTERADDRESS,
    D3DKMT_ADAPTER_PERFDATA, D3DKMT_CLOSEADAPTER, D3DKMT_NODE_PERFDATA, D3DKMT_OPENADAPTERFROMLUID,
    D3DKMT_QUERYADAPTERINFO, KMTQAITYPE_ADAPTERADDRESS, KMTQAITYPE_ADAPTERPERFDATA,
    KMTQAITYPE_NODEPERFDATA, KMTQUERYADAPTERINFOTYPE,
};
use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

use super::adl::Adl;
use super::igcl::Igcl;
use super::nvapi::NvApi;
use super::pdh::{Counter, Query};
use super::GpuShare;
use crate::hwid::pci;
use crate::lock;
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{GpuSnapshot, SensorSnapshot};
use crate::usage::engines::{luid_key, parse_adapter_instance, summarize};
use crate::usage::gpu::{
    kmt_mhz, kmt_rpm, kmt_temp_c, likely_integrated, match_nvml, nvml_throttle, AdapterKey, NvmlPci,
};
use crate::usage::vendor::VendorReadings;
use crate::winutil::wide_array;

const MIB: f64 = 1024.0 * 1024.0;

struct Keys {
    usage: SensorKey,
    clock: SensorKey,
    mem_clock: SensorKey,
    temp: SensorKey,
    vram: SensorKey,
    power: SensorKey,
    fan: SensorKey,
    hotspot: SensorKey,
    mem_temp: SensorKey,
    voltage: SensorKey,
    engines: [SensorKey; 5],
}

struct Adapter {
    name: String,
    vendor_id: u32,
    device_id: u32,
    subsys_id: u32,
    luid: LUID,
    dedicated_mb: Option<f64>,
    integrated: bool,
    /// D3DKMT adapter handle, closed on drop.
    kmt: Option<u32>,
    /// PCI bus number (from D3DKMT), used to match vendor-library devices.
    bus: Option<u32>,
    nvml: Option<u32>,
    nvapi: Option<usize>,
    adl: Option<usize>,
    igcl: Option<usize>,
    keys: Option<Keys>,
}

impl Drop for Adapter {
    fn drop(&mut self) {
        if let Some(h) = self.kmt {
            // SAFETY: handle from D3DKMTOpenAdapterFromLuid, closed exactly once.
            let _ = unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: h }) };
        }
    }
}

struct Counters {
    engines: Counter,
    dedicated: Counter,
    query: Query,
}

pub struct GpuProvider {
    share: GpuShare,
    adapters: Vec<Adapter>,
    nvml: Option<Nvml>,
    nvapi: Option<NvApi>,
    adl: Option<Adl>,
    igcl: Option<Igcl>,
    pdh: Option<Counters>,
    /// Last ADL read error, so it is logged once rather than every tick.
    adl_error: Option<String>,
}

impl GpuProvider {
    pub fn new(share: GpuShare) -> Self {
        Self {
            share,
            adapters: Vec::new(),
            nvml: None,
            nvapi: None,
            adl: None,
            igcl: None,
            pdh: None,
            adl_error: None,
        }
    }
}

fn kmt_query<T>(h: u32, ty: KMTQUERYADAPTERINFOTYPE, mut v: T) -> Option<T> {
    let mut q = D3DKMT_QUERYADAPTERINFO {
        hAdapter: h,
        Type: ty,
        pPrivateDriverData: (&raw mut v).cast::<c_void>(),
        PrivateDriverDataSize: u32::try_from(std::mem::size_of::<T>()).ok()?,
    };
    // SAFETY: the private data points at a live `T` of the declared size.
    unsafe { D3DKMTQueryAdapterInfo(&mut q) }
        .is_ok()
        .then_some(v)
}

fn enumerate() -> Vec<Adapter> {
    let mut out: Vec<Adapter> = Vec::new();
    // SAFETY: plain factory creation.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    for i in 0.. {
        // SAFETY: enumerating until DXGI_ERROR_NOT_FOUND.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        // SAFETY: valid adapter.
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        let software = desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
        let luid = desc.AdapterLuid;
        if software
            || pci::is_basic_render_driver(desc.VendorId, desc.DeviceId)
            || out
                .iter()
                .any(|a| a.luid.HighPart == luid.HighPart && a.luid.LowPart == luid.LowPart)
        {
            continue;
        }
        let name = wide_array(&desc.Description);
        let dedicated_mb =
            (desc.DedicatedVideoMemory > 0).then(|| desc.DedicatedVideoMemory as f64 / MIB);
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: luid,
            hAdapter: 0,
        };
        // SAFETY: valid in/out struct.
        let kmt = unsafe { D3DKMTOpenAdapterFromLuid(&mut open) }
            .is_ok()
            .then_some(open.hAdapter);
        let bus = kmt.and_then(|h| {
            kmt_query(
                h,
                KMTQAITYPE_ADAPTERADDRESS,
                D3DKMT_ADAPTERADDRESS::default(),
            )
            .map(|addr| addr.BusNumber)
        });
        out.push(Adapter {
            integrated: likely_integrated(desc.VendorId, &name, dedicated_mb),
            name,
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            subsys_id: desc.SubSysId,
            luid,
            dedicated_mb,
            kmt,
            bus,
            nvml: None,
            nvapi: None,
            adl: None,
            igcl: None,
            keys: None,
        });
    }
    // Discrete first (the dashboard gauge shows the first GPU), then by VRAM.
    out.sort_by(|a, b| {
        a.integrated.cmp(&b.integrated).then(
            b.dedicated_mb
                .unwrap_or(0.0)
                .total_cmp(&a.dedicated_mb.unwrap_or(0.0)),
        )
    });
    out
}

fn init_nvml() -> Option<Nvml> {
    let nvml = Nvml::init()
        .or_else(|_| {
            Nvml::builder()
                .lib_path(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll".as_ref())
                .init()
        })
        .map_err(|e| log::info!("gpu: NVML unavailable: {e}"))
        .ok()?;
    log::info!(
        "gpu: NVML {} (driver {})",
        nvml.sys_nvml_version().unwrap_or_default(),
        nvml.sys_driver_version().unwrap_or_default()
    );
    Some(nvml)
}

fn register(reg: &mut Registry, i: usize, prefix: &str, src: Source) -> Keys {
    let mut r = |what: &str, name: &str, unit, source, history: bool| {
        let d = SensorDesc::new(
            format!("gpu.{i}.{what}"),
            Group::Gpu,
            format!("{prefix}{name}"),
            unit,
            source,
        );
        reg.register(if history { d.with_history() } else { d })
    };
    Keys {
        usage: r("usage", "Usage", Unit::Percent, Source::Pdh, true),
        clock: r("clock.core", "Core Clock", Unit::Mhz, src, true),
        mem_clock: r("clock.mem", "Memory Clock", Unit::Mhz, src, false),
        temp: r("temp.edge", "Temperature", Unit::Celsius, src, true),
        vram: r("vram.used", "VRAM Used", Unit::Megabyte, src, true),
        power: r("power", "Power", Unit::Watt, src, true),
        fan: r("fan", "Fan", Unit::Percent, src, false),
        hotspot: r("temp.hotspot", "Hotspot", Unit::Celsius, src, true),
        mem_temp: r("temp.mem", "Memory Temperature", Unit::Celsius, src, false),
        voltage: r("voltage", "Core Voltage", Unit::Volt, src, false),
        engines: [
            r("engine.3d", "3D", Unit::Percent, Source::Pdh, false),
            r(
                "engine.decode",
                "Video Decode",
                Unit::Percent,
                Source::Pdh,
                false,
            ),
            r(
                "engine.encode",
                "Video Encode",
                Unit::Percent,
                Source::Pdh,
                false,
            ),
            r(
                "engine.compute",
                "Compute",
                Unit::Percent,
                Source::Pdh,
                false,
            ),
            r("engine.copy", "Copy", Unit::Percent, Source::Pdh, false),
        ],
    }
}

/// Fills vendor-library readings from NVML.
fn read_nvml(nvml: &Nvml, index: u32, g: &mut GpuSnapshot) {
    let Ok(d) = nvml.device_by_index(index) else {
        return;
    };
    g.mhz_core = d.clock_info(Clock::Graphics).ok().map(f64::from);
    g.mhz_mem = d.clock_info(Clock::Memory).ok().map(f64::from);
    g.temp_edge = d.temperature(TemperatureSensor::Gpu).ok().map(f64::from);
    if let Ok(m) = d.memory_info() {
        g.vram_used_mb = Some(m.used as f64 / MIB);
        g.vram_total_mb = Some(m.total as f64 / MIB);
    }
    g.power_w = d.power_usage().ok().map(|mw| f64::from(mw) / 1000.0);
    g.power_limit_w = d
        .enforced_power_limit()
        .ok()
        .map(|mw| f64::from(mw) / 1000.0);
    if d.num_fans().is_ok_and(|n| n > 0) {
        g.fan_pct = d.fan_speed(0).ok().map(f64::from);
    }
    g.pcie_gen = d.current_pcie_link_gen().ok();
    g.pcie_width = d.current_pcie_link_width().ok();
    if let Ok(t) = d.current_throttle_reasons() {
        let t = nvml_throttle(t.bits());
        g.throttle_thermal = Some(t.thermal);
        g.throttle_power_limit = Some(t.power);
        g.throttle_reasons = t.reasons;
    }
    if g.usage.is_none() {
        g.usage = d.utilization_rates().ok().map(|u| f64::from(u.gpu));
    }
}

fn log_once(last: &mut Option<String>, e: String) {
    if last.as_deref() != Some(e.as_str()) {
        log::info!("gpu: {e}");
        *last = Some(e);
    }
}

/// Fills readings still missing from a vendor library.
fn apply(g: &mut GpuSnapshot, r: VendorReadings) {
    g.mhz_core = g.mhz_core.or(r.mhz_core);
    g.mhz_mem = g.mhz_mem.or(r.mhz_mem);
    g.temp_edge = g.temp_edge.or(r.temp_edge);
    g.temp_hotspot = g.temp_hotspot.or(r.temp_hotspot);
    g.temp_mem = g.temp_mem.or(r.temp_mem);
    g.fan_rpm = g.fan_rpm.or(r.fan_rpm);
    g.fan_pct = g.fan_pct.or(r.fan_pct);
    g.voltage_v = g.voltage_v.or(r.voltage_v);
    g.power_w = g.power_w.or(r.power_w);
    g.throttle_power_limit = g.throttle_power_limit.or(r.throttle_power);
    g.throttle_thermal = g.throttle_thermal.or(r.throttle_thermal);
    if g.throttle_reasons.is_empty() {
        if r.throttle_power == Some(true) {
            g.throttle_reasons.push("power".to_owned());
        }
        if r.throttle_thermal == Some(true) {
            g.throttle_reasons.push("thermal".to_owned());
        }
    }
}

/// Fills gaps from WDDM performance data (driver-dependent; zeros mean "not reported").
fn read_kmt(h: u32, g: &mut GpuSnapshot) {
    if let Some(p) = kmt_query(
        h,
        KMTQAITYPE_ADAPTERPERFDATA,
        D3DKMT_ADAPTER_PERFDATA::default(),
    ) {
        g.temp_edge = g.temp_edge.or(kmt_temp_c(p.Temperature));
        g.fan_rpm = g.fan_rpm.or(kmt_rpm(p.FanRPM));
        g.mhz_mem = g.mhz_mem.or(kmt_mhz(p.MemoryFrequency));
    }
    if g.mhz_core.is_none() {
        // Node 0 is the 3D engine on every WDDM driver.
        let node = kmt_query(h, KMTQAITYPE_NODEPERFDATA, D3DKMT_NODE_PERFDATA::default());
        g.mhz_core = node.and_then(|n| kmt_mhz(n.Frequency));
    }
}

impl Provider for GpuProvider {
    fn name(&self) -> &'static str {
        "gpu"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        self.adapters = enumerate();
        if self.adapters.is_empty() {
            return Err("no hardware GPU found".into());
        }

        if self.adapters.iter().any(|a| a.vendor_id == 0x10DE) {
            self.nvml = init_nvml();
        }
        if let Some(nvml) = &self.nvml {
            let count = nvml.device_count().unwrap_or(0);
            let devices: Vec<NvmlPci> = (0..count)
                .filter_map(|i| nvml.device_by_index(i).ok()?.pci_info().ok())
                .map(|p| NvmlPci {
                    bus: p.bus,
                    pci_device_id: p.pci_device_id,
                    subsys_id: p.pci_sub_system_id,
                })
                .collect();
            let keys: Vec<AdapterKey> = self
                .adapters
                .iter()
                .map(|a| AdapterKey {
                    vendor_id: a.vendor_id,
                    device_id: a.device_id,
                    subsys_id: a.subsys_id,
                    bus: a.bus,
                })
                .collect();
            // Device indices are dense only when every handle opened; keep NVML's numbering.
            let indices: Vec<u32> = (0..count)
                .filter(|&i| nvml.device_by_index(i).and_then(|d| d.pci_info()).is_ok())
                .collect();
            for (a, m) in self.adapters.iter_mut().zip(match_nvml(&keys, &devices)) {
                a.nvml = m.and_then(|m| indices.get(m).copied());
            }
        }

        let has = |v: u32| self.adapters.iter().any(|a| a.vendor_id == v);
        let report = |what: &str, e: String| log::info!("gpu: {what} unavailable: {e}");
        if has(0x10DE) {
            self.nvapi = NvApi::load().map_err(|e| report("NVAPI", e)).ok();
        }
        if has(0x1002) {
            self.adl = Adl::load().map_err(|e| report("ADL", e)).ok();
        }
        if has(0x8086) {
            self.igcl = Igcl::load().map_err(|e| report("IGCL", e)).ok();
        }
        log::info!(
            "gpu: NVAPI={} ADL={} IGCL={}",
            self.nvapi.is_some(),
            self.adl.is_some(),
            self.igcl.is_some()
        );
        for a in &mut self.adapters {
            match a.vendor_id {
                0x10DE => a.nvapi = a.bus.and_then(|b| self.nvapi.as_ref()?.find(b)),
                0x1002 => a.adl = a.bus.and_then(|b| self.adl.as_ref()?.find(b)),
                0x8086 => a.igcl = self.igcl.as_ref().and_then(|l| l.find(a.luid)),
                _ => {}
            }
        }

        let multi = self.adapters.len() > 1;
        for (i, a) in self.adapters.iter_mut().enumerate() {
            let prefix = if multi {
                format!("GPU #{i} ")
            } else {
                "GPU ".to_owned()
            };
            let source = if a.nvml.is_some() {
                Source::Nvml
            } else if a.adl.is_some() {
                Source::Adl
            } else if a.igcl.is_some() {
                Source::Igcl
            } else {
                Source::Os
            };
            a.keys = Some(register(reg, i, &prefix, source));
            log::info!(
                "gpu {i}: {} ({}, bus={:?}, nvml={}, nvapi={}, adl={}, igcl={}, d3dkmt={})",
                a.name,
                if a.integrated {
                    "integrated"
                } else {
                    "discrete"
                },
                a.bus,
                a.nvml.is_some(),
                a.nvapi.is_some(),
                a.adl.is_some(),
                a.igcl.is_some(),
                a.kmt.is_some()
            );
        }

        match (|| -> Result<Counters, BoxError> {
            let query = Query::open()?;
            let engines = query.add(r"\GPU Engine(*)\Utilization Percentage")?;
            let dedicated = query.add(r"\GPU Adapter Memory(*)\Dedicated Usage")?;
            query.collect()?;
            Ok(Counters {
                engines,
                dedicated,
                query,
            })
        })() {
            Ok(c) => self.pdh = Some(c),
            Err(e) => log::warn!("gpu: engine counters unavailable: {e}"),
        }
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let (summary, dedicated) = match &self.pdh {
            Some(c) if c.query.collect().is_ok() => {
                let samples = c.engines.array();
                let summary = summarize(samples.iter().map(|(n, v)| (n.as_str(), *v)));
                let mut dedicated: HashMap<u64, f64> = HashMap::new();
                for (name, bytes) in c.dedicated.array() {
                    if let Some((luid, _)) = parse_adapter_instance(&name) {
                        *dedicated.entry(luid).or_default() += bytes;
                    }
                }
                (Some(summary), dedicated)
            }
            _ => (None, HashMap::new()),
        };
        *lock(&self.share) = summary
            .as_ref()
            .map(|s| s.per_pid.clone())
            .unwrap_or_default();

        for a in &self.adapters {
            let Some(k) = &a.keys else { continue };
            let luid = luid_key(a.luid.HighPart, a.luid.LowPart);
            let usage = summary.as_ref().and_then(|s| s.adapters.get(&luid));
            let mut g = GpuSnapshot {
                name: a.name.clone(),
                vendor: pci::vendor_name(a.vendor_id).map(str::to_owned),
                integrated: Some(a.integrated),
                usage: usage.map(|u| u.usage),
                engine: usage.map(|u| u.engines.clone()).unwrap_or_default(),
                vram_used_mb: dedicated.get(&luid).map(|b| b / MIB),
                vram_total_mb: a.dedicated_mb,
                ..Default::default()
            };
            if let (Some(nvml), Some(idx)) = (&self.nvml, a.nvml) {
                read_nvml(nvml, idx, &mut g);
            }
            if let (Some(nvapi), Some(idx)) = (&self.nvapi, a.nvapi) {
                apply(&mut g, nvapi.read(idx, &a.name));
            }
            if let (Some(adl), Some(idx)) = (&self.adl, a.adl) {
                match adl.read(idx) {
                    Ok(r) => apply(&mut g, r),
                    Err(e) => log_once(&mut self.adl_error, e),
                }
            }
            if let (Some(igcl), Some(idx)) = (&mut self.igcl, a.igcl) {
                apply(&mut g, igcl.read(idx).unwrap_or_default());
            }
            if let Some(h) = a.kmt {
                read_kmt(h, &mut g);
            }

            g.usage = reg.set(k.usage, g.usage);
            g.mhz_core = reg.set(k.clock, g.mhz_core);
            g.mhz_mem = reg.set(k.mem_clock, g.mhz_mem);
            g.temp_edge = reg.set(k.temp, g.temp_edge);
            g.vram_used_mb = reg.set(k.vram, g.vram_used_mb);
            g.power_w = reg.set(k.power, g.power_w);
            g.fan_pct = reg.set(k.fan, g.fan_pct);
            g.temp_hotspot = reg.set(k.hotspot, g.temp_hotspot);
            g.temp_mem = reg.set(k.mem_temp, g.temp_mem);
            g.voltage_v = reg.set(k.voltage, g.voltage_v);
            let e = &g.engine;
            for (key, v) in
                k.engines
                    .iter()
                    .zip([e.graphics3d, e.decode, e.encode, e.compute, e.copy])
            {
                reg.set(*key, v);
            }
            snap.gpu.push(g);
        }
        Ok(())
    }
}

//! Windows hardware inventory: SMBIOS, processor topology, DXGI adapters and registry.

use std::mem::{offset_of, size_of};

use windows::core::Interface;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIDevice, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, GetPhysicallyInstalledSystemMemory, GetSystemFirmwareTable,
    RelationAll, CACHE_RELATIONSHIP, FIRMWARE_TABLE_PROVIDER, GROUP_AFFINITY,
    PROCESSOR_RELATIONSHIP, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

use super::{pci, topology as t};
use crate::info::GpuId;
use crate::winutil::{wide_array, Key, HKEY_LOCAL_MACHINE};

// The topology decoder reads raw bytes; pin its offsets to the Windows header layouts.
const _: () = {
    assert!(offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous) == t::HEADER);
    assert!(
        t::HEADER + offset_of!(PROCESSOR_RELATIONSHIP, EfficiencyClass) == t::PROC_EFFICIENCY_CLASS
    );
    assert!(t::HEADER + offset_of!(PROCESSOR_RELATIONSHIP, GroupCount) == t::PROC_GROUP_COUNT);
    assert!(t::HEADER + offset_of!(PROCESSOR_RELATIONSHIP, GroupMask) == t::PROC_GROUP_MASK);
    assert!(t::HEADER + offset_of!(CACHE_RELATIONSHIP, Level) == t::CACHE_LEVEL);
    assert!(t::HEADER + offset_of!(CACHE_RELATIONSHIP, Associativity) == t::CACHE_ASSOCIATIVITY);
    assert!(t::HEADER + offset_of!(CACHE_RELATIONSHIP, LineSize) == t::CACHE_LINE_SIZE);
    assert!(t::HEADER + offset_of!(CACHE_RELATIONSHIP, CacheSize) == t::CACHE_SIZE);
    assert!(t::HEADER + offset_of!(CACHE_RELATIONSHIP, Type) == t::CACHE_TYPE);
    assert!(t::HEADER + offset_of!(CACHE_RELATIONSHIP, Anonymous) == t::CACHE_GROUP_MASK);
    assert!(size_of::<GROUP_AFFINITY>() == t::GROUP_AFFINITY_SIZE);
};

/// Raw `RawSMBIOSData` buffer.
pub fn smbios_raw() -> Option<Vec<u8>> {
    let rsmb = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    // SAFETY: size query with no buffer.
    let size = unsafe { GetSystemFirmwareTable(rsmb, 0, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` is `size` bytes.
    let got = unsafe { GetSystemFirmwareTable(rsmb, 0, Some(&mut buf)) };
    if got == 0 || got > size {
        return None;
    }
    buf.truncate(got as usize);
    Some(buf)
}

/// Raw `GetLogicalProcessorInformationEx(RelationAll)` buffer.
pub fn topology_raw() -> Option<Vec<u8>> {
    let mut len = 0u32;
    // SAFETY: size query; fails with ERROR_INSUFFICIENT_BUFFER and sets `len`.
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationAll, None, &mut len) };
    if len == 0 {
        return None;
    }
    // u64 storage keeps the records 8-byte aligned, as the API expects.
    let mut words = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: `words` holds at least `len` bytes.
    unsafe {
        GetLogicalProcessorInformationEx(RelationAll, Some(words.as_mut_ptr().cast()), &mut len)
    }
    .ok()?;
    let mut bytes: Vec<u8> = words.iter().flat_map(|w| w.to_ne_bytes()).collect();
    bytes.truncate(len as usize);
    Some(bytes)
}

pub fn installed_memory_mb() -> Option<f64> {
    let mut kb = 0u64;
    // SAFETY: valid out-pointer.
    unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }.ok()?;
    (kb > 0).then(|| kb as f64 / 1024.0)
}

const DISPLAY_CLASS: &str =
    r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

/// Hardware GPUs from DXGI, enriched with driver details from the display class registry key.
pub fn gpus() -> Vec<GpuId> {
    let mut out: Vec<GpuId> = Vec::new();
    // SAFETY: plain COM factory creation.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    let mut seen_luids = Vec::new();
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
        if software || pci::is_basic_render_driver(desc.VendorId, desc.DeviceId) {
            continue;
        }
        let luid = (desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart);
        if seen_luids.contains(&luid) {
            continue;
        }
        seen_luids.push(luid);
        // SAFETY: IDXGIDevice's IID is a valid GUID pointer for the duration of the call.
        let umd = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
            .ok()
            .map(|v| pci::format_umd_version(v as u64));
        const MIB: f64 = 1024.0 * 1024.0;
        out.push(GpuId {
            name: wide_array(&desc.Description),
            vendor: pci::vendor_name(desc.VendorId).map(str::to_owned),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            subsys_id: desc.SubSysId,
            revision: desc.Revision,
            vram_mb: (desc.DedicatedVideoMemory > 0)
                .then(|| desc.DedicatedVideoMemory as f64 / MIB),
            shared_mb: (desc.SharedSystemMemory > 0).then(|| desc.SharedSystemMemory as f64 / MIB),
            driver_version: umd,
            ..Default::default()
        });
    }
    enrich_from_registry(&mut out);
    out
}

fn enrich_from_registry(gpus: &mut [GpuId]) {
    let Some(class) = Key::open(HKEY_LOCAL_MACHINE, DISPLAY_CLASS) else {
        return;
    };
    for sub in class.subkeys() {
        if !(sub.len() == 4 && sub.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let Some(k) = class.subkey(&sub) else {
            continue;
        };
        let Some(id) = k
            .string("MatchingDeviceId")
            .and_then(|s| pci::parse_hardware_id(&s))
        else {
            continue;
        };
        let Some(gpu) = gpus
            .iter_mut()
            .find(|g| g.vendor_id == u32::from(id.vendor) && g.device_id == u32::from(id.device))
        else {
            continue;
        };
        if gpu.driver_version.is_none() {
            gpu.driver_version = k.string("DriverVersion");
        }
        gpu.driver_date = gpu.driver_date.take().or_else(|| k.string("DriverDate"));
        gpu.bios_version = gpu.bios_version.take().or_else(|| {
            k.value("HardwareInformation.BiosString")
                .and_then(|v| pci::decode_reg_text(&v.data))
        });
        gpu.memory_type = gpu.memory_type.take().or_else(|| {
            k.value("HardwareInformation.MemoryType")
                .and_then(|v| pci::decode_reg_text(&v.data))
        });
        gpu.vendor_driver_version = match gpu.vendor_id {
            0x10DE => gpu
                .driver_version
                .as_deref()
                .and_then(pci::nvidia_driver_version),
            0x1002 => k.string("RadeonSoftwareVersion"),
            _ => gpu.driver_version.clone(),
        };
    }
}

/// Chipset name from the PCI LPC / eSPI bridge (class 0601) description. Intel names the
/// chipset there; AMD boards usually report a generic "PCI standard ISA bridge" → `None`.
pub fn chipset() -> Option<String> {
    let pci = Key::open(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Enum\PCI")?;
    for dev in pci.subkeys() {
        let Some(dev_key) = pci.subkey(&dev) else {
            continue;
        };
        for inst in dev_key.subkeys() {
            let Some(k) = dev_key.subkey(&inst) else {
                continue;
            };
            let is_isa_bridge = k
                .value("CompatibleIDs")
                .map(|v| v.as_multi_string())
                .unwrap_or_default()
                .iter()
                .any(|id| id.eq_ignore_ascii_case(r"PCI\CC_0601"));
            if is_isa_bridge {
                if let Some(name) = k
                    .string("DeviceDesc")
                    .and_then(|d| pci::chipset_from_description(&d))
                {
                    return Some(name);
                }
            }
        }
    }
    None
}

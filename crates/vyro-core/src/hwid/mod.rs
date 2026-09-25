//! Hardware identification (Phase 1): CPU, memory, board and GPU inventory, read once at
//! startup. Decoders are platform-neutral and fixture-tested; Windows collection lives in
//! `win`.

pub mod cpuid;
pub mod pci;
pub mod smbios;
pub mod topology;
#[cfg(windows)]
mod win;

use crate::info::HardwareId;

/// Collects the hardware inventory. Slow-ish (tens to hundreds of ms): call once, off the UI
/// thread.
pub fn collect() -> HardwareId {
    let mut hw = HardwareId::default();
    hw.cpu.architecture = Some(
        match std::env::consts::ARCH {
            "x86_64" => "x86-64",
            "aarch64" => "ARM64",
            other => other,
        }
        .to_owned(),
    );
    hw.cpu.threads = std::thread::available_parallelism()
        .ok()
        .and_then(|n| u32::try_from(n.get()).ok());

    #[cfg(target_arch = "x86_64")]
    apply_cpuid(&mut hw, &cpuid::read_live());

    #[cfg(windows)]
    collect_windows(&mut hw);

    hw
}

/// Whether CPUID reports a hypervisor underneath the OS (`None` off x86-64).
pub fn cpuid_hypervisor_present() -> Option<bool> {
    #[cfg(target_arch = "x86_64")]
    return Some(cpuid::read_live().hypervisor_present);
    #[cfg(not(target_arch = "x86_64"))]
    None
}

#[cfg_attr(not(target_arch = "x86_64"), allow(dead_code))]
fn apply_cpuid(hw: &mut HardwareId, id: &cpuid::CpuidInfo) {
    let cpu = &mut hw.cpu;
    cpu.vendor = (!id.vendor.is_empty()).then(|| id.vendor.clone());
    cpu.brand = id.brand.clone();
    cpu.family = Some(id.family);
    cpu.model = Some(id.model);
    cpu.stepping = Some(id.stepping);
    cpu.isa = id.isa.clone();
    cpu.hybrid = id.hybrid;
    cpu.base_mhz = id.base_mhz.map(f64::from);
    cpu.max_mhz = id.max_mhz.map(f64::from);
}

/// Folds a decoded topology into the CPU record.
#[cfg_attr(not(windows), allow(dead_code))]
fn apply_topology(hw: &mut HardwareId, topo: topology::Topology) {
    let cpu = &mut hw.cpu;
    if topo.packages > 0 {
        cpu.packages = Some(topo.packages);
    }
    if !topo.cores.is_empty() {
        cpu.cores = Some(topo.cores.len() as u32);
        cpu.threads = Some(topo.threads);
    }
    cpu.performance_cores = topo.performance_cores;
    cpu.efficiency_cores = topo.efficiency_cores;
    cpu.hybrid |= topo.performance_cores.is_some();
    cpu.core_map = topo.cores;
    cpu.caches = topo.caches;
}

/// Folds decoded SMBIOS data into the record, without overriding better sources.
#[cfg_attr(not(windows), allow(dead_code))]
fn apply_smbios(hw: &mut HardwareId, s: &smbios::Smbios) {
    hw.smbios_version = Some(s.version());
    hw.board = smbios::board(s);
    hw.memory = smbios::memory(s);
    let p = smbios::processor(s);
    let cpu = &mut hw.cpu;
    cpu.socket = p.socket;
    cpu.base_mhz = cpu.base_mhz.or(p.current_mhz.map(f64::from));
    cpu.max_mhz = cpu.max_mhz.or(p.max_mhz.map(f64::from));
    if cpu.cores.is_none() {
        cpu.cores = p.cores;
    }
    if cpu.packages.is_none() && p.populated_sockets > 0 {
        cpu.packages = Some(p.populated_sockets);
    }
}

#[cfg(windows)]
fn collect_windows(hw: &mut HardwareId) {
    if let Some(raw) = win::smbios_raw() {
        if let Some(s) = smbios::Smbios::from_raw(&raw) {
            apply_smbios(hw, &s);
        }
    } else {
        log::warn!("hwid: SMBIOS table unavailable");
    }
    if let Some(raw) = win::topology_raw() {
        apply_topology(hw, topology::parse(&raw));
    }
    if let Some(mb) = win::installed_memory_mb() {
        hw.memory.total_mb = Some(mb);
    }
    hw.board.chipset = win::chipset();
    hw.gpus = win::gpus();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::CoreKind;

    #[test]
    fn collect_runs_on_any_host() {
        let hw = collect();
        assert!(hw.cpu.architecture.is_some());
        assert!(hw.cpu.threads.unwrap_or(0) >= 1);
        #[cfg(target_arch = "x86_64")]
        assert!(hw.cpu.vendor.is_some());
    }

    #[test]
    fn topology_overrides_smbios_counts_and_sets_hybrid() {
        let mut hw = HardwareId::default();
        let mut buf = Vec::new();
        buf.extend(topology::fixture::core(1, 0b11));
        buf.extend(topology::fixture::core(0, 0b100));
        buf.extend(topology::fixture::package(0b111));
        apply_topology(&mut hw, topology::parse(&buf));

        let raw = smbios::fixture::raw(
            3,
            4,
            &[(
                4,
                smbios::fixture::body(0x30, &[(0x05, &[3]), (0x18, &[0x41]), (0x23, &[16])]),
                &[],
            )],
        );
        apply_smbios(&mut hw, &smbios::Smbios::from_raw(&raw).unwrap());

        assert_eq!(
            hw.cpu.cores,
            Some(2),
            "topology wins over SMBIOS core count"
        );
        assert_eq!(hw.cpu.threads, Some(3));
        assert_eq!(hw.cpu.packages, Some(1));
        assert!(hw.cpu.hybrid);
        assert_eq!(hw.cpu.core_map[1].kind, CoreKind::Efficiency);
        assert_eq!(hw.smbios_version.as_deref(), Some("3.4"));
    }

    /// On a real Windows machine (CI), the decoders must agree with the OS.
    #[cfg(windows)]
    #[test]
    fn windows_inventory_is_consistent() {
        let hw = collect();
        let os_threads = std::thread::available_parallelism().unwrap().get() as u32;
        assert_eq!(
            hw.cpu.threads,
            Some(os_threads),
            "topology thread count matches the OS"
        );
        assert!(hw.cpu.cores.unwrap() >= 1);
        assert!(!hw.cpu.caches.is_empty(), "caches decoded");
        assert!(hw.smbios_version.is_some(), "SMBIOS table read");
        assert!(hw.board.bios_vendor.is_some() || hw.board.system_manufacturer.is_some());
    }
}

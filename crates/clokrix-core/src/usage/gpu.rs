//! Vendor-neutral GPU helpers: NVML throttle-reason decoding, NVML ↔ DXGI adapter matching and
//! D3DKMT performance-data unit conversion.

/// What the rest of the engine knows about a DXGI adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterKey {
    pub vendor_id: u32,
    pub device_id: u32,
    pub subsys_id: u32,
    /// PCI bus number from D3DKMT, when available.
    pub bus: Option<u32>,
}

/// An NVML device's PCI identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvmlPci {
    pub bus: u32,
    /// `device << 16 | vendor`, as NVML reports it.
    pub pci_device_id: u32,
    pub subsys_id: Option<u32>,
}

/// For each adapter, the index of the NVML device describing it. Matches on PCI bus number
/// when both sides know it, else on device + subsystem id in enumeration order (identical
/// cards without a bus number may then pair up in the wrong order).
pub fn match_nvml(adapters: &[AdapterKey], devices: &[NvmlPci]) -> Vec<Option<usize>> {
    let mut used = vec![false; devices.len()];
    let mut out = vec![None; adapters.len()];
    for (ai, a) in adapters.iter().enumerate() {
        if a.vendor_id != 0x10DE {
            continue;
        }
        let same_card = |d: &NvmlPci| {
            d.pci_device_id & 0xFFFF == a.vendor_id && d.pci_device_id >> 16 == a.device_id
        };
        let by_bus = a.bus.and_then(|bus| {
            devices
                .iter()
                .enumerate()
                .position(|(i, d)| !used[i] && d.bus == bus && same_card(d))
        });
        let by_id = || {
            devices.iter().enumerate().position(|(i, d)| {
                !used[i] && same_card(d) && d.subsys_id.is_none_or(|s| s == a.subsys_id)
            })
        };
        if let Some(i) = by_bus.or_else(by_id) {
            used[i] = true;
            out[ai] = Some(i);
        }
    }
    out
}

/// Decoded `nvmlClocksEventReasons` bits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Throttle {
    pub reasons: Vec<String>,
    pub thermal: bool,
    pub power: bool,
}

/// Decodes NVML clock-event ("throttle") reason bits. Idle and informational bits (GPU idle,
/// application / display clock settings, sync boost) are not reported as throttling.
pub fn nvml_throttle(bits: u64) -> Throttle {
    const SW_POWER_CAP: u64 = 0x4;
    const HW_SLOWDOWN: u64 = 0x8;
    const SW_THERMAL: u64 = 0x20;
    const HW_THERMAL: u64 = 0x40;
    const HW_POWER_BRAKE: u64 = 0x80;
    let mut t = Throttle::default();
    for (bit, name) in [
        (SW_POWER_CAP, "power cap"),
        (HW_SLOWDOWN, "hardware slowdown"),
        (SW_THERMAL, "thermal (driver)"),
        (HW_THERMAL, "thermal (hardware)"),
        (HW_POWER_BRAKE, "power brake"),
    ] {
        if bits & bit != 0 {
            t.reasons.push(name.to_owned());
        }
    }
    t.thermal = bits & (SW_THERMAL | HW_THERMAL) != 0;
    t.power = bits & (SW_POWER_CAP | HW_POWER_BRAKE) != 0;
    t
}

/// Best guess at whether an adapter is an integrated GPU (for ordering and labels). Discrete
/// AMD / Intel cards carry ≥ 4 GB of VRAM; iGPUs a small carve-out and a generic name.
pub fn likely_integrated(vendor_id: u32, name: &str, dedicated_mb: Option<f64>) -> bool {
    if !matches!(vendor_id, 0x1002 | 0x8086) {
        return false;
    }
    if dedicated_mb.is_none_or(|mb| mb <= 2048.0) {
        return true;
    }
    let n = name.to_ascii_lowercase();
    let generic_radeon = n.ends_with("radeon(tm) graphics") || n.ends_with("radeon graphics");
    // Mobile Radeon iGPUs: "Radeon 780M Graphics", "Radeon 890M".
    let radeon_m = n
        .split_whitespace()
        .any(|w| w.len() == 4 && w.ends_with('m') && w[..3].chars().all(|c| c.is_ascii_digit()));
    let intel_igpu = ["uhd", "iris", "hd graphics"].iter().any(|k| n.contains(k))
        || n.ends_with("intel(r) graphics")
        || n.ends_with("arc(tm) graphics");
    generic_radeon || radeon_m || intel_igpu
}

/// D3DKMT reports temperature in deci-°C; 0 means "not reported".
pub fn kmt_temp_c(deci: u32) -> Option<f64> {
    (deci > 0 && deci < 2000).then(|| f64::from(deci) / 10.0)
}

/// D3DKMT reports frequencies in Hz; 0 means "not reported".
pub fn kmt_mhz(hz: u64) -> Option<f64> {
    (hz > 0).then(|| hz as f64 / 1e6)
}

/// A non-zero RPM, as a reading.
pub fn kmt_rpm(rpm: u32) -> Option<f64> {
    (rpm > 0).then(|| f64::from(rpm))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nv(device: u32, subsys: u32, bus: Option<u32>) -> AdapterKey {
        AdapterKey {
            vendor_id: 0x10DE,
            device_id: device,
            subsys_id: subsys,
            bus,
        }
    }

    fn dev(bus: u32, device: u32, subsys: u32) -> NvmlPci {
        NvmlPci {
            bus,
            pci_device_id: device << 16 | 0x10DE,
            subsys_id: Some(subsys),
        }
    }

    #[test]
    fn matches_by_bus_then_ids() {
        let amd = AdapterKey {
            vendor_id: 0x1002,
            device_id: 0x164E,
            subsys_id: 0,
            bus: Some(0x0E),
        };
        let adapters = [
            amd,
            nv(0x2D04, 0x1111, Some(2)),
            nv(0x2D04, 0x1111, Some(1)),
        ];
        let devices = [dev(1, 0x2D04, 0x1111), dev(2, 0x2D04, 0x1111)];
        assert_eq!(
            match_nvml(&adapters, &devices),
            vec![None, Some(1), Some(0)]
        );
        // no bus numbers → id match in order
        let adapters = [nv(0x2D04, 0x1111, None), nv(0x2786, 0x2222, None)];
        let devices = [dev(5, 0x2786, 0x2222), dev(1, 0x2D04, 0x1111)];
        assert_eq!(match_nvml(&adapters, &devices), vec![Some(1), Some(0)]);
        // nothing matches
        assert_eq!(match_nvml(&[nv(0x1, 0, None)], &devices), vec![None]);
    }

    #[test]
    fn throttle_bits() {
        assert_eq!(
            nvml_throttle(0x1),
            Throttle::default(),
            "idle is not throttling"
        );
        let t = nvml_throttle(0x4 | 0x40);
        assert_eq!(t.reasons, vec!["power cap", "thermal (hardware)"]);
        assert!(t.thermal && t.power);
    }

    #[test]
    fn integrated_guess() {
        assert!(likely_integrated(
            0x1002,
            "AMD Radeon(TM) Graphics",
            Some(512.0)
        ));
        assert!(likely_integrated(
            0x1002,
            "AMD Radeon 780M Graphics",
            Some(4096.0)
        ));
        assert!(likely_integrated(
            0x8086,
            "Intel(R) UHD Graphics 770",
            Some(128.0)
        ));
        assert!(likely_integrated(
            0x8086,
            "Intel(R) Arc(TM) Graphics",
            Some(8192.0)
        ));
        assert!(!likely_integrated(
            0x8086,
            "Intel(R) Arc(TM) A770 Graphics",
            Some(16256.0)
        ));
        assert!(!likely_integrated(
            0x1002,
            "AMD Radeon RX 7900 XTX",
            Some(24560.0)
        ));
        assert!(!likely_integrated(
            0x10DE,
            "NVIDIA GeForce RTX 5060 Ti",
            Some(512.0)
        ));
    }

    #[test]
    fn kmt_units() {
        assert_eq!(kmt_temp_c(455), Some(45.5));
        assert_eq!(kmt_temp_c(0), None);
        assert_eq!(kmt_mhz(2_550_000_000), Some(2550.0));
        assert_eq!(kmt_mhz(0), None);
        assert_eq!(kmt_rpm(0), None);
    }
}

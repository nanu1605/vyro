//! AMD Zen 1–5 CPU sensors through PawnIO's `AMDFamily17` module (Phase 6b): Tctl/Tdie and
//! per-CCD temperatures from SMN, package and per-core power from the RAPL energy counters,
//! the requested core VID, and the effective clock from APERF. Decoders here are pure;
//! `win` does the reads. Register layouts follow Linux `k10temp` and LibreHardwareMonitor.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::AmdCpuProvider;

/// SMN: `THM_TCON_CUR_TMP`, the control temperature Tctl.
pub const SMN_THM_TCON_CUR_TMP: u32 = 0x0005_9800;
/// SMN: first CCD temperature on Zen 2/3 desktop and Threadripper 3000.
pub const SMN_CCD_TEMP_ZEN2: u32 = 0x0005_9954;
/// SMN: first CCD temperature on Raphael (Zen 4) and Granite Ridge (Zen 5).
pub const SMN_CCD_TEMP_ZEN4: u32 = 0x0005_9B08;
pub const MAX_CCDS: u32 = 8;

pub const MSR_MPERF_RO: u32 = 0xC000_00E7;
pub const MSR_APERF_RO: u32 = 0xC000_00E8;
pub const MSR_HW_PSTATE_STATUS: u32 = 0xC001_0293;
pub const MSR_PWR_UNIT: u32 = 0xC001_0299;
pub const MSR_CORE_ENERGY: u32 = 0xC001_029A;
pub const MSR_PKG_ENERGY: u32 = 0xC001_029B;

const RANGE_SEL: u32 = 1 << 19;
const TJ_SEL: u32 = 0b11 << 16;
const CCD_VALID: u32 = 1 << 11;

/// Families the `AMDFamily17` module accepts.
pub fn supported(vendor: &str, family: u32) -> bool {
    vendor == "AuthenticAMD" && (0x17..=0x1A).contains(&family)
}

/// Tctl in °C from `THM_TCON_CUR_TMP`: bits 31:21 in 1/8 °C, 49 °C lower when the range
/// select bit (or, on newer parts, both TJ_SEL bits) is set.
pub fn tctl(raw: u32) -> f64 {
    let t = f64::from(raw >> 21) * 0.125;
    if raw & RANGE_SEL != 0 || raw & TJ_SEL == TJ_SEL {
        t - 49.0
    } else {
        t
    }
}

/// Tctl − Tdie for the few models that report an offset control temperature (k10temp table);
/// zero for everything from Zen 2 on.
pub fn tctl_offset(brand: &str) -> f64 {
    if ["1600X", "1700X", "1800X"]
        .iter()
        .any(|m| brand.contains(m))
    {
        20.0
    } else if brand.contains("Threadripper 19") || brand.contains("Threadripper 29") {
        27.0
    } else if brand.contains("2700X") {
        10.0
    } else {
        0.0
    }
}

/// SMN address of the first CCD temperature register, for models known to have them.
pub fn ccd_base(family: u32, model: u32) -> Option<u32> {
    match (family, model) {
        (0x19, 0x61) | (0x1A, 0x44) => Some(SMN_CCD_TEMP_ZEN4),
        (0x17, 0x31 | 0x71) | (0x19, 0x21) => Some(SMN_CCD_TEMP_ZEN2),
        _ => None,
    }
}

/// A CCD temperature in °C; `None` when the slot has no die (valid bit clear) or reads
/// implausibly.
pub fn ccd_temp(raw: u32) -> Option<f64> {
    if raw & CCD_VALID == 0 {
        return None;
    }
    let t = f64::from(raw & 0x7FF) * 0.125 - 49.0;
    (t > -40.0 && t < 125.0).then_some(t)
}

/// Joules per energy-counter increment: 1 / 2^ESU, ESU in `PWR_UNIT` bits 12:8.
pub fn energy_unit(pwr_unit: u64) -> f64 {
    0.5f64.powi(i32::try_from((pwr_unit >> 8) & 0x1F).unwrap_or(16))
}

/// `CurCpuVid` from `HW_PSTATE_STATUS` (bits 21:14).
pub const fn vid(pstate_status: u64) -> u32 {
    ((pstate_status >> 14) & 0xFF) as u32
}

/// Whether the part uses SVI3 voltage encoding (Zen 4 and later) rather than SVI2.
pub const fn svi3(family: u32, model: u32) -> bool {
    family >= 0x1A || (family == 0x19 && model >= 0x60)
}

/// Requested core voltage for a VID: SVI2 is 1.55 V − 6.25 mV·VID, SVI3 0.245 V + 5 mV·VID.
pub fn vid_volts(vid: u32, svi3: bool) -> f64 {
    if svi3 {
        0.245 + 0.005 * f64::from(vid)
    } else {
        1.55 - 0.006_25 * f64::from(vid)
    }
}

/// Rate of a wrapping hardware counter: units per second between successive samples.
#[derive(Debug, Default, Clone)]
pub struct Rate {
    last: Option<(u64, f64)>,
}

impl Rate {
    /// Feeds a raw reading taken at `t` seconds; `bits` is the counter width. `None` on the
    /// first sample or when time did not advance.
    pub fn update(&mut self, raw: u64, t: f64, bits: u32) -> Option<f64> {
        let mask = if bits >= 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        let raw = raw & mask;
        let prev = self.last.replace((raw, t));
        let (p, pt) = prev?;
        let dt = t - pt;
        if dt <= 0.0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        let delta = raw.wrapping_sub(p) & mask;
        #[allow(clippy::cast_precision_loss)]
        Some(delta as f64 / dt)
    }
}

/// Power in W from an energy-counter rate (increments/s); implausible values are dropped
/// (a counter reset or a missed wrap), as are negatives that cannot occur.
pub fn watts(rate: Option<f64>, unit: f64, max_w: f64) -> Option<f64> {
    rate.map(|r| r * unit)
        .filter(|&w| (0.0..=max_w).contains(&w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_is_zen_family_on_amd_only() {
        assert!(supported("AuthenticAMD", 0x1A));
        assert!(supported("AuthenticAMD", 0x17));
        assert!(!supported("AuthenticAMD", 0x16));
        assert!(!supported("GenuineIntel", 0x19));
    }

    #[test]
    fn decodes_tctl_with_and_without_range_offset() {
        // 45.5 °C = 364 eighths.
        assert!((tctl(364 << 21) - 45.5).abs() < 1e-9);
        assert!((tctl((364 << 21) | RANGE_SEL) - (45.5 - 49.0)).abs() < 1e-9);
        assert!((tctl((764 << 21) | TJ_SEL) - (95.5 - 49.0)).abs() < 1e-9);
        // Only one TJ_SEL bit: no offset.
        assert!((tctl((364 << 21) | (1 << 16)) - 45.5).abs() < 1e-9);
        assert!((tctl_offset("AMD Ryzen 7 1700X Eight-Core Processor") - 20.0).abs() < 1e-9);
        assert!(tctl_offset("AMD Ryzen 7 9700X 8-Core Processor").abs() < 1e-9);
    }

    #[test]
    fn ccd_registers_by_generation() {
        assert_eq!(ccd_base(0x1A, 0x44), Some(0x59B08)); // Granite Ridge (9700X)
        assert_eq!(ccd_base(0x19, 0x61), Some(0x59B08)); // Raphael
        assert_eq!(ccd_base(0x19, 0x21), Some(0x59954)); // Vermeer
        assert_eq!(ccd_base(0x17, 0x71), Some(0x59954)); // Matisse
        assert_eq!(ccd_base(0x19, 0x50), None); // Cezanne APU: no CCD sensors
    }

    #[test]
    fn decodes_ccd_temperature_and_rejects_empty_slots() {
        // 50 °C: (50 + 49) * 8 = 792 with the valid bit.
        assert_eq!(ccd_temp(CCD_VALID | 792), Some(50.0));
        assert_eq!(ccd_temp(792), None);
        assert_eq!(ccd_temp(0), None);
        assert_eq!(ccd_temp(CCD_VALID | 0x7FF), None); // 206.9 °C: bogus
                                                       // Same as LibreHardwareMonitor's raw * 0.125 − 305 on the 12-bit field.
        let raw = CCD_VALID | 700;
        assert!((ccd_temp(raw).expect("valid") - (f64::from(raw) * 0.125 - 305.0)).abs() < 1e-9);
    }

    #[test]
    fn energy_unit_from_esu() {
        // Typical PWR_UNIT 0x000A1003: ESU = 16 → 15.26 µJ.
        assert!((energy_unit(0x000A_1003) - 15.258_789e-6).abs() < 1e-12);
        assert!((energy_unit(0x0000_0E00) - 0.5f64.powi(14)).abs() < 1e-15);
    }

    #[test]
    fn vid_decoding() {
        let msr = (200u64 << 14) | 0x8C0;
        assert_eq!(vid(msr), 200);
        assert!((vid_volts(200, true) - 1.245).abs() < 1e-9);
        assert!((vid_volts(40, false) - 1.3).abs() < 1e-9);
        assert!(svi3(0x1A, 0x44));
        assert!(svi3(0x19, 0x61));
        assert!(!svi3(0x19, 0x21));
    }

    #[test]
    fn rates_handle_first_sample_wrap_and_stalled_clock() {
        let mut r = Rate::default();
        assert_eq!(r.update(1000, 1.0, 32), None);
        assert_eq!(r.update(3000, 2.0, 32), Some(2000.0));
        // 32-bit wrap: 0xFFFF_FF00 → 0x100 is 0x200 increments.
        let mut w = Rate::default();
        w.update(0xFFFF_FF00, 0.0, 32);
        assert_eq!(w.update(0x100, 0.5, 32), Some(1024.0));
        // Upper bits beyond the counter width are ignored.
        let mut m = Rate::default();
        m.update(0xAB_0000_0010, 0.0, 32);
        assert_eq!(m.update(0xCD_0000_0020, 1.0, 32), Some(16.0));
        assert_eq!(m.update(0x30, 1.0, 32), None);
    }

    #[test]
    fn watts_from_energy_rate() {
        let unit = energy_unit(0x000A_1003);
        // 65 W at 15.26 µJ/increment ≈ 4.26 M increments/s.
        let w = watts(Some(65.0 / unit), unit, 1000.0).expect("in range");
        assert!((w - 65.0).abs() < 1e-9);
        assert_eq!(watts(Some(1e12), unit, 1000.0), None);
        assert_eq!(watts(None, unit, 1000.0), None);
    }

    #[test]
    fn decodes_field_registers_from_a_ryzen_9700x() {
        // vyro-live on a Ryzen 7 9700X (family 0x1A model 0x44) with PawnIO 2.2.
        let t = tctl(0x74FB_0000);
        assert!((t - 67.875).abs() < 1e-9, "{t}");
        let ccd = ccd_temp(0x0000_0B74).expect("valid CCD slot");
        assert!((ccd - 61.5).abs() < 1e-9, "{ccd}");
        assert_eq!(ccd_temp(0), None);
        assert_eq!(ccd_base(0x1A, 0x44), Some(SMN_CCD_TEMP_ZEN4));
        assert!((energy_unit(0xA_1000) - 1.0 / 65536.0).abs() < 1e-15);
        assert_eq!(vid(0x0000_0080_0077_4447), 221);
        assert_eq!(vid(0x0000_0040_0037_4447), 221);
        assert!(svi3(0x1A, 0x44));
        // SVI3 gives a plausible boost VID; the SVI2 formula would give 0.17 V.
        assert!((vid_volts(221, true) - 1.35).abs() < 1e-9);
    }
}

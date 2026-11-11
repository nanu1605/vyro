//! What the CPU sensor readers share (2.1): which CPUs Clokrix reads, the rate of a wrapping
//! hardware counter, and power from the RAPL energy counters, whose unit AMD and Intel encode
//! the same way. AMD Zen is read in [`crate::amd`], Intel Core in [`crate::intel_cpu`].

/// Whether Clokrix reads the temperature and power of the CPU it runs on: AMD Zen and Intel Core
/// (2.1). Others show them as not read on this CPU, not as needing PawnIO (2.0.4).
pub fn reads_this_cpu() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let id = crate::hwid::cpuid::read_live();
        reads(&id.vendor, id.family)
    }
    #[cfg(not(target_arch = "x86_64"))]
    false
}

/// [`reads_this_cpu`] for a given CPUID vendor and family.
pub fn reads(vendor: &str, family: u32) -> bool {
    crate::amd::supported(vendor, family) || crate::intel_cpu::supported(vendor, family)
}

/// Joules per energy-counter increment: 1 / 2^ESU, ESU in bits 12:8 of AMD's `PWR_UNIT` and of
/// Intel's `MSR_RAPL_POWER_UNIT` alike.
pub fn energy_unit(pwr_unit: u64) -> f64 {
    0.5f64.powi(i32::try_from((pwr_unit >> 8) & 0x1F).unwrap_or(16))
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
    fn reads_zen_and_intel_core_only() {
        assert!(reads("AuthenticAMD", 0x19));
        assert!(reads("GenuineIntel", 6));
        assert!(!reads("AuthenticAMD", 0x15), "pre-Zen AMD");
        assert!(!reads("CentaurHauls", 6));
        assert!(!reads("GenuineIntel", 0xF), "NetBurst");
    }

    #[test]
    fn energy_unit_from_esu() {
        // Typical AMD PWR_UNIT 0x000A1003: ESU = 16 → 15.26 µJ.
        assert!((energy_unit(0x000A_1003) - 15.258_789e-6).abs() < 1e-12);
        assert!((energy_unit(0x0000_0E00) - 0.5f64.powi(14)).abs() < 1e-15);
        // Intel client parts: MSR_RAPL_POWER_UNIT 0x000A0E03, ESU = 14 → 61 µJ.
        assert!((energy_unit(0x000A_0E03) - 61.035_156e-6).abs() < 1e-12);
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
}

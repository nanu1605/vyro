//! Intel Core CPU sensors through PawnIO's `IntelMSR` module (2.1): package and per-core
//! temperatures from the digital thermal sensors, package, cores and graphics power from the RAPL
//! energy counters, the core voltage from `PERF_STATUS`, the effective clock from APERF, and
//! whether the package is held back by heat or by its power limit. Decoders here are pure;
//! `win` does the reads. Register layouts follow Intel's SDM (vol. 4, "Model-Specific
//! Registers") and Linux `coretemp`.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::IntelCpuProvider;

use crate::info::CoreKind;

pub const MSR_IA32_APERF: u32 = 0xE8;
/// Bits 47:32: the core's current voltage in 1/8192 V.
pub const MSR_IA32_PERF_STATUS: u32 = 0x198;
/// Per core: bit 31 reading valid, bits 22:16 degrees below TjMax, bit 0 PROCHOT (at the throttle
/// point now), bit 10 held below the requested clock by a power limit (Linux's
/// `THERM_STATUS_PROCHOT` and `THERM_STATUS_POWER_LIMIT`).
pub const MSR_IA32_THERM_STATUS: u32 = 0x19C;
/// Bits 23:16: TjMax, the temperature the digital readouts count down from.
pub const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
/// The package's own sensor and flags, laid out as [`MSR_IA32_THERM_STATUS`].
pub const MSR_IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;
pub const MSR_RAPL_POWER_UNIT: u32 = 0x606;
pub const MSR_PKG_ENERGY_STATUS: u32 = 0x611;
/// Power plane 0: all the cores.
pub const MSR_PP0_ENERGY_STATUS: u32 = 0x639;
/// Power plane 1: the integrated graphics.
pub const MSR_PP1_ENERGY_STATUS: u32 = 0x641;

const READING_VALID: u64 = 1 << 31;
const PROCHOT: u64 = 1;
const POWER_LIMITED: u64 = 1 << 10;

/// Intel CPUs the `IntelMSR` module's registers mean the same on: family 6, every Core from
/// Nehalem to Arrow Lake.
pub fn supported(vendor: &str, family: u32) -> bool {
    vendor == "GenuineIntel" && family == 6
}

/// TjMax in °C from `MSR_TEMPERATURE_TARGET`; `None` when the field reads implausibly (Clokrix does
/// not guess one: a wrong TjMax shifts every temperature).
pub fn tjmax(raw: u64) -> Option<f64> {
    let t = (raw >> 16) & 0xFF;
    #[allow(clippy::cast_precision_loss)]
    (50..=130).contains(&t).then_some(t as f64)
}

/// °C from a thermal status register: TjMax less the readout, when the valid bit is set.
pub fn dts_temp(status: u64, tjmax: f64) -> Option<f64> {
    if status & READING_VALID == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let below = ((status >> 16) & 0x7F) as f64;
    Some(tjmax - below)
}

/// Whether a thermal status register says PROCHOT is active: the sensor is at the throttle point
/// (TjMax, less any offset the firmware set) and the CPU slows to cool.
pub const fn prochot(status: u64) -> bool {
    status & PROCHOT != 0
}

/// Whether a thermal status register says a power limit holds the clock below what Windows
/// asked for.
pub const fn power_limited(status: u64) -> bool {
    status & POWER_LIMITED != 0
}

/// Core voltage in V from `IA32_PERF_STATUS` bits 47:32; `None` when the field is empty (older
/// cores report no voltage there) or implausible.
pub fn perf_status_volts(raw: u64) -> Option<f64> {
    let v = (raw >> 32) & 0xFFFF;
    #[allow(clippy::cast_precision_loss)]
    let v = v as f64 / 8192.0;
    (0.2..=2.0).contains(&v).then_some(v)
}

/// Names for the cores, in topology order: on a hybrid CPU "P-core #n" and "E-core #n", else
/// "Core #n", numbered from 0 across the whole CPU as Windows numbers them.
pub fn core_names(kinds: &[CoreKind]) -> Vec<String> {
    kinds
        .iter()
        .enumerate()
        .map(|(n, kind)| match kind {
            CoreKind::Performance => format!("P-core #{n}"),
            CoreKind::Efficiency => format!("E-core #{n}"),
            CoreKind::Uniform => format!("Core #{n}"),
        })
        .collect()
}

/// The package temperature: the package's own sensor, or the hottest core when that reads
/// nothing valid.
pub fn package_temp(package: Option<f64>, cores: &[f64]) -> Option<f64> {
    package.or_else(|| cores.iter().copied().reduce(f64::max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn support_is_family_6_intel_only() {
        assert!(supported("GenuineIntel", 6));
        assert!(!supported("GenuineIntel", 0xF));
        assert!(!supported("AuthenticAMD", 6));
    }

    #[test]
    fn reads_tjmax_and_rejects_nonsense() {
        // 100 °C, Intel's TjMax for the i7-12700K to the i9-14900K, with no offset in 29:24.
        assert_eq!(tjmax(0x0064_0000), Some(100.0));
        // The same with a 5 °C offset set in the firmware: TjMax is still 100.
        assert_eq!(tjmax(0x0564_0000), Some(100.0));
        assert_eq!(tjmax(0x0069_0A00), Some(105.0));
        assert_eq!(tjmax(0), None);
        assert_eq!(tjmax(0x00FF_0000), None);
    }

    #[test]
    fn digital_readout_counts_down_from_tjmax() {
        // Valid, 58 °C below a TjMax of 100: 42 °C.
        let status = READING_VALID | (58 << 16);
        assert_eq!(dts_temp(status, 100.0), Some(42.0));
        // At TjMax, throttling: readout 0 and PROCHOT.
        let hot = READING_VALID | PROCHOT;
        assert_eq!(dts_temp(hot, 100.0), Some(100.0));
        assert!(prochot(hot));
        assert!(!prochot(status));
        // Not valid: nothing, rather than TjMax.
        assert_eq!(dts_temp(58 << 16, 100.0), None);
        // Upper bits and the log bits around the readout do not leak into it.
        let noisy = 0xFFFF_FFFF_0000_0000 | READING_VALID | (0x7F << 24) | (30 << 16) | 0xFFFE;
        assert_eq!(dts_temp(noisy, 100.0), Some(70.0));
        assert!(power_limited(noisy));
        assert!(!power_limited(status));
    }

    #[test]
    fn decodes_whole_register_values() {
        // Constructed (no field dump from an Intel PC yet): valid, 0x42 = 66 below 100 → 34 °C,
        // with bit 27 set as well, which must not reach the readout.
        assert_eq!(dts_temp(0x8842_0000, 100.0), Some(34.0));
        // IA32_PERF_STATUS with 0x2333 / 8192 = 1.100 V in bits 47:32 and ratio 42 in 15:8.
        let v = perf_status_volts(0x0000_2333_0000_2A00).expect("voltage");
        assert!((v - 1.1).abs() < 0.001, "{v}");
        assert_eq!(
            perf_status_volts(0x0000_0000_0000_2A00),
            None,
            "no voltage field"
        );
        assert_eq!(
            perf_status_volts(0x0000_FFFF_0000_0000),
            None,
            "8 V: nonsense"
        );
    }

    #[test]
    fn names_hybrid_and_uniform_cores() {
        use CoreKind::{Efficiency, Performance, Uniform};
        assert_eq!(
            core_names(&[Performance, Performance, Efficiency, Efficiency]),
            ["P-core #0", "P-core #1", "E-core #2", "E-core #3"]
        );
        assert_eq!(core_names(&[Uniform, Uniform]), ["Core #0", "Core #1"]);
    }

    #[test]
    fn package_falls_back_to_the_hottest_core() {
        assert_eq!(package_temp(Some(51.0), &[60.0, 48.0]), Some(51.0));
        assert_eq!(package_temp(None, &[44.0, 61.0, 52.0]), Some(61.0));
        assert_eq!(package_temp(None, &[]), None);
    }
}

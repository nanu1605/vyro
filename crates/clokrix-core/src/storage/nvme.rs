//! NVMe decoders: the SMART / Health Information log page (0x02, 512 bytes) and the Identify
//! Controller data structure (CNS 0x01, 4096 bytes), per NVMe Base Specification 2.0 §5.16.1.3
//! and §5.17.2.1.

/// Kelvin → °C for the 16-bit temperature fields; 0 means "not implemented".
fn kelvin(k: u16) -> Option<f64> {
    (k != 0).then(|| f64::from(k) - 273.0)
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// 128-bit little-endian counter as f64 (exact up to 2^53, far beyond any real drive).
fn u128_at(b: &[u8], o: usize) -> f64 {
    let mut a = [0u8; 16];
    a.copy_from_slice(&b[o..o + 16]);
    u128::from_le_bytes(a) as f64
}

/// Bytes per NVMe "data unit" (1000 × 512-byte units).
pub const DATA_UNIT_BYTES: f64 = 512_000.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HealthLog {
    pub critical_warning: u8,
    pub composite_c: Option<f64>,
    pub available_spare_pct: u8,
    pub spare_threshold_pct: u8,
    /// Vendor estimate of life used; may exceed 100.
    pub percentage_used: u8,
    pub data_units_read: f64,
    pub data_units_written: f64,
    pub host_reads: f64,
    pub host_writes: f64,
    pub busy_minutes: f64,
    pub power_cycles: f64,
    pub power_on_hours: f64,
    pub unsafe_shutdowns: f64,
    pub media_errors: f64,
    pub error_log_entries: f64,
    pub warning_temp_minutes: u32,
    pub critical_temp_minutes: u32,
    /// Temperature Sensor 1–8; `None` where the controller does not implement one.
    pub sensors_c: [Option<f64>; 8],
    /// Thermal management temperature 1 / 2 transition counts and total seconds.
    pub throttle_events: [u32; 2],
    pub throttle_secs: [u32; 2],
}

impl HealthLog {
    pub fn read_gb(&self) -> f64 {
        self.data_units_read * DATA_UNIT_BYTES / 1e9
    }

    pub fn written_gb(&self) -> f64 {
        self.data_units_written * DATA_UNIT_BYTES / 1e9
    }
}

pub fn health_log(b: &[u8]) -> Option<HealthLog> {
    if b.len() < 232 {
        return None;
    }
    let mut sensors_c = [None; 8];
    for (i, s) in sensors_c.iter_mut().enumerate() {
        *s = kelvin(u16_at(b, 200 + i * 2));
    }
    Some(HealthLog {
        critical_warning: b[0],
        composite_c: kelvin(u16_at(b, 1)),
        available_spare_pct: b[3],
        spare_threshold_pct: b[4],
        percentage_used: b[5],
        data_units_read: u128_at(b, 32),
        data_units_written: u128_at(b, 48),
        host_reads: u128_at(b, 64),
        host_writes: u128_at(b, 80),
        busy_minutes: u128_at(b, 96),
        power_cycles: u128_at(b, 112),
        power_on_hours: u128_at(b, 128),
        unsafe_shutdowns: u128_at(b, 144),
        media_errors: u128_at(b, 160),
        error_log_entries: u128_at(b, 176),
        warning_temp_minutes: u32_at(b, 192),
        critical_temp_minutes: u32_at(b, 196),
        sensors_c,
        throttle_events: [u32_at(b, 216), u32_at(b, 220)],
        throttle_secs: [u32_at(b, 224), u32_at(b, 228)],
    })
}

/// Human-readable meaning of each set Critical Warning bit.
pub fn critical_warnings(bits: u8) -> Vec<&'static str> {
    const NAMES: [&str; 6] = [
        "available spare below threshold",
        "temperature outside the safe range",
        "reliability degraded by media errors",
        "media placed in read-only mode",
        "volatile memory backup failed",
        "persistent memory region read-only",
    ];
    NAMES
        .iter()
        .enumerate()
        .filter(|(i, _)| bits & (1 << i) != 0)
        .map(|(_, n)| *n)
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identify {
    pub pci_vendor: u16,
    /// Local display only.
    pub serial: String,
    pub model: String,
    pub firmware: String,
    /// "1.4", "2.0", … (`None` for pre-1.2 controllers that leave VER zero).
    pub version: Option<String>,
    pub warn_temp_c: Option<f64>,
    pub crit_temp_c: Option<f64>,
    pub capacity_bytes: Option<f64>,
}

fn ascii(b: &[u8]) -> String {
    String::from_utf8_lossy(b)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_owned()
}

pub fn identify(b: &[u8]) -> Option<Identify> {
    if b.len() < 512 {
        return None;
    }
    let ver = u32_at(b, 80);
    let version = (ver != 0).then(|| {
        let (major, minor, ter) = (ver >> 16, (ver >> 8) & 0xFF, ver & 0xFF);
        if ter == 0 {
            format!("{major}.{minor}")
        } else {
            format!("{major}.{minor}.{ter}")
        }
    });
    let cap = u128_at(b, 280);
    Some(Identify {
        pci_vendor: u16_at(b, 0),
        serial: ascii(&b[4..24]),
        model: ascii(&b[24..64]),
        firmware: ascii(&b[64..72]),
        version,
        warn_temp_c: kelvin(u16_at(b, 266)),
        crit_temp_c: kelvin(u16_at(b, 268)),
        capacity_bytes: (cap > 0.0).then_some(cap),
    })
}

/// Synthetic log / identify sectors for tests and the browser sample.
pub(crate) mod fixtures {
    fn put16(b: &mut [u8], o: usize, v: u16) {
        b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put128(b: &mut [u8], o: usize, v: u128) {
        b[o..o + 16].copy_from_slice(&v.to_le_bytes());
    }

    /// A health log shaped like a Samsung 980 PRO after a year of desktop use.
    pub(crate) fn health_log() -> Vec<u8> {
        let mut b = vec![0u8; 512];
        put16(&mut b, 1, 273 + 41);
        b[3] = 100;
        b[4] = 10;
        b[5] = 2;
        put128(&mut b, 32, 36_210_345);
        put128(&mut b, 48, 29_870_112);
        put128(&mut b, 64, 412_345_678);
        put128(&mut b, 80, 523_456_789);
        put128(&mut b, 96, 1_234);
        put128(&mut b, 112, 812);
        put128(&mut b, 128, 3_456);
        put128(&mut b, 144, 37);
        put128(&mut b, 176, 1_520);
        put16(&mut b, 200, 273 + 41);
        put16(&mut b, 202, 273 + 47);
        b[216] = 3;
        b[224] = 90;
        b
    }

    pub(crate) fn identify() -> Vec<u8> {
        let mut b = vec![0u8; 4096];
        put16(&mut b, 0, 0x144D);
        b[4..24].copy_from_slice(b"S5GXNX0TESTSERIAL   ");
        let model = b"Samsung SSD 980 PRO 1TB";
        b[24..64].fill(b' ');
        b[24..24 + model.len()].copy_from_slice(model);
        b[64..72].copy_from_slice(b"5B2QGXA7");
        b[80..84].copy_from_slice(&0x0001_0300u32.to_le_bytes());
        put16(&mut b, 266, 355);
        put16(&mut b, 268, 358);
        put128(&mut b, 280, 1_000_204_886_016);
        b
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{health_log as sample_log, identify as sample_identify};
    use super::*;

    #[test]
    fn decodes_health_log() {
        let h = health_log(&sample_log()).expect("log");
        assert_eq!(h.composite_c, Some(41.0));
        assert_eq!(h.available_spare_pct, 100);
        assert_eq!(h.percentage_used, 2);
        assert_eq!(h.power_on_hours, 3456.0);
        assert_eq!(h.power_cycles, 812.0);
        assert_eq!(h.unsafe_shutdowns, 37.0);
        assert!((h.read_gb() - 18_539.7).abs() < 0.1);
        assert!((h.written_gb() - 15_293.5).abs() < 0.1);
        assert_eq!(h.sensors_c[0], Some(41.0));
        assert_eq!(h.sensors_c[1], Some(47.0));
        assert_eq!(h.sensors_c[2], None);
        assert_eq!(h.throttle_events, [3, 0]);
        assert_eq!(h.throttle_secs, [90, 0]);
        assert!(health_log(&[0; 100]).is_none());
    }

    #[test]
    fn decodes_identify() {
        let id = identify(&sample_identify()).expect("identify");
        assert_eq!(id.pci_vendor, 0x144D);
        assert_eq!(id.model, "Samsung SSD 980 PRO 1TB");
        assert_eq!(id.firmware, "5B2QGXA7");
        assert_eq!(id.version.as_deref(), Some("1.3"));
        assert_eq!(id.warn_temp_c, Some(82.0));
        assert_eq!(id.crit_temp_c, Some(85.0));
        assert_eq!(id.capacity_bytes, Some(1_000_204_886_016.0));
    }

    #[test]
    fn names_critical_warning_bits() {
        assert!(critical_warnings(0).is_empty());
        assert_eq!(
            critical_warnings(0b1001),
            [
                "available spare below threshold",
                "media placed in read-only mode"
            ]
        );
    }
}

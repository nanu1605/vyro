//! ATA decoders: IDENTIFY DEVICE (ACS-4 §7.13) and the SMART READ DATA / READ THRESHOLDS
//! sectors (30 attribute slots of 12 bytes after a 2-byte revision). Attribute meaning and raw
//! units are vendor-specific; the table and the unit overrides below follow smartmontools'
//! drive database for the common cases.

fn word(b: &[u8], w: usize) -> u16 {
    u16::from_le_bytes([b[w * 2], b[w * 2 + 1]])
}

/// ATA strings store two characters per word, high byte first.
fn ata_string(b: &[u8], first: usize, last: usize) -> String {
    let mut s = Vec::with_capacity((last - first + 1) * 2);
    for w in first..=last {
        s.extend_from_slice(&[b[w * 2 + 1], b[w * 2]]);
    }
    String::from_utf8_lossy(&s)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_owned()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identify {
    /// Local display only.
    pub serial: String,
    pub firmware: String,
    pub model: String,
    pub smart_supported: bool,
    pub smart_enabled: bool,
    pub capacity_bytes: Option<f64>,
    /// `Some(0)` for SSDs (non-rotating), RPM for disks, `None` when not reported.
    pub rpm: Option<u32>,
    pub trim: bool,
    /// Highest supported and currently negotiated SATA link, Gb/s.
    pub sata_max_gbps: Option<f64>,
    pub sata_current_gbps: Option<f64>,
}

fn sata_gen(bits: u16) -> Option<f64> {
    match bits {
        3 => Some(6.0),
        2 => Some(3.0),
        1 => Some(1.5),
        _ => None,
    }
}

pub fn identify(b: &[u8]) -> Option<Identify> {
    if b.len() < 512 {
        return None;
    }
    // Words 60–61: 28-bit LBA count; 100–103: 48-bit count when word 83 bit 10 is set.
    let lba28 = u64::from(word(b, 60)) | u64::from(word(b, 61)) << 16;
    let lba48 = (0..4).fold(0u64, |acc, i| acc | u64::from(word(b, 100 + i)) << (16 * i));
    let sectors = if word(b, 83) & (1 << 10) != 0 && lba48 > 0 {
        lba48
    } else {
        lba28
    };
    // Word 106: bit 12 = logical sector longer than 256 words (size in words 117–118).
    let w106 = word(b, 106);
    let sector = if w106 & 0xC000 == 0x4000 && w106 & (1 << 12) != 0 {
        (u64::from(word(b, 117)) | u64::from(word(b, 118)) << 16) * 2
    } else {
        512
    };
    let rpm = match word(b, 217) {
        1 => Some(0),
        r @ 0x0401..=0xFFFE => Some(u32::from(r)),
        _ => None,
    };
    let valid = |w: u16| w != 0 && w != 0xFFFF;
    let w76 = word(b, 76);
    let sata_max_gbps = valid(w76)
        .then(|| (1..=3).rev().find(|g| w76 & (1 << g) != 0))
        .flatten()
        .and_then(sata_gen);
    let w77 = word(b, 77);
    let sata_current_gbps = valid(w76).then(|| sata_gen((w77 >> 1) & 0x7)).flatten();
    Some(Identify {
        serial: ata_string(b, 10, 19),
        firmware: ata_string(b, 23, 26),
        model: ata_string(b, 27, 46),
        smart_supported: word(b, 82) & 1 != 0,
        smart_enabled: word(b, 85) & 1 != 0,
        capacity_bytes: (sectors > 0).then(|| (sectors * sector) as f64),
        rpm,
        trim: word(b, 169) & 1 != 0,
        sata_max_gbps,
        sata_current_gbps,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub id: u8,
    pub flags: u16,
    pub current: u8,
    pub worst: u8,
    pub threshold: Option<u8>,
    /// The 48-bit raw field.
    pub raw: u64,
}

impl Attribute {
    pub fn prefail(&self) -> bool {
        self.flags & 1 != 0
    }

    /// The normalized value is at or below a non-zero threshold: the drive predicts failure.
    pub fn failing(&self) -> bool {
        matches!(self.threshold, Some(t) if t > 0 && t < 0xFE && self.current <= t)
    }

    pub fn raw_lo16(&self) -> u64 {
        self.raw & 0xFFFF
    }

    pub fn raw_lo32(&self) -> u64 {
        self.raw & 0xFFFF_FFFF
    }
}

/// Sum of all 512 bytes must be 0 mod 256 (byte 511 is the checksum).
pub fn checksum_ok(b: &[u8]) -> bool {
    b.len() >= 512 && b[..512].iter().fold(0u8, |a, &x| a.wrapping_add(x)) == 0
}

/// Decodes SMART READ DATA, pairing each attribute with its threshold when given.
pub fn attributes(data: &[u8], thresholds: Option<&[u8]>) -> Vec<Attribute> {
    if data.len() < 362 {
        return Vec::new();
    }
    let thresh = |id: u8| -> Option<u8> {
        let t = thresholds.filter(|t| t.len() >= 362)?;
        (0..30)
            .map(|i| &t[2 + i * 12..2 + i * 12 + 12])
            .find(|e| e[0] == id)
            .map(|e| e[1])
    };
    (0..30)
        .map(|i| &data[2 + i * 12..2 + i * 12 + 12])
        .filter(|e| e[0] != 0)
        .map(|e| Attribute {
            id: e[0],
            flags: u16::from_le_bytes([e[1], e[2]]),
            current: e[3],
            worst: e[4],
            threshold: thresh(e[0]),
            raw: e[5..11]
                .iter()
                .rev()
                .fold(0u64, |acc, &x| acc << 8 | u64::from(x)),
        })
        .collect()
}

/// Common attribute names; unknown ids read "Vendor specific".
pub fn name(id: u8, ssd: bool) -> &'static str {
    match id {
        1 => "Read Error Rate",
        2 => "Throughput Performance",
        3 => "Spin-Up Time",
        4 => "Start/Stop Count",
        5 => "Reallocated Sectors Count",
        7 => "Seek Error Rate",
        8 => "Seek Time Performance",
        9 => "Power-On Hours",
        10 => "Spin Retry Count",
        11 => "Recalibration Retries",
        12 => "Power Cycle Count",
        13 => "Soft Read Error Rate",
        22 => "Current Helium Level",
        160 => "Uncorrectable Sectors (read/write)",
        161 => "Valid Spare Blocks",
        163 => "Initial Invalid Blocks",
        164 => "Total Erase Count",
        165 => "Maximum Erase Count",
        166 => "Minimum Erase Count",
        167 => "Average Erase Count",
        168 => "Max NAND Erase Count (spec)",
        169 => "Remaining Life",
        170 => "Available Reserved Space",
        171 => "Program Fail Count",
        172 => "Erase Fail Count",
        173 => "Wear Leveling Count",
        174 => "Unexpected Power Loss Count",
        175 => "Power Loss Protection Failure",
        176 => "Erase Fail Count (chip)",
        177 => "Wear Leveling Count",
        178 => "Used Reserved Block Count (chip)",
        179 => "Used Reserved Block Count",
        180 => "Unused Reserved Block Count",
        181 => "Program Fail Count",
        182 => "Erase Fail Count",
        183 => "SATA Downshift Error Count",
        184 => "End-to-End Error",
        187 => "Reported Uncorrectable Errors",
        188 => "Command Timeout",
        189 => "High Fly Writes",
        190 => "Airflow Temperature",
        191 => "G-Sense Error Rate",
        192 => "Power-Off Retract Count",
        193 => "Load/Unload Cycle Count",
        194 => "Temperature",
        195 => "Hardware ECC Recovered",
        196 => "Reallocation Event Count",
        197 => "Current Pending Sector Count",
        198 => "Uncorrectable Sector Count",
        199 => "UltraDMA CRC Error Count",
        200 => "Write Error Rate",
        202 if ssd => "Percent Lifetime Remaining",
        202 => "Data Address Mark Errors",
        206 => "Flying Height",
        210 => "RAID Consistency Error Count",
        220 => "Disk Shift",
        222 => "Loaded Hours",
        223 => "Load/Unload Retry Count",
        224 => "Load Friction",
        225 if ssd => "Host Writes (32 MiB)",
        225 => "Load/Unload Cycle Count",
        226 => "Load-in Time",
        227 => "Torque Amplification Count",
        228 => "Power-Off Retract Cycle",
        230 if ssd => "Drive Life Protection Status",
        230 => "GMR Head Amplitude",
        231 if ssd => "SSD Life Left",
        231 => "Temperature",
        232 => "Endurance Remaining",
        233 if ssd => "Media Wearout Indicator",
        233 => "Power-On Hours (alt)",
        234 => "Average Erase Count",
        235 => "Good Block Count",
        240 => "Head Flying Hours",
        241 => "Total Host Writes",
        242 => "Total Host Reads",
        243 => "Total Host Writes (expanded)",
        244 => "Total Host Reads (expanded)",
        245 => "Remaining Rated Write Endurance",
        246 => "Cumulative Host Sectors Written",
        247 => "Host Program Page Count",
        248 => "FTL Program Page Count",
        249 => "NAND Writes (GiB)",
        250 => "Read Error Retry Rate",
        251 => "Minimum Spares Remaining",
        252 => "Newly Added Bad Flash Block",
        254 => "Free Fall Protection",
        _ => "Vendor specific",
    }
}

/// Bytes per unit of the 241 / 242 host-traffic counters for this model.
fn host_unit_bytes(model: &str) -> f64 {
    let m = model.to_ascii_uppercase();
    if m.starts_with("INTEL") {
        32.0 * 1024.0 * 1024.0
    } else if ["KINGSTON", "SANDISK", "SA400", "ADATA", "TRANSCEND"]
        .iter()
        .any(|p| m.starts_with(p))
    {
        1024.0 * 1024.0 * 1024.0
    } else {
        512.0
    }
}

/// Values derived from the attribute table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Derived {
    pub temp_c: Option<f64>,
    pub power_on_hours: Option<f64>,
    pub power_cycles: Option<f64>,
    pub read_gb: Option<f64>,
    pub written_gb: Option<f64>,
    /// Remaining SSD life from the vendor's wear attribute.
    pub life_pct: Option<f64>,
    pub reallocated: Option<u64>,
    pub pending: Option<u64>,
    pub uncorrectable: Option<u64>,
}

pub fn derive(attrs: &[Attribute], model: &str, ssd: bool) -> Derived {
    let get = |id: u8| attrs.iter().find(|a| a.id == id);
    let plausible_temp = |t: u64| (1..=120).contains(&t).then_some(t as f64);
    let temp_c = get(194)
        .and_then(|a| plausible_temp(a.raw & 0xFF))
        .or_else(|| get(190).and_then(|a| plausible_temp(a.raw & 0xFF)));
    let unit = host_unit_bytes(model);
    let gb = |id: u8, unit: f64| get(id).map(|a| a.raw as f64 * unit / 1e9);
    let written_gb = gb(241, unit).or_else(|| gb(246, 512.0));
    let read_gb = gb(242, unit);
    let life_pct = if ssd {
        [231u8, 169, 202, 233, 177, 173]
            .iter()
            .filter_map(|&id| get(id))
            .find(|a| a.current <= 100)
            .map(|a| f64::from(a.current))
    } else {
        None
    };
    Derived {
        temp_c,
        power_on_hours: get(9).map(|a| a.raw_lo32() as f64),
        power_cycles: get(12).map(|a| a.raw_lo32() as f64),
        read_gb,
        written_gb,
        life_pct,
        reallocated: get(5).map(Attribute::raw_lo32),
        pending: get(197).map(Attribute::raw_lo32),
        uncorrectable: get(198).map(Attribute::raw_lo32),
    }
}

/// Synthetic IDENTIFY / SMART sectors for tests and the browser sample.
pub(crate) mod fixtures {

    fn put_word(b: &mut [u8], w: usize, v: u16) {
        b[w * 2..w * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn put_string(b: &mut [u8], first: usize, last: usize, s: &str) {
        let mut bytes = s.as_bytes().to_vec();
        bytes.resize((last - first + 1) * 2, b' ');
        for (i, pair) in bytes.chunks(2).enumerate() {
            b[(first + i) * 2] = pair[1];
            b[(first + i) * 2 + 1] = pair[0];
        }
    }

    fn fix_checksum(b: &mut [u8]) {
        let sum = b[..511].iter().fold(0u8, |a, &x| a.wrapping_add(x));
        b[511] = 0u8.wrapping_sub(sum);
    }

    #[cfg(test)]
    /// IDENTIFY for a Samsung 870 EVO 500 GB on a 6 Gb/s port.
    pub(crate) fn sample_identify() -> Vec<u8> {
        let mut b = vec![0u8; 512];
        put_string(&mut b, 10, 19, "S62ANJ0TESTSERIAL");
        put_string(&mut b, 23, 26, "SVT02B6Q");
        put_string(&mut b, 27, 46, "Samsung SSD 870 EVO 500GB");
        put_word(&mut b, 60, 0xFFFF);
        put_word(&mut b, 61, 0x0FFF);
        put_word(&mut b, 76, 0b1110);
        put_word(&mut b, 77, 3 << 1);
        put_word(&mut b, 82, 1);
        put_word(&mut b, 83, 1 << 10);
        put_word(&mut b, 85, 1);
        let sectors: u64 = 976_773_168;
        for i in 0..4 {
            put_word(&mut b, 100 + i, (sectors >> (16 * i)) as u16);
        }
        put_word(&mut b, 106, 0x4000);
        put_word(&mut b, 169, 1);
        put_word(&mut b, 217, 1);
        b
    }

    #[cfg(test)]
    /// (id, flags, current, worst, raw, threshold)
    const SAMSUNG: [(u8, u16, u8, u8, u64, u8); 10] = [
        (5, 0x33, 100, 100, 0, 10),
        (9, 0x32, 97, 97, 12_345, 0),
        (12, 0x32, 99, 99, 1_021, 0),
        (177, 0x13, 98, 98, 21, 0),
        (179, 0x13, 100, 100, 0, 10),
        (187, 0x32, 100, 100, 0, 0),
        (190, 0x32, 67, 55, 33, 0),
        (194, 0x22, 67, 45, 0x0037_0014_0021, 0),
        (197, 0x32, 100, 100, 0, 0),
        (241, 0x32, 99, 99, 41_222_333_444, 0),
    ];

    /// IDENTIFY for a Seagate BarraCuda 2 TB (7200 RPM) on a port negotiated at 3 Gb/s.
    pub(crate) fn hdd_identify() -> Vec<u8> {
        let mut b = vec![0u8; 512];
        put_string(&mut b, 10, 19, "ZFL0TESTSERIAL");
        put_string(&mut b, 23, 26, "0001");
        put_string(&mut b, 27, 46, "ST2000DM008-2UB102");
        put_word(&mut b, 76, 0b1110);
        put_word(&mut b, 77, 2 << 1);
        put_word(&mut b, 82, 1);
        put_word(&mut b, 83, 1 << 10);
        put_word(&mut b, 85, 1);
        let sectors: u64 = 3_907_029_168;
        for i in 0..4 {
            put_word(&mut b, 100 + i, (sectors >> (16 * i)) as u16);
        }
        put_word(&mut b, 106, 0x6003);
        put_word(&mut b, 217, 7200);
        b
    }

    const SEAGATE: [(u8, u16, u8, u8, u64, u8); 10] = [
        (5, 0x33, 100, 100, 8, 10),
        (1, 0x0F, 81, 64, 123_456_789, 6),
        (3, 0x03, 97, 97, 0, 0),
        (4, 0x32, 100, 100, 1_532, 20),
        (9, 0x32, 85, 85, 13_210, 0),
        (12, 0x32, 100, 100, 1_530, 20),
        (190, 0x22, 66, 55, 0x001E_001D_0022, 45),
        (194, 0x22, 34, 45, 0x0000_0012_0022, 0),
        (197, 0x12, 100, 100, 0, 0),
        (198, 0x10, 100, 100, 0, 0),
    ];

    pub(crate) fn hdd_smart() -> (Vec<u8>, Vec<u8>) {
        smart(&SEAGATE)
    }

    #[cfg(test)]
    pub(crate) fn ssd_smart() -> (Vec<u8>, Vec<u8>) {
        smart(&SAMSUNG)
    }

    fn smart(table: &[(u8, u16, u8, u8, u64, u8)]) -> (Vec<u8>, Vec<u8>) {
        let mut data = vec![0u8; 512];
        let mut thr = vec![0u8; 512];
        data[0] = 1;
        thr[0] = 1;
        for (i, &(id, flags, cur, worst, raw, t)) in table.iter().enumerate() {
            let e = &mut data[2 + i * 12..2 + i * 12 + 12];
            e[0] = id;
            e[1..3].copy_from_slice(&flags.to_le_bytes());
            e[3] = cur;
            e[4] = worst;
            e[5..11].copy_from_slice(&raw.to_le_bytes()[..6]);
            thr[2 + i * 12] = id;
            thr[2 + i * 12 + 1] = t;
        }
        fix_checksum(&mut data);
        fix_checksum(&mut thr);
        (data, thr)
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{sample_identify, ssd_smart as sample_smart};
    use super::*;

    #[test]
    fn decodes_identify() {
        let id = identify(&sample_identify()).expect("identify");
        assert_eq!(id.model, "Samsung SSD 870 EVO 500GB");
        assert_eq!(id.firmware, "SVT02B6Q");
        assert_eq!(id.serial, "S62ANJ0TESTSERIAL");
        assert_eq!(id.capacity_bytes, Some(500_107_862_016.0));
        assert_eq!(id.rpm, Some(0));
        assert!(id.smart_supported && id.smart_enabled && id.trim);
        assert_eq!(id.sata_max_gbps, Some(6.0));
        assert_eq!(id.sata_current_gbps, Some(6.0));
    }

    #[test]
    fn decodes_attributes_and_derived_values() {
        let (data, thr) = sample_smart();
        assert!(checksum_ok(&data) && checksum_ok(&thr));
        let attrs = attributes(&data, Some(&thr));
        assert_eq!(attrs.len(), 10);
        let temp = &attrs[7];
        assert_eq!((temp.id, temp.current, temp.worst), (194, 67, 45));
        assert_eq!(temp.raw, 0x0037_0014_0021);
        assert_eq!(attrs[0].threshold, Some(10));
        assert!(attrs[0].prefail() && !attrs[0].failing());

        let d = derive(&attrs, "Samsung SSD 870 EVO 500GB", true);
        assert_eq!(d.temp_c, Some(33.0));
        assert_eq!(d.power_on_hours, Some(12_345.0));
        assert_eq!(d.power_cycles, Some(1_021.0));
        assert_eq!(d.life_pct, Some(98.0));
        assert!((d.written_gb.expect("writes") - 21_105.8).abs() < 0.1);
        assert_eq!(d.read_gb, None);
        assert_eq!(
            (d.reallocated, d.pending, d.uncorrectable),
            (Some(0), Some(0), None)
        );
    }

    #[test]
    fn failing_needs_nonzero_threshold() {
        let mut a = Attribute {
            id: 5,
            flags: 0x33,
            current: 9,
            worst: 9,
            threshold: Some(10),
            raw: 400,
        };
        assert!(a.failing());
        a.threshold = Some(0);
        assert!(!a.failing());
        a.threshold = None;
        assert!(!a.failing());
    }

    #[test]
    fn host_units_follow_vendor() {
        let a = |raw| Attribute {
            id: 241,
            flags: 0x32,
            current: 100,
            worst: 100,
            threshold: None,
            raw,
        };
        let w = |model| derive(&[a(1000)], model, true).written_gb.expect("writes");
        assert!((w("KINGSTON SA400S37480G") - 1073.7).abs() < 0.1);
        assert!((w("INTEL SSDSC2KW512G8") - 33.55).abs() < 0.01);
        assert!((w("WDC WDS500G2B0A") - 0.000_512).abs() < 1e-9);
    }

    #[test]
    fn hdd_has_no_life_and_temp_falls_back_to_airflow() {
        let a = |id, current, raw| Attribute {
            id,
            flags: 0x32,
            current,
            worst: current,
            threshold: None,
            raw,
        };
        let d = derive(
            &[a(177, 50, 0), a(190, 64, 36), a(194, 0, 0)],
            "ST2000DM008",
            false,
        );
        assert_eq!(d.life_pct, None);
        assert_eq!(d.temp_c, Some(36.0));
        assert_eq!(name(231, false), "Temperature");
        assert_eq!(name(231, true), "SSD Life Left");
    }
}

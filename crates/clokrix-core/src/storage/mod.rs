//! Storage (spec §2 Storage, Phase 4). The Windows side fills a [`RawDrive`] per physical
//! drive with the raw sectors it could read (NVMe log pages, ATA IDENTIFY / SMART); [`build`]
//! decodes and merges them into the [`DriveInfo`] the Storage panel shows, and grades health.
//! Everything here is pure so each rule is tested on Linux.

pub mod ata;
pub mod nvme;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{collect, StorageProvider};

use crate::info::{DriveInfo, LocalOnly, NvmeHealth, SmartAttribute, VolumeInfo};
use crate::snapshot::{Status, StorageKind, StorageSnapshot};

const MB: f64 = 1024.0 * 1024.0;

/// The parts of `STORAGE_DEVICE_DESCRIPTOR` we use.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Descriptor {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub revision: Option<String>,
    /// Local display only.
    pub serial: Option<String>,
    /// `STORAGE_BUS_TYPE`.
    pub bus_type: u32,
    pub removable: bool,
}

/// Decodes a `STORAGE_DEVICE_DESCRIPTOR` buffer (strings are NUL-terminated at offsets).
pub fn descriptor(b: &[u8]) -> Option<Descriptor> {
    if b.len() < 36 {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let string = |o: usize| -> Option<String> {
        let off = u32_at(o) as usize;
        if off == 0 || off >= b.len() {
            return None;
        }
        let end = b[off..]
            .iter()
            .position(|&c| c == 0)
            .map_or(b.len(), |p| off + p);
        let s = String::from_utf8_lossy(&b[off..end]).trim().to_owned();
        (!s.is_empty()).then_some(s)
    };
    Some(Descriptor {
        vendor: string(12),
        product: string(16),
        revision: string(20),
        serial: string(24),
        bus_type: u32_at(28),
        removable: b[10] != 0,
    })
}

/// `STORAGE_BUS_TYPE` → (label, kind).
pub fn bus(bus_type: u32) -> (&'static str, StorageKind) {
    use StorageKind::{Nvme, Other, Sata, Usb};
    match bus_type {
        1 => ("SCSI", Other),
        2 => ("ATAPI", Other),
        3 => ("ATA", Sata),
        4 => ("IEEE 1394", Other),
        5 => ("SSA", Other),
        6 => ("Fibre Channel", Other),
        7 => ("USB", Usb),
        8 => ("RAID", Other),
        9 => ("iSCSI", Other),
        10 => ("SAS", Other),
        11 => ("SATA", Sata),
        12 => ("SD", Other),
        13 => ("MMC", Other),
        14 => ("Virtual", Other),
        15 => ("File-backed virtual", Other),
        16 => ("Storage Spaces", Other),
        17 => ("NVMe", Nvme),
        18 => ("SCM", Other),
        19 => ("UFS", Other),
        _ => ("Unknown", Other),
    }
}

/// Parses a `\PhysicalDisk(*)` instance name such as `0 C: D:` into (disk number, letters).
pub fn pdh_instance(name: &str) -> Option<(u32, Vec<String>)> {
    let mut parts = name.split_whitespace();
    let n = parts.next()?.parse().ok()?;
    Some((n, parts.map(str::to_owned).collect()))
}

/// Manufacturer from the model string when the descriptor only says "ATA" / "NVMe".
fn vendor_from_model(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_uppercase();
    const TABLE: [(&str, &str); 22] = [
        ("SAMSUNG", "Samsung"),
        ("WDC", "Western Digital"),
        ("WD", "Western Digital"),
        ("WESTERN DIGITAL", "Western Digital"),
        ("SANDISK", "SanDisk"),
        ("ST", "Seagate"),
        ("SEAGATE", "Seagate"),
        ("CT", "Crucial"),
        ("CRUCIAL", "Crucial"),
        ("MICRON", "Micron"),
        ("KINGSTON", "Kingston"),
        ("INTEL", "Intel"),
        ("SK HYNIX", "SK hynix"),
        ("HFM", "SK hynix"),
        ("TOSHIBA", "Toshiba"),
        ("KIOXIA", "Kioxia"),
        ("KBG", "Kioxia"),
        ("HGST", "HGST"),
        ("ADATA", "ADATA"),
        ("CORSAIR", "Corsair"),
        ("SABRENT", "Sabrent"),
        ("PHISON", "Phison"),
    ];
    TABLE
        .iter()
        .find(|(p, _)| m.starts_with(p))
        .map(|(_, v)| *v)
}

/// `StorageDeviceTemperatureProperty` (works unelevated on most NVMe and some SATA drives).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TempProperty {
    pub current_c: Option<f64>,
    pub warn_c: Option<f64>,
    pub crit_c: Option<f64>,
}

/// Decodes `STORAGE_TEMPERATURE_DATA_DESCRIPTOR`: sensor 0 is the drive's own temperature.
pub fn temperature_descriptor(b: &[u8]) -> Option<TempProperty> {
    if b.len() < 24 {
        return None;
    }
    let i16_at = |o: usize| i16::from_le_bytes([b[o], b[o + 1]]);
    let plausible = |t: i16| (1..=150).contains(&t).then_some(f64::from(t));
    let count = usize::from(u16::from_le_bytes([b[12], b[13]]));
    let current_c = (0..count)
        .map(|i| 24 + i * 16)
        .take_while(|o| o + 16 <= b.len())
        .find(|&o| u16::from_le_bytes([b[o], b[o + 1]]) == 0)
        .and_then(|o| plausible(i16_at(o + 2)));
    Some(TempProperty {
        current_c,
        warn_c: plausible(i16_at(10)),
        crit_c: plausible(i16_at(8)),
    })
}

/// Everything the Windows side gathers about one physical drive.
#[derive(Debug, Clone, Default)]
pub struct RawDrive {
    pub index: u32,
    pub descriptor: Option<Descriptor>,
    pub size_bytes: Option<u64>,
    /// `StorageDeviceSeekPenaltyProperty`: true for spinning disks.
    pub seek_penalty: Option<bool>,
    pub temp: Option<TempProperty>,
    pub nvme_identify: Option<Vec<u8>>,
    pub nvme_log: Option<Vec<u8>>,
    pub ata_identify: Option<Vec<u8>>,
    pub smart_data: Option<Vec<u8>>,
    pub smart_thresholds: Option<Vec<u8>>,
    pub volumes: Vec<VolumeInfo>,
    pub system: bool,
    pub smart_error: Option<String>,
}

fn nvme_health(h: &nvme::HealthLog) -> NvmeHealth {
    NvmeHealth {
        critical_warning: u32::from(h.critical_warning),
        critical_warnings: nvme::critical_warnings(h.critical_warning)
            .into_iter()
            .map(str::to_owned)
            .collect(),
        available_spare_pct: u32::from(h.available_spare_pct),
        spare_threshold_pct: u32::from(h.spare_threshold_pct),
        percentage_used: u32::from(h.percentage_used),
        data_units_read: h.data_units_read,
        data_units_written: h.data_units_written,
        host_reads: h.host_reads,
        host_writes: h.host_writes,
        busy_minutes: h.busy_minutes,
        unsafe_shutdowns: h.unsafe_shutdowns,
        media_errors: h.media_errors,
        error_log_entries: h.error_log_entries,
        warning_temp_minutes: h.warning_temp_minutes,
        critical_temp_minutes: h.critical_temp_minutes,
        sensors_c: h.sensors_c.to_vec(),
        throttle_events: h.throttle_events.to_vec(),
        throttle_secs: h.throttle_secs.to_vec(),
    }
}

/// Remaining-life level at which a drive is flagged, as CrystalDiskInfo does by default.
const LIFE_CAUTION_PCT: f64 = 10.0;

fn grade_nvme(h: &nvme::HealthLog, notes: &mut Vec<String>) -> Status {
    let mut status = Status::Ok;
    let fail_bits = h.critical_warning & !0b10;
    if fail_bits != 0 {
        status = Status::Fail;
        notes.extend(nvme::critical_warnings(fail_bits).iter().map(|n| cap(n)));
    }
    if h.critical_warning & 0b10 != 0 {
        status = worst(status, Status::Warn);
        notes.push("Temperature outside the safe range".into());
    }
    let life = 100.0 - f64::from(h.percentage_used.min(100));
    if life <= LIFE_CAUTION_PCT {
        status = worst(status, Status::Warn);
        notes.push(format!("{}% of rated endurance used", h.percentage_used));
    }
    if h.media_errors > 0.0 {
        status = worst(status, Status::Warn);
        notes.push(format!("{} media / data integrity errors", h.media_errors));
    }
    status
}

fn grade_ata(
    attrs: &[ata::Attribute],
    d: &ata::Derived,
    ssd: bool,
    notes: &mut Vec<String>,
) -> Status {
    let mut status = Status::Ok;
    for a in attrs.iter().filter(|a| a.failing()) {
        status = Status::Fail;
        notes.push(format!(
            "{} ({:02X}) at or below its failure threshold",
            ata::name(a.id, ssd),
            a.id
        ));
    }
    for (count, what) in [
        (d.reallocated, "reallocated sectors"),
        (d.pending, "pending sectors"),
        (d.uncorrectable, "uncorrectable sectors"),
    ] {
        if let Some(n) = count.filter(|&n| n > 0) {
            status = worst(status, Status::Warn);
            notes.push(format!("{n} {what}"));
        }
    }
    if let Some(life) = d.life_pct.filter(|&l| l <= LIFE_CAUTION_PCT) {
        status = worst(status, Status::Warn);
        notes.push(format!("{life}% life remaining"));
    }
    status
}

fn worst(a: Status, b: Status) -> Status {
    let rank = |s| match s {
        Status::Fail => 3,
        Status::Warn => 2,
        Status::Ok => 1,
        Status::Unknown => 0,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

pub fn build(raw: &RawDrive) -> DriveInfo {
    let desc = raw.descriptor.clone().unwrap_or_default();
    let (bus_label, mut kind) = bus(desc.bus_type);
    let nid = raw.nvme_identify.as_deref().and_then(nvme::identify);
    let log = raw.nvme_log.as_deref().and_then(nvme::health_log);
    let aid = raw.ata_identify.as_deref().and_then(ata::identify);
    if nid.is_some() || log.is_some() {
        kind = StorageKind::Nvme;
    }

    let descriptor_model = || {
        let v = desc
            .vendor
            .as_deref()
            .filter(|v| !matches!(*v, "ATA" | "NVMe"));
        match (v, desc.product.as_deref()) {
            (Some(v), Some(p)) if !p.starts_with(v) => Some(format!("{v} {p}")),
            (_, Some(p)) => Some(p.to_owned()),
            (Some(v), None) => Some(v.to_owned()),
            (None, None) => None,
        }
    };
    let model = nid
        .as_ref()
        .map(|i| i.model.clone())
        .or_else(|| aid.as_ref().map(|i| i.model.clone()))
        .filter(|m| !m.is_empty())
        .or_else(descriptor_model)
        .unwrap_or_else(|| format!("Disk {}", raw.index));
    let vendor = vendor_from_model(&model).map(str::to_owned).or_else(|| {
        desc.vendor
            .clone()
            .filter(|v| !matches!(v.as_str(), "ATA" | "NVMe"))
    });
    let serial = nid
        .as_ref()
        .map(|i| i.serial.clone())
        .or_else(|| aid.as_ref().map(|i| i.serial.clone()))
        .filter(|s| !s.is_empty())
        .or_else(|| desc.serial.clone())
        .map(LocalOnly);
    let firmware = nid
        .as_ref()
        .map(|i| i.firmware.clone())
        .or_else(|| aid.as_ref().map(|i| i.firmware.clone()))
        .filter(|f| !f.is_empty())
        .or_else(|| desc.revision.clone());
    let ssd = if kind == StorageKind::Nvme {
        Some(true)
    } else {
        aid.as_ref()
            .and_then(|i| i.rpm)
            .map(|r| r == 0)
            .or(raw.seek_penalty.map(|p| !p))
    };
    let interface = match (&nid, &aid) {
        (Some(n), _) => Some(match &n.version {
            Some(v) => format!("PCIe NVMe {v}"),
            None => "PCIe NVMe".to_owned(),
        }),
        (None, Some(a)) => a.sata_max_gbps.map(|max| match a.sata_current_gbps {
            Some(cur) if cur < max => format!("SATA {max:.1} Gb/s (running at {cur:.1} Gb/s)"),
            _ => format!("SATA {max:.1} Gb/s"),
        }),
        _ => None,
    }
    .or_else(|| (kind == StorageKind::Nvme).then(|| "PCIe NVMe".to_owned()));
    let capacity = raw
        .size_bytes
        .map(|b| b as f64)
        .or_else(|| nid.as_ref().and_then(|i| i.capacity_bytes))
        .or_else(|| aid.as_ref().and_then(|i| i.capacity_bytes));

    let mut info = DriveInfo {
        id: format!("disk{}", raw.index),
        index: raw.index,
        model,
        vendor,
        serial,
        firmware,
        kind,
        bus: bus_label.to_owned(),
        interface,
        ssd,
        rpm: aid.as_ref().and_then(|i| i.rpm).filter(|&r| r > 0),
        removable: desc.removable,
        system: raw.system,
        trim: aid.as_ref().map(|i| i.trim),
        capacity_mb: capacity.map(|b| b / MB),
        volumes: raw.volumes.clone(),
        temp_c: raw.temp.and_then(|t| t.current_c),
        temp_warn_c: raw.temp.and_then(|t| t.warn_c),
        temp_crit_c: raw.temp.and_then(|t| t.crit_c),
        smart_error: raw.smart_error.clone(),
        ..Default::default()
    };
    if !raw.volumes.is_empty() {
        let sum = |f: fn(&VolumeInfo) -> Option<f64>| raw.volumes.iter().filter_map(f).sum::<f64>();
        let total = sum(|v| v.total_mb);
        let free = sum(|v| v.free_mb);
        info.used_mb = Some(total - free);
        info.free_mb = Some(free);
    }

    let mut notes = Vec::new();
    if let Some(h) = &log {
        info.health = grade_nvme(h, &mut notes);
        info.health_pct = Some(100.0 - f64::from(h.percentage_used.min(100)));
        info.temp_c = h.composite_c.or(info.temp_c);
        info.power_on_hours = Some(h.power_on_hours);
        info.power_cycles = Some(h.power_cycles);
        info.total_read_gb = Some(h.read_gb());
        info.total_write_gb = Some(h.written_gb());
        info.nvme = Some(nvme_health(h));
    }
    if let Some(n) = &nid {
        info.temp_warn_c = n.warn_temp_c.or(info.temp_warn_c);
        info.temp_crit_c = n.crit_temp_c.or(info.temp_crit_c);
    }
    if let Some(data) = raw.smart_data.as_deref() {
        let attrs = ata::attributes(data, raw.smart_thresholds.as_deref());
        if !attrs.is_empty() {
            let is_ssd = ssd == Some(true);
            let d = ata::derive(&attrs, &info.model, is_ssd);
            info.health = grade_ata(&attrs, &d, is_ssd, &mut notes);
            info.health_pct = d.life_pct;
            info.temp_c = d.temp_c.or(info.temp_c);
            info.power_on_hours = d.power_on_hours;
            info.power_cycles = d.power_cycles;
            info.total_read_gb = d.read_gb;
            info.total_write_gb = d.written_gb;
            info.smart = attrs
                .iter()
                .map(|a| SmartAttribute {
                    id: u32::from(a.id),
                    name: ata::name(a.id, is_ssd).to_owned(),
                    current: u32::from(a.current),
                    worst: u32::from(a.worst),
                    threshold: a.threshold.map(u32::from),
                    raw: a.raw as f64,
                    raw_hex: format!("{:012X}", a.raw),
                    prefail: a.prefail(),
                    failing: a.failing(),
                })
                .collect();
        }
    }
    info.health_notes = notes;
    info
}

/// The live-snapshot entry for a drive; rates are filled in by the provider.
pub fn snapshot_of(d: &DriveInfo) -> StorageSnapshot {
    StorageSnapshot {
        id: d.id.clone(),
        model: d.model.clone(),
        volumes: d
            .volumes
            .iter()
            .filter(|v| !v.letter.is_empty())
            .map(|v| v.letter.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        health: d.health,
        kind: d.kind,
        temp_c: d.temp_c,
        health_pct: d.health_pct,
        capacity_mb: d.capacity_mb,
        used_mb: d.used_mb,
        free_mb: d.free_mb,
        total_read_gb: d.total_read_gb,
        total_write_gb: d.total_write_gb,
        power_on_hours: d.power_on_hours,
        power_cycles: d.power_cycles,
        nvme_pct_used: d.nvme.as_ref().map(|n| f64::from(n.percentage_used)),
        nvme_available_spare_pct: d.nvme.as_ref().map(|n| f64::from(n.available_spare_pct)),
        nvme_critical_warning: d.nvme.as_ref().map(|n| n.critical_warning),
        ..Default::default()
    }
}

/// Drives ordered system drive first, then by index.
pub fn sort(drives: &mut [DriveInfo]) {
    drives.sort_by_key(|d| (!d.system, d.index));
}

/// Two made-up drives (NVMe system SSD + SATA HDD) for the browser build and non-Windows runs.
pub fn sample() -> Vec<DriveInfo> {
    let (smart, thr) = ata::fixtures::hdd_smart();
    let nvme = RawDrive {
        index: 0,
        descriptor: Some(Descriptor {
            vendor: Some("NVMe".into()),
            bus_type: 17,
            ..Default::default()
        }),
        size_bytes: Some(1_000_204_886_016),
        nvme_identify: Some(nvme::fixtures::identify()),
        nvme_log: Some(nvme::fixtures::health_log()),
        volumes: vec![VolumeInfo {
            letter: "C:".into(),
            label: Some("Windows".into()),
            file_system: Some("NTFS".into()),
            total_mb: Some(953_250.0),
            free_mb: Some(512_430.0),
        }],
        system: true,
        ..Default::default()
    };
    let hdd = RawDrive {
        index: 1,
        descriptor: Some(Descriptor {
            vendor: Some("ATA".into()),
            product: Some("ST2000DM008-2UB102".into()),
            bus_type: 11,
            ..Default::default()
        }),
        size_bytes: Some(2_000_398_934_016),
        seek_penalty: Some(true),
        ata_identify: Some(ata::fixtures::hdd_identify()),
        smart_data: Some(smart),
        smart_thresholds: Some(thr),
        volumes: vec![VolumeInfo {
            letter: "D:".into(),
            label: Some("Data".into()),
            file_system: Some("NTFS".into()),
            total_mb: Some(1_907_712.0),
            free_mb: Some(1_021_880.0),
        }],
        ..Default::default()
    };
    let mut drives: Vec<DriveInfo> = [nvme, hdd].iter().map(build).collect();
    for d in &mut drives {
        d.serial = None;
    }
    drives
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor_bytes(vendor: &str, product: &str, serial: &str, bus: u32) -> Vec<u8> {
        let mut b = vec![0u8; 40];
        b[10] = 0;
        let add = |b: &mut Vec<u8>, at: usize, s: &str| {
            let off = b.len() as u32;
            b[at..at + 4].copy_from_slice(&off.to_le_bytes());
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        };
        add(&mut b, 12, vendor);
        add(&mut b, 16, product);
        add(&mut b, 24, serial);
        b[28..32].copy_from_slice(&bus.to_le_bytes());
        b
    }

    #[test]
    fn decodes_descriptor() {
        let d = descriptor(&descriptor_bytes(
            "NVMe    ",
            "Samsung SSD 980 PRO 1TB ",
            "0025_38B1",
            17,
        ))
        .expect("descriptor");
        assert_eq!(d.vendor.as_deref(), Some("NVMe"));
        assert_eq!(d.product.as_deref(), Some("Samsung SSD 980 PRO 1TB"));
        assert_eq!(d.revision, None);
        assert_eq!(d.bus_type, 17);
        assert!(!d.removable);
        assert!(descriptor(&[0; 10]).is_none());
    }

    #[test]
    fn decodes_temperature_descriptor() {
        let mut b = vec![0u8; 40];
        b[8..10].copy_from_slice(&84i16.to_le_bytes());
        b[10..12].copy_from_slice(&80i16.to_le_bytes());
        b[12] = 1;
        b[26..28].copy_from_slice(&43i16.to_le_bytes());
        let t = temperature_descriptor(&b).expect("temps");
        assert_eq!(
            (t.current_c, t.warn_c, t.crit_c),
            (Some(43.0), Some(80.0), Some(84.0))
        );
        b[12] = 0;
        b[8] = 0;
        let t = temperature_descriptor(&b).expect("temps");
        assert_eq!((t.current_c, t.crit_c), (None, None));
    }

    #[test]
    fn parses_pdh_instances() {
        assert_eq!(
            pdh_instance("0 C: D:"),
            Some((0, vec!["C:".into(), "D:".into()]))
        );
        assert_eq!(pdh_instance("2"), Some((2, vec![])));
        assert_eq!(pdh_instance("_Total"), None);
    }

    #[test]
    fn builds_nvme_drive() {
        let d = &sample()[0];
        assert_eq!(d.id, "disk0");
        assert_eq!(d.model, "Samsung SSD 980 PRO 1TB");
        assert_eq!(d.vendor.as_deref(), Some("Samsung"));
        assert_eq!(d.kind, StorageKind::Nvme);
        assert_eq!(d.interface.as_deref(), Some("PCIe NVMe 1.3"));
        assert_eq!(d.ssd, Some(true));
        assert_eq!(d.health, Status::Ok);
        assert_eq!(d.health_pct, Some(98.0));
        assert_eq!(d.temp_c, Some(41.0));
        assert_eq!((d.temp_warn_c, d.temp_crit_c), (Some(82.0), Some(85.0)));
        assert_eq!(d.power_on_hours, Some(3456.0));
        assert_eq!(d.used_mb, Some(953_250.0 - 512_430.0));
        assert!(d.nvme.is_some() && d.smart.is_empty());
        let s = snapshot_of(d);
        assert_eq!((s.volumes.as_str(), s.nvme_pct_used), ("C:", Some(2.0)));
    }

    #[test]
    fn builds_sata_hdd() {
        let d = &sample()[1];
        assert_eq!(d.model, "ST2000DM008-2UB102");
        assert_eq!(d.vendor.as_deref(), Some("Seagate"));
        assert_eq!(d.kind, StorageKind::Sata);
        assert_eq!(d.ssd, Some(false));
        assert_eq!(d.rpm, Some(7200));
        assert_eq!(
            d.interface.as_deref(),
            Some("SATA 6.0 Gb/s (running at 3.0 Gb/s)")
        );
        assert_eq!(d.health, Status::Warn);
        assert_eq!(d.health_notes, ["8 reallocated sectors"]);
        assert_eq!(d.health_pct, None);
        assert_eq!(d.temp_c, Some(34.0));
        assert!(d
            .smart
            .iter()
            .any(|a| a.id == 5 && a.raw_hex == "000000000008"));
    }

    #[test]
    fn nvme_grading() {
        let mut h = nvme::health_log(&nvme::fixtures::health_log()).expect("log");
        let grade = |h: &nvme::HealthLog| {
            let mut n = Vec::new();
            (grade_nvme(h, &mut n), n)
        };
        assert_eq!(grade(&h).0, Status::Ok);
        h.critical_warning = 0b10;
        assert_eq!(grade(&h).0, Status::Warn);
        h.critical_warning = 0b1;
        let (s, n) = grade(&h);
        assert_eq!(s, Status::Fail);
        assert_eq!(n, ["Available spare below threshold"]);
        h.critical_warning = 0;
        h.percentage_used = 93;
        h.media_errors = 2.0;
        let (s, n) = grade(&h);
        assert_eq!(s, Status::Warn);
        assert_eq!(
            n,
            [
                "93% of rated endurance used",
                "2 media / data integrity errors"
            ]
        );
    }

    #[test]
    fn failing_attribute_is_bad() {
        let (mut data, thr) = ata::fixtures::hdd_smart();
        // Slot 0 is attribute 5 with threshold 10: drop its normalized value to 5.
        data[2 + 3] = 5;
        let raw = RawDrive {
            ata_identify: Some(ata::fixtures::hdd_identify()),
            smart_data: Some(data),
            smart_thresholds: Some(thr),
            ..Default::default()
        };
        let d = build(&raw);
        assert_eq!(d.health, Status::Fail);
        assert_eq!(
            d.health_notes[0],
            "Reallocated Sectors Count (05) at or below its failure threshold"
        );
        assert!(d.smart[0].failing);
    }

    #[test]
    fn without_smart_health_is_unknown_and_falls_back_to_descriptor() {
        let raw = RawDrive {
            index: 3,
            descriptor: Some(Descriptor {
                vendor: Some("Generic".into()),
                product: Some("USB Flash".into()),
                bus_type: 7,
                removable: true,
                ..Default::default()
            }),
            temp: Some(TempProperty {
                current_c: Some(30.0),
                ..Default::default()
            }),
            smart_error: Some("USB bridge does not pass SMART through".into()),
            ..Default::default()
        };
        let d = build(&raw);
        assert_eq!(d.model, "Generic USB Flash");
        assert_eq!((d.kind, d.bus.as_str()), (StorageKind::Usb, "USB"));
        assert_eq!(d.health, Status::Unknown);
        assert_eq!(d.temp_c, Some(30.0));
        assert!(d.removable && d.used_mb.is_none());
    }

    #[test]
    fn serial_never_in_debug_and_sample_has_none() {
        let raw = RawDrive {
            nvme_identify: Some(nvme::fixtures::identify()),
            ..Default::default()
        };
        let d = build(&raw);
        assert_eq!(
            d.serial.as_ref().map(|s| s.0.as_str()),
            Some("S5GXNX0TESTSERIAL")
        );
        assert!(!format!("{d:?}").contains("TESTSERIAL"));
        assert!(sample().iter().all(|d| d.serial.is_none()));
    }

    #[test]
    fn system_drive_sorts_first() {
        let mut v = sample();
        v.reverse();
        sort(&mut v);
        assert!(v[0].system);
    }
}

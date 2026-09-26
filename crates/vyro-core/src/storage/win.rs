//! Windows storage collector: `\\.\PhysicalDriveN` property queries, NVMe log pages through
//! `IOCTL_STORAGE_QUERY_PROPERTY`, ATA SMART through `IOCTL_ATA_PASS_THROUGH` (with the legacy
//! `SMART_RCV_DRIVE_DATA` as fallback), volumes, and `\PhysicalDisk(*)` PDH counters.
//!
//! SMART needs a read/write handle, which Windows only grants to administrators; unelevated,
//! drives still list with capacity, volumes, rates and (usually) temperature, and `smartError`
//! says why the rest is missing. Spinning disks are asked for SMART rarely and never while spun
//! down, so monitoring does not keep them awake.

use std::collections::{BTreeMap, HashMap};
use std::mem::size_of;
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::Foundation::{
    CloseHandle, E_ACCESSDENIED, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING,
};
use windows::Win32::Storage::IscsiDisc::{
    ATA_FLAGS_DATA_IN, ATA_FLAGS_DRDY_REQUIRED, ATA_PASS_THROUGH_EX, IOCTL_ATA_PASS_THROUGH,
};
use windows::Win32::System::Ioctl::{
    IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_STORAGE_QUERY_PROPERTY, SMART_RCV_DRIVE_DATA,
};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::Win32::System::IO::DeviceIoControl;

use super::{build, descriptor, pdh_instance, snapshot_of, sort, temperature_descriptor, RawDrive};
use crate::info::{DriveInfo, VolumeInfo};
use crate::live::pdh::{Counter, Query};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::{SensorSnapshot, StorageKind};

const MAX_DRIVES: u32 = 32;
const MIB: f64 = 1024.0 * 1024.0;
/// How often health (SMART / NVMe log), temperature and volumes are re-read.
const REFRESH: Duration = Duration::from_secs(5);
/// Spinning disks: SMART this rarely, so polling never holds off their spin-down timer.
const HDD_SMART: Duration = Duration::from_secs(300);

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;

// STORAGE_PROPERTY_ID values.
const PROP_DEVICE: u32 = 0;
const PROP_SEEK_PENALTY: u32 = 7;
const PROP_TEMPERATURE: u32 = 48;
const PROP_PROTOCOL_SPECIFIC: u32 = 50;
// STORAGE_PROTOCOL_TYPE / STORAGE_PROTOCOL_NVME_DATA_TYPE.
const PROTOCOL_NVME: u32 = 3;
const NVME_IDENTIFY: u32 = 1;
const NVME_LOG_PAGE: u32 = 2;
const NVME_CNS_CONTROLLER: u32 = 1;
const NVME_LOG_HEALTH: u32 = 2;

const NEED_ADMIN: &str = "Run Vyro as administrator to read SMART / health data";

/// An open device handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW and is closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open(path: &str, access: u32) -> windows::core::Result<Handle> {
    // SAFETY: plain CreateFileW on a device path; the handle is owned by `Handle`.
    unsafe {
        CreateFileW(
            &HSTRING::from(path),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map(Handle)
}

/// Sends an IOCTL with `buf` as both input (first `in_len` bytes) and output; returns the
/// number of bytes written.
fn ioctl(h: &Handle, code: u32, buf: &mut [u8], in_len: usize) -> windows::core::Result<usize> {
    let mut returned = 0u32;
    let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    let ptr: *mut std::ffi::c_void = buf.as_mut_ptr().cast();
    // SAFETY: `buf` is valid for reads of `in_len` and writes of `len` bytes for the call;
    // DeviceIoControl copies the input before writing the output (METHOD_BUFFERED).
    unsafe {
        DeviceIoControl(
            h.0,
            code,
            Some(ptr.cast_const()),
            u32::try_from(in_len).unwrap_or(len),
            Some(ptr),
            len,
            Some(&mut returned),
            None,
        )
    }?;
    Ok(returned as usize)
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// `IOCTL_STORAGE_QUERY_PROPERTY` standard query for `id`.
fn property(h: &Handle, id: u32, out_len: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; out_len.max(12)];
    buf[..4].copy_from_slice(&id.to_le_bytes());
    let n = ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &mut buf, 12).ok()?;
    buf.truncate(n);
    (n >= 8).then_some(buf)
}

/// NVMe Identify / Get Log Page through `StorageDeviceProtocolSpecificProperty`.
fn nvme(h: &Handle, data_type: u32, value: u32, len: usize) -> windows::core::Result<Vec<u8>> {
    // STORAGE_PROPERTY_QUERY header (8) + STORAGE_PROTOCOL_SPECIFIC_DATA (40) + data.
    let mut buf = vec![0u8; 48 + len];
    let put = |b: &mut [u8], o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut buf, 0, PROP_PROTOCOL_SPECIFIC);
    put(&mut buf, 8, PROTOCOL_NVME);
    put(&mut buf, 12, data_type);
    put(&mut buf, 16, value);
    put(&mut buf, 24, 40);
    put(&mut buf, 28, u32::try_from(len).unwrap_or(0));
    let in_len = buf.len();
    ioctl(h, IOCTL_STORAGE_QUERY_PROPERTY, &mut buf, in_len)?;
    // Reply: STORAGE_PROTOCOL_DATA_DESCRIPTOR = Version, Size, then the same 40-byte block.
    let off = 8 + u32_at(&buf, 24).unwrap_or(0) as usize;
    let got = (u32_at(&buf, 28).unwrap_or(0) as usize).min(len);
    buf.get(off..off + got)
        .filter(|d| d.len() == len)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))
}

/// Task file: features, count, LBA low, LBA mid, LBA high, device, command, reserved.
fn ata(h: &Handle, task: [u8; 8], data_in: bool) -> windows::core::Result<(Vec<u8>, [u8; 8])> {
    let hdr = size_of::<ATA_PASS_THROUGH_EX>();
    let len = if data_in { 512 } else { 0 };
    let mut buf = vec![0u8; hdr + len];
    let flags = ATA_FLAGS_DRDY_REQUIRED | if data_in { ATA_FLAGS_DATA_IN } else { 0 };
    let apt = ATA_PASS_THROUGH_EX {
        Length: hdr as u16,
        AtaFlags: flags as u16,
        DataTransferLength: len as u32,
        TimeOutValue: 3,
        DataBufferOffset: hdr,
        CurrentTaskFile: task,
        ..Default::default()
    };
    // SAFETY: `buf` holds at least `hdr` bytes; unaligned write of a plain-data struct.
    unsafe { std::ptr::write_unaligned(buf.as_mut_ptr().cast(), apt) };
    let in_len = buf.len();
    ioctl(h, IOCTL_ATA_PASS_THROUGH, &mut buf, in_len)?;
    // SAFETY: the driver wrote the header back in place.
    let out: ATA_PASS_THROUGH_EX = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast()) };
    // Status register bit 0 = ERR.
    if out.CurrentTaskFile[6] & 1 != 0 {
        return Err(windows::core::Error::from_hresult(
            windows::Win32::Foundation::E_FAIL,
        ));
    }
    Ok((buf.split_off(hdr), out.CurrentTaskFile))
}

/// Legacy `SMART_RCV_DRIVE_DATA` (SENDCMDINPARAMS is 1-byte packed; 32 bytes without data).
fn smart_rcv(h: &Handle, drive: u32, features: u8, command: u8) -> windows::core::Result<Vec<u8>> {
    let mut buf = vec![0u8; 16 + 512];
    buf[..4].copy_from_slice(&512u32.to_le_bytes());
    let (mid, high) = if command == 0xB0 {
        (0x4F, 0xC2)
    } else {
        (0, 0)
    };
    buf[4..12].copy_from_slice(&[
        features,
        1,
        1,
        mid,
        high,
        0xA0 | ((drive as u8 & 1) << 4),
        command,
        0,
    ]);
    buf[12] = drive as u8;
    ioctl(h, SMART_RCV_DRIVE_DATA, &mut buf, 32)?;
    Ok(buf.split_off(16))
}

const IDENTIFY: [u8; 8] = [0, 1, 0, 0, 0, 0, 0xEC, 0];
const SMART_DATA: [u8; 8] = [0xD0, 1, 1, 0x4F, 0xC2, 0, 0xB0, 0];
const SMART_THRESHOLDS: [u8; 8] = [0xD1, 1, 1, 0x4F, 0xC2, 0, 0xB0, 0];
const CHECK_POWER_MODE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xE5, 0];

fn access_denied(e: &windows::core::Error) -> bool {
    e.code() == E_ACCESSDENIED
}

/// What to read on this pass.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Smart {
    Skip,
    Read,
}

/// Reads one drive. `None` when `\\.\PhysicalDriveN` does not exist.
fn read(
    index: u32,
    smart: Smart,
    volumes: &[(u32, VolumeInfo)],
    system_disk: Option<u32>,
) -> Option<RawDrive> {
    let path = format!(r"\\.\PhysicalDrive{index}");
    let rw = open(&path, GENERIC_READ.0 | GENERIC_WRITE.0);
    let elevated = rw.is_ok();
    let h = match rw {
        Ok(h) => h,
        Err(_) => open(&path, 0).ok()?,
    };
    let mut raw = RawDrive {
        index,
        descriptor: property(&h, PROP_DEVICE, 1024)
            .as_deref()
            .and_then(descriptor),
        seek_penalty: property(&h, PROP_SEEK_PENALTY, 12)
            .map(|b| b.get(8).is_some_and(|&p| p != 0)),
        temp: property(&h, PROP_TEMPERATURE, 24 + 16 * 8)
            .as_deref()
            .and_then(temperature_descriptor),
        volumes: volumes
            .iter()
            .filter(|(d, _)| *d == index)
            .map(|(_, v)| v.clone())
            .collect(),
        system: system_disk == Some(index),
        ..Default::default()
    };
    let mut geo = vec![0u8; 256];
    if let Ok(n) = ioctl(&h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &mut geo, 0) {
        if n >= 32 {
            let size = i64::from_le_bytes(geo[24..32].try_into().unwrap_or_default());
            raw.size_bytes = u64::try_from(size).ok().filter(|&s| s > 0);
        }
    }
    if smart == Smart::Skip {
        return Some(raw);
    }

    let bus = raw.descriptor.as_ref().map_or(0, |d| d.bus_type);
    match super::bus(bus).1 {
        StorageKind::Nvme => {
            let log = nvme(&h, NVME_LOG_PAGE, NVME_LOG_HEALTH, 512);
            raw.nvme_identify = nvme(&h, NVME_IDENTIFY, NVME_CNS_CONTROLLER, 4096).ok();
            match log {
                Ok(l) => raw.nvme_log = Some(l),
                Err(e) if !elevated || access_denied(&e) => {
                    raw.smart_error = Some(NEED_ADMIN.into())
                }
                Err(e) => raw.smart_error = Some(format!("NVMe health log not available ({e})")),
            }
        }
        StorageKind::Sata if !elevated => raw.smart_error = Some(NEED_ADMIN.into()),
        StorageKind::Sata => {
            // Spinning disk asleep? CHECK POWER MODE does not wake it; SMART would.
            if raw.seek_penalty == Some(true) {
                if let Ok((_, tf)) = ata(&h, CHECK_POWER_MODE, false) {
                    if tf[1] == 0 {
                        raw.smart_error =
                            Some("Disk is spun down; SMART skipped so it stays asleep".into());
                        return Some(raw);
                    }
                }
            }
            match ata(&h, IDENTIFY, true) {
                Ok((id, _)) => {
                    raw.ata_identify = Some(id);
                    raw.smart_data = ata(&h, SMART_DATA, true).ok().map(|r| r.0);
                    raw.smart_thresholds = ata(&h, SMART_THRESHOLDS, true).ok().map(|r| r.0);
                }
                Err(_) => {
                    raw.ata_identify = smart_rcv(&h, index, 0, 0xEC).ok();
                    raw.smart_data = smart_rcv(&h, index, 0xD0, 0xB0).ok();
                    raw.smart_thresholds = smart_rcv(&h, index, 0xD1, 0xB0).ok();
                }
            }
            if raw.smart_data.is_none() {
                raw.smart_error = Some("The controller did not return SMART data".into());
            }
        }
        StorageKind::Usb => raw.smart_error = Some("SMART is not read through USB bridges".into()),
        StorageKind::Other => {
            raw.smart_error = Some(match bus {
                8 => "Behind a RAID / Intel RST controller; SMART is not reachable".into(),
                14..=16 => "Virtual disk; no SMART".into(),
                _ => format!("SMART over {} is not supported", super::bus(bus).0),
            });
        }
    }
    Some(raw)
}

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// Mounted fixed / removable volumes as (disk number, volume), and the disk holding Windows.
fn volumes() -> (Vec<(u32, VolumeInfo)>, Option<u32>) {
    let mut windir = [0u16; 260];
    // SAFETY: valid buffer.
    let n = unsafe { GetSystemWindowsDirectoryW(Some(&mut windir)) } as usize;
    let system_letter = (n >= 2).then(|| wide_to_string(&windir[..2]).to_ascii_uppercase());
    // SAFETY: no arguments.
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    let mut system_disk = None;
    for i in 0..26u8 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:", char::from(b'A' + i));
        let root = HSTRING::from(format!("{letter}\\"));
        // SAFETY: valid root path.
        let kind = unsafe { GetDriveTypeW(&root) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        let Ok(h) = open(&format!(r"\\.\{letter}"), 0) else {
            continue;
        };
        // VOLUME_DISK_EXTENTS: count, pad, then {DiskNumber, pad, offset, length} records.
        let mut ext = vec![0u8; 8 + 24 * 8];
        let Ok(got) = ioctl(&h, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &mut ext, 0) else {
            continue;
        };
        let Some(disk) = (got >= 32).then(|| u32_at(&ext, 8)).flatten() else {
            continue;
        };
        let mut label = [0u16; 261];
        let mut fs = [0u16; 261];
        // SAFETY: valid root path and buffers.
        let named = unsafe {
            GetVolumeInformationW(&root, Some(&mut label), None, None, None, Some(&mut fs))
        }
        .is_ok();
        let (mut total, mut free) = (0u64, 0u64);
        // SAFETY: valid root path and out-pointers.
        let sized =
            unsafe { GetDiskFreeSpaceExW(&root, None, Some(&mut total), Some(&mut free)) }.is_ok();
        if system_letter.as_deref() == Some(letter.as_str()) {
            system_disk = Some(disk);
        }
        let text = |w: &[u16]| Some(wide_to_string(w)).filter(|s| named && !s.is_empty());
        out.push((
            disk,
            VolumeInfo {
                letter,
                label: text(&label),
                file_system: text(&fs),
                total_mb: sized.then(|| total as f64 / MIB),
                free_mb: sized.then(|| free as f64 / MIB),
            },
        ));
    }
    (out, system_disk)
}

fn log_drive(d: &DriveInfo) {
    let smart = if d.nvme.is_some() {
        "NVMe log"
    } else if !d.smart.is_empty() {
        "ATA SMART"
    } else {
        d.smart_error.as_deref().unwrap_or("none")
    };
    log::info!(
        "{}: {} [{} / {}] {:.0} GB, health {:?}, temp {:?} °C, volumes [{}], health data: {smart}",
        d.id,
        d.model,
        d.bus,
        d.interface.as_deref().unwrap_or("?"),
        d.capacity_mb.unwrap_or(0.0) * MIB / 1e9,
        d.health,
        d.temp_c,
        d.volumes
            .iter()
            .map(|v| v.letter.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
}

/// Every physical drive with full health data (SMART read now, spun-down disks excepted).
pub fn collect() -> Vec<DriveInfo> {
    let (vols, system_disk) = volumes();
    let mut drives: Vec<DriveInfo> = (0..MAX_DRIVES)
        .filter_map(|i| read(i, Smart::Read, &vols, system_disk))
        .map(|raw| build(&raw))
        .collect();
    sort(&mut drives);
    drives.iter().for_each(log_drive);
    drives
}

struct Keys {
    temp: SensorKey,
    read: SensorKey,
    write: SensorKey,
    activity: SensorKey,
}

struct Tracked {
    raw: RawDrive,
    info: DriveInfo,
    last_smart: Option<Instant>,
    keys: Keys,
}

/// PDH counters by `\PhysicalDisk(*)` for rates and active time.
struct Rates {
    query: Query,
    read_bytes: Counter,
    write_bytes: Counter,
    reads: Counter,
    writes: Counter,
    idle: Counter,
}

impl Rates {
    fn open() -> Result<Self, BoxError> {
        let query = Query::open()?;
        let c = |name: &str| query.add(&format!(r"\PhysicalDisk(*)\{name}"));
        let rates = Self {
            read_bytes: c("Disk Read Bytes/sec")?,
            write_bytes: c("Disk Write Bytes/sec")?,
            reads: c("Disk Reads/sec")?,
            writes: c("Disk Writes/sec")?,
            idle: c("% Idle Time")?,
            query,
        };
        rates.query.collect()?;
        Ok(rates)
    }

    /// Per disk number: [read B/s, write B/s, reads/s, writes/s, idle %].
    fn sample(&self) -> HashMap<u32, [Option<f64>; 5]> {
        let mut out: HashMap<u32, [Option<f64>; 5]> = HashMap::new();
        if self.query.collect().is_err() {
            return out;
        }
        for (slot, c) in [
            &self.read_bytes,
            &self.write_bytes,
            &self.reads,
            &self.writes,
            &self.idle,
        ]
        .into_iter()
        .enumerate()
        {
            for (name, v) in c.array() {
                if let Some((disk, _)) = pdh_instance(&name) {
                    out.entry(disk).or_default()[slot] = Some(v);
                }
            }
        }
        out
    }
}

/// Live storage: rates every tick, health / temperature / volumes every [`REFRESH`].
#[derive(Default)]
pub struct StorageProvider {
    drives: BTreeMap<u32, Tracked>,
    rates: Option<Rates>,
    last_refresh: Option<Instant>,
}

impl StorageProvider {
    pub fn new() -> Self {
        Self::default()
    }

    fn refresh(&mut self, reg: &mut Registry) {
        let now = Instant::now();
        let (vols, system_disk) = volumes();
        let mut seen = Vec::new();
        for index in 0..MAX_DRIVES {
            let prev = self.drives.get(&index);
            let hdd = prev.is_some_and(|t| t.info.ssd == Some(false));
            let due = prev
                .and_then(|t| t.last_smart)
                .is_none_or(|at| now - at >= if hdd { HDD_SMART } else { REFRESH });
            let smart = if due { Smart::Read } else { Smart::Skip };
            let Some(mut raw) = read(index, smart, &vols, system_disk) else {
                continue;
            };
            seen.push(index);
            if let Some(t) = self.drives.get_mut(&index) {
                // Keep the last health data when this pass skipped or could not re-read it.
                let old = &t.raw;
                raw.nvme_identify = raw.nvme_identify.or_else(|| old.nvme_identify.clone());
                raw.nvme_log = raw.nvme_log.or_else(|| old.nvme_log.clone());
                raw.ata_identify = raw.ata_identify.or_else(|| old.ata_identify.clone());
                raw.smart_data = raw.smart_data.or_else(|| old.smart_data.clone());
                raw.smart_thresholds = raw
                    .smart_thresholds
                    .or_else(|| old.smart_thresholds.clone());
                if smart == Smart::Skip {
                    raw.smart_error = old.smart_error.clone();
                }
                t.info = build(&raw);
                t.raw = raw;
                if due {
                    t.last_smart = Some(now);
                }
            } else {
                let info = build(&raw);
                log_drive(&info);
                let keys = register(reg, &info);
                self.drives.insert(
                    index,
                    Tracked {
                        raw,
                        info,
                        last_smart: Some(now),
                        keys,
                    },
                );
            }
        }
        self.drives.retain(|i, t| {
            let keep = seen.contains(i);
            if !keep {
                log::info!("{}: removed", t.info.id);
            }
            keep
        });
        self.last_refresh = Some(now);
    }
}

fn register(reg: &mut Registry, d: &DriveInfo) -> Keys {
    let id = &d.id;
    let name = &d.model;
    let r = |reg: &mut Registry, what: &str, label: &str, unit, source, history: bool| {
        let desc = SensorDesc::new(
            format!("storage.{id}.{what}"),
            Group::Storage,
            format!("{name} {label}"),
            unit,
            source,
        );
        reg.register(if history { desc.with_history() } else { desc })
    };
    Keys {
        temp: r(
            reg,
            "temp",
            "Temperature",
            Unit::Celsius,
            Source::Smart,
            true,
        ),
        read: r(reg, "read", "Read", Unit::MegabytePerSec, Source::Pdh, true),
        write: r(
            reg,
            "write",
            "Write",
            Unit::MegabytePerSec,
            Source::Pdh,
            true,
        ),
        activity: r(
            reg,
            "activity",
            "Active Time",
            Unit::Percent,
            Source::Pdh,
            false,
        ),
    }
}

impl Provider for StorageProvider {
    fn name(&self) -> &'static str {
        "storage"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        match Rates::open() {
            Ok(r) => self.rates = Some(r),
            Err(e) => log::warn!("storage: PDH disk counters unavailable: {e}"),
        }
        self.refresh(reg);
        if self.drives.is_empty() && self.rates.is_none() {
            return Err("no physical drives found".into());
        }
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        if self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH) {
            self.refresh(reg);
        }
        let rates = self.rates.as_ref().map(Rates::sample).unwrap_or_default();
        for (index, t) in &self.drives {
            let mut s = snapshot_of(&t.info);
            let r = rates.get(index).copied().unwrap_or_default();
            s.temp_c = reg.set(t.keys.temp, t.info.temp_c);
            s.read_mbs = reg.set(t.keys.read, r[0].map(|v| v / MIB));
            s.write_mbs = reg.set(t.keys.write, r[1].map(|v| v / MIB));
            s.read_iops = r[2];
            s.write_iops = r[3];
            s.activity_pct = reg.set(
                t.keys.activity,
                r[4].map(|idle| (100.0 - idle).clamp(0.0, 100.0)),
            );
            snap.storage.push(s);
        }
        // System drive first, like the Storage panel.
        snap.storage.sort_by_key(|s| {
            let sys = self
                .drives
                .values()
                .any(|t| t.info.id == s.id && t.info.system);
            (
                !sys,
                s.id.trim_start_matches("disk")
                    .parse::<u32>()
                    .unwrap_or(u32::MAX),
            )
        });
        Ok(())
    }
}

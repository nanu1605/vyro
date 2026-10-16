//! Payloads served on demand by commands rather than in the 1 Hz `sensors` event (spec §4.4).
//!
//! `SystemInfo` and `HardwareId` are collected once at startup (Phase 1). Every field is
//! optional: `null` means "could not be determined on this machine", and admin-only fields are
//! `null` when the app is not elevated.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::{NetKind, Status, StorageKind};

// ---------------------------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemInfo {
    pub os: OsInfo,
    /// Unix time, seconds.
    pub boot_time: Option<i64>,
    /// Seconds since boot when this payload was produced.
    pub uptime_secs: Option<i64>,
    pub security: SecurityInfo,
    pub processes: ProcessOverview,
    pub startup_apps: Vec<StartupApp>,
    /// Newest first.
    pub power_events: Vec<PowerEvent>,
    /// Whether the app runs elevated. Admin-only fields are `null` when it does not.
    pub elevated: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OsInfo {
    /// e.g. "Windows 11 Pro".
    pub name: Option<String>,
    /// Edition id, e.g. "Professional".
    pub edition: Option<String>,
    /// Feature update, e.g. "24H2".
    pub display_version: Option<String>,
    /// Build with update revision, e.g. "26100.2033".
    pub build: Option<String>,
    /// Unix time, seconds.
    pub install_date: Option<i64>,
    /// e.g. "x64".
    pub architecture: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SecurityInfo {
    /// Firmware boots in UEFI mode (vs. legacy BIOS).
    pub uefi: Option<bool>,
    pub secure_boot: Option<bool>,
    pub tpm_present: Option<bool>,
    /// "2.0" or "1.2".
    pub tpm_version: Option<String>,
    /// Hardware virtualization (VT-x / AMD-V) enabled in firmware.
    pub virtualization_enabled: Option<bool>,
    /// A hypervisor (Hyper-V, VBS, …) is running underneath Windows.
    pub hypervisor_present: Option<bool>,
    /// BitLocker protection on the system drive. Requires elevation.
    pub bitlocker_system_drive: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessOverview {
    pub processes: Option<u32>,
    pub threads: Option<u32>,
    pub handles: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum StartupSource {
    /// `HKLM\…\Run` (all users).
    RegistryMachine,
    /// `HKCU\…\Run` (current user).
    RegistryUser,
    /// All-users Startup folder.
    FolderMachine,
    /// Current user's Startup folder.
    FolderUser,
    /// Task Scheduler task with a logon or boot trigger.
    ScheduledTask,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StartupApp {
    pub name: String,
    pub command: String,
    pub source: StartupSource,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PowerEventKind {
    Boot,
    Shutdown,
    /// The previous session ended without a clean shutdown.
    UnexpectedShutdown,
    Sleep,
    Wake,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PowerEvent {
    /// Unix time, seconds.
    pub ts: i64,
    pub kind: PowerEventKind,
    /// e.g. the wake source.
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Hardware identification
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HardwareId {
    pub cpu: CpuId,
    pub memory: MemoryId,
    pub board: BoardId,
    pub gpus: Vec<GpuId>,
    /// e.g. "3.5".
    pub smbios_version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CpuId {
    /// CPUID vendor string, e.g. "GenuineIntel".
    pub vendor: Option<String>,
    pub brand: Option<String>,
    pub architecture: Option<String>,
    pub family: Option<u32>,
    pub model: Option<u32>,
    pub stepping: Option<u32>,
    pub socket: Option<String>,
    pub packages: Option<u32>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    /// Set on hybrid CPUs only.
    pub performance_cores: Option<u32>,
    pub efficiency_cores: Option<u32>,
    pub hybrid: bool,
    /// One entry per physical core, in OS order.
    pub core_map: Vec<CoreInfo>,
    pub caches: Vec<CacheInfo>,
    /// Notable instruction-set extensions, e.g. "AVX2", "AES".
    pub isa: Vec<String>,
    pub base_mhz: Option<f64>,
    pub max_mhz: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CoreKind {
    Performance,
    Efficiency,
    /// Non-hybrid CPU: all cores alike.
    Uniform,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CoreInfo {
    pub index: u32,
    pub kind: CoreKind,
    /// Logical processor numbers (group × 64 + bit) served by this core.
    pub logical: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum CacheKind {
    Data,
    Instruction,
    Unified,
    Trace,
}

/// Identical cache instances are aggregated: `count` × `size_kb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CacheInfo {
    pub level: u8,
    pub kind: CacheKind,
    pub size_kb: u32,
    pub ways: Option<u32>,
    pub line_bytes: u32,
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryId {
    /// Installed memory as reported by firmware.
    pub total_mb: Option<f64>,
    pub max_capacity_mb: Option<f64>,
    pub slots_total: Option<u32>,
    pub slots_used: u32,
    /// Populated channels, decoded from slot names; approximate.
    pub channels: Option<u32>,
    pub dimms: Vec<DimmInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DimmInfo {
    pub slot: String,
    pub bank: Option<String>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    #[serde(rename = "sizeMB")]
    pub size_mb: f64,
    /// e.g. "DDR5".
    pub memory_type: Option<String>,
    /// e.g. "DIMM", "SODIMM".
    pub form_factor: Option<String>,
    /// Rated transfer rate, MT/s.
    pub speed_mts: Option<u32>,
    /// Transfer rate the firmware configured, MT/s.
    pub configured_speed_mts: Option<u32>,
    pub configured_voltage_mv: Option<u32>,
    pub rank: Option<u32>,
}

/// What kind of computer this is (SMBIOS type 3 chassis type, 1.1), so My PC can draw a laptop
/// on a laptop. `Unknown` when the firmware does not say (or says "Other").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FormFactor {
    #[default]
    Unknown,
    Desktop,
    /// Laptops, notebooks, convertibles, tablets and handhelds: anything portable.
    Laptop,
    AllInOne,
    /// Mini PCs and stick PCs.
    Mini,
    Server,
}

impl FormFactor {
    /// From the chassis type byte (bit 7 is the lock flag).
    pub fn from_chassis(code: u8) -> Self {
        match code & 0x7F {
            0x03..=0x07 | 0x0F | 0x10 | 0x18 => Self::Desktop,
            0x08..=0x0B | 0x0E | 0x1E..=0x20 => Self::Laptop,
            0x0D => Self::AllInOne,
            0x22..=0x24 => Self::Mini,
            0x11 | 0x17 | 0x19 | 0x1C | 0x1D => Self::Server,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BoardId {
    pub system_manufacturer: Option<String>,
    pub system_product: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    /// Displayed locally only; never logged, persisted or transmitted (spec §2.10).
    pub serial: Option<String>,
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
    pub chipset: Option<String>,
    /// Desktop or laptop (1.1; files and services from before it read as `Unknown`).
    #[serde(default)]
    pub form_factor: FormFactor,
}

impl std::fmt::Debug for BoardId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoardId")
            .field("system_manufacturer", &self.system_manufacturer)
            .field("system_product", &self.system_product)
            .field("manufacturer", &self.manufacturer)
            .field("product", &self.product)
            .field("version", &self.version)
            .field("serial", &self.serial.as_ref().map(|_| "<redacted>"))
            .field("bios_vendor", &self.bios_vendor)
            .field("bios_version", &self.bios_version)
            .field("bios_date", &self.bios_date)
            .field("chipset", &self.chipset)
            .field("form_factor", &self.form_factor)
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GpuId {
    pub name: String,
    pub vendor: Option<String>,
    pub vendor_id: u32,
    pub device_id: u32,
    pub subsys_id: u32,
    pub revision: u32,
    #[serde(rename = "vramMB")]
    pub vram_mb: Option<f64>,
    #[serde(rename = "sharedMB")]
    pub shared_mb: Option<f64>,
    /// Windows driver version, e.g. "32.0.15.6094".
    pub driver_version: Option<String>,
    /// Vendor-facing version, e.g. NVIDIA "560.94".
    pub vendor_driver_version: Option<String>,
    pub driver_date: Option<String>,
    pub bios_version: Option<String>,
    pub memory_type: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Later phases
// ---------------------------------------------------------------------------------------------

/// A string shown locally but never logged: `Debug` prints `<redacted>` (spec §2.10).
/// Serializes as the bare string (serde newtype).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, type = "string")]
pub struct LocalOnly(pub String);

impl std::fmt::Debug for LocalOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

// ---------------------------------------------------------------------------------------------
// Displays (Phase 3)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DisplayInfo {
    /// Stable key for this output within a session (adapter LUID + target id).
    pub id: String,
    pub name: String,
    pub manufacturer: Option<String>,
    /// PNP id from the EDID, e.g. "GSM".
    pub manufacturer_id: Option<String>,
    /// Monitor name from the EDID, e.g. "LG ULTRAGEAR".
    pub model: Option<String>,
    pub product_code: Option<String>,
    /// Local display only; never logged, persisted or transmitted.
    pub serial: Option<LocalOnly>,
    pub manufactured: Option<String>,
    pub primary: bool,
    /// GDI source name, e.g. `\\.\DISPLAY1`.
    pub gdi_name: Option<String>,
    /// GPU driving this output.
    pub adapter: Option<String>,
    /// "HDMI", "DisplayPort", "Internal (eDP)", ...
    pub connection: Option<String>,
    pub internal: bool,
    /// Desktop resolution.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub native_width: Option<u32>,
    pub native_height: Option<u32>,
    pub refresh_hz: Option<f64>,
    /// Highest refresh the EDID offers at native resolution.
    pub max_refresh_hz: Option<f64>,
    pub bits_per_color: Option<u32>,
    /// "RGB", "YCbCr 4:4:4", ...
    pub color_encoding: Option<String>,
    /// "sRGB", "BT.2100 PQ (HDR10)", ...
    pub color_space: Option<String>,
    pub hdr: HdrInfo,
    pub vrr: VrrInfo,
    pub dpi: Option<u32>,
    pub scale_pct: Option<u32>,
    /// Degrees clockwise: 0, 90, 180, 270.
    pub orientation: Option<u32>,
    #[serde(rename = "widthMM")]
    #[ts(rename = "widthMM")]
    pub width_mm: Option<u32>,
    #[serde(rename = "heightMM")]
    #[ts(rename = "heightMM")]
    pub height_mm: Option<u32>,
    pub diagonal_in: Option<f64>,
    /// Backlight / DDC/CI brightness, percent.
    pub brightness_pct: Option<u32>,
    /// Desktop-space rectangle for the arrangement view: `[x, y, width, height]`.
    pub rect: Option<[i32; 4]>,
    pub edid: Option<EdidInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HdrInfo {
    pub supported: Option<bool>,
    pub enabled: Option<bool>,
    /// Brightness Windows maps SDR white to while HDR is on.
    pub sdr_white_nits: Option<f64>,
    pub max_nits: Option<f64>,
    pub max_full_frame_nits: Option<f64>,
    pub min_nits: Option<f64>,
    /// Transfer functions the EDID declares: "SDR", "PQ (ST 2084)", "HLG", ...
    pub eotfs: Vec<String>,
    /// Wide gamuts the EDID declares: "BT.2020", "DCI-P3".
    pub gamuts: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VrrInfo {
    pub supported: Option<bool>,
    /// Switched on for this display, from the GPU vendor's driver API.
    pub enabled: Option<bool>,
    /// Refresh rate is varying right now (a game or video drives it); NVIDIA only.
    pub active: Option<bool>,
    pub min_hz: Option<f64>,
    pub max_hz: Option<f64>,
    /// "G-SYNC", "G-SYNC Compatible", "FreeSync", "HDMI VRR", "Adaptive-Sync".
    pub technology: Option<String>,
    /// Where the answer came from: "NVAPI", "ADL", "EDID".
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EdidInfo {
    pub version: String,
    pub checksum_ok: bool,
    /// Declared input (EDID 1.4): "DisplayPort", "HDMI", "DVI".
    pub interface: Option<String>,
    pub bits_per_color: Option<u32>,
    pub gamma: Option<f64>,
    pub extensions: Vec<String>,
    /// Vertical range from the range-limits descriptor, Hz.
    pub range_hz: Option<[u32; 2]>,
    /// Raw EDID as hex with serial fields zeroed.
    pub hex: String,
}

// ---------------------------------------------------------------------------------------------
// Storage (Phase 4)
// ---------------------------------------------------------------------------------------------

/// One physical drive with its health data, served by `get_drives`. Live rates and the
/// current temperature ride in the `sensors` event instead ([`crate::snapshot::StorageSnapshot`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DriveInfo {
    /// `disk<N>`, matching `\\.\PhysicalDrive<N>` and the snapshot's `id`.
    pub id: String,
    pub index: u32,
    pub model: String,
    pub vendor: Option<String>,
    /// Local display only; never logged, persisted or transmitted.
    pub serial: Option<LocalOnly>,
    pub firmware: Option<String>,
    #[serde(rename = "type")]
    pub kind: StorageKind,
    /// Windows bus type: "NVMe", "SATA", "USB", "RAID", "SAS", "SD", ...
    pub bus: String,
    /// "PCIe NVMe 1.4", "SATA 6.0 Gb/s (at 6.0 Gb/s)".
    pub interface: Option<String>,
    pub ssd: Option<bool>,
    pub rpm: Option<u32>,
    pub removable: bool,
    /// Holds the Windows volume.
    pub system: bool,
    pub trim: Option<bool>,
    #[serde(rename = "capacityMB")]
    pub capacity_mb: Option<f64>,
    /// Sum over this drive's mounted volumes.
    #[serde(rename = "usedMB")]
    pub used_mb: Option<f64>,
    #[serde(rename = "freeMB")]
    pub free_mb: Option<f64>,
    pub volumes: Vec<VolumeInfo>,
    /// Good / caution / bad in CrystalDiskInfo's sense; `unknown` without SMART data.
    pub health: Status,
    /// Remaining life estimate (NVMe `100 - percentage used`, or the SSD wear attribute).
    pub health_pct: Option<f64>,
    /// Why the health is what it is, e.g. "3 reallocated sectors".
    pub health_notes: Vec<String>,
    pub temp_c: Option<f64>,
    pub temp_warn_c: Option<f64>,
    pub temp_crit_c: Option<f64>,
    pub power_on_hours: Option<f64>,
    pub power_cycles: Option<f64>,
    #[serde(rename = "totalReadGB")]
    pub total_read_gb: Option<f64>,
    #[serde(rename = "totalWriteGB")]
    pub total_write_gb: Option<f64>,
    pub nvme: Option<NvmeHealth>,
    /// ATA SMART attribute table (SATA drives).
    pub smart: Vec<SmartAttribute>,
    /// Why SMART / health data is missing, e.g. "… needs the Vyro sensor service".
    pub smart_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VolumeInfo {
    /// "C:"; empty for volumes without a letter.
    pub letter: String,
    pub label: Option<String>,
    pub file_system: Option<String>,
    #[serde(rename = "totalMB")]
    pub total_mb: Option<f64>,
    #[serde(rename = "freeMB")]
    pub free_mb: Option<f64>,
}

/// NVMe SMART / Health Information log (page 0x02).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NvmeHealth {
    pub critical_warning: u32,
    pub critical_warnings: Vec<String>,
    pub available_spare_pct: u32,
    pub spare_threshold_pct: u32,
    pub percentage_used: u32,
    pub data_units_read: f64,
    pub data_units_written: f64,
    pub host_reads: f64,
    pub host_writes: f64,
    pub busy_minutes: f64,
    pub unsafe_shutdowns: f64,
    pub media_errors: f64,
    pub error_log_entries: f64,
    pub warning_temp_minutes: u32,
    pub critical_temp_minutes: u32,
    /// Temperature Sensor 1–8 as reported (`null` = not implemented). Their placement
    /// (controller, NAND) is vendor-specific.
    pub sensors_c: Vec<Option<f64>>,
    pub throttle_events: Vec<u32>,
    pub throttle_secs: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SmartAttribute {
    pub id: u32,
    pub name: String,
    pub current: u32,
    pub worst: u32,
    pub threshold: Option<u32>,
    pub raw: f64,
    /// The 48-bit raw field as 12 hex digits.
    pub raw_hex: String,
    pub prefail: bool,
    pub failing: bool,
}

// ---------------------------------------------------------------------------------------------
// Network (Phase 5)
// ---------------------------------------------------------------------------------------------

/// One network adapter's configuration, served by `get_network`. Rates, signal and ping ride
/// in the `sensors` event ([`crate::snapshot::NetworkSnapshot`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetAdapterInfo {
    /// `if<N>` (interface index).
    pub id: String,
    /// Windows connection name, e.g. "Ethernet", "Wi-Fi".
    pub name: String,
    /// Driver description, e.g. "Realtek Gaming 2.5GbE Family Controller".
    pub description: String,
    #[serde(rename = "type")]
    pub kind: NetKind,
    pub connected: bool,
    pub primary: bool,
    /// Not a physical NIC (VPN, Hyper-V switch, ...).
    pub virtual_adapter: bool,
    /// Local display only.
    pub mac: Option<LocalOnly>,
    pub link_mbps: Option<f64>,
    pub mtu: Option<u32>,
    pub dhcp: Option<bool>,
    pub ipv4: Vec<String>,
    /// Local display only (global addresses identify the connection).
    pub ipv6: Vec<LocalOnly>,
    pub gateways: Vec<String>,
    pub dns: Vec<String>,
    pub wifi: Option<WifiInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WifiInfo {
    /// Local display only.
    pub ssid: Option<LocalOnly>,
    /// Local display only.
    pub bssid: Option<LocalOnly>,
    /// "Wi-Fi 6 (802.11ax)".
    pub standard: Option<String>,
    pub band: Option<String>,
    pub channel: Option<u32>,
    pub frequency_mhz: Option<u32>,
    pub signal_pct: Option<f64>,
    pub rssi_dbm: Option<f64>,
    pub rx_mbps: Option<f64>,
    pub tx_mbps: Option<f64>,
    /// "WPA3-Personal", "WPA2-Personal", "Open", ...
    pub security: Option<String>,
    /// Why some details are missing, e.g. Windows location access being off.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HealthReport {
    pub overall: Status,
    pub subsystems: Vec<SubsystemHealth>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SubsystemHealth {
    pub name: String,
    pub status: Status,
    /// What is wrong (warnings and problems), worst first.
    pub messages: Vec<String>,
    /// Coverage gaps worth stating, e.g. a sensor that needs PawnIO.
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_debug_never_prints_the_serial() {
        let board = BoardId {
            serial: Some("SN-SECRET-123".into()),
            ..Default::default()
        };
        let text = format!("{board:?}");
        assert!(!text.contains("SN-SECRET-123"));
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn chassis_types_map_to_form_factors() {
        use FormFactor::*;
        for (code, want) in [
            (0x03, Desktop),
            (0x07, Desktop),
            (0x09, Laptop),
            (0x0A, Laptop),
            (0x1F, Laptop),
            (0x89, Laptop), // with the lock bit
            (0x0D, AllInOne),
            (0x23, Mini),
            (0x17, Server),
            (0x01, Unknown),
            (0x02, Unknown),
            (0x7F, Unknown),
        ] {
            assert_eq!(FormFactor::from_chassis(code), want, "{code:#x}");
        }
        // Saved before 1.1: no form factor yet.
        let b: BoardId = serde_json::from_str(r#"{"systemManufacturer":null,"systemProduct":null,"manufacturer":"MSI","product":null,"version":null,"serial":null,"biosVendor":null,"biosVersion":null,"biosDate":null,"chipset":null}"#).unwrap();
        assert_eq!(b.form_factor, Unknown);
    }
}

// ---------------------------------------------------------------------------------------------
// Low-level access (Phase 6, PawnIO)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PawnIoState {
    /// Driver reachable and the modules Vyro needs load.
    Ready,
    /// Not installed (no uninstall entry and no device).
    #[default]
    NotInstalled,
    /// Installed but older than Vyro's minimum.
    Outdated,
    /// Installed but its device is not running.
    NotRunning,
    /// The device refuses this process (Vyro is not elevated).
    NoAccess,
    /// Anything else; see `detail`.
    Error,
    /// Not applicable on this platform.
    Unsupported,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PawnIoStatus {
    pub state: PawnIoState,
    /// Installed version from the PawnIO uninstall entry.
    pub version: Option<String>,
    pub detail: Option<String>,
}

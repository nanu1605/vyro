//! Payloads served on demand by commands rather than in the 1 Hz `sensors` event (spec §4.4).
//!
//! `SystemInfo` and `HardwareId` are collected once at startup (Phase 1). Every field is
//! optional: `null` means "could not be determined on this machine", and admin-only fields are
//! `null` when the app is not elevated.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::Status;

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
    /// Currently active, from the GPU vendor's driver API.
    pub enabled: Option<bool>,
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
    pub messages: Vec<String>,
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
}

//! Fast telemetry payload (`sensors` event, ≈1 Hz) — spec §4.4.
//!
//! Every reading is optional: `None` serializes to `null` and means "unavailable on this
//! machine / not implemented yet". The frontend renders around gaps.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SensorSnapshot {
    /// Unix time, seconds.
    pub ts: i64,
    pub cpu: CpuSnapshot,
    pub gpu: Vec<GpuSnapshot>,
    pub memory: MemorySnapshot,
    pub storage: Vec<StorageSnapshot>,
    pub board: BoardSnapshot,
    pub fans: Vec<FanSnapshot>,
    pub voltages: Vec<VoltageReading>,
    pub network: Vec<NetworkSnapshot>,
    pub ping: PingSnapshot,
    pub power: PowerSnapshot,
    pub processes: Vec<ProcessSnapshot>,
    pub app: AppSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CpuSnapshot {
    pub usage_total: Option<f64>,
    pub usage_per_core: Vec<f64>,
    pub mhz_current: Option<f64>,
    pub mhz_per_core: Vec<f64>,
    pub mhz_effective: Option<f64>,
    pub multiplier: Option<f64>,
    /// Derived; approximate.
    pub bclk_mhz: Option<f64>,
    pub mhz_base: Option<f64>,
    pub mhz_boost: Option<f64>,
    /// Tdie (AMD) / package (Intel), °C.
    pub temp_package: Option<f64>,
    pub temp_per_core: Vec<f64>,
    /// AMD per-CCD (chiplet) temperatures, °C.
    pub temp_ccd: Vec<f64>,
    pub power_pkg_w: Option<f64>,
    /// Sum of the cores' power.
    pub power_core_w: Option<f64>,
    /// Per physical core, W.
    pub power_per_core_w: Vec<f64>,
    pub power_soc_w: Option<f64>,
    pub voltage_pkg: Option<f64>,
    pub voltage_core: Option<f64>,
    /// Highest core voltage the cores request (VID), V. Not a measurement of the rail.
    pub vid_core: Option<f64>,
    pub throttle_thermal: Option<bool>,
    pub throttle_power_limit: Option<bool>,
    pub throttle_current_limit: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GpuSnapshot {
    pub name: String,
    pub vendor: Option<String>,
    /// Integrated (shares system memory) vs discrete; `None` when unknown.
    pub integrated: Option<bool>,
    pub usage: Option<f64>,
    pub engine: GpuEngines,
    pub mhz_core: Option<f64>,
    pub mhz_mem: Option<f64>,
    pub temp_edge: Option<f64>,
    pub temp_hotspot: Option<f64>,
    pub temp_mem: Option<f64>,
    #[serde(rename = "vramUsedMB")]
    pub vram_used_mb: Option<f64>,
    #[serde(rename = "vramTotalMB")]
    pub vram_total_mb: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub fan_pct: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub pcie_gen: Option<u32>,
    pub pcie_width: Option<u32>,
    pub throttle_thermal: Option<bool>,
    pub throttle_power_limit: Option<bool>,
    /// Active clock limiters as reported by the vendor library, e.g. "power cap", "thermal".
    pub throttle_reasons: Vec<String>,
}

/// Per-engine utilization, %. Tensor / ray-trace are usually unavailable (spec §2.10).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GpuEngines {
    pub graphics3d: Option<f64>,
    pub decode: Option<f64>,
    pub encode: Option<f64>,
    pub compute: Option<f64>,
    pub copy: Option<f64>,
    pub tensor: Option<f64>,
    pub raytrace: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemorySnapshot {
    #[serde(rename = "totalMB")]
    pub total_mb: Option<f64>,
    #[serde(rename = "usedMB")]
    pub used_mb: Option<f64>,
    #[serde(rename = "availableMB")]
    pub available_mb: Option<f64>,
    #[serde(rename = "cachedMB")]
    pub cached_mb: Option<f64>,
    #[serde(rename = "committedMB")]
    pub committed_mb: Option<f64>,
    #[serde(rename = "commitLimitMB")]
    pub commit_limit_mb: Option<f64>,
    #[serde(rename = "pageFileUsedMB")]
    pub page_file_used_mb: Option<f64>,
    #[serde(rename = "pageFileTotalMB")]
    pub page_file_total_mb: Option<f64>,
    /// Standby list (Task Manager's "Cached" = standby + modified).
    #[serde(rename = "standbyMB")]
    pub standby_mb: Option<f64>,
    #[serde(rename = "modifiedMB")]
    pub modified_mb: Option<f64>,
    /// Free + zeroed pages.
    #[serde(rename = "freeMB")]
    pub free_mb: Option<f64>,
    #[serde(rename = "pagedPoolMB")]
    pub paged_pool_mb: Option<f64>,
    #[serde(rename = "nonPagedPoolMB")]
    pub non_paged_pool_mb: Option<f64>,
    #[serde(rename = "compressedMB")]
    pub compressed_mb: Option<f64>,
    pub page_faults_per_sec: Option<f64>,
    /// Disk reads to resolve hard page faults (`\Memory\Page Reads/sec`).
    pub hard_faults_per_sec: Option<f64>,
    /// Decoded / heuristic; approximate.
    pub channels: Option<u32>,
    pub dimms: Vec<DimmSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DimmSnapshot {
    pub slot: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    #[serde(rename = "sizeMB")]
    pub size_mb: Option<f64>,
    #[serde(rename = "speedMHz")]
    pub speed_mhz: Option<f64>,
    pub temp_c: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum StorageKind {
    #[serde(rename = "NVMe")]
    Nvme,
    #[serde(rename = "SATA")]
    Sata,
    #[serde(rename = "USB")]
    Usb,
    #[default]
    #[serde(rename = "other")]
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StorageSnapshot {
    /// `disk<N>`, matching [`crate::info::DriveInfo::id`].
    pub id: String,
    pub model: String,
    /// Drive letters on this disk, e.g. "C: D:".
    pub volumes: String,
    /// From SMART / NVMe health; `unknown` when unavailable (e.g. not elevated).
    pub health: Status,
    #[serde(rename = "type")]
    pub kind: StorageKind,
    pub temp_c: Option<f64>,
    pub temp_controller_c: Option<f64>,
    pub temp_nand_c: Option<f64>,
    pub health_pct: Option<f64>,
    #[serde(rename = "capacityMB")]
    pub capacity_mb: Option<f64>,
    #[serde(rename = "usedMB")]
    pub used_mb: Option<f64>,
    #[serde(rename = "freeMB")]
    pub free_mb: Option<f64>,
    #[serde(rename = "readMBs")]
    pub read_mbs: Option<f64>,
    #[serde(rename = "writeMBs")]
    pub write_mbs: Option<f64>,
    pub read_iops: Option<f64>,
    pub write_iops: Option<f64>,
    pub activity_pct: Option<f64>,
    #[serde(rename = "totalReadGB")]
    pub total_read_gb: Option<f64>,
    #[serde(rename = "totalWriteGB")]
    pub total_write_gb: Option<f64>,
    pub power_on_hours: Option<f64>,
    pub power_cycles: Option<f64>,
    pub nvme_pct_used: Option<f64>,
    pub nvme_available_spare_pct: Option<f64>,
    pub nvme_critical_warning: Option<u32>,
}

/// Motherboard sensors; most are `null` unless the board wires them to the Super-I/O.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BoardSnapshot {
    /// Motherboard ("System") sensor.
    pub temp_system: Option<f64>,
    pub temp_chipset: Option<f64>,
    pub temp_vrm: Option<f64>,
    pub temp_vrm_mos: Option<f64>,
    pub temp_pch: Option<f64>,
    pub temp_socket: Option<f64>,
    pub temp_pcie: Option<f64>,
    pub temp_m2: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum FanKind {
    #[default]
    Fan,
    Pump,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanSnapshot {
    pub name: String,
    pub rpm: Option<f64>,
    pub pct: Option<f64>,
    pub pwm: Option<f64>,
    pub min_rpm: Option<f64>,
    pub max_rpm: Option<f64>,
    pub kind: FanKind,
    pub status: Status,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VoltageReading {
    pub name: String,
    pub value: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum NetKind {
    Ethernet,
    Wifi,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetworkSnapshot {
    /// `if<N>` (interface index), matching [`crate::info::NetAdapterInfo::id`].
    pub id: String,
    pub adapter: String,
    #[serde(rename = "type")]
    pub kind: NetKind,
    pub connected: bool,
    /// Carries the default route (the ping is attributed to it).
    pub primary: bool,
    pub down_mbps: Option<f64>,
    pub up_mbps: Option<f64>,
    /// Busier direction against the link speed, as Task Manager does.
    pub utilization_pct: Option<f64>,
    pub link_mbps: Option<f64>,
    /// Since the adapter came up, decimal MB.
    #[serde(rename = "receivedMB")]
    pub received_mb: Option<f64>,
    #[serde(rename = "sentMB")]
    pub sent_mb: Option<f64>,
    pub ping_ms: Option<f64>,
    /// Sampled over a rolling window of ICMP probes, not exact.
    pub packet_loss_pct: Option<f64>,
    pub wifi_signal_pct: Option<f64>,
    pub wifi_rssi_dbm: Option<f64>,
    pub wifi_channel: Option<u32>,
    /// "2.4 GHz", "5 GHz", "6 GHz".
    pub wifi_band: Option<String>,
}

/// ICMP echo against the configured host, over a rolling window of probes (1 per second).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingSnapshot {
    pub host: String,
    /// Latest reply; `null` when the last probe was lost.
    pub rtt_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Mean difference between consecutive replies.
    pub jitter_ms: Option<f64>,
    pub loss_pct: Option<f64>,
    /// Probes in the window.
    pub samples: u32,
    /// Why probes fail, e.g. "host not found".
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PowerSnapshot {
    pub est_total_w: Option<f64>,
    pub cpu_w: Option<f64>,
    pub gpu_w: Option<f64>,
    pub ram_w: Option<f64>,
    pub storage_w: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: Option<f64>,
    #[serde(rename = "ramMB")]
    pub ram_mb: Option<f64>,
    pub gpu_pct: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum EngineStatus {
    #[default]
    Starting,
    Running,
    /// Polling at the slow (hidden / tray) interval.
    Throttled,
    Stopped,
}

/// The app's own footprint (spec §1 "App self-monitoring").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppSnapshot {
    pub cpu_pct: Option<f64>,
    #[serde(rename = "ramMB")]
    pub ram_mb: Option<f64>,
    pub gpu_pct: Option<f64>,
    pub poll_hz: Option<f64>,
    /// Reported by the frontend; the backend leaves it `null`.
    pub ui_fps: Option<f64>,
    pub active_sensors: u32,
    pub engine: EngineStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn serializes_spec_field_names_and_nulls() {
        let snap = SensorSnapshot {
            ts: 1_758_800_000,
            gpu: vec![GpuSnapshot {
                name: "GPU".into(),
                vram_used_mb: Some(4820.0),
                ..Default::default()
            }],
            storage: vec![StorageSnapshot {
                kind: StorageKind::Nvme,
                read_mbs: Some(12.5),
                total_read_gb: Some(18200.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        let v: Value = serde_json::to_value(&snap).unwrap();

        assert_eq!(v["ts"], json!(1_758_800_000));
        assert_eq!(v["cpu"]["usageTotal"], Value::Null);
        assert_eq!(v["cpu"]["powerPkgW"], Value::Null);
        assert!(v["cpu"]["bclkMhz"].is_null());
        assert_eq!(v["gpu"][0]["vramUsedMB"], json!(4820.0));
        assert!(v["gpu"][0]["engine"]["graphics3d"].is_null());
        assert_eq!(v["storage"][0]["type"], json!("NVMe"));
        assert_eq!(v["storage"][0]["readMBs"], json!(12.5));
        assert_eq!(v["storage"][0]["totalReadGB"], json!(18200.0));
        assert!(v["memory"]["commitLimitMB"].is_null());
        assert!(v["board"]["tempM2"].is_null());
        assert_eq!(v["app"]["engine"], json!("starting"));
    }

    #[test]
    fn round_trips() {
        let snap = SensorSnapshot {
            ts: 1,
            fans: vec![FanSnapshot {
                name: "PUMP".into(),
                kind: FanKind::Pump,
                status: Status::Ok,
                rpm: Some(2400.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        let text = serde_json::to_string(&snap).unwrap();
        let back: SensorSnapshot = serde_json::from_str(&text).unwrap();
        assert_eq!(back, snap);
    }
}

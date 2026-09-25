//! Payloads served on demand by commands rather than in the 1 Hz `sensors` event (spec §4.4).
//!
//! Phase 0 defines the shapes the command surface returns; the fields are filled in by the
//! phases that implement each area (P1 system / hardware, P3 displays, P7 health).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::Status;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemInfo {
    pub os_edition: Option<String>,
    pub os_version: Option<String>,
    pub os_build: Option<String>,
    /// Unix time, seconds.
    pub install_date: Option<i64>,
    /// Unix time, seconds.
    pub boot_time: Option<i64>,
    pub uptime_secs: Option<i64>,
    pub secure_boot: Option<bool>,
    pub tpm_version: Option<String>,
    pub virtualization: Option<bool>,
    pub bitlocker: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HardwareId {
    pub cpu_model: Option<String>,
    pub cpu_vendor: Option<String>,
    pub cpu_cores: Option<u32>,
    pub cpu_threads: Option<u32>,
    pub gpus: Vec<String>,
    pub board_manufacturer: Option<String>,
    pub board_model: Option<String>,
    pub bios_version: Option<String>,
    /// Displayed locally only; never logged, persisted or transmitted.
    pub board_serial: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DisplayInfo {
    pub name: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub primary: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub refresh_hz: Option<f64>,
    pub scale_pct: Option<u32>,
    pub hdr: Option<bool>,
    pub vrr: Option<bool>,
    /// Desktop-space rectangle for the arrangement view: `[x, y, width, height]`.
    pub rect: Option<[i32; 4]>,
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

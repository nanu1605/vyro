//! What the companion page sees: the readings a phone needs at a glance, from each snapshot.
//! Built here so nothing else crosses the network: no processes, serial numbers, addresses,
//! adapter or user names, Health notes or alert diagnostics.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::snapshot::{FanKind, Status, StorageKind};
use crate::units::{temps_in, TempUnit};
use crate::SensorSnapshot;

/// Alerts kept for the page's "Recent" list.
pub const RECENT: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteView {
    /// Unix seconds of the reading.
    pub ts: i64,
    pub cpu: CpuView,
    pub gpus: Vec<GpuView>,
    pub memory: MemoryView,
    pub drives: Vec<DriveView>,
    pub fans: Vec<FanView>,
    /// Board and memory temperatures (°C).
    pub temps: Vec<TempView>,
    pub power: PowerView,
    /// While a game's frames are measured.
    pub game: Option<GameView>,
    pub network: NetView,
    pub health: HealthView,
    /// Alerts whose conditions still hold, oldest first.
    pub alerts: Vec<AlertView>,
    /// Alerts fired lately, newest first.
    pub recent: Vec<FiredView>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CpuView {
    pub usage: Option<f64>,
    pub temp: Option<f64>,
    pub mhz: Option<f64>,
    pub power_w: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GpuView {
    /// Model, e.g. "NVIDIA GeForce RTX 4070".
    pub name: String,
    pub integrated: bool,
    pub usage: Option<f64>,
    pub temp: Option<f64>,
    pub hotspot: Option<f64>,
    pub mhz: Option<f64>,
    pub power_w: Option<f64>,
    pub vram_used_mb: Option<f64>,
    pub vram_total_mb: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryView {
    pub used_mb: Option<f64>,
    pub total_mb: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DriveView {
    /// Model, e.g. "Samsung SSD 990 PRO 2TB".
    pub model: String,
    pub kind: StorageKind,
    pub temp: Option<f64>,
    pub used_pct: Option<f64>,
    pub health: Status,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FanView {
    pub name: String,
    pub rpm: Option<f64>,
    pub pct: Option<f64>,
    pub pump: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TempView {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PowerView {
    pub total_w: Option<f64>,
    pub cpu_w: Option<f64>,
    pub gpu_w: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameView {
    /// The game's program name without ".exe".
    pub app: String,
    pub fps: Option<f64>,
    pub low1_fps: Option<f64>,
    pub frame_ms: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetView {
    /// All connected adapters together, Mbit/s.
    pub down_mbps: Option<f64>,
    pub up_mbps: Option<f64>,
    pub ping_ms: Option<f64>,
    pub loss_pct: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HealthView {
    pub overall: Status,
    /// Parts with a warning or a problem, worst first, with their first finding.
    pub issues: Vec<HealthIssue>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HealthIssue {
    pub part: String,
    pub status: Status,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlertView {
    pub name: String,
    pub since: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FiredView {
    pub ts: i64,
    pub title: String,
    pub message: String,
}

/// Adds the alerts a snapshot fired to `recent` (newest first, at most [`RECENT`]): only the
/// ones Windows would show, not repeats or log-only rules.
pub fn note_fired(recent: &mut VecDeque<FiredView>, snap: &SensorSnapshot, unit: TempUnit) {
    for a in snap.alerts.iter().filter(|a| a.notify && a.also.is_none()) {
        recent.push_front(FiredView {
            ts: a.ts,
            title: temps_in(&a.title, unit),
            message: temps_in(&a.message, unit),
        });
    }
    recent.truncate(RECENT);
}

fn sum(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values
        .flatten()
        .fold(None, |acc, v| Some(acc.unwrap_or(0.0) + v))
}

fn rank(s: Status) -> u8 {
    match s {
        Status::Fail => 0,
        Status::Warn => 1,
        Status::Unknown => 2,
        Status::Ok => 3,
    }
}

/// The page's view of a snapshot. `unit` rewrites temperatures inside texts (numbers stay
/// °C; the page converts them).
pub fn build(snap: &SensorSnapshot, recent: &VecDeque<FiredView>, unit: TempUnit) -> RemoteView {
    let cpu = &snap.cpu;
    let b = &snap.board;
    let mut temps: Vec<TempView> = [
        ("System", b.temp_system),
        ("Chipset", b.temp_chipset.or(b.temp_pch)),
        ("VRM", b.temp_vrm.or(b.temp_vrm_mos)),
        ("Socket", b.temp_socket),
        ("PCIe", b.temp_pcie),
        ("M.2", b.temp_m2),
    ]
    .into_iter()
    .filter_map(|(name, v)| {
        v.map(|value| TempView {
            name: name.into(),
            value,
        })
    })
    .collect();
    temps.extend(snap.memory.dimms.iter().filter_map(|d| {
        d.temp_c.map(|value| TempView {
            name: format!("RAM {}", d.slot),
            value,
        })
    }));

    let mut issues: Vec<HealthIssue> = snap
        .health
        .subsystems
        .iter()
        .filter(|s| matches!(s.status, Status::Warn | Status::Fail))
        .map(|s| HealthIssue {
            part: s.name.clone(),
            status: s.status,
            message: s.messages.first().map(|m| temps_in(m, unit)),
        })
        .collect();
    issues.sort_by_key(|i| rank(i.status));

    let connected = || snap.network.iter().filter(|n| n.connected);
    RemoteView {
        ts: snap.ts,
        cpu: CpuView {
            usage: cpu.usage_total,
            temp: cpu.temp_package,
            mhz: cpu.mhz_effective.or(cpu.mhz_current),
            power_w: cpu.power_pkg_w,
        },
        gpus: snap
            .gpu
            .iter()
            .map(|g| GpuView {
                name: g.name.clone(),
                integrated: g.integrated == Some(true),
                usage: g.usage,
                temp: g.temp_edge,
                hotspot: g.temp_hotspot,
                mhz: g.mhz_core,
                power_w: g.power_w,
                vram_used_mb: g.vram_used_mb,
                vram_total_mb: g.vram_total_mb,
            })
            .collect(),
        memory: MemoryView {
            used_mb: snap.memory.used_mb,
            total_mb: snap.memory.total_mb,
        },
        drives: snap
            .storage
            .iter()
            .map(|d| DriveView {
                model: d.model.clone(),
                kind: d.kind,
                temp: d.temp_c,
                used_pct: match (d.used_mb, d.capacity_mb) {
                    (Some(u), Some(c)) if c > 0.0 => Some(u / c * 100.0),
                    _ => None,
                },
                health: d.health,
            })
            .collect(),
        fans: snap
            .fans
            .iter()
            .map(|f| FanView {
                name: f.name.clone(),
                rpm: f.rpm,
                pct: f.pct,
                pump: f.kind == FanKind::Pump,
            })
            .collect(),
        temps,
        power: PowerView {
            total_w: snap.power.est_total_w,
            cpu_w: snap.power.cpu_w,
            gpu_w: snap.power.gpu_w,
        },
        game: snap.game.as_ref().filter(|g| g.fps.is_some()).map(|g| {
            let app = g.app.strip_suffix(".exe").unwrap_or(&g.app);
            GameView {
                app: app.into(),
                fps: g.fps,
                low1_fps: g.low1_fps,
                frame_ms: g.frame_ms,
            }
        }),
        network: NetView {
            down_mbps: sum(connected().map(|n| n.down_mbps)),
            up_mbps: sum(connected().map(|n| n.up_mbps)),
            ping_ms: snap.ping.rtt_ms.or(snap.ping.avg_ms),
            loss_pct: snap.ping.loss_pct,
        },
        health: HealthView {
            overall: snap.health.overall,
            issues,
        },
        alerts: snap
            .alert_state
            .active
            .iter()
            .map(|a| AlertView {
                name: a.name.clone(),
                since: a.since,
            })
            .collect(),
        recent: recent.iter().cloned().collect(),
    }
}

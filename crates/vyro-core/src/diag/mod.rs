//! One-click diagnostics (P21): a passive check-up from what Vyro already records, as a report
//! of 13 checks (checks v1, `docs/diagnostics.md`). Each check is Pass, Attention, Info or
//! Unavailable, and every Attention cites the telemetry behind it: the measured values, where
//! they come from and the sensor to open in History.
//!
//! The engine works the report out ([`checks::evaluate`] over [`checks::DiagInputs`]); the app
//! adds what only it knows ([`DiagAppInfo`]) and writes the exports (`export`, `pdf`) from the
//! same report, so every format shows the same values. Serial numbers are never part of a
//! report; an export adds them only when asked.

pub mod checks;
pub mod export;
pub mod pdf;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::units::TempUnit;

/// The checks these reports follow.
pub const CHECKS_VERSION: u32 = 1;
/// Reports kept in the history store.
pub const KEEP_REPORTS: usize = 50;

/// Limits (checks v1); see `docs/diagnostics.md`.
pub mod limits {
    /// Degrees left below the limit under load, °C.
    pub const HEADROOM_C: f64 = 10.0;
    /// Clock sag over long full load, %.
    pub const SAG_PCT: f64 = 5.0;
    /// Share of loaded time throttled for heat or current, %.
    pub const THROTTLE_PCT: f64 = 2.0;
    /// CPU hotter than usual for the load, °C; fan at or below this share of its usual, %.
    pub const HOTTER_C: f64 = 5.0;
    pub const FAN_PCT: f64 = 70.0;
    /// RAM in use, p95, %.
    pub const MEMORY_PCT: f64 = 90.0;
    /// SSD life left, %; drive temperature, °C; free space on the system drive, %.
    pub const LIFE_PCT: f64 = 10.0;
    pub const DRIVE_C: f64 = 70.0;
    pub const FREE_PCT: f64 = 10.0;
    /// Ping p95, ms; lost replies, %.
    pub const PING_MS: f64 = 150.0;
    pub const LOSS_PCT: f64 = 3.0;
    /// Corrected hardware errors in 30 days.
    pub const CORRECTED: u32 = 10;
    /// The window the history checks look back over, and the event log's.
    pub const WINDOW_SECS: i64 = 7 * 86_400;
    pub const EVENT_LOG_SECS: i64 = 30 * 86_400;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CheckId {
    Thermals,
    ClockStability,
    PowerLimits,
    Throttling,
    Cooling,
    Memory,
    Storage,
    Network,
    HardwareErrors,
    Changes,
    Drivers,
    Sensors,
    Service,
}

impl CheckId {
    pub const ALL: [Self; 13] = [
        Self::Thermals,
        Self::ClockStability,
        Self::PowerLimits,
        Self::Throttling,
        Self::Cooling,
        Self::Memory,
        Self::Storage,
        Self::Network,
        Self::HardwareErrors,
        Self::Changes,
        Self::Drivers,
        Self::Sensors,
        Self::Service,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Thermals => "Thermals",
            Self::ClockStability => "Clock stability",
            Self::PowerLimits => "Power limits",
            Self::Throttling => "Throttling",
            Self::Cooling => "Fans and pump",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Network => "Network",
            Self::HardwareErrors => "Hardware errors",
            Self::Changes => "Changes from usual",
            Self::Drivers => "Drivers",
            Self::Sensors => "Sensors",
            Self::Service => "Service",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CheckStatus {
    Pass,
    Attention,
    Info,
    Unavailable,
}

impl CheckStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pass => "Pass",
            Self::Attention => "Attention",
            Self::Info => "Info",
            Self::Unavailable => "Unavailable",
        }
    }
}

/// Where a cited value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Origin {
    /// The week of 1-minute history.
    History,
    /// The minute digests (limit flags, programs).
    Digests,
    HealthLog,
    Smart,
    EventLog,
    /// The readings now.
    Live,
    /// The intelligence engine's findings (P16).
    Insights,
    Inventory,
    Service,
}

impl Origin {
    pub const fn label(self) -> &'static str {
        match self {
            Self::History => "7-day history",
            Self::Digests => "minute digests",
            Self::HealthLog => "Health log",
            Self::Smart => "drive health (SMART)",
            Self::EventLog => "Windows System log",
            Self::Live => "live reading",
            Self::Insights => "Insights",
            Self::Inventory => "inventory",
            Self::Service => "Vyro",
        }
    }
}

/// A measured value behind a check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Citation {
    pub label: String,
    pub value: Option<f64>,
    /// Display unit; "°C" values are temperatures (shown in °F if chosen).
    pub unit: String,
    /// A temperature difference (converts without the offset).
    #[serde(default)]
    pub difference: bool,
    pub digits: u8,
    /// A value that is not a number ("good", "576.02").
    pub text: Option<String>,
    pub source: Origin,
    /// The sensor to open in History.
    pub metric: Option<String>,
    /// How long before the report it happened, s (events).
    pub ago: Option<i64>,
}

impl Citation {
    pub fn number(
        label: impl Into<String>,
        value: f64,
        unit: &str,
        digits: u8,
        source: Origin,
    ) -> Self {
        Self {
            label: label.into(),
            value: Some(value),
            unit: unit.into(),
            difference: false,
            digits,
            text: None,
            source,
            metric: None,
            ago: None,
        }
    }

    pub fn text(label: impl Into<String>, text: impl Into<String>, source: Origin) -> Self {
        Self {
            label: label.into(),
            value: None,
            unit: String::new(),
            difference: false,
            digits: 0,
            text: Some(text.into()),
            source,
            metric: None,
            ago: None,
        }
    }

    #[must_use]
    pub fn metric(mut self, id: impl Into<String>) -> Self {
        self.metric = Some(id.into());
        self
    }

    #[must_use]
    pub fn diff(mut self) -> Self {
        self.difference = true;
        self
    }

    #[must_use]
    pub fn ago(mut self, secs: i64) -> Self {
        self.ago = Some(secs.max(0));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagCheck {
    pub id: CheckId,
    pub status: CheckStatus,
    pub summary: String,
    /// What to do about it (Attention), or why there is no data (Unavailable).
    pub advice: Option<String>,
    pub citations: Vec<Citation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Field {
    pub label: String,
    pub value: String,
}

/// A GPU's driver, from the app's inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GpuDriver {
    pub name: String,
    pub version: Option<String>,
    /// As Windows gives it, e.g. "2025-09-30".
    pub date: Option<String>,
}

/// What only the app knows, sent with the request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagAppInfo {
    pub app_version: String,
    /// The app reaches the engine through the Vyro service (`false`: its own engine).
    pub service: bool,
    /// The service's version, when connected.
    pub service_version: Option<String>,
    /// Why the service is not used, when it is not.
    pub service_detail: Option<String>,
    /// CPU, GPUs, board, BIOS, RAM, Windows; never serial numbers.
    pub pc: Vec<Field>,
    pub drivers: Vec<GpuDriver>,
    /// e.g. "Windows 11 Pro 24H2 26100.4652".
    pub windows: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagReport {
    /// Store id; 0 before it is kept.
    pub id: i64,
    pub ts: i64,
    pub version: u32,
    pub app_version: String,
    /// "Vyro service 0.4.0" or "the app's own engine".
    pub engine: String,
    pub pc: Vec<Field>,
    pub checks: Vec<DiagCheck>,
}

impl DiagReport {
    pub fn count(&self, s: CheckStatus) -> usize {
        self.checks.iter().filter(|c| c.status == s).count()
    }
}

/// A kept report in the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiagSummary {
    pub id: i64,
    pub ts: i64,
    pub version: u32,
    pub attention: u32,
    pub unavailable: u32,
}

/// "17 °C", "61 %", "0.4 %", "good"; temperatures in `unit` (a difference without the offset).
/// The UI formats the same way (`src/diag.ts`), so exports match the screen.
pub fn fmt_citation(c: &Citation, unit: TempUnit) -> String {
    if let Some(t) = &c.text {
        return crate::units::temps_in(t, unit);
    }
    let Some(mut v) = c.value else {
        return "—".into();
    };
    let mut u = c.unit.as_str();
    if u == "°C" && unit == TempUnit::F {
        v = if c.difference {
            v * 1.8
        } else {
            v * 1.8 + 32.0
        };
        u = "°F";
    }
    // Halves round away from zero, as JavaScript's `toFixed` does (Rust's formatting rounds
    // them to even), so the screen and the exports agree on a median like 78.5.
    let p = 10f64.powi(i32::from(c.digits));
    let v = (v.abs() * p).round() / p * v.signum();
    let n = format!("{v:.*}", usize::from(c.digits));
    match u {
        "" => n,
        "%" => format!("{n}%"),
        _ => format!("{n} {u}"),
    }
}

/// "12 min before", "5 h before", "3 d before".
pub fn fmt_ago(secs: i64) -> String {
    let s = secs.max(0);
    if s < 3600 {
        format!("{} min before", (s / 60).max(1))
    } else if s < 86_400 {
        format!("{} h before", s / 3600)
    } else {
        format!("{} d before", s / 86_400)
    }
}

//! GPU tuning (1.6, decision 37): core and memory clock offsets and the power limit of an NVIDIA
//! card, through the driver's own interface (NVML) and only in the service. Off by default.
//!
//! What it allows is what the driver reports: tuning is offered only on the families in
//! [`Family`] (desktop and laptop), and each setting only where the driver gives a range for it.
//! A laptop often has offsets but no power limit. Every preset is scaled to those ranges.
//!
//! Rules, in order:
//! - **Crash guard:** a marker is kept while the card runs tuned. At start, a marker left with an
//!   unexpected shutdown or a display driver reset since → Stock, "apply at start" off, and a
//!   notice ([`crash_verdict`]).
//! - **Temperature guard:** the card at [`EDGE_LIMIT_C`] or its hotspot at [`HOTSPOT_LIMIT_C`]
//!   for [`HOT_READINGS`] readings in a row → Stock until Vyro restarts ([`TempGuard`]).
//! - **Drift:** the driver's values are read back every few seconds. Values that changed (a
//!   driver reset, or another tuning program) are written again, and if that keeps happening
//!   tuning ends ([`drifted`]).
//! - **Stock on every exit:** the service stopping, the PC shutting down or going to sleep and
//!   an uninstall all give the card its stock values; after waking they are applied again.
//! - **Apply at start** applies only settings that passed the stability test ([`TuneConfig::tested`]).

pub mod controller;
#[cfg(windows)]
mod nvidia;
pub mod search;
pub mod stress;
#[cfg(test)]
mod tests;

#[cfg(windows)]
pub(crate) use nvidia::load_nvml;
#[cfg(windows)]
pub use nvidia::open_device;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::profiles::Profile;

/// The card's edge temperature that sends it back to Stock...
pub const EDGE_LIMIT_C: f64 = 90.0;
/// ...or its hotspot...
pub const HOTSPOT_LIMIT_C: f64 = 105.0;
/// ...on this many readings in a row.
pub const HOT_READINGS: u32 = 2;
/// Most presets the owner can keep.
pub const MAX_PRESETS: usize = 16;
/// Longest preset name.
pub const MAX_NAME: usize = 32;
/// Settings that passed the stability test, kept per card (the newest).
pub const MAX_TESTED: usize = 32;
/// Event kind in the history store.
pub const EVENT_KIND: &str = "gpu";
/// Ids of the presets Vyro suggests; the owner's have other ids.
pub const STOCK: &str = "stock";
pub const QUIET: &str = "quiet";
pub const EFFICIENT: &str = "efficient";
pub const PERFORMANCE: &str = "performance";
/// The preset "Find my best" saves.
pub const BEST: &str = "best";

/// A range the driver allows for one setting, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TuneRange")]
pub struct Range {
    pub min: i32,
    pub max: i32,
}

impl Range {
    pub fn contains(&self, v: i32) -> bool {
        (self.min..=self.max).contains(&v)
    }

    pub fn clamp(&self, v: i32) -> i32 {
        v.clamp(self.min, self.max)
    }
}

/// What the driver lets Vyro change on this card. A setting without a range stays at stock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TuneLimits")]
pub struct Limits {
    /// Core clock offset, MHz.
    pub core: Option<Range>,
    /// Memory clock offset, MHz, as the driver counts it.
    pub mem: Option<Range>,
    /// Power limit, % of the card's default limit.
    pub power: Option<Range>,
    /// The card's default power limit, W.
    pub power_default_w: Option<f64>,
}

impl Limits {
    /// Whether anything can be changed.
    pub fn any(&self) -> bool {
        self.core.is_some() || self.mem.is_some() || self.power.is_some()
    }

    /// `s` brought within these limits; a setting the driver does not allow goes to stock.
    pub fn clamp(&self, s: Settings) -> Settings {
        Settings {
            core_mhz: self.core.map_or(0, |r| r.clamp(s.core_mhz)),
            mem_mhz: self.mem.map_or(0, |r| r.clamp(s.mem_mhz)),
            power_pct: self.power.map_or(100, |r| r.clamp(s.power_pct)),
        }
    }

    /// Whether `s` is within these limits (a setting the driver does not allow must be stock).
    pub fn allows(&self, s: &Settings) -> bool {
        self.clamp(*s) == *s
    }
}

/// One tuning: offsets on the stock clocks and the power limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TuneSettings")]
pub struct Settings {
    pub core_mhz: i32,
    pub mem_mhz: i32,
    /// % of the card's default power limit.
    pub power_pct: i32,
}

impl Settings {
    pub const STOCK: Self = Self {
        core_mhz: 0,
        mem_mhz: 0,
        power_pct: 100,
    };

    pub fn is_stock(&self) -> bool {
        *self == Self::STOCK
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::STOCK
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TunePresetKind")]
pub enum PresetKind {
    Stock,
    /// Vyro's suggestion for this card's family, scaled to its limits.
    Suggested,
    /// Made by the owner, or by "Find my best".
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TunePreset")]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub settings: Settings,
    pub kind: PresetKind,
    /// What it does, in a sentence (Vyro's presets).
    #[serde(default)]
    pub note: Option<String>,
}

/// Settings that passed the stability test on this card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TunedSettings")]
pub struct Tested {
    pub settings: Settings,
    /// Unix seconds.
    pub at: i64,
}

/// NVIDIA families Vyro has suggestions for. Older and data-centre cards are not tuned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "GpuFamily")]
pub enum Family {
    /// RTX 20, GTX 16.
    Turing,
    /// RTX 30.
    Ampere,
    /// RTX 40.
    Ada,
    /// RTX 50.
    Blackwell,
}

impl Family {
    /// From NVML's architecture name ("Turing", "Ada", ...).
    pub fn from_arch(arch: &str) -> Option<Self> {
        Some(match arch.to_ascii_lowercase().as_str() {
            "turing" => Self::Turing,
            "ampere" => Self::Ampere,
            "ada" | "ada lovelace" | "adalovelace" => Self::Ada,
            "blackwell" => Self::Blackwell,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Turing => "Turing (RTX 20 / GTX 16)",
            Self::Ampere => "Ampere (RTX 30)",
            Self::Ada => "Ada Lovelace (RTX 40)",
            Self::Blackwell => "Blackwell (RTX 50)",
        }
    }
}

/// Whether NVIDIA's name for the card is a laptop GPU ("... Laptop GPU", older "... with Max-Q").
pub fn is_laptop(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("laptop") || n.contains("max-q") || n.ends_with(" mobile")
}

/// The card being tuned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TuneGpu")]
pub struct GpuInfo {
    pub name: String,
    /// The driver's id for the card: presets and tested settings belong to it.
    pub uuid: String,
    pub family: Option<Family>,
    pub laptop: bool,
    pub driver: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "lowercase")]
#[ts(export)]
pub enum TuneCapability {
    /// Tuning is possible on this card.
    Writable { gpu: String },
    /// A card Vyro sees, but not one it tunes (another family, no ranges from the driver).
    ReadOnly { reason: String },
    /// No NVIDIA card, no driver, or not running in the service.
    Unavailable { reason: String },
}

/// What the owner set. Kept in `gpu-tune.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TuneConfig {
    /// Off by default; the window asks for confirmation before turning it on.
    pub enabled: bool,
    /// The preset in force where a profile has none of its own.
    pub preset: String,
    /// Apply it again when Vyro starts (only settings that passed the stability test).
    pub apply_at_start: bool,
    /// The owner's presets.
    #[serde(default)]
    pub presets: Vec<Preset>,
    /// Profiles with their own preset.
    #[serde(default)]
    pub profiles: BTreeMap<Profile, String>,
    /// The card the presets and tested settings were made on ([`GpuInfo::uuid`]). On another
    /// card they are kept but not applied.
    #[serde(default)]
    pub gpu: Option<String>,
    #[serde(default)]
    pub tested: Vec<Tested>,
}

impl Default for TuneConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            preset: STOCK.into(),
            apply_at_start: false,
            presets: Vec::new(),
            profiles: BTreeMap::new(),
            gpu: None,
            tested: Vec::new(),
        }
    }
}

impl TuneConfig {
    /// The preset in force while `profile` is active.
    pub fn preset_for(&self, profile: Profile) -> &str {
        self.profiles.get(&profile).unwrap_or(&self.preset)
    }

    /// When `s` passed the stability test, if it did (stock always counts as tested).
    pub fn tested_at(&self, s: &Settings) -> Option<i64> {
        if s.is_stock() {
            return Some(0);
        }
        self.tested
            .iter()
            .filter(|t| t.settings == *s)
            .map(|t| t.at)
            .max()
    }

    /// Records that `s` passed the stability test at `at`.
    pub fn mark_tested(&mut self, s: Settings, at: i64) {
        self.tested.retain(|t| t.settings != s);
        self.tested.push(Tested { settings: s, at });
        let extra = self.tested.len().saturating_sub(MAX_TESTED);
        self.tested.drain(..extra);
    }
}

/// A preset with what the window shows about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TunePresetView")]
pub struct PresetView {
    #[serde(flatten)]
    pub preset: Preset,
    /// When its settings passed the stability test on this card.
    pub tested_at: Option<i64>,
}

/// The card's clocks as the driver reports them now, MHz.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TuneClocks")]
pub struct Clocks {
    /// The highest core clock it may boost to.
    pub core_max_mhz: Option<f64>,
    pub mem_max_mhz: Option<f64>,
    pub power_limit_w: Option<f64>,
}

/// A stability test or "Find my best" under way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "TuneTrial")]
pub struct Trial {
    /// The settings on trial now.
    pub settings: Settings,
    /// What the window says ("Core +105 MHz", "Checking the result").
    pub step: String,
    /// 0–1.
    pub done: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TuneStatus {
    pub capability: TuneCapability,
    pub gpu: Option<GpuInfo>,
    pub limits: Limits,
    pub config: TuneConfig,
    /// Stock, the suggestions for this card and the owner's presets.
    pub presets: Vec<PresetView>,
    /// What the driver reports now.
    pub current: Option<Settings>,
    pub clocks: Clocks,
    /// The preset in force (none while the card runs stock).
    pub applied: Option<String>,
    /// The profile whose preset is in force.
    pub profile: Profile,
    /// Why a guard holds the card at stock (heat, a failed write, another program) until the
    /// owner applies a preset again.
    pub held: Option<String>,
    /// Why the card is at stock although tuning is on (a crash was found, "Return to stock", a
    /// start that applies only tested settings): applying a preset puts it back.
    pub waiting: Option<String>,
    pub last_error: Option<String>,
    /// A stability test under way.
    pub trial: Option<Trial>,
}

impl TuneStatus {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            capability: TuneCapability::Unavailable {
                reason: reason.into(),
            },
            gpu: None,
            limits: Limits::default(),
            config: TuneConfig::default(),
            presets: Vec::new(),
            current: None,
            clocks: Clocks::default(),
            applied: None,
            profile: Profile::default(),
            held: None,
            waiting: None,
            last_error: None,
            trial: None,
        }
    }
}

/// Tuning as each sensor snapshot reports it, for the header chip and the tray.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TuneBrief {
    /// The card runs other than stock.
    pub tuned: bool,
    /// A guard sent it back to stock.
    pub held: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TuneEventKind {
    On,
    Off,
    /// A preset was applied.
    Applied,
    /// "Return GPU to stock".
    Stock,
    /// The temperature guard sent the card back to stock.
    Hot,
    /// A crash or driver reset while tuned: stock, and "apply at start" off.
    Crash,
    /// The driver's values changed and were written again.
    Rewritten,
    /// Something kept changing them, or a write failed: tuning ended.
    Fault,
    Sleep,
    Wake,
    /// Another profile's preset is now in force.
    Profile,
    /// Settings passed the stability test.
    Passed,
    /// Settings failed it.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TuneEvent {
    /// Unix seconds.
    pub ts: i64,
    pub kind: TuneEventKind,
    pub message: String,
}

/// Change per family, desktop and laptop: (Efficient core, Performance core, Performance memory)
/// in MHz. Deliberately below what most cards of the family reach, so the suggestions run on
/// nearly every card; "Find my best" finds what this one does.
fn table(family: Family, laptop: bool) -> (i32, i32, i32) {
    match (family, laptop) {
        (Family::Turing, false) => (30, 60, 300),
        (Family::Turing, true) => (25, 50, 200),
        (Family::Ampere, false) => (40, 75, 400),
        (Family::Ampere, true) => (30, 60, 300),
        (Family::Ada, false) => (50, 100, 600),
        (Family::Ada, true) => (40, 75, 400),
        (Family::Blackwell, false) => (75, 150, 800),
        (Family::Blackwell, true) => (50, 100, 500),
    }
}

/// Stock and Vyro's suggestions for this card, scaled to what its driver allows. A suggestion
/// that would come out as stock (nothing it changes is allowed) is left out.
pub fn suggestions(family: Option<Family>, laptop: bool, limits: &Limits) -> Vec<Preset> {
    let preset = |id: &str, name: &str, kind, s: Settings, note: &str| Preset {
        id: id.into(),
        name: name.into(),
        settings: s,
        kind,
        note: Some(note.into()),
    };
    let mut out = vec![preset(
        STOCK,
        "Stock",
        PresetKind::Stock,
        Settings::STOCK,
        "The card's own clocks and power limit.",
    )];
    let Some(family) = family else {
        return out;
    };
    let (eff_core, perf_core, perf_mem) = table(family, laptop);
    let power_max = limits.power.map_or(100, |r| r.max.max(100));
    let candidates = [
        preset(
            QUIET,
            "Quiet",
            PresetKind::Suggested,
            limits.clamp(Settings {
                power_pct: 75,
                ..Settings::STOCK
            }),
            "Power limit 75 %: cooler and quieter, a few percent slower.",
        ),
        preset(
            EFFICIENT,
            "Efficient",
            PresetKind::Suggested,
            limits.clamp(Settings {
                core_mhz: eff_core,
                mem_mhz: 0,
                power_pct: 85,
            }),
            "Power limit 85 % and a small core offset: nearly the same speed for less power.",
        ),
        preset(
            PERFORMANCE,
            "Performance",
            PresetKind::Suggested,
            limits.clamp(Settings {
                core_mhz: perf_core,
                mem_mhz: perf_mem,
                power_pct: power_max,
            }),
            "The highest power limit and modest offsets most cards of this family run.",
        ),
    ];
    for p in candidates {
        if !p.settings.is_stock() && !out.iter().any(|o| o.settings == p.settings) {
            out.push(p);
        }
    }
    out
}

/// Stock, the suggestions and the owner's presets, as the window lists them.
pub fn all_presets(
    cfg: &TuneConfig,
    family: Option<Family>,
    laptop: bool,
    limits: &Limits,
) -> Vec<PresetView> {
    suggestions(family, laptop, limits)
        .into_iter()
        .chain(cfg.presets.iter().cloned())
        .map(|p| PresetView {
            tested_at: cfg.tested_at(&p.settings),
            preset: p,
        })
        .collect()
}

/// The settings of preset `id`.
pub fn find(presets: &[PresetView], id: &str) -> Option<Settings> {
    presets
        .iter()
        .find(|p| p.preset.id == id)
        .map(|p| p.preset.settings)
}

/// Checks a configuration before it is saved or applied. `limits` is `None` where tuning is not
/// possible: it may then not be turned on.
pub fn validate(
    cfg: &TuneConfig,
    limits: Option<&Limits>,
    suggested: &[Preset],
) -> Result<(), String> {
    if cfg.enabled && limits.is_none_or(|l| !l.any()) {
        return Err("GPU tuning is not available on this PC".into());
    }
    if cfg.presets.len() > MAX_PRESETS {
        return Err(format!("at most {MAX_PRESETS} presets"));
    }
    for (i, p) in cfg.presets.iter().enumerate() {
        if p.kind != PresetKind::Custom {
            return Err(format!("\"{}\": only your own presets are saved", p.name));
        }
        let id_ok = !p.id.is_empty()
            && p.id.len() <= 40
            && p.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if !id_ok
            || suggested.iter().any(|s| s.id == p.id)
            || cfg.presets[..i].iter().any(|o| o.id == p.id)
        {
            return Err(format!("\"{}\" needs another id", p.name));
        }
        let name = p.name.trim();
        if name.is_empty() || name != p.name || name.chars().count() > MAX_NAME {
            return Err(format!(
                "preset names are 1–{MAX_NAME} characters without spaces at either end"
            ));
        }
        if let Some(l) = limits {
            if !l.allows(&p.settings) {
                return Err(format!("\"{}\" is outside what the driver allows", p.name));
            }
        }
    }
    let known =
        |id: &str| suggested.iter().any(|s| s.id == id) || cfg.presets.iter().any(|p| p.id == id);
    if !known(&cfg.preset) {
        return Err(format!("no preset \"{}\"", cfg.preset));
    }
    for (profile, id) in &cfg.profiles {
        if !known(id) {
            return Err(format!("{}: no preset \"{id}\"", profile.spec().name));
        }
    }
    Ok(())
}

/// Readings in a row at the limit; [`TempGuard::check`] says when to go back to stock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TempGuard {
    hot: u32,
}

impl TempGuard {
    /// One reading of the tuned card; `Some(reason)` when it must go back to stock now.
    pub fn check(&mut self, edge: Option<f64>, hotspot: Option<f64>) -> Option<String> {
        let reason = match (edge, hotspot) {
            (Some(e), _) if e >= EDGE_LIMIT_C => {
                format!("the card reached {e:.0} °C (limit {EDGE_LIMIT_C:.0} °C)")
            }
            (_, Some(h)) if h >= HOTSPOT_LIMIT_C => {
                format!("its hotspot reached {h:.0} °C (limit {HOTSPOT_LIMIT_C:.0} °C)")
            }
            _ => {
                self.hot = 0;
                return None;
            }
        };
        self.hot += 1;
        (self.hot >= HOT_READINGS).then_some(reason)
    }
}

/// Kept on disk while the card runs tuned (`gpu-tuned.json`): a start that finds it knows the
/// last run did not give the card its stock values back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    /// Unix seconds the settings were applied.
    pub since: i64,
    pub preset: String,
    pub settings: Settings,
    /// The settings were on trial for the stability test (not the preset in force).
    #[serde(default)]
    pub trial: bool,
}

/// Events in the Windows logs that mean the PC or the display driver crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashKind {
    /// Kernel-Power 41: the PC stopped without shutting down.
    Shutdown,
    /// Display 4101, or nvlddmkm 153: the display driver stopped responding and was reset.
    DriverReset,
    /// A LiveKernelEvent 141 or 117 report: the GPU hung.
    GpuHang,
}

impl CrashKind {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Shutdown => "the PC stopped without shutting down",
            Self::DriverReset => "the display driver stopped responding and was reset",
            Self::GpuHang => "the GPU stopped responding",
        }
    }
}

/// What a start makes of a marker the last run left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// None was left: the last run gave the card its stock values.
    Clean,
    /// One was left but nothing crashed (Vyro itself stopped): stock, then as configured.
    Unclean,
    /// One was left and the PC or the driver crashed after the settings went on: stock, and
    /// "apply at start" off.
    Crashed {
        kind: CrashKind,
        at: i64,
        marker: Marker,
    },
}

/// Whether the PC or driver crashed while the card ran tuned. `events` are the crash events of
/// the logs (Unix seconds); one shortly before the marker counts too (clocks and logging differ).
pub fn crash_verdict(marker: Option<Marker>, events: &[(i64, CrashKind)]) -> Verdict {
    const SLACK: i64 = 5;
    let Some(marker) = marker else {
        return Verdict::Clean;
    };
    match events
        .iter()
        .filter(|(ts, _)| *ts >= marker.since - SLACK)
        .min_by_key(|(ts, _)| *ts)
    {
        Some(&(at, kind)) => Verdict::Crashed { kind, at, marker },
        None => Verdict::Unclean,
    }
}

/// Whether what the driver reports differs from what Vyro set (power within 1 %, offsets
/// exactly; a setting the driver does not allow is not compared).
pub fn drifted(want: &Settings, read: &Settings, limits: &Limits) -> bool {
    (limits.core.is_some() && want.core_mhz != read.core_mhz)
        || (limits.mem.is_some() && want.mem_mhz != read.mem_mhz)
        || (limits.power.is_some() && (want.power_pct - read.power_pct).abs() > 1)
}

/// "+105 MHz core, +600 MHz memory, power 110 %" (what changes from stock).
pub fn describe(s: &Settings) -> String {
    if s.is_stock() {
        return "stock".into();
    }
    let mut parts = Vec::new();
    if s.core_mhz != 0 {
        parts.push(format!("core {:+} MHz", s.core_mhz));
    }
    if s.mem_mhz != 0 {
        parts.push(format!("memory {:+} MHz", s.mem_mhz));
    }
    if s.power_pct != 100 {
        parts.push(format!("power {} %", s.power_pct));
    }
    parts.join(", ")
}

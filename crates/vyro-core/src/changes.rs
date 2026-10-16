//! PC change history (P19): what changed in the PC between two runs of Vyro, with before → after.
//!
//! The app builds a [`PcInventory`] from what it already collects (hardware id, Windows, monitors,
//! drives, network adapters) with [`fingerprint`]; the engine keeps the last one it saw
//! ([`Known`]) and [`record`] compares the two. Only identity and version fields take part, never
//! live readings.
//!
//! - Fixed parts (CPU, board, memory modules, GPUs, internal drives, Windows, firmware security)
//!   are compared directly: added, removed, replaced (another device in the same place) or a
//!   field changed (BIOS, driver, firmware, RAM speed, Windows build, …).
//! - Hot-plug parts (monitors, USB drives, network adapters) come and go: a monitor switched off
//!   must not count as removed at every start. They are logged once, the first time they are
//!   seen, with the devices of that kind before and after; monitors and network adapters not
//!   seen for 30 days are noted once.
//! - A value that could not be read this time is not a change: missing fields keep their last
//!   value and a class that came back empty is left as it was.
//!
//! Serial numbers never reach the store: where one tells two identical devices apart, the store
//! turns it into a salted hash inside the key ([`InventoryItem::serial`]) and drops it.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::info::{DisplayInfo, DriveInfo, HardwareId, LocalOnly, NetAdapterInfo, SystemInfo};
use crate::snapshot::NetKind;

/// Limits on what a client may send, so a bad one cannot bloat the store.
pub const MAX_ITEMS: usize = 256;
pub const MAX_FIELDS: usize = 64;
pub const MAX_TEXT: usize = 256;
/// Changes kept in the store; the oldest go first.
pub const MAX_CHANGES: i64 = 1000;
/// Hot-plug devices remembered; the longest unseen are forgotten first.
pub const MAX_SEEN: usize = 200;
/// A monitor or network adapter not seen for this long is noted once.
pub const MISSING_SECS: i64 = 30 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ComponentClass {
    Cpu,
    Board,
    Memory,
    Gpu,
    Drive,
    RemovableDrive,
    Monitor,
    Network,
    Windows,
    Security,
}

impl ComponentClass {
    pub const ALL: [Self; 10] = [
        Self::Cpu,
        Self::Board,
        Self::Memory,
        Self::Gpu,
        Self::Drive,
        Self::RemovableDrive,
        Self::Monitor,
        Self::Network,
        Self::Windows,
        Self::Security,
    ];

    /// Comes and goes while the PC runs; logged when first seen rather than when missing.
    pub fn hot_plug(self) -> bool {
        matches!(self, Self::RemovableDrive | Self::Monitor | Self::Network)
    }

    /// Noted once when not seen for [`MISSING_SECS`] (USB drives come and go too often).
    fn noted_missing(self) -> bool {
        matches!(self, Self::Monitor | Self::Network)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ChangeCategory {
    /// A part added, removed or replaced.
    Hardware,
    /// BIOS, graphics card BIOS, drive firmware.
    Firmware,
    /// Graphics driver.
    Driver,
    /// Windows edition, feature update or build.
    Windows,
    /// Firmware settings: memory speed and voltage (XMP / EXPO), Secure Boot, TPM,
    /// virtualization.
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ChangeKind {
    /// Vyro started keeping track of this PC (the first inventory).
    Tracking,
    Added,
    Removed,
    /// Another device in the same place: `before` → `after` are the two devices.
    Replaced,
    /// `field` of the device changed from `before` to `after`.
    Changed,
    /// A monitor, USB drive or network adapter seen for the first time; `before` and `after`
    /// list the devices of that kind connected at the previous check and now.
    FirstSeen,
    /// A monitor or network adapter not seen for 30 days (`last_seen`).
    Missing,
}

/// One entry of the PC change history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HardwareChange {
    /// Row id in the store (newest is highest); 0 before it is stored.
    pub id: i64,
    /// Unix time, seconds: when Vyro noticed it.
    pub ts: i64,
    pub category: ChangeCategory,
    pub kind: ChangeKind,
    /// `null` for [`ChangeKind::Tracking`].
    pub class: Option<ComponentClass>,
    /// Where it sits, e.g. a memory slot "DIMM_A2".
    pub slot: Option<String>,
    /// The device (for `Replaced`, the new one).
    pub name: String,
    /// What changed on it, for `Changed`: "BIOS", "Driver", "Speed", "Build", …
    pub field: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    /// For `Missing`: Unix time it was last seen.
    pub last_seen: Option<i64>,
}

impl HardwareChange {
    fn new(ts: i64, category: ChangeCategory, kind: ChangeKind, c: &Component) -> Self {
        Self {
            id: 0,
            ts,
            category,
            kind,
            class: Some(c.class),
            slot: c.slot.clone(),
            name: c.name.clone(),
            field: None,
            before: None,
            after: None,
            last_seen: None,
        }
    }
}

/// A version or setting of a device. Compared by `value`; `note` is shown beside it (a BIOS
/// date, the vendor's driver version) without counting as a change on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub category: ChangeCategory,
}

impl Field {
    fn new(name: &str, value: impl Into<String>, category: ChangeCategory) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            note: None,
            category,
        }
    }

    fn with_note(mut self, note: Option<String>) -> Self {
        self.note = note.filter(|n| !n.is_empty() && *n != self.value);
        self
    }

    fn shown(&self) -> String {
        match &self.note {
            Some(n) => format!("{} ({n})", self.value),
            None => self.value.clone(),
        }
    }
}

/// One device as the app sees it, sent to the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub class: ComponentClass,
    /// Matches the same device or place across runs ("dimm:DIMM_A2", "gpu:10de:2d04:…#0").
    pub key: String,
    /// Tells identical devices apart. It crosses the local pipe only: the store replaces it with
    /// a salted hash inside the key and never keeps it.
    #[serde(default)]
    pub serial: Option<LocalOnly>,
    #[serde(default)]
    pub slot: Option<String>,
    pub name: String,
    /// What makes it this device rather than another in the same place; empty when unknown.
    #[serde(default)]
    pub identity: String,
    #[serde(default)]
    pub fields: Vec<Field>,
}

/// The PC as the app sees it now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PcInventory {
    pub items: Vec<InventoryItem>,
}

/// A device as stored: the serial is folded into `key` as a salted hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub class: ComponentClass,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    pub name: String,
    #[serde(default)]
    pub identity: String,
    #[serde(default)]
    pub fields: Vec<Field>,
}

/// A hot-plug device seen at some point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    pub class: ComponentClass,
    pub key: String,
    pub name: String,
    pub first: i64,
    pub last: i64,
    /// Already noted as not seen for 30 days.
    #[serde(default)]
    pub missing: bool,
}

/// What the engine remembers of the PC between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Known {
    /// When tracking started (Unix seconds).
    pub since: i64,
    /// Per-PC salt for the serial hashes in keys.
    pub salt: String,
    /// The last value of every fixed part, and the hot-plug devices present at the last check.
    pub components: Vec<Component>,
    pub seen: Vec<Seen>,
}

// ---------------------------------------------------------------------------------------------
// Comparing
// ---------------------------------------------------------------------------------------------

/// Compares `inventory` with what was known and returns the changes and what to remember. With
/// nothing known yet, it starts tracking.
pub fn record(
    prev: Option<Known>,
    inventory: PcInventory,
    ts: i64,
) -> (Vec<HardwareChange>, Known) {
    let salt = prev.as_ref().map_or_else(new_salt, |k| k.salt.clone());
    let now: Vec<Component> = sanitize(inventory)
        .items
        .into_iter()
        .map(|i| keyed(i, &salt))
        .collect();
    match prev {
        None => {
            let seen = now
                .iter()
                .filter(|c| c.class.hot_plug())
                .map(|c| Seen {
                    class: c.class,
                    key: c.key.clone(),
                    name: c.name.clone(),
                    first: ts,
                    last: ts,
                    missing: false,
                })
                .collect();
            let started = HardwareChange {
                id: 0,
                ts,
                category: ChangeCategory::Hardware,
                kind: ChangeKind::Tracking,
                class: None,
                slot: None,
                name: String::new(),
                field: None,
                before: None,
                after: None,
                last_seen: None,
            };
            let known = Known {
                since: ts,
                salt,
                components: now,
                seen,
            };
            (vec![started], known)
        }
        Some(prev) => compare(prev, now, ts),
    }
}

fn compare(prev: Known, now: Vec<Component>, ts: i64) -> (Vec<HardwareChange>, Known) {
    let mut changes = Vec::new();
    let mut kept = Vec::new();
    let mut seen = prev.seen;
    for class in ComponentClass::ALL {
        let before: Vec<&Component> = prev
            .components
            .iter()
            .filter(|c| c.class == class)
            .collect();
        let after: Vec<&Component> = now.iter().filter(|c| c.class == class).collect();
        if class.hot_plug() {
            hot_plug(&mut changes, &mut seen, &before, &after, ts);
            kept.extend(after.into_iter().cloned());
        } else if after.is_empty() {
            // Not collected this time (or nothing of it ever was): keep what was known.
            kept.extend(before.into_iter().cloned());
        } else {
            fixed(&mut changes, &mut kept, &before, &after, ts);
        }
    }
    for class in ComponentClass::ALL
        .into_iter()
        .filter(|c| c.noted_missing())
    {
        // Only while something of the kind is connected, so a failed read never ages them.
        if !now.iter().any(|c| c.class == class) {
            continue;
        }
        for s in seen.iter_mut().filter(|s| s.class == class) {
            if !s.missing && ts - s.last >= MISSING_SECS {
                s.missing = true;
                changes.push(HardwareChange {
                    id: 0,
                    ts,
                    category: ChangeCategory::Hardware,
                    kind: ChangeKind::Missing,
                    class: Some(class),
                    slot: None,
                    name: s.name.clone(),
                    field: None,
                    before: None,
                    after: None,
                    last_seen: Some(s.last),
                });
            }
        }
    }
    if seen.len() > MAX_SEEN {
        seen.sort_by_key(|s| std::cmp::Reverse(s.last));
        seen.truncate(MAX_SEEN);
    }
    let known = Known {
        since: prev.since,
        salt: prev.salt,
        components: kept,
        seen,
    };
    (changes, known)
}

fn names(list: &[&Component]) -> Option<String> {
    (!list.is_empty()).then(|| {
        list.iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    })
}

fn hot_plug(
    changes: &mut Vec<HardwareChange>,
    seen: &mut Vec<Seen>,
    before: &[&Component],
    after: &[&Component],
    ts: i64,
) {
    for c in after {
        match seen
            .iter_mut()
            .find(|s| s.class == c.class && s.key == c.key)
        {
            Some(s) => {
                s.last = ts;
                s.missing = false;
                s.name.clone_from(&c.name);
            }
            None => {
                let mut ch =
                    HardwareChange::new(ts, ChangeCategory::Hardware, ChangeKind::FirstSeen, c);
                ch.before = names(before);
                ch.after = names(after);
                changes.push(ch);
                seen.push(Seen {
                    class: c.class,
                    key: c.key.clone(),
                    name: c.name.clone(),
                    first: ts,
                    last: ts,
                    missing: false,
                });
            }
        }
    }
}

fn replaced(ts: i64, old: &Component, new: &Component) -> HardwareChange {
    let mut ch = HardwareChange::new(ts, ChangeCategory::Hardware, ChangeKind::Replaced, new);
    ch.before = Some(old.name.clone());
    ch.after = Some(new.name.clone());
    ch
}

fn fixed(
    changes: &mut Vec<HardwareChange>,
    kept: &mut Vec<Component>,
    before: &[&Component],
    after: &[&Component],
    ts: i64,
) {
    let mut added = Vec::new();
    for &a in after {
        let Some(&b) = before.iter().find(|b| b.key == a.key) else {
            added.push(a);
            continue;
        };
        if !a.identity.is_empty() && !b.identity.is_empty() && a.identity != b.identity {
            changes.push(replaced(ts, b, a));
            kept.push(a.clone());
            continue;
        }
        let mut merged = a.clone();
        for f in &a.fields {
            if let Some(old) = b.fields.iter().find(|o| o.name == f.name) {
                if old.value != f.value {
                    let mut ch = HardwareChange::new(ts, f.category, ChangeKind::Changed, a);
                    ch.field = Some(f.name.clone());
                    ch.before = Some(old.shown());
                    ch.after = Some(f.shown());
                    changes.push(ch);
                }
            }
        }
        // A field not read this time keeps its last value, so a later change still shows.
        for old in &b.fields {
            if !merged.fields.iter().any(|f| f.name == old.name) {
                merged.fields.push(old.clone());
            }
        }
        if merged.identity.is_empty() {
            merged.identity.clone_from(&b.identity);
        }
        kept.push(merged);
    }
    let removed: Vec<&Component> = before
        .iter()
        .copied()
        .filter(|b| !after.iter().any(|a| a.key == b.key))
        .collect();
    if let ([old], [new]) = (removed.as_slice(), added.as_slice()) {
        changes.push(replaced(ts, old, new));
    } else {
        for &r in &removed {
            let mut ch = HardwareChange::new(ts, ChangeCategory::Hardware, ChangeKind::Removed, r);
            ch.before = Some(r.name.clone());
            changes.push(ch);
        }
        for &a in &added {
            let mut ch = HardwareChange::new(ts, ChangeCategory::Hardware, ChangeKind::Added, a);
            ch.after = Some(a.name.clone());
            changes.push(ch);
        }
    }
    kept.extend(added.into_iter().cloned());
}

/// A fresh per-PC salt. It only has to differ between PCs, not be secret: it sits in the same
/// database as the hashes.
pub fn new_salt() -> String {
    format!("{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..))
}

fn serial_hash(salt: &str, serial: &str) -> String {
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update([0]);
    h.update(serial.as_bytes());
    h.finalize()[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The stored form of an item: its serial, if any, becomes a salted hash in the key.
fn keyed(item: InventoryItem, salt: &str) -> Component {
    let key = match item
        .serial
        .as_ref()
        .map(|s| s.0.trim())
        .filter(|s| !s.is_empty())
    {
        Some(serial) => format!("{}#{}", item.key, serial_hash(salt, serial)),
        None => item.key,
    };
    Component {
        class: item.class,
        key,
        slot: item.slot,
        name: item.name,
        identity: item.identity,
        fields: item.fields,
    }
}

fn clip(s: &mut String) {
    if s.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

/// Caps what a client sent at the store's limits.
pub fn sanitize(mut inv: PcInventory) -> PcInventory {
    inv.items.truncate(MAX_ITEMS);
    for item in &mut inv.items {
        clip(&mut item.key);
        clip(&mut item.name);
        clip(&mut item.identity);
        if let Some(s) = &mut item.slot {
            clip(s);
        }
        if let Some(s) = &mut item.serial {
            clip(&mut s.0);
        }
        item.fields.truncate(MAX_FIELDS);
        for f in &mut item.fields {
            clip(&mut f.name);
            clip(&mut f.value);
            if let Some(n) = &mut f.note {
                clip(n);
            }
        }
    }
    inv
}

// ---------------------------------------------------------------------------------------------
// Fingerprint
// ---------------------------------------------------------------------------------------------

fn text(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn join(parts: &[Option<&str>]) -> String {
    parts
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

fn on_off(v: Option<bool>) -> Option<&'static str> {
    v.map(|v| if v { "On" } else { "Off" })
}

/// `x` with at most `decimals` decimals, trailing zeros dropped.
fn trimmed(x: f64, decimals: usize) -> String {
    let s = format!("{x:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Binary sizes as the Storage tab shows them: "1.82 TB", "954 GB", "16 GB".
fn size(mb: f64) -> String {
    let gb = mb / 1024.0;
    if gb >= 1024.0 {
        format!("{} TB", trimmed(gb / 1024.0, 2))
    } else {
        format!("{} GB", trimmed(gb, if gb >= 100.0 { 0 } else { 1 }))
    }
}

/// Numbers keys of devices that share one, in order (`#0`, `#1`, …).
fn numbered(items: &mut [InventoryItem]) {
    let mut count: HashMap<String, usize> = HashMap::new();
    for item in items {
        let n = count.entry(item.key.clone()).or_default();
        item.key = format!("{}#{n}", item.key);
        *n += 1;
    }
}

/// The PC's identity and versions from what the app collects. Fields that could not be read are
/// left out, and so are virtual network adapters and outputs without a monitor's EDID.
pub fn fingerprint(
    hw: &HardwareId,
    sys: &SystemInfo,
    displays: &[DisplayInfo],
    drives: &[DriveInfo],
    adapters: &[NetAdapterInfo],
) -> PcInventory {
    use ChangeCategory as C;
    let mut items = Vec::new();
    let item = |class, key: String, name: String, identity: String, fields| InventoryItem {
        class,
        key,
        serial: None,
        slot: None,
        name,
        identity,
        fields,
    };

    if let Some(brand) = text(&hw.cpu.brand) {
        let mut identity = brand.to_string();
        if let (Some(c), Some(t)) = (hw.cpu.cores, hw.cpu.threads) {
            identity = format!("{brand} ({c} cores, {t} threads)");
        }
        items.push(item(
            ComponentClass::Cpu,
            "cpu".into(),
            brand.into(),
            identity,
            vec![],
        ));
    }

    let b = &hw.board;
    let board = join(&[text(&b.manufacturer), text(&b.product)]);
    let board = if board.is_empty() {
        join(&[text(&b.system_manufacturer), text(&b.system_product)])
    } else {
        board
    };
    let bios = text(&b.bios_version)
        .map(|v| Field::new("BIOS", v, C::Firmware).with_note(text(&b.bios_date).map(Into::into)));
    if !board.is_empty() || bios.is_some() {
        let name = if board.is_empty() {
            "Motherboard".to_string()
        } else {
            board.clone()
        };
        items.push(item(
            ComponentClass::Board,
            "board".into(),
            name,
            board,
            bios.into_iter().collect(),
        ));
    }

    for d in &hw.memory.dimms {
        let kind = join(&[Some(size(d.size_mb).as_str()), text(&d.memory_type)]);
        let maker = join(&[text(&d.manufacturer), text(&d.part_number)]);
        let name = if maker.is_empty() {
            kind.clone()
        } else {
            format!("{maker}, {kind}")
        };
        // Raw numbers, so a change in how sizes are written never reads as another module.
        let identity = format!(
            "{maker}|{}|{}|{}",
            d.size_mb.round(),
            text(&d.memory_type).unwrap_or_default(),
            d.speed_mts.unwrap_or(0)
        );
        let mut fields = Vec::new();
        if let Some(s) = d.configured_speed_mts.filter(|s| *s > 0) {
            fields.push(Field::new("Speed", format!("{s} MT/s"), C::Settings));
        }
        if let Some(v) = d.configured_voltage_mv.filter(|v| *v > 0) {
            fields.push(Field::new(
                "Voltage",
                format!("{:.2} V", f64::from(v) / 1000.0),
                C::Settings,
            ));
        }
        let mut it = item(
            ComponentClass::Memory,
            format!("dimm:{}", d.slot),
            name,
            identity,
            fields,
        );
        it.slot = Some(d.slot.clone());
        items.push(it);
    }

    let mut gpus: Vec<InventoryItem> = hw
        .gpus
        .iter()
        .map(|g| {
            let mut fields = Vec::new();
            if let Some(v) = text(&g.driver_version) {
                fields.push(
                    Field::new("Driver", v, C::Driver)
                        .with_note(text(&g.vendor_driver_version).map(Into::into)),
                );
            }
            if let Some(v) = text(&g.bios_version) {
                fields.push(Field::new("VBIOS", v, C::Firmware));
            }
            let identity = format!("{}|{}", g.name.trim(), g.vram_mb.unwrap_or(0.0).round());
            item(
                ComponentClass::Gpu,
                format!(
                    "gpu:{:04x}:{:04x}:{:08x}",
                    g.vendor_id, g.device_id, g.subsys_id
                ),
                g.name.trim().into(),
                identity,
                fields,
            )
        })
        .collect();
    numbered(&mut gpus);
    items.extend(gpus);

    if let Some(edition) = text(&sys.os.name) {
        let mut fields = vec![Field::new("Edition", edition, C::Windows)];
        if let Some(build) = text(&sys.os.build) {
            fields.push(
                Field::new("Build", build, C::Windows)
                    .with_note(text(&sys.os.display_version).map(Into::into)),
            );
        }
        items.push(item(
            ComponentClass::Windows,
            "windows".into(),
            "Windows".into(),
            String::new(),
            fields,
        ));
    }

    let s = &sys.security;
    let mut security = Vec::new();
    for (name, value) in [
        ("UEFI", on_off(s.uefi)),
        ("Secure Boot", on_off(s.secure_boot)),
        ("Virtualization", on_off(s.virtualization_enabled)),
        ("Hypervisor", on_off(s.hypervisor_present)),
    ] {
        if let Some(v) = value {
            security.push(Field::new(name, v, C::Settings));
        }
    }
    match (s.tpm_present, text(&s.tpm_version)) {
        (Some(true), Some(v)) => security.push(Field::new("TPM", v, C::Settings)),
        (Some(false), _) => security.push(Field::new("TPM", "None", C::Settings)),
        _ => {}
    }
    if !security.is_empty() {
        items.push(item(
            ComponentClass::Security,
            "security".into(),
            "Firmware security".into(),
            String::new(),
            security,
        ));
    }

    // Identical devices without a serial are told apart by order; with one, by its hash.
    let mut unnamed = Vec::new();
    for d in drives {
        let model = d.model.trim();
        if model.is_empty() {
            continue;
        }
        let bus = d.bus.to_ascii_uppercase();
        let class = if d.removable || bus == "USB" || bus == "SD" || bus == "MMC" {
            ComponentClass::RemovableDrive
        } else {
            ComponentClass::Drive
        };
        let capacity = d.capacity_mb.filter(|c| *c > 0.0).map(size);
        let name = match &capacity {
            Some(c) => format!("{model} ({c}, {})", d.bus),
            None => format!("{model} ({})", d.bus),
        };
        let fields = match (class, text(&d.firmware)) {
            (ComponentClass::Drive, Some(fw)) => vec![Field::new("Firmware", fw, C::Firmware)],
            _ => vec![],
        };
        let mut it = item(
            class,
            format!("drive:{model}"),
            name,
            format!(
                "{model}|{}",
                (d.capacity_mb.unwrap_or(0.0) / 1024.0).round()
            ),
            fields,
        );
        it.serial = d.serial.clone().filter(|s| !s.0.trim().is_empty());
        if it.serial.is_some() {
            items.push(it);
        } else {
            unnamed.push(it);
        }
    }

    for d in displays {
        let (Some(mid), Some(code)) = (text(&d.manufacturer_id), text(&d.product_code)) else {
            continue;
        };
        let name = text(&d.model)
            .map(str::to_string)
            .unwrap_or_else(|| join(&[text(&d.manufacturer).or(Some(mid)), Some(code)]));
        let key = format!("monitor:{mid}:{code}");
        let mut it = item(ComponentClass::Monitor, key.clone(), name, key, vec![]);
        it.serial = d.serial.clone().filter(|s| !s.0.trim().is_empty());
        if it.serial.is_some() {
            items.push(it);
        } else {
            unnamed.push(it);
        }
    }
    numbered(&mut unnamed);
    items.extend(unnamed);

    for a in adapters {
        let desc = a.description.trim();
        if a.virtual_adapter
            || desc.is_empty()
            || !matches!(a.kind, NetKind::Ethernet | NetKind::Wifi)
        {
            continue;
        }
        let key = format!("net:{desc}");
        if items.iter().any(|i| i.key == key) {
            continue;
        }
        items.push(item(
            ComponentClass::Network,
            key,
            desc.into(),
            String::new(),
            vec![],
        ));
    }

    PcInventory { items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::{BoardId, CpuId, DimmInfo, GpuId, MemoryId, OsInfo, SecurityInfo};

    const DAY: i64 = 86_400;

    fn hw() -> HardwareId {
        HardwareId {
            cpu: CpuId {
                brand: Some("AMD Ryzen 7 9700X 8-Core Processor".into()),
                cores: Some(8),
                threads: Some(16),
                ..Default::default()
            },
            memory: MemoryId {
                dimms: vec![
                    dimm("DIMM_A2", "KF560C30-16"),
                    dimm("DIMM_B2", "KF560C30-16"),
                ],
                ..Default::default()
            },
            board: BoardId {
                manufacturer: Some("Micro-Star International Co., Ltd.".into()),
                product: Some("B650M GAMING PLUS WIFI (MS-7E24)".into()),
                serial: Some("BOARDSERIAL0042".into()),
                bios_version: Some("1.A0".into()),
                bios_date: Some("07/12/2024".into()),
                ..Default::default()
            },
            gpus: vec![gpu("566.36", "32.0.15.6636")],
            smbios_version: None,
        }
    }

    fn dimm(slot: &str, part: &str) -> DimmInfo {
        DimmInfo {
            slot: slot.into(),
            manufacturer: Some("Kingston".into()),
            part_number: Some(part.into()),
            size_mb: 16384.0,
            memory_type: Some("DDR5".into()),
            speed_mts: Some(6000),
            configured_speed_mts: Some(4800),
            configured_voltage_mv: Some(1100),
            ..Default::default()
        }
    }

    fn gpu(vendor: &str, windows: &str) -> GpuId {
        GpuId {
            name: "NVIDIA GeForce RTX 5060 Ti".into(),
            vendor_id: 0x10de,
            device_id: 0x2d04,
            subsys_id: 0x1234_10de,
            vram_mb: Some(16384.0),
            driver_version: Some(windows.into()),
            vendor_driver_version: Some(vendor.into()),
            bios_version: Some("98.06.1f.00.dc".into()),
            ..Default::default()
        }
    }

    fn sys() -> SystemInfo {
        SystemInfo {
            os: OsInfo {
                name: Some("Windows 11 Pro".into()),
                display_version: Some("24H2".into()),
                build: Some("26100.2033".into()),
                ..Default::default()
            },
            security: SecurityInfo {
                uefi: Some(true),
                secure_boot: Some(true),
                tpm_present: Some(true),
                tpm_version: Some("2.0".into()),
                virtualization_enabled: Some(true),
                hypervisor_present: Some(false),
                bitlocker_system_drive: None,
            },
            ..Default::default()
        }
    }

    fn monitor(code: &str, model: &str, serial: Option<&str>) -> DisplayInfo {
        DisplayInfo {
            manufacturer_id: Some("GSM".into()),
            product_code: Some(code.into()),
            model: Some(model.into()),
            serial: serial.map(|s| LocalOnly(s.into())),
            ..Default::default()
        }
    }

    fn drive(model: &str, serial: &str, bus: &str, removable: bool) -> DriveInfo {
        DriveInfo {
            model: model.into(),
            serial: Some(LocalOnly(serial.into())),
            firmware: Some("4B2QJXD7".into()),
            bus: bus.into(),
            removable,
            capacity_mb: Some(1_907_729.0),
            ..Default::default()
        }
    }

    fn nic(desc: &str, virt: bool) -> NetAdapterInfo {
        NetAdapterInfo {
            description: desc.into(),
            kind: NetKind::Ethernet,
            virtual_adapter: virt,
            mac: Some(LocalOnly("00:11:22:33:44:55".into())),
            ..Default::default()
        }
    }

    struct Pc {
        hw: HardwareId,
        sys: SystemInfo,
        displays: Vec<DisplayInfo>,
        drives: Vec<DriveInfo>,
        nics: Vec<NetAdapterInfo>,
    }

    impl Pc {
        fn new() -> Self {
            Self {
                hw: hw(),
                sys: sys(),
                displays: vec![monitor("5B7F", "LG ULTRAGEAR", Some("MONSERIAL77"))],
                drives: vec![drive(
                    "Samsung SSD 990 PRO 2TB",
                    "S6Z2NJ0W123456",
                    "NVMe",
                    false,
                )],
                nics: vec![
                    nic("Realtek Gaming 2.5GbE Family Controller", false),
                    nic("Hyper-V Virtual Ethernet Adapter", true),
                ],
            }
        }

        fn inv(&self) -> PcInventory {
            fingerprint(
                &self.hw,
                &self.sys,
                &self.displays,
                &self.drives,
                &self.nics,
            )
        }
    }

    /// Records `pc` at `ts` over `known`, returning the changes.
    fn step(known: &mut Option<Known>, pc: &Pc, ts: i64) -> Vec<HardwareChange> {
        let (changes, k) = record(known.take(), pc.inv(), ts);
        *known = Some(k);
        changes
    }

    fn start(pc: &Pc) -> Option<Known> {
        let mut known = None;
        let first = step(&mut known, pc, 0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].kind, ChangeKind::Tracking);
        known
    }

    #[test]
    fn the_fingerprint_holds_identity_and_versions_only() {
        let inv = Pc::new().inv();
        let classes: Vec<ComponentClass> = inv.items.iter().map(|i| i.class).collect();
        use ComponentClass as K;
        assert_eq!(
            classes,
            [
                K::Cpu,
                K::Board,
                K::Memory,
                K::Memory,
                K::Gpu,
                K::Windows,
                K::Security,
                K::Drive,
                K::Monitor,
                K::Network
            ],
            "the Hyper-V adapter is left out"
        );
        let board = &inv.items[1];
        assert_eq!(
            board.fields,
            [Field::new("BIOS", "1.A0", ChangeCategory::Firmware)
                .with_note(Some("07/12/2024".into()))]
        );
        let dimm = &inv.items[2];
        assert_eq!(dimm.name, "Kingston KF560C30-16, 16 GB DDR5");
        assert_eq!(dimm.slot.as_deref(), Some("DIMM_A2"));
        assert_eq!(inv.items[4].key, "gpu:10de:2d04:123410de#0");
        assert_eq!(inv.items[7].name, "Samsung SSD 990 PRO 2TB (1.82 TB, NVMe)");
        // The board serial is not part of it at all; drive and monitor serials only as `serial`.
        let json = serde_json::to_string(&inv.items[1]).unwrap();
        assert!(!json.contains("BOARDSERIAL"));
    }

    #[test]
    fn nothing_changes_when_nothing_changed() {
        let pc = Pc::new();
        let mut known = start(&pc);
        assert!(step(&mut known, &pc, DAY).is_empty());
        assert!(step(&mut known, &pc, 2 * DAY).is_empty());
    }

    #[test]
    fn driver_bios_and_windows_updates_show_before_and_after() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        pc.hw.gpus = vec![gpu("572.16", "32.0.15.7216")];
        pc.hw.board.bios_version = Some("1.B0".into());
        pc.hw.board.bios_date = Some("09/30/2024".into());
        pc.sys.os.build = Some("26100.2314".into());
        let ch = step(&mut known, &pc, DAY);
        let summary: Vec<_> = ch
            .iter()
            .map(|c| {
                (
                    c.category,
                    c.kind,
                    c.field.as_deref().unwrap(),
                    c.before.as_deref().unwrap(),
                    c.after.as_deref().unwrap(),
                )
            })
            .collect();
        use ChangeCategory as C;
        assert_eq!(
            summary,
            [
                (
                    C::Firmware,
                    ChangeKind::Changed,
                    "BIOS",
                    "1.A0 (07/12/2024)",
                    "1.B0 (09/30/2024)"
                ),
                (
                    C::Driver,
                    ChangeKind::Changed,
                    "Driver",
                    "32.0.15.6636 (566.36)",
                    "32.0.15.7216 (572.16)"
                ),
                (
                    C::Windows,
                    ChangeKind::Changed,
                    "Build",
                    "26100.2033 (24H2)",
                    "26100.2314 (24H2)"
                ),
            ]
        );
        assert_eq!(ch[1].name, "NVIDIA GeForce RTX 5060 Ti");
    }

    #[test]
    fn memory_swaps_additions_and_xmp() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        // EXPO on: the configured speed and voltage change, the modules do not.
        for d in &mut pc.hw.memory.dimms {
            d.configured_speed_mts = Some(6000);
            d.configured_voltage_mv = Some(1350);
        }
        let ch = step(&mut known, &pc, DAY);
        assert_eq!(ch.len(), 4);
        assert!(ch
            .iter()
            .all(|c| c.category == ChangeCategory::Settings && c.kind == ChangeKind::Changed));
        assert_eq!(
            (
                ch[0].slot.as_deref(),
                ch[0].before.as_deref(),
                ch[0].after.as_deref()
            ),
            (Some("DIMM_A2"), Some("4800 MT/s"), Some("6000 MT/s"))
        );
        assert_eq!(ch[1].after.as_deref(), Some("1.35 V"));

        // A new kit in the same slots, plus two more modules.
        pc.hw.memory.dimms = ["DIMM_A1", "DIMM_A2", "DIMM_B1", "DIMM_B2"]
            .iter()
            .map(|s| {
                let mut d = dimm(s, "F5-6000J3038F16G");
                d.manufacturer = Some("G.Skill".into());
                d.configured_speed_mts = Some(6000);
                d.configured_voltage_mv = Some(1350);
                d
            })
            .collect();
        let ch = step(&mut known, &pc, 2 * DAY);
        let kinds: Vec<_> = ch
            .iter()
            .map(|c| (c.kind, c.slot.clone().unwrap()))
            .collect();
        assert_eq!(
            kinds,
            [
                (ChangeKind::Replaced, "DIMM_A2".into()),
                (ChangeKind::Replaced, "DIMM_B2".into()),
                (ChangeKind::Added, "DIMM_A1".into()),
                (ChangeKind::Added, "DIMM_B1".into()),
            ]
        );
        assert_eq!(
            ch[0].before.as_deref(),
            Some("Kingston KF560C30-16, 16 GB DDR5")
        );
        assert_eq!(
            ch[0].after.as_deref(),
            Some("G.Skill F5-6000J3038F16G, 16 GB DDR5")
        );

        pc.hw.memory.dimms.truncate(3);
        let ch = step(&mut known, &pc, 3 * DAY);
        assert_eq!(
            (ch.len(), ch[0].kind, ch[0].slot.as_deref()),
            (1, ChangeKind::Removed, Some("DIMM_B2"))
        );
    }

    #[test]
    fn a_graphics_card_swap_is_one_replacement_and_twins_keep_their_places() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        let mut amd = gpu("", "32.0.21001.9005");
        amd.name = "AMD Radeon RX 9070 XT".into();
        amd.vendor_id = 0x1002;
        amd.device_id = 0x7550;
        pc.hw.gpus = vec![amd];
        let ch = step(&mut known, &pc, DAY);
        assert_eq!(ch.len(), 1);
        assert_eq!(
            (ch[0].kind, ch[0].before.as_deref(), ch[0].after.as_deref()),
            (
                ChangeKind::Replaced,
                Some("NVIDIA GeForce RTX 5060 Ti"),
                Some("AMD Radeon RX 9070 XT")
            )
        );

        // Two identical cards, then one taken out.
        let mut pc = Pc::new();
        pc.hw.gpus = vec![gpu("566.36", "32.0.15.6636"), gpu("566.36", "32.0.15.6636")];
        let mut known = start(&pc);
        assert!(step(&mut known, &pc, DAY).is_empty());
        pc.hw.gpus.pop();
        let ch = step(&mut known, &pc, 2 * DAY);
        assert_eq!((ch.len(), ch[0].kind), (1, ChangeKind::Removed));
    }

    #[test]
    fn what_could_not_be_read_is_not_a_change() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        // The driver version and every module are missing this time.
        pc.hw.gpus[0].driver_version = None;
        pc.hw.gpus[0].vendor_driver_version = None;
        pc.hw.memory.dimms.clear();
        pc.sys.os.name = None;
        assert!(step(&mut known, &pc, DAY).is_empty());
        // Back, and updated: compared with the last value read.
        pc = Pc::new();
        pc.hw.gpus = vec![gpu("572.16", "32.0.15.7216")];
        let ch = step(&mut known, &pc, 2 * DAY);
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].before.as_deref(), Some("32.0.15.6636 (566.36)"));
    }

    #[test]
    fn a_monitor_switched_off_logs_nothing_and_a_new_one_logs_once() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        pc.displays.clear();
        assert!(step(&mut known, &pc, DAY).is_empty(), "switched off");
        pc.displays = vec![monitor("5B7F", "LG ULTRAGEAR", Some("MONSERIAL77"))];
        assert!(step(&mut known, &pc, 2 * DAY).is_empty(), "back on");

        // Swapped for another monitor.
        pc.displays = vec![monitor("A0B1", "DELL U2724D", Some("DELL123"))];
        pc.displays[0].manufacturer_id = Some("DEL".into());
        let ch = step(&mut known, &pc, 3 * DAY);
        assert_eq!(ch.len(), 1);
        assert_eq!(
            (ch[0].kind, ch[0].name.as_str()),
            (ChangeKind::FirstSeen, "DELL U2724D")
        );
        assert_eq!(
            (ch[0].before.as_deref(), ch[0].after.as_deref()),
            (Some("LG ULTRAGEAR"), Some("DELL U2724D"))
        );
        assert!(step(&mut known, &pc, 4 * DAY).is_empty());

        // The old one, not seen for 30 days, is noted once.
        let ch = step(&mut known, &pc, 2 * DAY + MISSING_SECS);
        assert_eq!(
            (ch.len(), ch[0].kind, ch[0].name.as_str(), ch[0].last_seen),
            (1, ChangeKind::Missing, "LG ULTRAGEAR", Some(2 * DAY))
        );
        assert!(step(&mut known, &pc, 3 * DAY + MISSING_SECS).is_empty());
        // Nothing connected at all is not evidence either way.
        pc.displays.clear();
        assert!(step(&mut known, &pc, 100 * DAY).is_empty());
    }

    #[test]
    fn identical_monitors_are_told_apart_by_serial() {
        let mut pc = Pc::new();
        pc.displays = vec![
            monitor("5B7F", "LG ULTRAGEAR", Some("AAA")),
            monitor("5B7F", "LG ULTRAGEAR", Some("BBB")),
        ];
        let mut known = start(&pc);
        pc.displays.reverse();
        assert!(
            step(&mut known, &pc, DAY).is_empty(),
            "order does not matter"
        );
        pc.displays[0].serial = Some(LocalOnly("CCC".into()));
        let ch = step(&mut known, &pc, 2 * DAY);
        assert_eq!((ch.len(), ch[0].kind), (1, ChangeKind::FirstSeen));
    }

    #[test]
    fn usb_drives_and_network_adapters_are_hot_plug() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        pc.drives
            .push(drive("SanDisk Ultra", "4C530001", "USB", true));
        pc.nics.push(nic("Intel(R) Wi-Fi 7 BE200 320MHz", false));
        let ch = step(&mut known, &pc, DAY);
        let got: Vec<_> = ch.iter().map(|c| (c.class.unwrap(), c.kind)).collect();
        assert_eq!(
            got,
            [
                (ComponentClass::RemovableDrive, ChangeKind::FirstSeen),
                (ComponentClass::Network, ChangeKind::FirstSeen)
            ]
        );
        pc.drives.pop();
        assert!(step(&mut known, &pc, 2 * DAY).is_empty(), "unplugged");
        // A USB drive gone for months is not noted.
        assert!(step(&mut known, &pc, 200 * DAY).is_empty());
    }

    #[test]
    fn an_internal_drive_swap_and_firmware_update() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        pc.drives[0].firmware = Some("4B2QJXD8".into());
        let ch = step(&mut known, &pc, DAY);
        assert_eq!(
            (ch.len(), ch[0].category, ch[0].field.as_deref()),
            (1, ChangeCategory::Firmware, Some("Firmware"))
        );
        // Same model, another unit: the serial hash tells them apart.
        pc.drives[0].serial = Some(LocalOnly("S6Z2NJ0W999999".into()));
        let ch = step(&mut known, &pc, 2 * DAY);
        assert_eq!((ch.len(), ch[0].kind), (1, ChangeKind::Replaced));
    }

    #[test]
    fn serials_never_reach_what_is_stored() {
        let mut pc = Pc::new();
        pc.drives
            .push(drive("SanDisk Ultra", "USBSERIAL999", "USB", true));
        let mut known = start(&pc);
        pc.displays
            .push(monitor("A0B1", "LG 27GL850", Some("MONSERIAL88")));
        let ch = step(&mut known, &pc, DAY);
        let stored = serde_json::to_string(&(known.as_ref().unwrap(), &ch)).unwrap();
        for secret in [
            "MONSERIAL77",
            "MONSERIAL88",
            "S6Z2NJ0W123456",
            "USBSERIAL999",
            "BOARDSERIAL",
            "00:11:22",
        ] {
            assert!(!stored.contains(secret), "{secret} stored");
        }
        // The same serial gives the same key on every run (the salt is kept).
        let key = known
            .as_ref()
            .unwrap()
            .components
            .iter()
            .find(|c| c.class == ComponentClass::Drive)
            .unwrap()
            .key
            .clone();
        assert!(key.starts_with("drive:Samsung SSD 990 PRO 2TB#") && key.len() > 31);
        assert!(step(&mut known, &pc, 2 * DAY).is_empty());
        assert_eq!(
            known
                .unwrap()
                .components
                .iter()
                .find(|c| c.class == ComponentClass::Drive)
                .unwrap()
                .key,
            key
        );
    }

    #[test]
    fn security_settings_and_cpu_swap() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        pc.sys.security.secure_boot = Some(false);
        pc.sys.security.hypervisor_present = Some(true);
        pc.hw.cpu.brand = Some("AMD Ryzen 7 9800X3D 8-Core Processor".into());
        let ch = step(&mut known, &pc, DAY);
        let got: Vec<_> = ch
            .iter()
            .map(|c| (c.kind, c.field.clone(), c.after.clone()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    ChangeKind::Replaced,
                    None,
                    Some("AMD Ryzen 7 9800X3D 8-Core Processor".into())
                ),
                (
                    ChangeKind::Changed,
                    Some("Secure Boot".into()),
                    Some("Off".into())
                ),
                (
                    ChangeKind::Changed,
                    Some("Hypervisor".into()),
                    Some("On".into())
                ),
            ]
        );
    }

    #[test]
    fn what_a_client_sends_is_capped() {
        let long = "x".repeat(1000);
        let item = InventoryItem {
            class: ComponentClass::Network,
            key: long.clone(),
            serial: None,
            slot: Some("é".repeat(300)),
            name: long.clone(),
            identity: String::new(),
            fields: vec![Field::new("a", long, ChangeCategory::Settings); 100],
        };
        let inv = sanitize(PcInventory {
            items: vec![item; 300],
        });
        assert_eq!(inv.items.len(), MAX_ITEMS);
        let it = &inv.items[0];
        assert_eq!(
            (it.key.len(), it.fields.len(), it.fields[0].value.len()),
            (MAX_TEXT, MAX_FIELDS, MAX_TEXT)
        );
        assert!(it.slot.as_ref().unwrap().len() <= MAX_TEXT);
    }

    #[test]
    fn hot_plug_memory_is_bounded() {
        let mut pc = Pc::new();
        let mut known = start(&pc);
        for i in 0..(MAX_SEEN + 20) {
            pc.drives = vec![drive("SanDisk Ultra", &format!("U{i}"), "USB", true)];
            step(&mut known, &pc, i as i64 + 1);
        }
        let seen = &known.unwrap().seen;
        assert_eq!(seen.len(), MAX_SEEN);
        assert!(
            seen.iter().all(|s| s.last > 20),
            "the longest unseen were forgotten"
        );
    }
}

//! Devices and peripherals (P22): what is plugged in or paired, how it connects, and its battery
//! where the device reports one. Windows gathers raw device containers (one per physical
//! device), the device nodes in them, audio endpoints and battery readings; [`build`] turns them
//! into the list the Devices tab shows. Building is pure so every rule is tested here.
//!
//! Privacy: Bluetooth addresses and device paths never leave the collector. Ids are short hashes
//! of the container id, and names stay on this PC (never in reports, cards or history).

pub mod hid;
#[cfg(windows)]
mod hid_win;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use hid_win::{collections as hid_collections, Collection as HidCollection};
#[cfg(windows)]
pub use win::{collect, watch};

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ---------------------------------------------------------------------------------------------
// Contracts
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
    /// Has both a keyboard and a mouse function and nothing tells which it is.
    Input,
    Controller,
    /// A wireless receiver (2.4 GHz dongle, Xbox Wireless Adapter).
    Receiver,
    Headset,
    Headphones,
    Speakers,
    Microphone,
    Camera,
    Printer,
    Phone,
    Storage,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Connection {
    Usb,
    Bluetooth,
    BluetoothLe,
    Network,
    /// Paired to a wireless receiver: `parent` is the receiver.
    Receiver,
    Other,
}

/// Where a battery reading came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BatterySource {
    /// Windows' own reading (the Bluetooth battery property Settings shows).
    Windows,
    /// The device's Bluetooth LE Battery Service (0x180F).
    BluetoothLe,
    /// XInput: four coarse levels.
    Xinput,
    /// Logitech's own protocol (HID++), through its receiver or directly.
    Logitech,
    /// A PlayStation controller's input report.
    PlayStation,
    /// The standard HID Battery Strength value.
    HidBattery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BatteryLevel {
    Empty,
    Low,
    Medium,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Battery {
    /// 0–100, when the device reports a percentage.
    pub percent: Option<u8>,
    /// XInput's level, when that is all there is.
    pub level: Option<BatteryLevel>,
    /// On a charger and charging; `None` where the device does not say.
    pub charging: Option<bool>,
    pub source: BatterySource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    /// Stable per device: a hash of its container id.
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub manufacturer: Option<String>,
    pub connection: Connection,
    /// Bluetooth, a wireless receiver, or a wireless controller.
    pub wireless: bool,
    /// False for a paired Bluetooth device that is off or out of range. Unplugged wired devices
    /// are not listed at all.
    pub connected: bool,
    /// Only while connected; `None` is shown as "—".
    pub battery: Option<Battery>,
    /// Device Manager problem code of a part of it (22 = disabled), while connected.
    pub problem: Option<u32>,
    /// The receiver a device is paired to (its id), for devices reached through one.
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEndpoint {
    pub id: String,
    pub name: String,
    /// Playback (true) or recording.
    pub output: bool,
    /// False: an empty jack.
    pub plugged: bool,
    /// Windows' default for this direction.
    pub default: bool,
    /// The listed device it belongs to; `None` for the PC's own audio.
    pub device: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Devices {
    pub devices: Vec<DeviceInfo>,
    pub audio: Vec<AudioEndpoint>,
}

// ---------------------------------------------------------------------------------------------
// Raw input (filled by the Windows collector)
// ---------------------------------------------------------------------------------------------

/// A device container: one physical device as Windows groups it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawContainer {
    /// Container GUID, any case, braces or not.
    pub id: String,
    pub name: Option<String>,
    pub manufacturer: Option<String>,
    pub connected: Option<bool>,
    /// The PC itself.
    pub local_machine: bool,
    pub network: bool,
    /// Windows marks it not interesting for display.
    pub hidden: bool,
    /// Canonical categories, e.g. "Input.Mouse", "Audio.Headset".
    pub categories: Vec<String>,
    /// The Bluetooth battery property, where Windows keeps it on the container.
    pub battery: Option<u8>,
}

/// A present device node.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawNode {
    pub container: Option<String>,
    /// Setup class name: "Keyboard", "Mouse", "HIDClass", "AudioEndpoint", ...
    pub class: Option<String>,
    /// Enumerator: "USB", "HID", "BTHENUM", "BTHLE", "SWD", ...
    pub enumerator: Option<String>,
    /// Friendly name, else description, else display name.
    pub name: Option<String>,
    /// The name the device reports on its bus (a USB product string).
    pub bus_name: Option<String>,
    pub hardware_ids: Vec<String>,
    pub compatible_ids: Vec<String>,
    pub problem: Option<u32>,
    /// The Bluetooth battery property, 0–100.
    pub battery: Option<u8>,
}

/// A paired Bluetooth device as Settings lists it (an association endpoint).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawAep {
    pub container: Option<String>,
    pub name: Option<String>,
    pub battery: Option<u8>,
}

/// `DEVICE_STATE_ACTIVE`.
pub const AUDIO_ACTIVE: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawEndpoint {
    /// Core Audio endpoint id.
    pub id: String,
    pub name: String,
    pub output: bool,
    /// `DEVICE_STATE_*`.
    pub state: u32,
    /// `EndpointFormFactor`: 1 speakers, 3 headphones, 4 microphone, 5 headset, 6 handset.
    pub form_factor: Option<u32>,
    pub container: Option<String>,
    pub default: bool,
}

/// `BATTERY_TYPE_WIRED`.
pub const PAD_WIRED: u8 = 1;
/// `BATTERY_TYPE_DISCONNECTED`.
pub const PAD_DISCONNECTED: u8 = 0;

/// A connected XInput slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawPad {
    /// 0–3; shown as player 1–4.
    pub slot: u32,
    /// Vendor and product, where XInput tells them.
    pub ids: Option<(u16, u16)>,
    pub wireless: bool,
    /// `BATTERY_TYPE_*`.
    pub battery_type: u8,
    /// `BATTERY_LEVEL_*`: 0 empty … 3 full.
    pub battery_level: u8,
}

/// A battery read through a maker's own protocol, and behind a receiver the device itself.
#[derive(Debug, Clone, PartialEq)]
pub struct RawHid {
    /// The container of the receiver or device it was read through.
    pub container: String,
    /// `None`: that device itself; `Some(slot)`: a device paired to that receiver.
    pub slot: Option<u8>,
    pub name: Option<String>,
    pub kind: Option<DeviceKind>,
    /// Answered just now. False: it answered earlier this session and is now asleep or off.
    pub awake: bool,
    pub charge: Option<hid::Charge>,
    pub source: BatterySource,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Raw {
    pub containers: Vec<RawContainer>,
    pub nodes: Vec<RawNode>,
    /// Paired Bluetooth devices, for the names Settings shows.
    pub aeps: Vec<RawAep>,
    pub endpoints: Vec<RawEndpoint>,
    /// Battery Service readings by container id.
    pub gatt: Vec<(String, u8)>,
    /// Readings through makers' protocols.
    pub hid: Vec<RawHid>,
    pub pads: Vec<RawPad>,
}

// ---------------------------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------------------------

/// A container GUID in one form: lowercase, no braces.
pub fn norm_guid(s: &str) -> String {
    s.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_ascii_lowercase()
}

/// The id the UI sees for a container or endpoint: a short hash, so nothing derived from a
/// Bluetooth address or device path leaves the collector.
pub fn key(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(norm_guid(s).as_bytes());
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// Text Windows gave as a name. An indirect string it left unresolved
/// ("@input.inf,%hid.mouse%;HID-compliant mouse") gives its fallback after the last ';'.
pub fn plain_text(s: &str) -> Option<String> {
    let s = s.trim();
    let s = match s.strip_prefix('@') {
        Some(rest) => rest.rsplit_once(';')?.1.trim(),
        None => s,
    };
    (!s.is_empty()).then(|| s.to_owned())
}

/// Names Windows gives a function of a device, not the device: "HID-compliant mouse",
/// "USB Composite Device", a Bluetooth service.
fn generic(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "hid-compliant",
        "hid keyboard",
        "usb input device",
        "usb composite device",
        "usb audio device",
        "generic ",
        "bluetooth low energy gatt",
        "microsoft bluetooth",
        "bluetooth le generic",
        "avrcp transport",
        "device information service",
        "battery service",
    ]
    .iter()
    .any(|w| n.starts_with(w))
        || n.ends_with(" service")
        || n.ends_with(" profile")
}

/// The name Settings shows, as far as Windows gives one: the device's own (its container),
/// the paired Bluetooth entry, the product string on its bus, then the most specific function
/// name (a Bluetooth device's own node before its services).
fn known_name(c: &RawContainer, nodes: &[&RawNode], aep: Option<&RawAep>) -> Option<String> {
    let own = |s: Option<&String>| s.filter(|s| !s.trim().is_empty()).cloned();
    if let Some(n) = own(c.name.as_ref()).or_else(|| own(aep.and_then(|a| a.name.as_ref()))) {
        return Some(n);
    }
    let specific = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty() && !generic(s));
    nodes
        .iter()
        .find_map(|n| specific(&n.bus_name))
        .or_else(|| {
            nodes
                .iter()
                .filter(|n| matches!(enumerator(n).as_str(), "BTHLE" | "BTHENUM"))
                .find_map(|n| specific(&n.name))
        })
        .or_else(|| nodes.iter().find_map(|n| specific(&n.name)))
}

/// What a device without any name is called: its kind.
fn kind_label(kind: DeviceKind) -> &'static str {
    use DeviceKind as K;
    match kind {
        K::Keyboard => "Keyboard",
        K::Mouse => "Mouse",
        K::Input => "Keyboard and mouse",
        K::Controller => "Controller",
        K::Receiver => "Wireless receiver",
        K::Headset => "Headset",
        K::Headphones => "Headphones",
        K::Speakers => "Speakers",
        K::Microphone => "Microphone",
        K::Camera => "Camera",
        K::Printer => "Printer",
        K::Phone => "Phone",
        K::Storage => "Storage",
        K::Other => "Device",
    }
}

fn lower(s: Option<&String>) -> String {
    s.map(|s| s.to_ascii_lowercase()).unwrap_or_default()
}

fn enumerator(n: &RawNode) -> String {
    n.enumerator.as_deref().unwrap_or("").to_ascii_uppercase()
}

fn class(n: &RawNode) -> String {
    lower(n.class.as_ref())
}

/// Bluetooth LE nodes: the device and its GATT services.
fn is_ble(n: &RawNode) -> bool {
    matches!(enumerator(n).as_str(), "BTHLE" | "BTHLEDEVICE")
}

fn is_bluetooth(n: &RawNode) -> bool {
    is_ble(n) || matches!(enumerator(n).as_str(), "BTHENUM" | "BTHHFENUM")
}

/// Software-only nodes: a container of nothing else is a virtual device (Print to PDF, Fax).
fn is_software(n: &RawNode) -> bool {
    matches!(enumerator(n).as_str(), "ROOT" | "SWD" | "SW" | "HTREE" | "")
}

/// XInput-capable: an Xbox class node, the Xbox Wireless enumerator, or the "IG_" interface
/// mark XInput HID devices carry.
fn is_xinput(n: &RawNode) -> bool {
    matches!(
        class(n).as_str(),
        "xboxcomposite" | "xnacomposite" | "xboxgipdevice"
    ) || enumerator(n) == "XBOXGIP"
        || n.hardware_ids
            .iter()
            .any(|h| h.to_ascii_uppercase().contains("&IG_"))
}

fn has_word(name: &str, words: &[&str]) -> bool {
    words.iter().any(|w| name.contains(w))
}

/// What the device is. `None`: leave it out (a monitor, a USB hub).
fn classify(
    c: &RawContainer,
    name: &str,
    nodes: &[&RawNode],
    endpoints: &[&RawEndpoint],
) -> Option<DeviceKind> {
    use DeviceKind as K;
    let classes: HashSet<String> = nodes.iter().map(|n| class(n)).collect();
    let has = |k: &str| classes.contains(k);
    if has("monitor") || nodes.iter().any(|n| enumerator(n) == "DISPLAY") {
        return None;
    }
    let name = name.to_ascii_lowercase();

    if has_word(
        &name,
        &[
            "receiver",
            "dongle",
            "unifying",
            "lightspeed",
            "hyperspeed",
            "wireless adapter",
        ],
    ) {
        return Some(K::Receiver);
    }
    let game = nodes.iter().any(|n| {
        is_xinput(n)
            || n.compatible_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case("HID_DEVICE_SYSTEM_GAME"))
    });
    let pad_names = [
        "gamepad",
        "joystick",
        "game controller",
        "wireless controller",
        "xbox controller",
        "pro controller",
        "dualsense",
        "dualshock",
    ];
    if game || has_word(&name, &pad_names) {
        return Some(K::Controller);
    }
    // The product's own name next: gaming mice expose a keyboard too, and keyboards a mouse.
    for (words, kind) in [
        (
            &["mouse", "trackball", "trackpad", "touchpad"][..],
            K::Mouse,
        ),
        (&["keyboard", "keypad"][..], K::Keyboard),
        (&["headset"][..], K::Headset),
        (
            &["headphone", "earbud", "buds", "airpods"][..],
            K::Headphones,
        ),
        (&["speaker", "soundbar"][..], K::Speakers),
        (&["microphone"][..], K::Microphone),
        (&["webcam", "camera"][..], K::Camera),
        (&["printer"][..], K::Printer),
    ] {
        if has_word(&name, words) {
            return Some(kind);
        }
    }
    for cat in &c.categories {
        let kind = match cat.to_ascii_lowercase().as_str() {
            "input.keyboard" => K::Keyboard,
            "input.mouse" => K::Mouse,
            "input.gaming" => K::Controller,
            "audio.headset" => K::Headset,
            "audio.headphone" => K::Headphones,
            "audio.speaker" => K::Speakers,
            "audio.microphone" => K::Microphone,
            "imaging.webcam" | "multimedia.camera" => K::Camera,
            "printfax.printer" | "printfax.mfp" => K::Printer,
            "communication.phone" => K::Phone,
            _ => continue,
        };
        return Some(kind);
    }

    if !endpoints.is_empty() {
        let out = endpoints.iter().any(|e| e.output);
        let rec = endpoints.iter().any(|e| !e.output);
        let form = |f: u32| endpoints.iter().any(|e| e.form_factor == Some(f));
        return Some(if form(5) || form(6) || (out && rec) {
            K::Headset
        } else if form(3) {
            K::Headphones
        } else if out {
            K::Speakers
        } else {
            K::Microphone
        });
    }
    let (kbd, mouse) = (has("keyboard"), has("mouse"));
    if kbd || mouse {
        return Some(match (kbd, mouse) {
            (true, false) => K::Keyboard,
            (false, true) => K::Mouse,
            _ => K::Input,
        });
    }
    if has("camera") || has("image") {
        return Some(K::Camera);
    }
    if has("printer") || has("printqueue") {
        return Some(K::Printer);
    }
    if has("wpd") || has("androidusbdeviceclass") {
        return Some(K::Phone);
    }
    if has("diskdrive") || nodes.iter().any(|n| enumerator(n) == "USBSTOR") {
        return Some(K::Storage);
    }
    let hub = |n: &&RawNode| class(n) == "usb" && lower(n.name.as_ref()).contains("hub");
    if !nodes.is_empty() && nodes.iter().all(hub) {
        return None;
    }
    Some(K::Other)
}

fn connection(c: &RawContainer, nodes: &[&RawNode]) -> Connection {
    if nodes.iter().any(|n| is_ble(n)) {
        Connection::BluetoothLe
    } else if nodes.iter().any(|n| is_bluetooth(n)) {
        Connection::Bluetooth
    } else if nodes
        .iter()
        .any(|n| matches!(enumerator(n).as_str(), "USB" | "USBSTOR" | "USBPRINT"))
    {
        Connection::Usb
    } else if c.network {
        Connection::Network
    } else {
        Connection::Other
    }
}

/// A manufacturer worth showing: not a placeholder like "(Standard keyboards)".
fn maker(s: Option<&String>) -> Option<String> {
    let s = s?.trim();
    (!s.is_empty() && !s.starts_with('(')).then(|| s.to_owned())
}

/// Windows' own battery reading: on a node, the container or the paired entry.
fn windows_battery(c: &RawContainer, nodes: &[&RawNode], aep: Option<&RawAep>) -> Option<u8> {
    nodes
        .iter()
        .filter_map(|n| n.battery)
        .chain(c.battery)
        .chain(aep.and_then(|a| a.battery))
        .find(|b| *b <= 100)
}

/// Bluetooth LE containers worth a Battery Service read: connected, and Windows has no
/// battery reading of its own for them.
pub fn gatt_wanted(raw: &Raw) -> Vec<String> {
    let groups = group(&raw.nodes);
    let aeps = aeps(&raw.aeps);
    raw.containers
        .iter()
        .filter(|c| c.connected != Some(false) && !c.local_machine)
        .filter_map(|c| {
            let id = norm_guid(&c.id);
            let nodes = groups.get(&id)?;
            let windows = windows_battery(c, nodes, aeps.get(&id).copied());
            (nodes.iter().any(|n| is_ble(n)) && windows.is_none()).then_some(id)
        })
        .collect()
}

/// Paired entries by container.
fn aeps(list: &[RawAep]) -> HashMap<String, &RawAep> {
    list.iter()
        .filter_map(|a| Some((norm_guid(a.container.as_deref()?), a)))
        .collect()
}

fn group(nodes: &[RawNode]) -> HashMap<String, Vec<&RawNode>> {
    let mut groups: HashMap<String, Vec<&RawNode>> = HashMap::new();
    for n in nodes {
        if let Some(c) = &n.container {
            groups.entry(norm_guid(c)).or_default().push(n);
        }
    }
    groups
}

fn pad_battery(p: &RawPad) -> Option<Battery> {
    if p.battery_type == PAD_WIRED || p.battery_type == PAD_DISCONNECTED {
        return None;
    }
    let level = match p.battery_level {
        0 => BatteryLevel::Empty,
        1 => BatteryLevel::Low,
        2 => BatteryLevel::Medium,
        _ => BatteryLevel::Full,
    };
    Some(Battery {
        percent: None,
        level: Some(level),
        charging: None,
        source: BatterySource::Xinput,
    })
}

fn charge_battery(c: Option<hid::Charge>, source: BatterySource) -> Option<Battery> {
    let c = c?;
    let percent = c.percent.filter(|p| *p <= 100);
    (percent.is_some() || c.level.is_some()).then_some(Battery {
        percent,
        level: if percent.is_some() { None } else { c.level },
        charging: c.charging,
        source,
    })
}

/// A controller's vendor and product from its hardware ids: "VID_045E&PID_0B12" (USB and HID)
/// or "VID&02045e_PID&0b13" (Bluetooth, with a two-digit source before the vendor).
fn vid_pid(ids: &[String]) -> Option<(u16, u16)> {
    let hex = |s: &str| u16::from_str_radix(s.get(..4)?, 16).ok();
    ids.iter().find_map(|id| {
        let id = id.to_ascii_uppercase();
        let vid = if let Some(i) = id.find("VID_") {
            hex(&id[i + 4..])?
        } else {
            hex(id.get(id.find("VID&")? + 6..)?)?
        };
        let at = id.find("PID_").or_else(|| id.find("PID&"))?;
        Some((vid, hex(&id[at + 4..])?))
    })
}

/// The Devices tab's list.
pub fn build(raw: &Raw) -> Devices {
    let groups = group(&raw.nodes);
    let aeps = aeps(&raw.aeps);
    let direct: HashMap<String, &RawHid> = raw
        .hid
        .iter()
        .filter(|h| h.slot.is_none() && h.awake)
        .map(|h| (norm_guid(&h.container), h))
        .collect();
    let gatt: HashMap<String, u8> = raw
        .gatt
        .iter()
        .filter(|(_, v)| *v <= 100)
        .map(|(c, v)| (norm_guid(c), *v))
        .collect();
    let mut by_container: HashMap<String, Vec<&RawEndpoint>> = HashMap::new();
    for e in &raw.endpoints {
        if let Some(c) = &e.container {
            by_container.entry(norm_guid(c)).or_default().push(e);
        }
    }

    let mut devices = Vec::new();
    let mut xinput = Vec::new();
    let mut listed = HashSet::new();
    let mut seen = HashSet::new();
    for c in &raw.containers {
        let id = norm_guid(&c.id);
        if c.local_machine || c.hidden || !seen.insert(id.clone()) {
            continue;
        }
        let Some(nodes) = groups.get(&id) else {
            continue;
        };
        if nodes.iter().all(|n| is_software(n)) {
            continue;
        }
        let endpoints = by_container.get(&id).map_or(&[][..], Vec::as_slice);
        let aep = aeps.get(&id).copied();
        let known = known_name(c, nodes, aep);
        let Some(kind) = classify(c, known.as_deref().unwrap_or(""), nodes, endpoints) else {
            continue;
        };
        let conn = connection(c, nodes);
        let bluetooth = matches!(conn, Connection::Bluetooth | Connection::BluetoothLe);
        let connected = !bluetooth || c.connected != Some(false);
        let name = known.unwrap_or_else(|| kind_label(kind).into());
        let battery = connected
            .then(|| {
                windows_battery(c, nodes, aep)
                    .map(|p| (p, BatterySource::Windows))
                    .or_else(|| gatt.get(&id).map(|p| (*p, BatterySource::BluetoothLe)))
                    .map(|(p, source)| Battery {
                        percent: Some(p),
                        level: None,
                        charging: None,
                        source,
                    })
                    .or_else(|| {
                        let h = direct.get(&id)?;
                        charge_battery(h.charge, h.source)
                    })
            })
            .flatten();
        if connected && nodes.iter().any(|n| is_xinput(n)) {
            let ids: Vec<String> = nodes
                .iter()
                .filter(|n| is_xinput(n))
                .flat_map(|n| n.hardware_ids.iter().cloned())
                .collect();
            xinput.push((devices.len(), vid_pid(&ids)));
        }
        let problem = connected
            .then(|| nodes.iter().filter_map(|n| n.problem).find(|p| *p != 0))
            .flatten();
        let lname = name.to_ascii_lowercase();
        devices.push(DeviceInfo {
            id: key(&id),
            wireless: bluetooth || kind == DeviceKind::Receiver || lname.contains("wireless"),
            name,
            kind,
            manufacturer: maker(c.manufacturer.as_ref()),
            connection: conn,
            connected,
            battery,
            problem,
            parent: None,
        });
        listed.insert(id);
    }

    // Devices paired to a receiver: each its own row, named as the device names itself.
    for h in &raw.hid {
        let (Some(slot), container) = (h.slot, norm_guid(&h.container)) else {
            continue;
        };
        if !listed.contains(&container) {
            continue;
        }
        let receiver = key(&container);
        let maker = devices
            .iter()
            .find(|d| d.id == receiver)
            .and_then(|d| d.manufacturer.clone());
        let kind = h.kind.unwrap_or(DeviceKind::Other);
        devices.push(DeviceInfo {
            id: key(&format!("{container}#{slot}")),
            name: h.name.clone().unwrap_or_else(|| kind_label(kind).into()),
            kind,
            manufacturer: maker,
            connection: Connection::Receiver,
            wireless: true,
            connected: h.awake,
            battery: if h.awake {
                charge_battery(h.charge, h.source)
            } else {
                None
            },
            problem: None,
            parent: Some(receiver),
        });
    }

    // XInput names no device for a slot. Its vendor and product match one controller where only
    // one has them; then one controller left and one slot left match; any other slot is listed
    // on its own.
    let pads: Vec<&RawPad> = raw
        .pads
        .iter()
        .filter(|p| p.battery_type != PAD_DISCONNECTED)
        .collect();
    let mut taken = HashSet::new();
    let mut left = Vec::new();
    for p in pads {
        let same: Vec<usize> = xinput
            .iter()
            .filter(|(i, ids)| p.ids.is_some() && *ids == p.ids && !taken.contains(i))
            .map(|(i, _)| *i)
            .collect();
        if let [i] = same.as_slice() {
            taken.insert(*i);
            if devices[*i].battery.is_none() {
                devices[*i].battery = pad_battery(p);
            }
        } else {
            left.push(p);
        }
    }
    let free: Vec<usize> = xinput
        .iter()
        .map(|(i, _)| *i)
        .filter(|i| !taken.contains(i))
        .collect();
    if let ([i], [p]) = (free.as_slice(), left.as_slice()) {
        if devices[*i].battery.is_none() {
            devices[*i].battery = pad_battery(p);
        }
    } else {
        for p in left {
            devices.push(DeviceInfo {
                id: format!("xinput-{}", p.slot),
                name: format!("Controller · player {}", p.slot + 1),
                kind: DeviceKind::Controller,
                manufacturer: None,
                connection: if p.wireless {
                    Connection::Other
                } else {
                    Connection::Usb
                },
                wireless: p.wireless,
                connected: true,
                battery: pad_battery(p),
                problem: None,
                parent: None,
            });
        }
    }

    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(a.kind.cmp(&b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let mut audio: Vec<AudioEndpoint> = raw
        .endpoints
        .iter()
        .map(|e| AudioEndpoint {
            id: key(&e.id),
            name: e.name.clone(),
            output: e.output,
            plugged: e.state == AUDIO_ACTIVE,
            default: e.default && e.state == AUDIO_ACTIVE,
            device: e
                .container
                .as_deref()
                .map(norm_guid)
                .filter(|c| listed.contains(c))
                .map(|c| key(&c)),
        })
        .collect();
    audio.sort_by(|a, b| {
        b.output
            .cmp(&a.output)
            .then(b.default.cmp(&a.default))
            .then(b.plugged.cmp(&a.plugged))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Devices { devices, audio }
}

/// What the browser preview and non-Windows builds show.
pub fn sample() -> Devices {
    build(&sample_raw())
}

fn sample_raw() -> Raw {
    let c = |id: &str, name: &str, maker: &str, connected: Option<bool>| RawContainer {
        id: id.into(),
        name: Some(name.into()),
        manufacturer: Some(maker.into()),
        connected,
        ..RawContainer::default()
    };
    let n = |container: &str, class: &str, en: &str| RawNode {
        container: Some(container.into()),
        class: Some(class.into()),
        enumerator: Some(en.into()),
        ..RawNode::default()
    };
    let e =
        |id: &str, name: &str, output: bool, container: Option<&str>, state: u32, default: bool| {
            RawEndpoint {
                id: id.into(),
                name: name.into(),
                output,
                state,
                form_factor: None,
                container: container.map(Into::into),
                default,
            }
        };
    let charge = |percent: u8, charging: bool| {
        Some(hid::Charge {
            percent: Some(percent),
            level: None,
            charging: Some(charging),
        })
    };
    let mut buds = n("c4", "Bluetooth", "BTHENUM");
    buds.battery = Some(64);
    let mut pad = n("c5", "HIDClass", "HID");
    pad.hardware_ids =
        vec!["HID\\{00001812-0000-1000-8000-00805f9b34fb}_VID&02045e_PID&0b13&IG_00".into()];
    let mut keys = c("c6", "MX Keys", "Logitech", Some(false));
    keys.categories = vec!["Input.Keyboard".into()];
    // A USB microphone whose container Windows leaves unnamed: its bus name is used.
    let mic = RawContainer {
        id: "c11".into(),
        manufacturer: Some("(Generic USB Audio)".into()),
        connected: Some(true),
        ..RawContainer::default()
    };
    let mut mic_usb = n("c11", "USB", "USB");
    mic_usb.bus_name = Some("USB Audio Condenser Mic".into());
    mic_usb.name = Some("USB Composite Device".into());
    Raw {
        containers: vec![
            c("c1", "G502 HERO Gaming Mouse", "Logitech", Some(true)),
            c("c2", "BlackWidow V3", "Razer", Some(true)),
            c("c3", "Arctis Nova 7", "SteelSeries", Some(true)),
            c("c4", "Galaxy Buds2 Pro", "Samsung", Some(true)),
            c("c5", "Xbox Wireless Controller", "Microsoft", Some(true)),
            keys,
            c("c7", "USB Receiver", "Logitech", Some(true)),
            c("c8", "MX Master 3S", "Logitech", Some(true)),
            c(
                "c9",
                "AURA LED Controller",
                "(Standard USB Host Controller)",
                Some(true),
            ),
            c("c10", "DualSense Wireless Controller", "Sony", Some(true)),
            mic,
        ],
        nodes: vec![
            n("c1", "Mouse", "HID"),
            n("c1", "Keyboard", "HID"),
            n("c1", "USB", "USB"),
            n("c2", "Keyboard", "HID"),
            n("c2", "USB", "USB"),
            n("c3", "AudioEndpoint", "SWD"),
            n("c3", "USB", "USB"),
            buds,
            n("c4", "AudioEndpoint", "SWD"),
            pad,
            n("c5", "Bluetooth", "BTHLE"),
            n("c6", "Bluetooth", "BTHLE"),
            n("c7", "USB", "USB"),
            n("c7", "Keyboard", "HID"),
            n("c7", "Mouse", "HID"),
            n("c8", "Mouse", "HID"),
            n("c8", "Bluetooth", "BTHLE"),
            n("c9", "HIDClass", "HID"),
            n("c9", "USB", "USB"),
            n("c10", "HIDClass", "HID"),
            n("c10", "USB", "USB"),
            mic_usb,
            n("c11", "AudioEndpoint", "SWD"),
        ],
        aeps: Vec::new(),
        endpoints: vec![
            e(
                "e1",
                "Headphones (Arctis Nova 7)",
                true,
                Some("c3"),
                AUDIO_ACTIVE,
                true,
            ),
            e(
                "e2",
                "Microphone (Arctis Nova 7)",
                false,
                Some("c3"),
                AUDIO_ACTIVE,
                false,
            ),
            e(
                "e3",
                "Headphones (Galaxy Buds2 Pro)",
                true,
                Some("c4"),
                AUDIO_ACTIVE,
                false,
            ),
            e(
                "e4",
                "Speakers (Realtek(R) Audio)",
                true,
                None,
                AUDIO_ACTIVE,
                false,
            ),
            e(
                "e5",
                "Front headphones (Realtek(R) Audio)",
                true,
                None,
                8,
                false,
            ),
            e(
                "e6",
                "Microphone (USB Audio Condenser Mic)",
                false,
                Some("c11"),
                AUDIO_ACTIVE,
                true,
            ),
        ],
        gatt: vec![("c8".into(), 17)],
        hid: vec![
            RawHid {
                container: "c7".into(),
                slot: Some(1),
                name: Some("G Pro X Superlight 2".into()),
                kind: Some(DeviceKind::Mouse),
                awake: true,
                charge: charge(82, false),
                source: BatterySource::Logitech,
            },
            RawHid {
                container: "c7".into(),
                slot: Some(2),
                name: Some("G915 TKL".into()),
                kind: Some(DeviceKind::Keyboard),
                awake: true,
                charge: charge(46, true),
                source: BatterySource::Logitech,
            },
            RawHid {
                container: "c10".into(),
                slot: None,
                name: None,
                kind: None,
                awake: true,
                charge: charge(75, true),
                source: BatterySource::PlayStation,
            },
        ],
        pads: vec![RawPad {
            slot: 0,
            ids: Some((0x045E, 0x0B13)),
            wireless: true,
            battery_type: 3,
            battery_level: 2,
        }],
    }
}

#[cfg(test)]
mod tests;

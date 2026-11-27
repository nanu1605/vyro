//! Ambient lighting (2.6): the colour glowing most on the screen ([`colour`]) on the keyboard,
//! mouse and other RGB devices, through OpenRGB ([`openrgb`]). The app captures the screen and
//! drives the lights; nothing of the screen leaves the PC, and it is kept only as one colour.
//!
//! What may be written (docs/control-safety.md, "Lighting through OpenRGB"): only devices the
//! owner picks, only in their "Direct" mode (colours sent, nothing saved to the device), at most
//! 30 times a second, and devices on the memory bus (SMBus/I2C, shared with Clokrix's own
//! readings) at most 5 times a second. Each device's mode is noted before the first colour and put
//! back when lighting stops, Clokrix closes, or at the next start after a crash.

pub mod colour;
pub mod openrgb;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use colour::{LightSpeed, NoColour, Rgb8};
use openrgb::Controller;

/// How lighting is set (`lighting.json` in the app's folder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LightingSettings {
    pub on: bool,
    /// The monitor followed (0: the primary).
    pub monitor: u32,
    /// The devices picked, by [`device_id`]; none until the owner picks.
    pub devices: Vec<String>,
    pub speed: LightSpeed,
    /// 10 to 100: the lights at their brightest.
    pub brightness: u8,
    pub no_colour: NoColour,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            on: false,
            monitor: 0,
            devices: Vec::new(),
            speed: LightSpeed::Auto,
            brightness: 100,
            no_colour: NoColour::White,
        }
    }
}

impl LightingSettings {
    /// Within its bounds, whatever the file or the window said.
    pub fn clamped(mut self) -> Self {
        self.brightness = self.brightness.clamp(10, 100);
        self.monitor = self.monitor.min(15);
        self.devices.truncate(256);
        self
    }
}

/// A device's mode before Clokrix chose Direct, to put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedMode {
    /// [`device_id`].
    pub device: String,
    pub mode: i32,
    pub name: String,
    /// The mode's data as the server gave it, in hex, under protocol `version`.
    pub raw: String,
    pub version: u32,
}

/// What OpenRGB is to Clokrix now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LightLink {
    /// Lighting is off.
    #[default]
    Off,
    /// OpenRGB does not answer: not running, or its SDK server is off.
    NotRunning,
    Connected,
}

/// A device as the Lighting view lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LightDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub leds: u32,
    /// It has a Direct mode; without one it is not driven.
    pub direct: bool,
    /// On the memory bus: colours at most 5 times a second.
    pub slow: bool,
    pub picked: bool,
}

/// What the Lighting view shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LightingStatus {
    pub link: LightLink,
    pub devices: Vec<LightDevice>,
    /// The colour shown now.
    pub colour: Option<Rgb8>,
    /// How the screen is taken ("Desktop Duplication", "GDI", or "with the phone's screen
    /// session").
    pub capture: Option<String>,
    /// Why the lights hold their colour now, if they do.
    pub held: Option<String>,
}

/// A device's lasting name: a hash of its name, maker and where it is, so the same device keeps
/// its choice when OpenRGB numbers its devices anew, and where it is (which can hold a serial
/// number) is never kept.
pub fn device_id(c: &Controller) -> String {
    let d = Sha256::digest(format!(
        "clokrix-light-1:{}\0{}\0{}",
        c.name, c.vendor, c.location
    ));
    d[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// On the memory bus (OpenRGB names it "I2C: ..."): memory modules, many boards and some cards.
pub fn on_memory_bus(c: &Controller) -> bool {
    c.location
        .get(..3)
        .is_some_and(|l| l.eq_ignore_ascii_case("i2c"))
}

/// How often a device takes a colour at most.
pub fn interval(c: &Controller) -> Duration {
    if on_memory_bus(c) {
        Duration::from_millis(200)
    } else {
        Duration::from_millis(33)
    }
}

/// OpenRGB's kinds of device, in words.
pub fn kind_name(kind: i32) -> &'static str {
    match kind {
        0 => "Motherboard",
        1 => "Memory",
        2 => "Graphics card",
        3 => "Cooler",
        4 => "LED strip",
        5 => "Keyboard",
        6 => "Mouse",
        7 => "Mouse mat",
        8 => "Headset",
        9 => "Headset stand",
        10 => "Gamepad",
        11 => "Light",
        12 => "Speaker",
        15 => "Case",
        16 => "Microphone",
        18 => "Keypad",
        _ => "Device",
    }
}

pub fn info(c: &Controller, picked: &[String]) -> LightDevice {
    let id = device_id(c);
    LightDevice {
        picked: picked.contains(&id),
        id,
        name: c.name.clone(),
        kind: kind_name(c.kind).to_owned(),
        leds: c.leds as u32,
        direct: c.direct_mode().is_some(),
        slow: on_memory_bus(c),
    }
}

/// `c`'s mode now, to put back later.
pub fn saved_mode(c: &Controller, version: u32) -> Option<SavedMode> {
    let i = usize::try_from(c.active_mode).ok()?;
    let m = c.modes.get(i)?;
    Some(SavedMode {
        device: device_id(c),
        mode: c.active_mode,
        name: m.name.clone(),
        raw: m.raw.iter().map(|b| format!("{b:02x}")).collect(),
        version,
    })
}

/// The mode to put `c` back in, from what was saved: as saved when the protocol is the same,
/// else the mode of that name as `c` describes it now. Never Direct: that is Clokrix's own.
pub fn restore_mode(c: &Controller, saved: &SavedMode, version: u32) -> Option<(i32, Vec<u8>)> {
    if saved.name.eq_ignore_ascii_case("direct") {
        return None;
    }
    if saved.version == version {
        let raw = (0..saved.raw.len() / 2)
            .map(|i| u8::from_str_radix(saved.raw.get(i * 2..i * 2 + 2)?, 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        if c.modes.get(usize::try_from(saved.mode).ok()?)?.name == saved.name {
            return Some((saved.mode, raw));
        }
    }
    let i = c.modes.iter().position(|m| m.name == saved.name)?;
    Some((i as i32, c.modes[i].raw.clone()))
}

/// The settings as the picker takes them.
pub fn pick_settings(s: &LightingSettings, game: bool, accent: Rgb8) -> colour::PickSettings {
    colour::PickSettings {
        seconds: s.speed.seconds(game),
        brightness: f32::from(s.brightness.clamp(10, 100)) / 100.0,
        no_colour: s.no_colour,
        accent,
    }
}

#[cfg(test)]
mod tests {
    use super::openrgb::{parse_controller, tests::keyboard, VERSION};
    use super::*;

    fn device(location: &str) -> Controller {
        parse_controller(&keyboard(VERSION, 4, location), 0, VERSION).unwrap()
    }

    #[test]
    fn names_a_device_lastingly_without_keeping_where_it_is() {
        let a = device("HID: \\\\?\\hid#vid_1234&pid_5678#SERIAL123");
        let mut moved = a.clone();
        moved.index = 3;
        assert_eq!(device_id(&a), device_id(&moved));
        assert_ne!(device_id(&a), device_id(&device("HID: elsewhere")));
        assert_eq!(device_id(&a).len(), 16);
        let i = info(&a, &[device_id(&a)]);
        assert!(i.picked && i.direct && !i.slow);
        assert_eq!((i.kind.as_str(), i.leds), ("Keyboard", 4));
        // Nothing of where it is goes to the view or the settings.
        let shown = serde_json::to_string(&i).unwrap();
        assert!(!shown.contains("SERIAL") && !shown.contains("hid#"));
    }

    #[test]
    fn devices_on_the_memory_bus_take_colours_slowly() {
        let ram = device("I2C: PIIX4, address 0x58");
        assert!(on_memory_bus(&ram));
        assert_eq!(interval(&ram), Duration::from_millis(200));
        let usb = device("USB: 1-2");
        assert!(!on_memory_bus(&usb));
        assert!(interval(&usb) <= Duration::from_millis(34));
        assert!(!on_memory_bus(&device("")));
    }

    #[test]
    fn puts_a_device_back_in_the_mode_it_was_in() {
        let k = device("USB: 1-2");
        let saved = saved_mode(&k, VERSION).unwrap();
        assert_eq!((saved.mode, saved.name.as_str()), (0, "Static"));
        assert_eq!(
            restore_mode(&k, &saved, VERSION),
            Some((0, k.modes[0].raw.clone()))
        );
        // Under another protocol: the mode of that name as described now.
        let older = parse_controller(&keyboard(3, 4, "USB: 1-2"), 0, 3).unwrap();
        assert_eq!(
            restore_mode(&older, &saved, 3),
            Some((0, older.modes[0].raw.clone()))
        );
        // Never into Direct (Clokrix's own), and nothing from a damaged note.
        let mut direct = saved.clone();
        direct.name = "Direct".into();
        assert_eq!(restore_mode(&k, &direct, VERSION), None);
        let mut bad = saved.clone();
        bad.raw = "zz".into();
        bad.name = "Gone".into();
        assert_eq!(restore_mode(&k, &bad, VERSION), None);
        // A saved note round-trips through the settings file.
        let json = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<SavedMode>(&json).unwrap(), saved);
    }

    #[test]
    fn settings_stay_within_bounds() {
        let s: LightingSettings =
            serde_json::from_str(r#"{"on":true,"brightness":0,"monitor":99}"#).unwrap();
        let s = s.clamped();
        assert!(s.on);
        assert_eq!(
            (s.brightness, s.monitor, s.speed),
            (10, 15, LightSpeed::Auto)
        );
        let p = pick_settings(&LightingSettings::default(), true, Rgb8::default());
        assert_eq!((p.seconds, p.brightness), (0.1, 1.0));
    }
}

//! Batteries only the maker's own protocol reports (1.3): Logitech HID++ 2.0, through a receiver
//! or directly; Sony's controllers' input reports; and the standard HID Battery Strength value.
//! Parsing is pure and tested here; `hid_win.rs` sends and reads the reports.

use super::{BatteryLevel, DeviceKind};

pub const LOGITECH: u16 = 0x046D;
pub const SONY: u16 = 0x054C;

/// HID++ long report: id, device index, feature index, function and software id, 16 parameters.
pub const LONG: u8 = 0x11;
pub const SHORT: u8 = 0x10;
pub const LONG_LEN: usize = 20;
/// Our software id, so replies are told apart from the device's own notifications.
pub const SW_ID: u8 = 0x0A;
/// A device connected directly (USB cable or Bluetooth), not through a receiver.
pub const DIRECT: u8 = 0xFF;
/// The slots a receiver pairs devices to.
pub const SLOTS: std::ops::RangeInclusive<u8> = 1..=6;

/// HID++ 2.0 feature ids.
pub mod feature {
    pub const ROOT: u16 = 0x0000;
    pub const DEVICE_NAME: u16 = 0x0005;
    pub const BATTERY_STATUS: u16 = 0x1000;
    pub const BATTERY_VOLTAGE: u16 = 0x1001;
    pub const UNIFIED_BATTERY: u16 = 0x1004;
}

/// A battery reading, however the device gave it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Charge {
    pub percent: Option<u8>,
    pub level: Option<BatteryLevel>,
    /// On a charger and charging; `None` where the device does not say.
    pub charging: Option<bool>,
}

/// A HID++ request as a long report.
pub fn request(device: u8, index: u8, function: u8, params: &[u8]) -> [u8; LONG_LEN] {
    let mut r = [0u8; LONG_LEN];
    r[0] = LONG;
    r[1] = device;
    r[2] = index;
    r[3] = (function << 4) | SW_ID;
    for (d, s) in r[4..].iter_mut().zip(params) {
        *d = *s;
    }
    r
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reply<'a> {
    /// The reply's parameters.
    Ok(&'a [u8]),
    /// An error code: HID++ 2.0's, or 1.0's from a receiver about a device it cannot reach.
    Error(u8),
    /// A notification, or the reply to something else.
    Other,
}

/// What a report read back means for `req`.
pub fn reply<'a>(req: &[u8; LONG_LEN], got: &'a [u8]) -> Reply<'a> {
    if got.len() < 7 || !matches!(got[0], SHORT | LONG) || got[1] != req[1] {
        return Reply::Other;
    }
    // Errors echo the request's feature index and function: 0xFF in HID++ 2.0, 0x8F in 1.0.
    if matches!(got[2], 0xFF | 0x8F) && got[3] == req[2] && got[4] == req[3] {
        return Reply::Error(got[5]);
    }
    if got[2] == req[2] && got[3] == req[3] {
        return Reply::Ok(&got[4..]);
    }
    Reply::Other
}

/// IRoot.getProtocolVersion's reply: HID++ 2.0 or later.
pub fn is_hidpp2(p: &[u8]) -> bool {
    p.first().is_some_and(|major| *major >= 2)
}

/// 0x1004 Unified Battery, getStatus: state of charge, level flags, charging status.
pub fn unified(p: &[u8]) -> Option<Charge> {
    let (soc, flags, status) = (*p.first()?, *p.get(1)?, *p.get(2)?);
    let level = if flags & 8 != 0 {
        Some(BatteryLevel::Full)
    } else if flags & 4 != 0 {
        Some(BatteryLevel::Medium)
    } else if flags & 2 != 0 {
        Some(BatteryLevel::Low)
    } else if flags & 1 != 0 {
        Some(BatteryLevel::Empty)
    } else {
        None
    };
    // 4 is a charging error: the reading is not to be trusted.
    if status > 3 {
        return None;
    }
    let percent = (1..=100).contains(&soc).then_some(soc);
    (percent.is_some() || level.is_some()).then_some(Charge {
        percent,
        level: if percent.is_some() { None } else { level },
        charging: Some(matches!(status, 1 | 2)),
    })
}

/// 0x1000 Battery Status, getBatteryLevelStatus: discharge level, next level, status.
pub fn status(p: &[u8]) -> Option<Charge> {
    let (pct, st) = (*p.first()?, *p.get(2)?);
    // 5 and 6: invalid battery, thermal error.
    if st >= 5 {
        return None;
    }
    let percent = (1..=100)
        .contains(&pct)
        .then_some(pct)
        .or((st == 3).then_some(100))?;
    Some(Charge {
        percent: Some(percent),
        level: None,
        charging: Some(matches!(st, 1 | 2 | 4)),
    })
}

/// 0x1001 Battery Voltage, getBatteryInfo: millivolts and flags (bit 7: on a charger; then the
/// low bits: 0 charging, 1 full).
pub fn voltage(p: &[u8]) -> Option<Charge> {
    let mv = u16::from_be_bytes([*p.first()?, *p.get(1)?]);
    let flags = *p.get(2)?;
    if mv == 0 {
        return None;
    }
    let plugged = flags & 0x80 != 0;
    let full = plugged && flags & 0x07 == 1;
    Some(Charge {
        percent: Some(if full { 100 } else { percent_from_mv(mv) }),
        level: None,
        charging: Some(plugged && flags & 0x07 == 0),
    })
}

/// A Logitech lithium cell's charge from its voltage (Solaar's table, millivolts to percent,
/// interpolated between rows).
const CURVE: [(u16, u8); 13] = [
    (4186, 100),
    (4067, 90),
    (3989, 80),
    (3922, 70),
    (3859, 60),
    (3811, 50),
    (3778, 40),
    (3751, 30),
    (3717, 20),
    (3671, 10),
    (3646, 5),
    (3579, 2),
    (3500, 0),
];

pub fn percent_from_mv(mv: u16) -> u8 {
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((hi_mv, hi), (lo_mv, lo)) = (w[0], w[1]);
        if mv >= lo_mv {
            let span = u32::from(hi_mv - lo_mv);
            let up = u32::from(mv - lo_mv) * u32::from(hi - lo);
            return lo + u8::try_from((up + span / 2) / span).unwrap_or(0);
        }
    }
    0
}

/// 0x0005 Device Name's parts (getDeviceName gives up to 16 characters a call) joined.
pub fn name(parts: &[Vec<u8>], len: usize) -> Option<String> {
    let bytes: Vec<u8> = parts
        .iter()
        .flatten()
        .copied()
        .take(len)
        .take_while(|b| *b != 0)
        .collect();
    let s = String::from_utf8_lossy(&bytes).trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// 0x0005 Device Name, getDeviceType.
pub fn device_kind(t: u8) -> DeviceKind {
    use DeviceKind as K;
    match t {
        0 | 2 => K::Keyboard,
        3..=5 => K::Mouse,
        8 => K::Headset,
        9 => K::Camera,
        10..=12 => K::Controller,
        14 => K::Speakers,
        15 => K::Microphone,
        _ => K::Other,
    }
}

// ---------------------------------------------------------------------------------------------
// Sony controllers
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pad {
    DualSense,
    DualShock4,
    /// The DualShock 4 USB wireless adapter: it reports even with no controller connected.
    DualShock4Dongle,
}

pub fn sony_pad(product: u16) -> Option<Pad> {
    match product {
        0x0CE6 | 0x0DF2 => Some(Pad::DualSense),
        0x05C4 | 0x09CC => Some(Pad::DualShock4),
        0x0BA0 => Some(Pad::DualShock4Dongle),
        _ => None,
    }
}

/// The battery in an input report. Over USB the controller sends its full report (id 0x01);
/// over Bluetooth only once something has switched it to the full report (0x31 or 0x11), which
/// Vyro never does itself: it breaks some games' input until the controller reconnects.
pub fn pad_charge(pad: Pad, r: &[u8]) -> Option<Charge> {
    match pad {
        Pad::DualSense => {
            let s = match (*r.first()?, r.len()) {
                (0x01, 54..) => r[53],
                (0x31, 55..) => r[54],
                _ => return None,
            };
            let level = s & 0x0F;
            let pct = (level * 10 + 5).min(100);
            match s >> 4 {
                0 => Some((pct, false)),
                1 => Some((pct, true)),
                2 => Some((100, false)),
                _ => None,
            }
        }
        Pad::DualShock4 | Pad::DualShock4Dongle => {
            let at = match (*r.first()?, r.len()) {
                (0x01, 32..) => 30,
                (0x11, 34..) => 32,
                _ => return None,
            };
            if pad == Pad::DualShock4Dongle && r[at + 1] & 0x04 != 0 {
                return None;
            }
            let (s, cable) = (r[at] & 0x0F, r[at] & 0x10 != 0);
            match (cable, s) {
                (true, 0..=9) => Some((s * 10 + 5, true)),
                (true, 10) => Some((100, true)),
                (true, 11) => Some((100, false)),
                (true, _) => None,
                (false, 0..=9) => Some((s * 10 + 5, false)),
                (false, _) => Some((100, false)),
            }
        }
    }
    .map(|(pct, charging)| Charge {
        percent: Some(pct),
        level: None,
        charging: Some(charging),
    })
}

// ---------------------------------------------------------------------------------------------
// Standard HID battery
// ---------------------------------------------------------------------------------------------

/// Generic Device Controls page, Battery Strength usage.
pub const BATTERY_PAGE: u16 = 0x06;
pub const BATTERY_STRENGTH: u16 = 0x20;

/// A Battery Strength value as a percentage of its logical range.
pub fn strength(value: i64, min: i64, max: i64) -> Option<u8> {
    if max <= min || value < min || value > max {
        return None;
    }
    u8::try_from((value - min) * 100 / (max - min)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(device: u8, index: u8, fn_sw: u8, params: &[u8]) -> Vec<u8> {
        let mut r = vec![LONG, device, index, fn_sw];
        r.extend_from_slice(params);
        r.resize(LONG_LEN, 0);
        r
    }

    #[test]
    fn requests_are_long_reports_with_our_software_id() {
        let r = request(2, 0x00, 1, &[0, 0, 0xAA]);
        assert_eq!(r.len(), 20);
        assert_eq!(&r[..7], &[0x11, 2, 0x00, 0x1A, 0, 0, 0xAA]);
        assert!(r[7..].iter().all(|b| *b == 0));
        // getFeature(0x1004)
        let r = request(DIRECT, 0x00, 0, &[0x10, 0x04]);
        assert_eq!(&r[..6], &[0x11, 0xFF, 0x00, 0x0A, 0x10, 0x04]);
    }

    #[test]
    fn replies_errors_and_notifications_are_told_apart() {
        let req = request(1, 0x05, 1, &[]);
        assert_eq!(
            reply(&req, &long(1, 0x05, 0x1A, &[57, 4, 1])),
            Reply::Ok(&long(1, 0x05, 0x1A, &[57, 4, 1])[4..])
        );
        // HID++ 2.0 error, and a receiver's 1.0 error about a device it cannot reach (short).
        assert_eq!(
            reply(&req, &long(1, 0xFF, 0x05, &[0x1A, 0x05])),
            Reply::Error(0x05)
        );
        assert_eq!(
            reply(&req, &[0x10, 1, 0x8F, 0x05, 0x1A, 0x09, 0]),
            Reply::Error(0x09)
        );
        // Another device, a notification (software id 0), a short garbage report.
        assert_eq!(reply(&req, &long(2, 0x05, 0x1A, &[1])), Reply::Other);
        assert_eq!(reply(&req, &long(1, 0x05, 0x10, &[1])), Reply::Other);
        assert_eq!(reply(&req, &[0x11, 1]), Reply::Other);
    }

    #[test]
    fn protocol_version_two_or_later_is_hidpp2() {
        assert!(is_hidpp2(&[4, 2, 0xAA]));
        assert!(!is_hidpp2(&[1, 0, 0xAA]));
        assert!(!is_hidpp2(&[]));
    }

    #[test]
    fn unified_battery() {
        // 57 %, good, discharging.
        assert_eq!(
            unified(&[57, 4, 0, 0]),
            Some(Charge {
                percent: Some(57),
                level: None,
                charging: Some(false)
            })
        );
        // Charging; full and done on the charger is not "charging".
        assert_eq!(unified(&[80, 4, 1, 1]).unwrap().charging, Some(true));
        assert_eq!(unified(&[100, 8, 3, 1]).unwrap().charging, Some(false));
        // No percentage: the level only.
        assert_eq!(
            unified(&[0, 2, 0, 0]),
            Some(Charge {
                percent: None,
                level: Some(BatteryLevel::Low),
                charging: Some(false)
            })
        );
        assert_eq!(unified(&[0, 0, 0, 0]), None);
        assert_eq!(unified(&[50, 4, 4, 0]), None);
        assert_eq!(unified(&[50]), None);
    }

    #[test]
    fn battery_status() {
        assert_eq!(status(&[70, 50, 0]).unwrap().percent, Some(70));
        assert_eq!(status(&[70, 50, 1]).unwrap().charging, Some(true));
        assert_eq!(status(&[0, 0, 3]).unwrap().percent, Some(100));
        assert_eq!(status(&[0, 0, 0]), None);
        assert_eq!(status(&[70, 50, 5]), None);
    }

    #[test]
    fn battery_voltage_follows_the_curve() {
        assert_eq!(percent_from_mv(4200), 100);
        assert_eq!(percent_from_mv(4186), 100);
        assert_eq!(percent_from_mv(3811), 50);
        // Halfway between 3811 (50 %) and 3859 (60 %).
        assert_eq!(percent_from_mv(3835), 55);
        assert_eq!(percent_from_mv(3500), 0);
        assert_eq!(percent_from_mv(3000), 0);
        let c = voltage(&[0x0E, 0xE3, 0x00]).unwrap(); // 3811 mV
        assert_eq!((c.percent, c.charging), (Some(50), Some(false)));
        let c = voltage(&[0x0E, 0xE3, 0x80]).unwrap();
        assert_eq!(c.charging, Some(true));
        let c = voltage(&[0x0F, 0xA0, 0x81]).unwrap();
        assert_eq!((c.percent, c.charging), (Some(100), Some(false)));
        assert_eq!(voltage(&[0, 0, 0]), None);
    }

    #[test]
    fn names_are_joined_from_parts_and_cut_at_the_length() {
        let parts = vec![b"G Pro X Superlig".to_vec(), b"ht 2\0\0\0".to_vec()];
        assert_eq!(name(&parts, 20).as_deref(), Some("G Pro X Superlight 2"));
        assert_eq!(name(&parts, 5).as_deref(), Some("G Pro"));
        assert_eq!(name(&[], 5), None);
    }

    #[test]
    fn device_types() {
        assert_eq!(device_kind(0), DeviceKind::Keyboard);
        assert_eq!(device_kind(3), DeviceKind::Mouse);
        assert_eq!(device_kind(8), DeviceKind::Headset);
        assert_eq!(device_kind(12), DeviceKind::Controller);
        assert_eq!(device_kind(99), DeviceKind::Other);
    }

    #[test]
    fn dualsense_reports() {
        assert_eq!(sony_pad(0x0CE6), Some(Pad::DualSense));
        let mut usb = vec![0u8; 64];
        usb[0] = 0x01;
        usb[53] = 0x07; // level 7, discharging
        assert_eq!(
            pad_charge(Pad::DualSense, &usb),
            Some(Charge {
                percent: Some(75),
                level: None,
                charging: Some(false)
            })
        );
        usb[53] = 0x14; // charging, level 4
        assert_eq!(
            pad_charge(Pad::DualSense, &usb).unwrap().charging,
            Some(true)
        );
        usb[53] = 0x2A; // full
        assert_eq!(pad_charge(Pad::DualSense, &usb).unwrap().percent, Some(100));
        usb[53] = 0xA3; // voltage error
        assert_eq!(pad_charge(Pad::DualSense, &usb), None);
        let mut bt = vec![0u8; 78];
        bt[0] = 0x31;
        bt[54] = 0x0A;
        assert_eq!(pad_charge(Pad::DualSense, &bt).unwrap().percent, Some(100));
        // Bluetooth's simple report has no battery.
        let mut simple = vec![0u8; 10];
        simple[0] = 0x01;
        assert_eq!(pad_charge(Pad::DualSense, &simple), None);
    }

    #[test]
    fn dualshock4_reports() {
        let mut usb = vec![0u8; 64];
        usb[0] = 0x01;
        usb[30] = 0x06; // no cable, level 6
        assert_eq!(
            pad_charge(Pad::DualShock4, &usb),
            Some(Charge {
                percent: Some(65),
                level: None,
                charging: Some(false)
            })
        );
        usb[30] = 0x13; // cable, charging, level 3
        let c = pad_charge(Pad::DualShock4, &usb).unwrap();
        assert_eq!((c.percent, c.charging), (Some(35), Some(true)));
        usb[30] = 0x1B; // cable, full
        let c = pad_charge(Pad::DualShock4, &usb).unwrap();
        assert_eq!((c.percent, c.charging), (Some(100), Some(false)));
        // The wireless adapter with no controller.
        usb[30] = 0x06;
        usb[31] = 0x04;
        assert_eq!(pad_charge(Pad::DualShock4Dongle, &usb), None);
        assert!(pad_charge(Pad::DualShock4, &usb).is_some());
        let mut bt = vec![0u8; 78];
        bt[0] = 0x11;
        bt[32] = 0x09;
        assert_eq!(pad_charge(Pad::DualShock4, &bt).unwrap().percent, Some(95));
    }

    #[test]
    fn battery_strength_scales_to_its_range() {
        assert_eq!(strength(50, 0, 100), Some(50));
        assert_eq!(strength(255, 0, 255), Some(100));
        assert_eq!(strength(128, 0, 255), Some(50));
        assert_eq!(strength(5, 0, 0), None);
        assert_eq!(strength(300, 0, 255), None);
    }
}

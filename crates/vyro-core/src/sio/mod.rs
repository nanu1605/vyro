//! Motherboard Super-I/O hardware monitor through PawnIO's `LpcIO` module (Phase 6c): fans,
//! board temperatures and voltages. Starts with the Nuvoton NCT6687D(-R) that MSI uses on AM5
//! and LGA1700/1851 boards. Decoders and board layouts here are pure; `win` does the port I/O.
//! Register layout follows LibreHardwareMonitor's `Nct677X` and the Linux `nct6687d` driver.
//!
//! Vyro only reads: unlike other tools it never writes the chip's monitor-enable or fan
//! registers, so a board whose firmware left monitoring off simply reports nothing.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{detect, read_block, Found, SioProvider};

use crate::snapshot::FanKind;

/// Super-I/O configuration index ports; PawnIO `LpcIO` slot 0 and 1.
pub const CONFIG_PORTS: [u16; 2] = [0x2E, 0x4E];
/// Winbond/Nuvoton/Fintek "enter extended function mode" key, written twice.
pub const ENTER_KEY: u8 = 0x87;
pub const EXIT_KEY: u8 = 0xAA;
pub const REG_LDN: u8 = 0x07;
pub const REG_CHIP_ID: u8 = 0x20;
pub const REG_CHIP_REV: u8 = 0x21;
pub const REG_BASE: u8 = 0x60;
/// Logical device of the Nuvoton hardware monitor.
pub const LDN_HWM: u8 = 0x0B;

/// NCT6687 EC space, reached through the hardware-monitor base: page, index, data.
pub const EC_PAGE: u16 = 4;
pub const EC_INDEX: u16 = 5;
pub const EC_DATA: u16 = 6;
/// Page register value meaning "free"; writing it back releases the EC for other tools.
pub const EC_FREE: u8 = 0xFF;

/// First temperature register (two bytes each: integer, then bit 7 = +0.5 °C).
pub const TEMP_BASE: u16 = 0x100;
pub const TEMP_COUNT: u16 = 11;
/// First voltage register (two bytes: millivolts = 16 × high + low >> 4).
pub const VOLT_BASE: u16 = 0x120;
pub const VOLT_COUNT: u16 = 16;
/// First fan register (two bytes: RPM, big-endian).
pub const FAN_BASE: u16 = 0x140;
pub const FAN_COUNT: u16 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Nct6687,
}

impl Chip {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Nct6687 => "Nuvoton NCT6687",
        }
    }
}

/// Chips Vyro can read, from the configuration-space ID and revision.
pub const fn identify(id: u8, _rev: u8) -> Option<Chip> {
    match id {
        // D and DR share the monitor registers Vyro reads; they differ in fan control.
        0xD5 => Some(Chip::Nct6687),
        _ => None,
    }
}

/// A plausible I/O base: in the ISA range, not the fixed legacy ports, 8-byte aligned.
pub const fn valid_base(base: u16) -> bool {
    base >= 0x100 && base != 0xFFFF && base & 0x07 == 0
}

/// Volts from a voltage register pair (12-bit millivolts).
pub fn voltage(high: u8, low: u8) -> f64 {
    f64::from(u16::from(high) * 16 + u16::from(low >> 4)) / 1000.0
}

/// °C from a temperature register pair; `None` outside what a board sensor can read.
pub fn temperature(high: u8, low: u8) -> Option<f64> {
    #[allow(clippy::cast_possible_wrap)]
    let t = f64::from(high as i8) + if low & 0x80 != 0 { 0.5 } else { 0.0 };
    (-40.0..=125.0).contains(&t).then_some(t)
}

/// RPM from a fan register pair; `None` when the register is unimplemented (all ones).
pub fn rpm(high: u8, low: u8) -> Option<f64> {
    let v = u16::from_be_bytes([high, low]);
    (v != 0xFFFF).then_some(f64::from(v))
}

/// Where a board temperature goes in [`crate::snapshot::BoardSnapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardTemp {
    /// Registry only (CPU is already read from the CPU itself).
    None,
    System,
    VrmMos,
    Chipset,
    Socket,
    Pcie,
    M2,
}

/// Names for one board's sensor channels. Channels not listed are shown under their raw
/// name when they read something.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub temps: &'static [(u16, &'static str, BoardTemp)],
    pub fans: &'static [(u16, &'static str, FanKind)],
}

/// NCT6687 defaults, used on MSI boards (LibreHardwareMonitor's MSI mappings and the
/// `nct6687d` driver agree on these channels).
pub const MSI_NCT6687: Layout = Layout {
    temps: &[
        (0, "CPU", BoardTemp::None),
        (1, "System", BoardTemp::System),
        (2, "VRM MOS", BoardTemp::VrmMos),
        (3, "Chipset", BoardTemp::Chipset),
        (4, "CPU Socket", BoardTemp::Socket),
    ],
    fans: &[
        (0, "CPU Fan", FanKind::Fan),
        (1, "Pump Fan", FanKind::Pump),
        (2, "System Fan 1", FanKind::Fan),
        (3, "System Fan 2", FanKind::Fan),
        (4, "System Fan 3", FanKind::Fan),
    ],
};

/// Layout for a board, from the SMBIOS board manufacturer.
pub fn layout(manufacturer: Option<&str>, chip: Chip) -> Option<Layout> {
    let msi = manufacturer.is_some_and(|m| m.contains("Micro-Star") || m.contains("MSI"));
    match chip {
        Chip::Nct6687 if msi => Some(MSI_NCT6687),
        Chip::Nct6687 => None,
    }
}

/// Voltage channel name and divider. Only the two rails every NCT6687 board wires the same
/// way are named; the rest keep the chip's input name until checked per board.
pub fn voltage_channel(index: u16) -> (String, f64) {
    match index {
        0 => ("+12V".to_owned(), 12.0),
        1 => ("+5V".to_owned(), 5.0),
        n => (format!("VIN{n}"), 1.0),
    }
}

/// Monitor registers read each tick: temperatures, voltages and fans are contiguous.
pub const BLOCK_START: u16 = TEMP_BASE;
pub const BLOCK_LEN: u16 = FAN_BASE + FAN_COUNT * 2 - TEMP_BASE;

/// Decoded channels of one monitor block, before names: `(channel, value)`.
pub struct Decoded {
    pub temps: Vec<(u16, Option<f64>)>,
    pub volts: Vec<(u16, f64)>,
    pub fans: Vec<(u16, Option<f64>)>,
}

pub fn decode(block: &[u8]) -> Decoded {
    let pair = |reg: u16| {
        let i = usize::from(reg - BLOCK_START);
        (block.get(i).copied(), block.get(i + 1).copied())
    };
    let pairs = |base: u16, count: u16| {
        (0..count).filter_map(move |n| match pair(base + n * 2) {
            (Some(h), Some(l)) => Some((n, h, l)),
            _ => None,
        })
    };
    Decoded {
        temps: pairs(TEMP_BASE, TEMP_COUNT)
            .map(|(n, h, l)| (n, temperature(h, l)))
            .collect(),
        volts: pairs(VOLT_BASE, VOLT_COUNT)
            .map(|(n, h, l)| (n, voltage(h, l) * voltage_channel(n).1))
            .collect(),
        fans: pairs(FAN_BASE, FAN_COUNT)
            .map(|(n, h, l)| (n, rpm(h, l)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn identifies_nct6687_only() {
        assert_eq!(identify(0xD5, 0x92), Some(Chip::Nct6687));
        assert_eq!(identify(0xD4, 0x28), None);
        assert_eq!(identify(0xFF, 0xFF), None);
    }

    #[test]
    fn base_address_sanity() {
        assert!(valid_base(0x0A20));
        assert!(!valid_base(0));
        assert!(!valid_base(0xFFFF));
        assert!(!valid_base(0x0A25));
        assert!(!valid_base(0x0060));
    }

    // Raw register pairs from HWMonitor on an MSI B650M GAMING PLUS WIFI (NCT6687, rev 0x92).
    #[test]
    fn decodes_field_voltages() {
        assert!(close(voltage(0x3E, 0xE0), 1.006));
        assert!(close(voltage(0x3E, 0xE0) * voltage_channel(0).1, 12.072));
        assert!(close(voltage(0x3F, 0x60) * voltage_channel(1).1, 5.07));
        assert!(close(voltage(0x4B, 0x60), 1.206));
        assert!(close(voltage(0xD1, 0x00), 3.344));
        assert!(close(voltage(0, 0), 0.0));
        assert_eq!(voltage_channel(6).0, "VIN6");
    }

    #[test]
    fn decodes_temperatures_with_half_degree() {
        assert_eq!(temperature(57, 0x00), Some(57.0));
        assert_eq!(temperature(41, 0x80), Some(41.5));
        assert_eq!(temperature(0xF6, 0x00), Some(-10.0));
        assert_eq!(temperature(0x7F, 0x80), None);
        assert_eq!(temperature(0x80, 0x00), None);
    }

    #[test]
    fn decodes_fans() {
        assert_eq!(rpm(0x05, 0x68), Some(1384.0));
        assert_eq!(rpm(0x11, 0x6C), Some(4460.0));
        assert_eq!(rpm(0, 0), Some(0.0));
        assert_eq!(rpm(0xFF, 0xFF), None);
    }

    #[test]
    fn msi_boards_get_the_msi_layout() {
        let l = layout(Some("Micro-Star International Co., Ltd."), Chip::Nct6687).expect("msi");
        assert_eq!(l.fans[1], (1, "Pump Fan", FanKind::Pump));
        assert!(layout(Some("ASRock"), Chip::Nct6687).is_none());
        assert!(layout(None, Chip::Nct6687).is_none());
    }

    #[test]
    fn decodes_a_monitor_block() {
        let mut block = vec![0u8; usize::from(BLOCK_LEN)];
        let at = |reg: u16| usize::from(reg - BLOCK_START);
        block[at(TEMP_BASE)] = 57;
        block[at(TEMP_BASE + 2)] = 42;
        block[at(TEMP_BASE + 3)] = 0x80;
        block[at(VOLT_BASE)] = 0x3E;
        block[at(VOLT_BASE + 1)] = 0xE0;
        block[at(FAN_BASE)] = 0x05;
        block[at(FAN_BASE + 1)] = 0x68;
        block[at(FAN_BASE + 2)] = 0xFF;
        block[at(FAN_BASE + 3)] = 0xFF;
        let d = decode(&block);
        assert_eq!(d.temps[0], (0, Some(57.0)));
        assert_eq!(d.temps[1], (1, Some(42.5)));
        assert!((d.volts[0].1 - 12.072).abs() < 1e-9);
        assert_eq!(d.fans[0], (0, Some(1384.0)));
        assert_eq!(d.fans[1], (1, None));
        assert_eq!(d.temps.len(), usize::from(TEMP_COUNT));
        assert_eq!(d.fans.len(), usize::from(FAN_COUNT));
    }
}

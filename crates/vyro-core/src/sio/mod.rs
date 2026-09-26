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
    /// Voltage inputs checked on this board (name only; +12V/+5V scaling is universal).
    pub volts: &'static [(u16, &'static str)],
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
    volts: &[],
};

/// MSI B650M GAMING PLUS WIFI (MS-7E24). Checked against HWMonitor at idle and under an
/// all-core load: VIN2 holds 1.21 V with VDDCR_SOC at 1.20 V; VIN3 follows VDDCR_VDD from
/// 1.34 V idle to 1.02 V loaded; VIN6 reads 3.34 V. LibreHardwareMonitor's labels for this
/// board (Vcore on VIN4, +3.3V on 0x130) do not match these readings.
pub const MSI_B650M_GAMING_PLUS_WIFI: Layout = Layout {
    volts: &[(2, "CPU SoC"), (3, "Vcore"), (6, "+3.3V")],
    ..MSI_NCT6687
};

/// Layout for a board, from the SMBIOS board manufacturer and product.
pub fn layout(manufacturer: Option<&str>, product: Option<&str>, chip: Chip) -> Option<Layout> {
    let msi = manufacturer.is_some_and(|m| m.contains("Micro-Star") || m.contains("MSI"));
    let product = product.unwrap_or("");
    match chip {
        Chip::Nct6687 if msi && product.contains("MS-7E24") => Some(MSI_B650M_GAMING_PLUS_WIFI),
        Chip::Nct6687 if msi => Some(MSI_NCT6687),
        Chip::Nct6687 => None,
    }
}

/// Byte-wide port I/O, so the EC access sequence can be tested without hardware.
pub trait PortIo {
    type Error;
    fn inb(&mut self, port: u16) -> Result<u8, Self::Error>;
    fn outb(&mut self, port: u16, value: u8) -> Result<(), Self::Error>;
    /// Pause between polls of a busy EC.
    fn pause(&mut self) {}
}

/// Reads `len` bytes of NCT6687 EC space from `start` (base = hardware-monitor I/O base).
///
/// The EC takes a request when its page register goes from free (0xFF) to a page, so every
/// byte is a full transaction, as in LibreHardwareMonitor: wait for free, page, index, data,
/// free. Rewriting the page without releasing it first returns the first byte over and over
/// (seen in the field). `None` when another program holds the EC (page register not free):
/// the tick is skipped rather than forcing the page, which would corrupt that program's read.
/// Call with the ISA-bus mutex held.
pub fn read_ec<P: PortIo>(
    io: &mut P,
    base: u16,
    start: u16,
    len: u16,
) -> Result<Option<Vec<u8>>, P::Error> {
    let mut out = Vec::with_capacity(usize::from(len));
    for addr in start..start + len {
        let mut free = false;
        for _ in 0..5 {
            if io.inb(base + EC_PAGE)? == EC_FREE {
                free = true;
                break;
            }
            io.pause();
        }
        if !free {
            return Ok(None);
        }
        let [page, index] = addr.to_be_bytes();
        io.outb(base + EC_PAGE, page)?;
        let byte = io
            .outb(base + EC_INDEX, index)
            .and_then(|()| io.inb(base + EC_DATA));
        // Release the EC for other tools even if the read failed.
        let release = io.outb(base + EC_PAGE, EC_FREE);
        out.push(byte?);
        release?;
    }
    Ok(Some(out))
}

/// A monitor block where every byte is the same is a stuck read, not sensor data.
pub fn plausible(block: &[u8]) -> bool {
    block.first().is_some_and(|b| block.iter().any(|x| x != b))
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
        let msi = Some("Micro-Star International Co., Ltd.");
        let l = layout(
            msi,
            Some("MAG X670E TOMAHAWK WIFI (MS-7E12)"),
            Chip::Nct6687,
        )
        .expect("msi");
        assert_eq!(l.fans[1], (1, "Pump Fan", FanKind::Pump));
        assert!(l.volts.is_empty());
        let b650 =
            layout(msi, Some("B650M GAMING PLUS WIFI (MS-7E24)"), Chip::Nct6687).expect("b650");
        assert_eq!(b650.volts[1], (3, "Vcore"));
        assert_eq!(b650.fans, MSI_NCT6687.fans);
        assert!(layout(Some("ASRock"), None, Chip::Nct6687).is_none());
        assert!(layout(None, None, Chip::Nct6687).is_none());
    }

    /// An NCT6687 EC that, like the real one, takes a request only when the page register goes
    /// from free to a page; the data register keeps returning the last request's byte.
    struct FakeEc {
        mem: Vec<u8>,
        page_reg: u8,
        addr: Option<u16>,
        armed: bool,
    }

    impl FakeEc {
        fn new(page_reg: u8) -> Self {
            Self {
                mem: (0..=0xFFFFu32)
                    .map(|a| (a as u8) ^ ((a >> 8) as u8))
                    .collect(),
                page_reg,
                addr: None,
                armed: false,
            }
        }
    }

    impl PortIo for FakeEc {
        type Error = ();
        fn inb(&mut self, port: u16) -> Result<u8, ()> {
            match port - 0xA20 {
                EC_PAGE => Ok(self.page_reg),
                EC_DATA => Ok(self.addr.map_or(0xFF, |a| self.mem[usize::from(a)])),
                _ => Err(()),
            }
        }
        fn outb(&mut self, port: u16, value: u8) -> Result<(), ()> {
            match port - 0xA20 {
                EC_PAGE => {
                    self.armed = self.page_reg == EC_FREE && value != EC_FREE;
                    self.page_reg = value;
                }
                EC_INDEX if self.armed => {
                    self.addr = Some(u16::from_be_bytes([self.page_reg, value]));
                    self.armed = false;
                }
                EC_INDEX => {}
                _ => return Err(()),
            }
            Ok(())
        }
    }

    #[test]
    fn reads_every_ec_byte_and_releases_the_page() {
        let mut ec = FakeEc::new(EC_FREE);
        let block = read_ec(&mut ec, 0xA20, BLOCK_START, BLOCK_LEN)
            .unwrap()
            .expect("free");
        assert_eq!(block.len(), usize::from(BLOCK_LEN));
        for (i, b) in block.iter().enumerate() {
            let a = BLOCK_START + u16::try_from(i).unwrap();
            assert_eq!(*b, ec.mem[usize::from(a)], "byte at {a:#x}");
        }
        assert_eq!(ec.page_reg, EC_FREE);
    }

    #[test]
    fn rejects_stuck_blocks() {
        assert!(!plausible(&[0x3B; 96]));
        assert!(!plausible(&[]));
        assert!(plausible(&[0x3B, 0x00, 0x2A]));
    }

    #[test]
    fn fake_ec_reproduces_the_field_failure() {
        // Writing the page again without releasing it: every byte reads as the first one.
        let mut ec = FakeEc::new(EC_FREE);
        let mut bytes = Vec::new();
        for index in 0..4u8 {
            ec.outb(0xA20 + EC_PAGE, 0x01).unwrap();
            ec.outb(0xA20 + EC_INDEX, index).unwrap();
            bytes.push(ec.inb(0xA20 + EC_DATA).unwrap());
        }
        assert!(bytes.iter().all(|b| *b == ec.mem[0x100]), "{bytes:?}");
    }

    #[test]
    fn skips_while_another_tool_holds_the_ec() {
        let mut ec = FakeEc::new(0x01);
        assert_eq!(read_ec(&mut ec, 0xA20, BLOCK_START, 4), Ok(None));
        assert_eq!(ec.page_reg, 0x01, "never forced");
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

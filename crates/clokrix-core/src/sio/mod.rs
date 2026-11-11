//! Motherboard Super-I/O hardware monitor through PawnIO's `LpcIO` module (Phase 6c): fans,
//! board temperatures and voltages. Started with the Nuvoton NCT6687D(-R) that MSI uses on AM5
//! and LGA1700/1851 boards; from 2.1 also the Nuvoton NCT6791D to NCT6799D of most ASUS boards
//! and many ASRock ones ([`nct679x`]); from 2.3 the ITE IT8620E to IT8698E of Gigabyte boards
//! ([`ite`]). Decoders and board layouts here are pure; `win` does the port I/O. The NCT6687's
//! layout follows LibreHardwareMonitor's `Nct677X` and the Linux `nct6687d` driver.
//!
//! Monitoring only reads: Clokrix never writes the chip's monitor-enable registers, so a board whose
//! firmware left monitoring off simply reports nothing. One exception (2.1): on a NCT679x whose
//! firmware locked the monitor's I/O ports, so that nothing can be read, the lock bit is cleared,
//! as Linux `nct6775` and LibreHardwareMonitor do; nothing else about the chip changes. Register
//! banks are selected to read, and put back (an ITE chip's) or left at bank 0 (a NCT679x's). Fan
//! control (`control`, Phase 12; `control679x` for the NCT679x, 2.3) writes only the fan
//! registers, only on allow-listed boards.

pub mod control;
pub mod control679x;
pub mod ite;
pub mod nct679x;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{detect, read_block, unlock_monitor, Found, Ports, SioProvider};

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
    Nct6791,
    Nct6792,
    Nct6793,
    Nct6795,
    Nct6796,
    Nct6797,
    Nct6798,
    Nct6799,
    /// ASUS's newest (X870, B850): read as a NCT6799D, as Linux `nct6775` reads it.
    Nct6701,
    /// Gigabyte's (2.3): 9- and 100-series boards.
    It8620,
    It8628,
    /// AM4 and Intel 300/400-series boards.
    It8686,
    /// X570, B550, Z390 and Z490 boards.
    It8688,
    /// AM5 (B650, X670) and LGA1700 (Z690 to B760) boards.
    It8689,
    /// X870, B850 and Z890 boards.
    It8696,
    It8698,
}

/// How a chip's monitor registers are reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// The NCT6687's EC space: page, index, data.
    Nct6687,
    /// A NCT679x's banks: bank select, then address and data.
    Nct679x,
    /// An ITE environment controller: address and data, bank 0.
    Ite,
}

impl Chip {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Nct6687 => "Nuvoton NCT6687",
            Self::Nct6791 => "Nuvoton NCT6791D",
            Self::Nct6792 => "Nuvoton NCT6792D",
            Self::Nct6793 => "Nuvoton NCT6793D",
            Self::Nct6795 => "Nuvoton NCT6795D",
            Self::Nct6796 => "Nuvoton NCT6796D",
            Self::Nct6797 => "Nuvoton NCT6797D",
            Self::Nct6798 => "Nuvoton NCT6798D",
            Self::Nct6799 => "Nuvoton NCT6799D",
            Self::Nct6701 => "Nuvoton NCT6701D",
            Self::It8620 => "ITE IT8620E",
            Self::It8628 => "ITE IT8628E",
            Self::It8686 => "ITE IT8686E",
            Self::It8688 => "ITE IT8688E",
            Self::It8689 => "ITE IT8689E",
            Self::It8696 => "ITE IT8696E",
            Self::It8698 => "ITE IT8698E",
        }
    }

    pub const fn family(self) -> Family {
        match self {
            Self::Nct6687 => Family::Nct6687,
            Self::It8620
            | Self::It8628
            | Self::It8686
            | Self::It8688
            | Self::It8689
            | Self::It8696
            | Self::It8698 => Family::Ite,
            _ => Family::Nct679x,
        }
    }

    /// A banked NCT679x rather than the NCT6687's EC space or an ITE chip.
    pub const fn banked(self) -> bool {
        matches!(self.family(), Family::Nct679x)
    }

    /// Name of a temperature channel (the NCT6687's), source (a NCT679x's) or input (an ITE
    /// chip's) no board layout names.
    pub fn temp_name(self, n: u16) -> String {
        match self.family() {
            Family::Nct6687 => format!("TMPIN{n}"),
            Family::Nct679x => nct679x::source_name(n),
            Family::Ite => ite::temp_name(n),
        }
    }

    /// Name of a voltage input no board layout names.
    pub fn voltage_name(self, n: u16) -> String {
        match self.family() {
            Family::Nct6687 => voltage_channel(n).0,
            Family::Nct679x => nct679x::input_name(n),
            Family::Ite => ite::input_name(n),
        }
    }
}

/// Chips Clokrix can read, from the configuration-space ID and revision.
pub const fn identify(id: u8, rev: u8) -> Option<Chip> {
    if id == 0xD5 {
        // D and DR share the monitor registers Clokrix reads; they differ in fan control.
        return Some(Chip::Nct6687);
    }
    let full = u16::from_be_bytes([id, rev]);
    if full == 0xD806 {
        // Masked it is a NCT6799D's ID; LibreHardwareMonitor tells the NCT6701D by the whole one.
        return Some(Chip::Nct6701);
    }
    // The NCT679x IDs, with the revision's low three bits masked as Linux `nct6775` does.
    match full & 0xFFF8 {
        0xC800 => Some(Chip::Nct6791),
        0xC910 => Some(Chip::Nct6792),
        0xD120 => Some(Chip::Nct6793),
        0xD350 => Some(Chip::Nct6795),
        0xD420 => Some(Chip::Nct6796),
        0xD450 => Some(Chip::Nct6797),
        0xD428 => Some(Chip::Nct6798),
        0xD800 => Some(Chip::Nct6799),
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
    /// Voltage-regulator temperature ("VR" in HWiNFO), separate from the MOSFET sensor.
    Vrm,
    Pcie,
    M2,
}

/// Names for one board's sensor channels. Channels not listed are shown under their raw
/// name when they read something.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// Temperatures by channel (NCT6687), source (NCT679x) or input (ITE): name and where it
    /// goes.
    pub temps: &'static [(u16, &'static str, BoardTemp)],
    pub fans: &'static [(u16, &'static str, FanKind)],
    /// Voltage channels named on this board: channel (the NCT6687's register 0x120 + 2n, the
    /// NCT679x's or ITE chip's input n), name and the board's divider (the NCT6687's +12V/+5V
    /// scaling is universal and already applied).
    pub volts: &'static [(u16, &'static str, f64)],
    /// The list is every voltage checked on this board: show only these. Otherwise the others
    /// show under the chip's names when they read something.
    pub volts_only: bool,
}

/// NCT6687 defaults, used on MSI boards (LibreHardwareMonitor's MSI mappings and the
/// `nct6687d` driver agree on these channels).
pub const MSI_NCT6687: Layout = Layout {
    temps: &[
        (0, "CPU", BoardTemp::None),
        (1, "System", BoardTemp::System),
        (2, "VRM MOS", BoardTemp::VrmMos),
        (3, "Chipset", BoardTemp::Chipset),
        (4, "VR", BoardTemp::Vrm),
    ],
    fans: &[
        (0, "CPU Fan", FanKind::Fan),
        (1, "Pump Fan", FanKind::Pump),
        (2, "System Fan 1", FanKind::Fan),
        (3, "System Fan 2", FanKind::Fan),
        (4, "System Fan 3", FanKind::Fan),
    ],
    volts: &[],
    volts_only: false,
};

/// MSI B650M GAMING PLUS WIFI (MS-7E24), as in LibreHardwareMonitor and checked against a
/// raw register dump next to HWMonitor (whose "VINn" numbering is not register order: its
/// VIN3 is 0x128). Vcore (0x128) follows VDDCR_VDD from 1.34 V idle to 1.02 V under load;
/// SoC (0x124) holds 1.21 V with VDDCR_SOC at 1.20 V; VDDIO (0x126) is halved on the board.
/// VTT (0x13A) reads 2.046 V, the same as HWiNFO's VTT on the owner's board.
pub const MSI_B650M_GAMING_PLUS_WIFI: Layout = Layout {
    volts: &[
        (0, "+12V", 1.0),
        (1, "+5V", 1.0),
        (2, "CPU SoC", 1.0),
        (3, "CPU VDDIO", 2.0),
        (4, "Vcore", 1.0),
        (8, "+3.3V", 1.0),
        (11, "+3.3V Standby", 1.0),
        (12, "AVSB", 1.0),
        (13, "VTT", 1.0),
        (14, "CMOS Battery", 1.0),
    ],
    volts_only: true,
    ..MSI_NCT6687
};

/// ASUS boards with a NCT679x (2.1). What every ASUS mapping in LibreHardwareMonitor shares:
/// SYSTIN is the motherboard sensor, CPUTIN the CPU socket's, PECI agent 0 the CPU as the board
/// reads it, the PCH source the chipset; fan 0 the first chassis header, fan 1 the CPU's; +5V on
/// VIN1 through a divider of 5 and +12V on VIN0 through 12. The rest, which differs from board
/// to board, keeps the chip's names.
pub const ASUS_NCT679X: Layout = Layout {
    temps: &[
        (1, "Motherboard", BoardTemp::System),
        (2, "CPU Socket", BoardTemp::None),
        (16, "CPU (PECI)", BoardTemp::None),
        (19, "Chipset", BoardTemp::Chipset),
    ],
    fans: &[
        (0, "Chassis Fan 1", FanKind::Fan),
        (1, "CPU Fan", FanKind::Fan),
    ],
    volts: &[(1, "+5V", 5.0), (4, "+12V", 12.0)],
    volts_only: false,
};

/// Layout for a board, from the SMBIOS board manufacturer and product.
pub fn layout(manufacturer: Option<&str>, product: Option<&str>, chip: Chip) -> Option<Layout> {
    let maker = manufacturer
        .map(str::to_ascii_uppercase)
        .unwrap_or_default();
    let msi = maker.contains("MICRO-STAR") || maker.contains("MSI");
    let product = product.unwrap_or("");
    match chip.family() {
        Family::Nct6687 if msi && product.contains("MS-7E24") => Some(MSI_B650M_GAMING_PLUS_WIFI),
        Family::Nct6687 if msi => Some(MSI_NCT6687),
        Family::Nct679x if maker.contains("ASUSTEK") => Some(ASUS_NCT679X),
        Family::Ite if maker.contains("GIGABYTE") => Some(ite::gigabyte(product)),
        _ => None,
    }
}

/// Byte-wide port I/O, so the EC access sequence can be tested without hardware.
pub trait PortIo {
    type Error;
    fn inb(&mut self, port: u16) -> Result<u8, Self::Error>;
    fn outb(&mut self, port: u16, value: u8) -> Result<(), Self::Error>;
    /// Pause between polls of a busy EC.
    fn pause(&mut self) {}
    /// Wait for the EC to act on a fan-control request (tens of milliseconds on hardware).
    fn settle(&mut self) {}
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
    fn identifies_nct6687_and_the_nct679x() {
        assert_eq!(identify(0xD5, 0x92), Some(Chip::Nct6687));
        // NCT6798D rev 0x2B (ASUS Z690 and B760): the revision's low bits are masked.
        assert_eq!(identify(0xD4, 0x2B), Some(Chip::Nct6798));
        assert_eq!(identify(0xD4, 0x28), Some(Chip::Nct6798));
        // NCT6796D differs from the 6798 in bit 3 of the revision byte only.
        assert_eq!(identify(0xD4, 0x23), Some(Chip::Nct6796));
        assert_eq!(identify(0xD8, 0x02), Some(Chip::Nct6799));
        assert_eq!(identify(0xD8, 0x06), Some(Chip::Nct6701));
        assert_eq!(Chip::Nct6701.name(), "Nuvoton NCT6701D");
        assert_eq!(identify(0xC8, 0x03), Some(Chip::Nct6791));
        assert_eq!(identify(0xD4, 0x52), Some(Chip::Nct6797));
        assert_eq!(identify(0xC5, 0x62), None, "NCT6779D: not read yet");
        assert_eq!(identify(0xFF, 0xFF), None);
        assert!(Chip::Nct6798.banked());
        assert!(!Chip::Nct6687.banked());
        assert!(!Chip::It8689.banked());
        assert_eq!(Chip::It8689.family(), Family::Ite);
        assert_eq!(Chip::Nct6701.family(), Family::Nct679x);
        assert_eq!(Chip::Nct6687.family(), Family::Nct6687);
        assert_eq!(Chip::Nct6798.name(), "Nuvoton NCT6798D");
    }

    #[test]
    fn asus_boards_with_a_nct679x_get_the_asus_layout() {
        let asus = Some("ASUSTeK COMPUTER INC.");
        let l = layout(asus, Some("TUF GAMING B760M-PLUS WIFI D4"), Chip::Nct6798).expect("asus");
        assert_eq!(l.temps[0], (1, "Motherboard", BoardTemp::System));
        assert_eq!(l.fans[1], (1, "CPU Fan", FanKind::Fan));
        assert!(!l.volts_only);
        assert!(layout(Some("ASRock"), None, Chip::Nct6798).is_none());
        assert!(
            layout(asus, None, Chip::Nct6687).is_none(),
            "an EC chip on ASUS: not mapped"
        );
        assert_eq!(Chip::Nct6798.temp_name(4), "AUXTIN1");
        assert_eq!(Chip::Nct6687.temp_name(4), "TMPIN4");
        assert_eq!(Chip::Nct6798.voltage_name(4), "VIN0");
        assert_eq!(Chip::Nct6687.voltage_name(4), "VIN4");
        assert_eq!(Chip::It8689.voltage_name(7), "+3.3V Standby");
        assert_eq!(Chip::It8689.temp_name(0), "TMPIN1");
    }

    #[test]
    fn gigabyte_boards_with_an_ite_chip_get_the_gigabyte_layout() {
        let gb = Some("Gigabyte Technology Co., Ltd.");
        let l = layout(gb, Some("B650 AORUS ELITE AX"), Chip::It8689).expect("gigabyte");
        assert_eq!(l.temps[4], (4, "VRM MOS", BoardTemp::VrmMos));
        assert_eq!(l.fans[0], (0, "CPU Fan", FanKind::Fan));
        let x570 = layout(gb, Some("X570 AORUS ELITE"), Chip::It8688).expect("x570");
        assert_eq!(x570.fans[3].1, "Chipset Fan");
        assert!(layout(Some("ASRock"), None, Chip::It8689).is_none());
        assert!(
            layout(gb, None, Chip::Nct6798).is_none(),
            "a Nuvoton chip on Gigabyte"
        );
        assert!(layout(None, None, Chip::It8686).is_none());
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
        assert_eq!(b650.volts[4], (4, "Vcore", 1.0));
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

    /// Monitor block (0x100–0x15F) from the MSI B650M GAMING PLUS WIFI via `clokrix-live`, with
    /// HWMonitor alongside: CPU fan 1542 RPM, pump 4411, TMPIN0–5 59/42/40/42/39/81 °C,
    /// Vcore 1.22 V, SoC 1.21 V, VDDIO/2 0.622 V, +3.3V 3.34 V.
    const FIELD_BLOCK: [u8; 96] = [
        0x38, 0x00, 0x2A, 0x80, 0x29, 0x80, 0x2A, 0x00, 0x28, 0x00, 0x51, 0x80, 0, 0, 0, 0, //
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
        0x3E, 0xE0, 0x3F, 0x60, 0x4B, 0x60, 0x26, 0xE0, 0x4A, 0xE0, 0x11, 0x80, 0x2C, 0xE0, 0x5F,
        0x80, //
        0xD1, 0x00, 0x00, 0x00, 0x00, 0x00, 0xD2, 0x80, 0xD2, 0x40, 0x7F, 0xE0, 0xC1, 0x00, 0,
        0, //
        0x05, 0xD8, 0x11, 0x5C, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    #[test]
    fn decodes_the_field_block_with_the_board_layout() {
        assert!(plausible(&FIELD_BLOCK));
        let d = decode(&FIELD_BLOCK);
        let l = MSI_B650M_GAMING_PLUS_WIFI;
        let volt = |name: &str| {
            let (n, _, scale) = *l.volts.iter().find(|c| c.1 == name).expect(name);
            d.volts
                .iter()
                .find(|v| v.0 == n)
                .map(|v| v.1 * scale)
                .expect(name)
        };
        assert!(close(volt("+12V"), 12.072));
        assert!(close(volt("+5V"), 5.07));
        assert!(close(volt("CPU SoC"), 1.206));
        assert!(close(volt("CPU VDDIO"), 1.244));
        assert!(close(volt("Vcore"), 1.198));
        assert!(close(volt("+3.3V"), 3.344));
        assert!(close(volt("CMOS Battery"), 3.088));
        let temps: Vec<_> = d.temps.iter().take(6).map(|t| t.1).collect();
        assert_eq!(
            temps,
            [
                Some(56.0),
                Some(42.5),
                Some(41.5),
                Some(42.0),
                Some(40.0),
                Some(81.5)
            ]
        );
        assert_eq!(d.fans[0], (0, Some(1496.0)));
        assert_eq!(d.fans[1], (1, Some(4444.0)));
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

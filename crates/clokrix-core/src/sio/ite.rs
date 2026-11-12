//! ITE environment controllers (2.3): the hardware monitor of Gigabyte boards, from the IT8620E
//! and IT8628E of their 9- and 100-series boards to the IT8686E–IT8689E of the AM4, AM5 and
//! LGA1700 ones and the IT8696E and IT8698E of the newest; from 2.4 also the second chip many of
//! them have at 0x4E (an IT8790E, IT8792E or IT87952E, with three more fans and temperatures). The
//! register facts are ITE's, as the Linux `it87` driver (and its out-of-tree version, which knows
//! the newer chips) and LibreHardwareMonitor's `IT87XX` read them. Pure: the port I/O is behind
//! [`PortIo`], so the access sequence is tested without hardware.
//!
//! Configuration mode on 0x2E opens with 0x87 0x01 0x55 0x55 and closes by writing 0x02 to
//! register 0x02. The environment controller is logical device 4; its registers are reached by
//! writing the register's address to its base + 5 and reading the value at + 6.
//!
//! The second chip is reached as Linux `it87` and LibreHardwareMonitor reach it: its ID is read
//! first without the key, since these chips stay in configuration mode for the board's embedded
//! controller, which uses them too; the key (ending 0xAA on 0x4E) is sent only when nothing
//! answers, and configuration mode is never left. Its fans are read, never driven: the embedded
//! controller drives them, and would have to be stopped first.
//!
//! Monitoring only reads. One write is made, as the Linux driver makes it: on the chips with
//! register banks, a bank another program left selected is switched to bank 0, which holds every
//! register read here, and put back after the read.

use super::{BoardTemp, Chip, Decoded, Layout, PortIo};
use crate::snapshot::FanKind;

/// Opens configuration mode on 0x2E.
pub const ENTER_2E: [u8; 4] = [0x87, 0x01, 0x55, 0x55];
/// Opens it on 0x4E, the second chip's port.
pub const ENTER_4E: [u8; 4] = [0x87, 0x01, 0x55, 0xAA];
/// Configuration control register: writing [`EXIT`] to it leaves configuration mode.
pub const CR_CONFIG_CONTROL: u8 = 0x02;
pub const EXIT: u8 = 0x02;
/// Chip version, in the low four bits.
pub const CR_VERSION: u8 = 0x22;
/// Logical device of the environment controller.
pub const LDN_EC: u8 = 0x04;
/// The selected logical device's activate register, bit 0.
pub const CR_ACTIVATE: u8 = 0x30;

/// Address and data ports, from the environment controller's base.
pub const ADDR_PORT: u16 = 5;
pub const DATA_PORT: u16 = 6;

/// Configuration: bit 0 set = monitoring started, bit 7 set = the chip is resetting.
pub const CONFIG: u8 = 0x00;
/// Bank select in bits 7:5 on the chips with banks (registers 0x10–0xAF change with it).
pub const BANK: u8 = 0x06;
const BANK_BITS: u8 = 0xE0;
/// Bits 4, 5 and 2: fans 4, 5 and 6 enabled.
pub const FAN_16BIT: u8 = 0x0C;
/// Bits 6:4: fans 1–3 enabled.
pub const FAN_MAIN_CTRL: u8 = 0x13;
/// Two bits per input 4–6 (registers 0x2C–0x2E): 2 or 3 makes it a temperature.
pub const TEMP456: u8 = 0x77;
/// Vendor ID: 0x90 (0x7F on some chips, which LibreHardwareMonitor accepts too).
pub const VENDOR: u8 = 0x58;
pub const VENDOR_IDS: [u8; 2] = [0x90, 0x7F];

/// Voltage inputs 0–8 (8 is the battery) and 9, AVCC3, on the chips that have it.
pub const VIN: [u8; 10] = [0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2F];
/// Temperatures 1–6 in signed °C (4–6 only where [`TEMP456`] makes them temperatures).
pub const TEMP: [u8; 6] = [0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E];
/// Tachometer counts of fans 1–6, 16 bits each: low byte, then high byte.
pub const FAN_LOW: [u8; 6] = [0x0D, 0x0E, 0x0F, 0x80, 0x82, 0x4C];
pub const FAN_HIGH: [u8; 6] = [0x18, 0x19, 0x1A, 0x81, 0x83, 0x4D];
/// Microvolts per count: 12 mV on the first chips and the IT8790E.
pub const LSB_UV: u16 = 12_000;
/// 11 mV on the IT8792E and IT87952E: LibreHardwareMonitor's, which matches Gigabyte's own software
/// and SIV (Linux `it87` uses 10.9 mV).
pub const LSB_UV_SECOND: u16 = 11_000;

/// Chips Clokrix reads, from the configuration-space ID (registers 0x20 and 0x21).
pub const fn identify(id: u16) -> Option<Chip> {
    match id {
        0x8620 => Some(Chip::It8620),
        0x8628 => Some(Chip::It8628),
        0x8686 => Some(Chip::It8686),
        0x8688 => Some(Chip::It8688),
        0x8689 => Some(Chip::It8689),
        0x8696 => Some(Chip::It8696),
        0x8698 => Some(Chip::It8698),
        _ => None,
    }
}

/// The second chip, at 0x4E: the IT8790E, the IT8792E (whose ID the IT8791E and IT8795E share)
/// and the IT87952E.
pub const fn identify_second(id: u16) -> Option<Chip> {
    match id {
        0x8790 => Some(Chip::It8790),
        0x8733 => Some(Chip::It8792),
        0x8695 => Some(Chip::It87952),
        _ => None,
    }
}

/// One of the second chips: three fans, three temperatures, nine voltage inputs.
pub const fn is_second(chip: Chip) -> bool {
    matches!(chip, Chip::It8790 | Chip::It8792 | Chip::It87952)
}

/// Microvolts per count of the chip's voltage inputs.
pub const fn lsb_uv(chip: Chip) -> u16 {
    match chip {
        Chip::It8792 | Chip::It87952 => LSB_UV_SECOND,
        _ => LSB_UV,
    }
}

/// An ITE chip answered the key (one Clokrix reads or not): it leaves configuration mode the ITE
/// way. Anything else is sent the common exit key instead.
pub const fn is_ite(id: u16) -> bool {
    matches!(id >> 8, 0x86 | 0x87)
}

/// The chip has register banks (bank select in [`BANK`]).
pub const fn has_banks(chip: Chip) -> bool {
    matches!(
        chip,
        Chip::It8686 | Chip::It8688 | Chip::It8689 | Chip::It8696 | Chip::It8698
    )
}

/// The chip has input 9, AVCC3.
pub const fn has_avcc3(chip: Chip) -> bool {
    matches!(
        chip,
        Chip::It8628 | Chip::It8686 | Chip::It8688 | Chip::It8689 | Chip::It8696 | Chip::It8698
    )
}

/// What the firmware set up, read once at start: the fans (bit n = fan n + 1) and temperatures
/// enabled, whether input 9 exists, and the voltage step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    pub fans: u8,
    pub temps: u8,
    pub avcc3: bool,
    pub lsb_uv: u16,
}

/// The registers [`setup`] needs, read in bank 0.
pub const SETUP_REGS: [u8; 3] = [FAN_MAIN_CTRL, FAN_16BIT, TEMP456];

/// The setup from [`SETUP_REGS`]: fans 1–3 from the main control register, fans 4–6 from the
/// 16-bit counter register, temperatures 1–3 always and 4–6 where their field is 2 or 3. A second
/// chip has fans and temperatures 1–3 only.
pub fn setup(chip: Chip, fan_main: u8, fan_16bit: u8, temp456: u8) -> Setup {
    let mut fans = (fan_main >> 4) & 0x07;
    let mut temps = 0x07;
    if !is_second(chip) {
        for (bit, fan) in [(4, 3), (5, 4), (2, 5)] {
            if fan_16bit & (1 << bit) != 0 {
                fans |= 1 << fan;
            }
        }
        for i in 0..3 {
            if (temp456 >> (2 * i)) & 0x03 >= 2 {
                temps |= 1 << (3 + i);
            }
        }
    }
    Setup {
        fans,
        temps,
        avcc3: has_avcc3(chip),
        lsb_uv: lsb_uv(chip),
    }
}

/// Every fan and temperature the chip has counted in, for `clokrix-live`'s dump.
pub fn all(chip: Chip) -> Setup {
    let n = if is_second(chip) { 0x07 } else { 0x3F };
    Setup {
        fans: n,
        temps: n,
        avcc3: has_avcc3(chip),
        lsb_uv: lsb_uv(chip),
    }
}

/// Every register [`decode`] reads with this setup, sorted.
pub fn registers(s: Setup) -> Vec<u8> {
    let mut regs: Vec<u8> = VIN[..if s.avcc3 { 10 } else { 9 }].to_vec();
    regs.extend(
        TEMP.iter()
            .enumerate()
            .filter(|(n, _)| s.temps & (1 << n) != 0)
            .map(|(_, &r)| r),
    );
    for n in (0..6).filter(|n| s.fans & (1 << n) != 0) {
        regs.extend([FAN_LOW[n], FAN_HIGH[n]]);
    }
    regs.sort_unstable();
    regs.dedup();
    regs
}

/// Reads one environment-controller register: its address to base + 5, the value from + 6.
pub fn read_reg<P: PortIo>(io: &mut P, base: u16, reg: u8) -> Result<u8, P::Error> {
    io.outb(base + ADDR_PORT, reg)?;
    io.inb(base + DATA_PORT)
}

/// Writes one environment-controller register.
pub fn write_reg<P: PortIo>(io: &mut P, base: u16, reg: u8, value: u8) -> Result<(), P::Error> {
    io.outb(base + ADDR_PORT, reg)?;
    io.outb(base + DATA_PORT, value)
}

/// Runs `f` in bank 0, which holds every register Clokrix reads or writes. On a chip with banks a
/// bank another program left selected is switched to bank 0 first and put back afterwards, also
/// when `f` failed; a bank register reading all ones is nothing answering, and is left alone. Call
/// with the ISA-bus mutex held.
pub fn in_bank0<P: PortIo, R, E: From<P::Error>>(
    io: &mut P,
    base: u16,
    banks: bool,
    f: impl FnOnce(&mut P) -> Result<R, E>,
) -> Result<R, E> {
    let other = if banks {
        Some(read_reg(io, base, BANK)?).filter(|&b| b & BANK_BITS != 0 && b != 0xFF)
    } else {
        None
    };
    if let Some(b) = other {
        write_reg(io, base, BANK, b & !BANK_BITS)?;
    }
    let out = f(io);
    let back = other.map_or(Ok(()), |b| write_reg(io, base, BANK, b));
    let out = out?;
    back?;
    Ok(out)
}

/// Reads `regs` (all in bank 0; see [`in_bank0`]). Call with the ISA-bus mutex held.
pub fn read<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    regs: &[u8],
) -> Result<Vec<(u8, u8)>, P::Error> {
    in_bank0(io, base, banks, |io| {
        regs.iter()
            .map(|&r| read_reg(io, base, r).map(|v| (r, v)))
            .collect()
    })
}

/// Whether the environment controller answers as an ITE one with monitoring running: its
/// vendor ID, and a configuration register that is not resetting and has monitoring started.
pub fn answers(vendor: u8, config: u8) -> Result<(), &'static str> {
    if !VENDOR_IDS.contains(&vendor) || config & 0x80 != 0 {
        Err("its monitor does not answer")
    } else if config & 0x01 == 0 {
        Err("the firmware left its hardware monitor off")
    } else {
        Ok(())
    }
}

/// °C from a temperature register; `None` outside what a board sensor reads (an open input reads
/// -128 or 127).
fn temperature(raw: u8) -> Option<f64> {
    #[allow(clippy::cast_possible_wrap)]
    let t = f64::from(raw as i8);
    (-40.0..=125.0).contains(&t).then_some(t)
}

/// RPM from a tachometer count: 1,350,000 / (2 × count). 0xFFFF is a stopped fan or an empty
/// header; a count under 64 (over 10,000 RPM) is not a reading.
pub fn rpm(count: u16) -> Option<f64> {
    match count {
        0xFFFF => Some(0.0),
        c if c < 0x40 => None,
        c => Some((675_000.0 / f64::from(c)).round()),
    }
}

/// Decodes the registers [`registers`] lists (`read` gives each one's value). Voltages are the
/// pin's in V with the chip's own halving of inputs 7–9 (+3.3V standby, the battery and AVCC3)
/// undone; the board's dividers are the layout's.
pub fn decode(s: Setup, read: impl Fn(u8) -> Option<u8>) -> Decoded {
    let volts = VIN[..if s.avcc3 { 10 } else { 9 }]
        .iter()
        .zip(0u16..)
        .filter_map(|(&reg, n)| {
            let halved = if n >= 7 { 2.0 } else { 1.0 };
            Some((
                n,
                f64::from(read(reg)?) * f64::from(s.lsb_uv) * halved / 1_000_000.0,
            ))
        })
        .collect();
    let temps = TEMP
        .iter()
        .zip(0u16..)
        .filter(|(_, n)| s.temps & (1 << n) != 0)
        .filter_map(|(&reg, n)| Some((n, temperature(read(reg)?))))
        .collect();
    let fans = (0..6)
        .filter(|n| s.fans & (1 << n) != 0)
        .filter_map(|n| {
            let count = u16::from_le_bytes([read(FAN_LOW[n])?, read(FAN_HIGH[n])?]);
            Some((u16::try_from(n).ok()?, rpm(count)))
        })
        .collect();
    Decoded { temps, volts, fans }
}

/// Name of a temperature input no board layout names (ITE's datasheets number them from 1).
pub fn temp_name(n: u16) -> String {
    format!("TMPIN{}", n + 1)
}

/// Name of a voltage input before a board names it: the chip's own rails by what they are, the
/// others by pin.
pub fn input_name(n: u16) -> String {
    match n {
        7 => "+3.3V Standby".into(),
        8 => "CMOS Battery".into(),
        9 => "AVCC3".into(),
        n => format!("VIN{n}"),
    }
}

/// What every Gigabyte mapping in LibreHardwareMonitor for these chips shares: input 1 the system
/// sensor, 3 the CPU as the board reads it, 4 the first PCIe slot, 5 the VRM MOSFETs; the CPU fan
/// and the first three chassis headers; Vcore, and +3.3V, +12V and +5V through dividers of 1.65,
/// 6 and 2.5. Input 2 is the chipset on most of them ([`gigabyte`] has the exceptions); the rest,
/// which differs from board to board, keeps the chip's names.
pub const GIGABYTE: Layout = Layout {
    temps: &[
        (0, "System", BoardTemp::System),
        (1, "Chipset", BoardTemp::Chipset),
        (2, "CPU", BoardTemp::None),
        (3, "PCIe x16", BoardTemp::Pcie),
        (4, "VRM MOS", BoardTemp::VrmMos),
    ],
    fans: &[
        (0, "CPU Fan", FanKind::Fan),
        (1, "System Fan 1", FanKind::Fan),
        (2, "System Fan 2", FanKind::Fan),
        (3, "System Fan 3", FanKind::Fan),
    ],
    volts: &[
        (0, "Vcore", 1.0),
        (1, "+3.3V", 1.65),
        (2, "+12V", 6.0),
        (3, "+5V", 2.5),
    ],
    volts_only: false,
};

/// X570 and B550 boards: the chipset's sensor is input 6 and input 2 differs between them.
pub const GIGABYTE_B550: Layout = Layout {
    temps: &[
        (0, "System", BoardTemp::System),
        (2, "CPU", BoardTemp::None),
        (3, "PCIe x16", BoardTemp::Pcie),
        (4, "VRM MOS", BoardTemp::VrmMos),
        (5, "Chipset", BoardTemp::Chipset),
    ],
    ..GIGABYTE
};

/// X570 boards also cool their chipset with a fan, on the fourth header.
pub const GIGABYTE_X570: Layout = Layout {
    fans: &[
        (0, "CPU Fan", FanKind::Fan),
        (1, "System Fan 1", FanKind::Fan),
        (2, "System Fan 2", FanKind::Fan),
        (3, "Chipset Fan", FanKind::Fan),
    ],
    ..GIGABYTE_B550
};

/// The layout of a Gigabyte board with one of these chips, from its SMBIOS product.
pub fn gigabyte(product: &str) -> Layout {
    if product.contains("X570") {
        GIGABYTE_X570
    } else if product.contains("B550") {
        GIGABYTE_B550
    } else {
        GIGABYTE
    }
}

/// The second chip on Gigabyte boards: what LibreHardwareMonitor's mappings of it share (its X570,
/// B550, Z390, Z690, Z790, X870 and X870E boards): fans 1 and 2 on the two headers that take a
/// pump too, fan 3 on the fourth chassis header; input 2 the header for an external sensor, 3 a
/// second system sensor. None of it goes into the board's own readings, which are the first
/// chip's. Its voltages differ from board to board and are named only on the boards
/// [`gigabyte_second`] knows; its standby rail and battery repeat the first chip's, so no other
/// voltage of it is shown.
pub const GIGABYTE_SECOND: Layout = Layout {
    temps: &[
        (1, "External 2", BoardTemp::None),
        (2, "System 2", BoardTemp::None),
    ],
    fans: &[
        (0, "System Fan 5 / Pump", FanKind::Fan),
        (1, "System Fan 6 / Pump", FanKind::Fan),
        (2, "System Fan 4", FanKind::Fan),
    ],
    volts: &[],
    volts_only: true,
};

const SECOND_PCIE_X8: &[(u16, &str, BoardTemp)] = &[
    (0, "PCIe x8", BoardTemp::None),
    (1, "External 2", BoardTemp::None),
    (2, "System 2", BoardTemp::None),
];

const SECOND_PCIE_X4: &[(u16, &str, BoardTemp)] = &[
    (0, "PCIe x4", BoardTemp::None),
    (1, "External 2", BoardTemp::None),
    (2, "System 2", BoardTemp::None),
];

/// X570 and B550 AORUS boards LibreHardwareMonitor maps.
pub const GIGABYTE_SECOND_AM4: Layout = Layout {
    temps: SECOND_PCIE_X8,
    volts: &[
        (1, "DDR VTT", 1.0),
        (2, "Chipset Core", 1.0),
        (4, "CPU VDD18", 1.0),
        (5, "PM_CLDO12", 1.0),
    ],
    ..GIGABYTE_SECOND
};

/// Z390 AORUS PRO.
pub const GIGABYTE_SECOND_Z390: Layout = Layout {
    temps: SECOND_PCIE_X8,
    volts: &[
        (1, "DDR VTT", 1.0),
        (2, "Chipset Core", 1.0),
        (4, "VCCIO", 1.0),
        (6, "DDR VPP", 1.0),
    ],
    ..GIGABYTE_SECOND
};

/// Z690 AORUS PRO and MASTER, Z790 AORUS PRO X.
pub const GIGABYTE_SECOND_Z690: Layout = Layout {
    temps: SECOND_PCIE_X4,
    volts: &[
        (1, "DIMM I/O", 1.0),
        (2, "Chipset +0.82V", 1.0),
        (4, "CPU System Agent", 1.0),
        (5, "Chipset +1.8V", 1.0),
    ],
    ..GIGABYTE_SECOND
};

/// X870E AORUS PRO, PRO ICE and XTREME AI TOP.
pub const GIGABYTE_SECOND_X870E: Layout = Layout {
    temps: SECOND_PCIE_X4,
    volts: &[
        (2, "PM_VCC18", 1.0),
        (4, "CPU VDD18", 1.0),
        (5, "PM_VDD1V", 1.0),
    ],
    ..GIGABYTE_SECOND
};

/// X870 AORUS ELITE WIFI7: its voltages are not named.
pub const GIGABYTE_SECOND_X870: Layout = Layout {
    temps: SECOND_PCIE_X4,
    ..GIGABYTE_SECOND
};

/// The second chip's layout on a Gigabyte board, from its SMBIOS product as LibreHardwareMonitor
/// matches it (the whole name).
pub fn gigabyte_second(product: &str) -> Layout {
    let p = product.trim().to_ascii_uppercase();
    let is = |names: &[&str]| names.contains(&p.as_str());
    if is(&[
        "X570 AORUS MASTER",
        "X570 AORUS PRO",
        "X570 AORUS ULTRA",
        "B550 AORUS MASTER",
        "B550 AORUS PRO",
        "B550 AORUS PRO V2",
        "B550 AORUS PRO AC",
        "B550 AORUS PRO AX",
        "B550 VISION D",
        "B550 VISION D-P",
    ]) {
        GIGABYTE_SECOND_AM4
    } else if is(&["Z390 AORUS PRO-CF"]) {
        GIGABYTE_SECOND_Z390
    } else if is(&[
        "Z690 AORUS PRO",
        "Z690 AORUS MASTER",
        "Z790 AORUS PRO X",
        "Z790 AORUS PRO X WIFI7",
    ]) {
        GIGABYTE_SECOND_Z690
    } else if is(&[
        "X870E AORUS PRO",
        "X870E AORUS PRO ICE",
        "X870E AORUS XTREME AI TOP",
    ]) {
        GIGABYTE_SECOND_X870E
    } else if is(&["X870 AORUS ELITE WIFI7", "X870 AORUS ELITE WIFI7 ICE"]) {
        GIGABYTE_SECOND_X870
    } else {
        GIGABYTE_SECOND
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const BASE: u16 = 0xA40;

    /// An ITE environment controller: address then data port, four banks of 0x10–0xAF.
    struct FakeEc {
        regs: HashMap<(u8, u8), u8>,
        bank_reg: u8,
        addr: u8,
        writes: Vec<(u8, u8)>,
    }

    impl FakeEc {
        fn new(bank_reg: u8, bank0: &[(u8, u8)]) -> Self {
            let mut regs: HashMap<(u8, u8), u8> = bank0.iter().map(|&(r, v)| ((0, r), v)).collect();
            // Bank 2 holds something else at the same addresses.
            for &(r, _) in bank0 {
                regs.insert((2, r), 0xEE);
            }
            Self {
                regs,
                bank_reg,
                addr: 0,
                writes: Vec::new(),
            }
        }

        fn bank(&self) -> u8 {
            (self.bank_reg & BANK_BITS) >> 5
        }
    }

    impl PortIo for FakeEc {
        type Error = ();
        fn inb(&mut self, port: u16) -> Result<u8, ()> {
            match port - BASE {
                DATA_PORT if self.addr == BANK => Ok(self.bank_reg),
                DATA_PORT => {
                    let bank = if (0x10..=0xAF).contains(&self.addr) {
                        self.bank()
                    } else {
                        0
                    };
                    Ok(*self.regs.get(&(bank, self.addr)).unwrap_or(&0))
                }
                _ => Err(()),
            }
        }
        fn outb(&mut self, port: u16, value: u8) -> Result<(), ()> {
            match port - BASE {
                ADDR_PORT => self.addr = value,
                DATA_PORT if self.addr == BANK => {
                    self.writes.push((BANK, value));
                    self.bank_reg = value;
                }
                // The monitor reader writes nothing else.
                _ => return Err(()),
            }
            Ok(())
        }
    }

    #[test]
    fn identifies_the_gigabyte_chips() {
        assert_eq!(identify(0x8689), Some(Chip::It8689));
        assert_eq!(identify(0x8686), Some(Chip::It8686));
        assert_eq!(identify(0x8696), Some(Chip::It8696));
        assert_eq!(identify(0x8698), Some(Chip::It8698));
        assert_eq!(identify(0x8620), Some(Chip::It8620));
        assert_eq!(
            identify(0x8733),
            None,
            "IT8792E: a second chip, not at 0x2E"
        );
        assert_eq!(identify(0xFFFF), None);
        assert_eq!(identify_second(0x8733), Some(Chip::It8792));
        assert_eq!(identify_second(0x8695), Some(Chip::It87952));
        assert_eq!(identify_second(0x8790), Some(Chip::It8790));
        assert_eq!(identify_second(0x8689), None, "a first chip");
        assert_eq!(
            identify_second(0x8883),
            None,
            "what an IT8792E answers when sent the key it did not need"
        );
        assert!(is_second(Chip::It87952));
        assert!(!is_second(Chip::It8689));
        assert!(!has_banks(Chip::It8792));
        assert!(!has_avcc3(Chip::It87952));
        assert_eq!(lsb_uv(Chip::It8792), 11_000);
        assert_eq!(lsb_uv(Chip::It8790), 12_000);
        assert_eq!(lsb_uv(Chip::It8689), 12_000);
        assert_eq!(Chip::It87952.name(), "ITE IT87952E");
        assert_eq!(ENTER_4E[3], 0xAA);
        assert!(is_ite(0x8733));
        assert!(
            is_ite(0x8613),
            "an ITE chip Clokrix does not read still leaves the ITE way"
        );
        assert!(!is_ite(0xFFFF));
        assert!(!is_ite(0xD592), "a Nuvoton chip");
        assert!(has_banks(Chip::It8689));
        assert!(!has_banks(Chip::It8628));
        assert!(has_avcc3(Chip::It8628));
        assert!(!has_avcc3(Chip::It8620));
        assert_eq!(Chip::It8689.name(), "ITE IT8689E");
    }

    #[test]
    fn setup_reads_the_enable_bits() {
        // Fans 1 and 2 on (bits 6:4 = 011), fan 4 and fan 6 on; inputs 4 and 5 temperatures
        // (fields 3 and 2), input 6 a voltage (field 1). Constructed values.
        let s = setup(Chip::It8689, 0x37, 0x10 | 0x04, 0b01_10_11);
        assert_eq!(s.fans, 0b10_1011);
        assert_eq!(s.temps, 0b01_1111);
        assert!(s.avcc3);
        let none = setup(Chip::It8620, 0x00, 0x00, 0x00);
        assert_eq!((none.fans, none.temps, none.avcc3), (0, 0x07, false));
        // A second chip: fans and temperatures 1–3 only, whatever registers 0x0C and 0x77 hold.
        let second = setup(Chip::It87952, 0x77, 0xFF, 0xFF);
        assert_eq!((second.fans, second.temps), (0b111, 0b111));
        assert_eq!(second.lsb_uv, 11_000);
        assert_eq!(all(Chip::It8792).fans, 0x07);
        assert_eq!(all(Chip::It8689).temps, 0x3F);
        let regs = registers(second);
        assert!(!regs.contains(&0x80) && !regs.contains(&0x2C) && !regs.contains(&0x2F));
        assert_eq!(
            regs.len(),
            9 + 3 + 6,
            "nine inputs, three temperatures, three fans"
        );
    }

    #[test]
    fn registers_cover_what_is_enabled() {
        let s = Setup {
            fans: 0b00_0011,
            temps: 0b00_1111,
            avcc3: false,
            lsb_uv: LSB_UV,
        };
        let regs = registers(s);
        assert!(regs.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
        for r in [0x20, 0x28, 0x29, 0x2C, 0x0D, 0x18, 0x0E, 0x19] {
            assert!(regs.contains(&r), "{r:#x}");
        }
        for r in [0x2F, 0x2D, 0x0F, 0x80] {
            assert!(!regs.contains(&r), "{r:#x}");
        }
        assert!(registers(Setup { avcc3: true, ..s }).contains(&0x2F));
    }

    #[test]
    fn reads_in_bank_zero_and_puts_another_bank_back() {
        let bank0 = [(0x20, 0x70), (0x29, 33), (0x0D, 0x68)];
        // Another program left bank 2 selected (bits 6:5 = 10) with bit 0 set.
        let mut ec = FakeEc::new(0x41, &bank0);
        let got = read(&mut ec, BASE, true, &[0x0D, 0x20, 0x29]).expect("read");
        assert_eq!(got, [(0x0D, 0x68), (0x20, 0x70), (0x29, 33)]);
        assert_eq!(ec.writes, [(BANK, 0x01), (BANK, 0x41)], "bank 0, then back");
        assert_eq!(ec.bank_reg, 0x41);
        // Already in bank 0: nothing is written.
        let mut ec = FakeEc::new(0x01, &bank0);
        read(&mut ec, BASE, true, &[0x20]).expect("read");
        assert!(ec.writes.is_empty());
        // A chip without banks never has its register 0x06 touched.
        let mut ec = FakeEc::new(0x41, &bank0);
        let got = read(&mut ec, BASE, false, &[0x20]).expect("read");
        assert!(ec.writes.is_empty());
        assert_eq!(got, [(0x20, 0xEE)], "whatever the selected bank holds");
        // Nothing answering (all ones): nothing is written.
        let mut ec = FakeEc::new(0xFF, &bank0);
        read(&mut ec, BASE, true, &[0x20]).expect("read");
        assert!(ec.writes.is_empty());
    }

    #[test]
    fn the_monitor_must_answer_and_run() {
        assert_eq!(answers(0x90, 0x11), Ok(()));
        assert_eq!(answers(0x7F, 0x13), Ok(()));
        assert!(answers(0xFF, 0xFF).is_err(), "nothing at the base");
        assert!(answers(0x90, 0x91).is_err(), "resetting");
        assert_eq!(
            answers(0x90, 0x10),
            Err("the firmware left its hardware monitor off")
        );
    }

    #[test]
    fn decodes_fans() {
        assert_eq!(rpm(0x021C), Some(1250.0), "540 counts");
        assert_eq!(rpm(0x0040), Some(10547.0));
        assert_eq!(rpm(0x003F), None);
        assert_eq!(rpm(0xFFFF), Some(0.0), "stopped or empty");
        assert_eq!(rpm(0), None);
    }

    /// A constructed IT8689E dump in the arrangement LibreHardwareMonitor's Gigabyte B650
    /// mappings describe (no field dump from a Gigabyte board yet): Vcore 1.224 V, +3.3V through
    /// its divider, +12V and +5V, the standby rail and battery halved inside the chip; System
    /// 34 °C, chipset 47, CPU 52, PCIe 35, VRM MOS 41, input 6 open (-128); the CPU fan at
    /// 1,250 RPM, the first chassis fan stopped, the pump header (fan 5) at 2,700.
    fn b650_dump() -> HashMap<u8, u8> {
        HashMap::from([
            (0x20, 102), // 1.224 V
            (0x21, 167), // 2.004 V × 1.65 = 3.307 V
            (0x22, 168), // 2.016 V × 6 = 12.096 V
            (0x23, 167), // 2.004 V × 2.5 = 5.01 V
            (0x24, 104),
            (0x27, 137), // 1.644 V × 2 = 3.288 V
            (0x28, 128), // 1.536 V × 2 = 3.072 V
            (0x2F, 138),
            (0x29, 34),
            (0x2A, 47),
            (0x2B, 52),
            (0x2C, 35),
            (0x2D, 41),
            (0x2E, 0x80),
            (0x0D, 0x1C),
            (0x18, 0x02),
            (0x0E, 0xFF),
            (0x19, 0xFF),
            (0x82, 0xFA),
            (0x83, 0x00),
        ])
    }

    #[test]
    fn decodes_a_gigabyte_dump() {
        let regs = b650_dump();
        let s = setup(Chip::It8689, 0x70, 0x20, 0b10_10_10);
        let d = decode(s, |r| regs.get(&r).copied().or(Some(0)));
        let v = |n: u16| d.volts.iter().find(|x| x.0 == n).expect("input").1;
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(v(0), 1.224));
        assert!(close(v(1) * 1.65, 3.3066));
        assert!(close(v(2) * 6.0, 12.096));
        assert!(close(v(3) * 2.5, 5.01));
        assert!(close(v(7), 3.288), "standby rail halved inside");
        assert!(close(v(8), 3.072), "battery halved inside");
        assert!(close(v(9), 3.312), "AVCC3 halved inside");
        assert_eq!(d.volts.len(), 10);
        assert_eq!(
            d.temps,
            [
                (0, Some(34.0)),
                (1, Some(47.0)),
                (2, Some(52.0)),
                (3, Some(35.0)),
                (4, Some(41.0)),
                (5, None),
            ]
        );
        // Fan 3 reads count 0 here (no register in the dump): not a reading.
        assert_eq!(
            d.fans,
            [
                (0, Some(1250.0)),
                (1, Some(0.0)),
                (2, None),
                (4, Some(2700.0))
            ]
        );
    }

    #[test]
    fn a_chip_without_avcc3_reads_nine_inputs() {
        let regs = b650_dump();
        let d = decode(setup(Chip::It8620, 0x10, 0, 0), |r| regs.get(&r).copied());
        assert_eq!(d.volts.len(), 7, "inputs 5 and 6 absent from the dump");
        assert!(d.volts.iter().all(|v| v.0 < 9));
        assert_eq!(d.temps.len(), 3);
        assert_eq!(d.fans, [(0, Some(1250.0))]);
    }

    /// A constructed IT87952E dump (no field dump of a second chip yet): VIN1 1.2 V, VIN4 1.804 V,
    /// the standby rail and battery halved inside; PCIe 38 °C, the external input open, System 2
    /// 31 °C; a pump at 2,700 RPM on fan 1, fan 2 stopped, fan 3 at 900.
    fn second_dump() -> HashMap<u8, u8> {
        HashMap::from([
            (0x21, 109), // 1.199 V at 11 mV
            (0x24, 164), // 1.804 V
            (0x27, 150), // 1.65 V × 2 = 3.3 V
            (0x28, 140), // 1.54 V × 2 = 3.08 V
            (0x29, 38),
            (0x2A, 0x80),
            (0x2B, 31),
            (0x0D, 0xFA),
            (0x18, 0x00),
            (0x0E, 0xFF),
            (0x19, 0xFF),
            (0x0F, 0xEE),
            (0x1A, 0x02),
        ])
    }

    #[test]
    fn decodes_a_second_chip() {
        let regs = second_dump();
        let s = setup(Chip::It87952, 0x70, 0, 0);
        let d = decode(s, |r| regs.get(&r).copied().or(Some(0)));
        let v = |n: u16| d.volts.iter().find(|x| x.0 == n).expect("input").1;
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert_eq!(d.volts.len(), 9, "no AVCC3");
        assert!(close(v(1), 1.199));
        assert!(close(v(4), 1.804));
        assert!(close(v(7), 3.3), "standby rail halved inside");
        assert!(close(v(8), 3.08), "battery halved inside");
        assert_eq!(d.temps, [(0, Some(38.0)), (1, None), (2, Some(31.0))]);
        assert_eq!(
            d.fans,
            [(0, Some(2700.0)), (1, Some(0.0)), (2, Some(900.0))]
        );
    }

    #[test]
    fn gigabyte_second_chip_layouts() {
        let am4 = gigabyte_second("B550 AORUS PRO AC");
        assert_eq!(am4.temps[0], (0, "PCIe x8", BoardTemp::None));
        assert_eq!(am4.volts[0], (1, "DDR VTT", 1.0));
        assert!(am4.volts_only);
        assert_eq!(gigabyte_second(" x570 aorus master ").volts, am4.volts);
        assert_eq!(
            gigabyte_second("B550 AORUS ELITE").volts,
            GIGABYTE_SECOND.volts,
            "whole names only: another B550 board's voltages are not named"
        );
        let z690 = gigabyte_second("Z790 AORUS PRO X WIFI7");
        assert_eq!(z690.temps[0].1, "PCIe x4");
        assert_eq!(z690.volts[2], (4, "CPU System Agent", 1.0));
        assert_eq!(
            gigabyte_second("Z390 AORUS PRO-CF").volts[3],
            (6, "DDR VPP", 1.0)
        );
        assert_eq!(gigabyte_second("X870E AORUS PRO ICE").volts.len(), 3);
        let x870 = gigabyte_second("X870 AORUS ELITE WIFI7");
        assert!(x870.volts.is_empty());
        assert_eq!(x870.temps[0].1, "PCIe x4");
        let other = gigabyte_second("Z890 AORUS ELITE WIFI7");
        assert!(other.volts.is_empty() && other.volts_only);
        assert!(
            other.temps.iter().all(|t| t.0 != 0),
            "input 1 unnamed elsewhere"
        );
        for l in [am4, z690, x870, other] {
            assert_eq!(l.fans[0], (0, "System Fan 5 / Pump", FanKind::Fan));
            assert_eq!(l.fans[2].1, "System Fan 4");
            assert!(
                l.temps.iter().all(|t| t.2 == BoardTemp::None),
                "the board's own readings stay the first chip's"
            );
        }
    }

    #[test]
    fn gigabyte_layouts() {
        let l = gigabyte("B650 AORUS ELITE AX");
        assert_eq!(l.temps[1], (1, "Chipset", BoardTemp::Chipset));
        assert_eq!(l.volts[2], (2, "+12V", 6.0));
        let b550 = gigabyte("B550 AORUS PRO AC");
        assert!(
            b550.temps.iter().all(|t| t.0 != 1),
            "input 2 differs on B550"
        );
        assert_eq!(b550.temps[4], (5, "Chipset", BoardTemp::Chipset));
        assert_eq!(b550.fans[3].1, "System Fan 3");
        let x570 = gigabyte("X570 AORUS MASTER");
        assert_eq!(x570.fans[3].1, "Chipset Fan");
        assert_eq!(x570.temps, b550.temps);
        assert_eq!(temp_name(5), "TMPIN6");
        assert_eq!(input_name(4), "VIN4");
        assert_eq!(input_name(8), "CMOS Battery");
        assert_eq!(input_name(9), "AVCC3");
    }
}

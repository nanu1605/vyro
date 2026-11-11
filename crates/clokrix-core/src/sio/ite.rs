//! ITE environment controllers (2.3): the hardware monitor of Gigabyte boards, from the IT8620E
//! and IT8628E of their 9- and 100-series boards to the IT8686E–IT8689E of the AM4, AM5 and
//! LGA1700 ones and the IT8696E and IT8698E of the newest. The register facts are ITE's, as the
//! Linux `it87` driver (and its out-of-tree version, which knows the newer chips) and
//! LibreHardwareMonitor's `IT87XX` read them. Pure: the port I/O is behind [`PortIo`], so the
//! access sequence is tested without hardware.
//!
//! Configuration mode on 0x2E opens with 0x87 0x01 0x55 0x55 and closes by writing 0x02 to
//! register 0x02. The environment controller is logical device 4; its registers are reached by
//! writing the register's address to its base + 5 and reading the value at + 6. Only the chip at
//! 0x2E is read. The second ITE chip of many Gigabyte boards (an IT8792E or IT87952E at 0x4E, with
//! a few more fans and temperatures) sits behind the board's embedded controller, which would
//! have to be stopped first, so it is left alone.
//!
//! Monitoring only reads. One write is made, as the Linux driver makes it: on the chips with
//! register banks, a bank another program left selected is switched to bank 0, which holds every
//! register read here, and put back after the read.

use super::{BoardTemp, Chip, Decoded, Layout, PortIo};
use crate::snapshot::FanKind;

/// Opens configuration mode on 0x2E (on 0x4E the last byte would be 0xAA).
pub const ENTER_2E: [u8; 4] = [0x87, 0x01, 0x55, 0x55];
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
/// Millivolts per count on every chip read here.
pub const MV_PER_COUNT: f64 = 12.0;

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
/// enabled, and whether input 9 exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Setup {
    pub fans: u8,
    pub temps: u8,
    pub avcc3: bool,
}

/// The registers [`setup`] needs, read in bank 0.
pub const SETUP_REGS: [u8; 3] = [FAN_MAIN_CTRL, FAN_16BIT, TEMP456];

/// The setup from [`SETUP_REGS`]: fans 1–3 from the main control register, fans 4–6 from the
/// 16-bit counter register, temperatures 1–3 always and 4–6 where their field is 2 or 3.
pub fn setup(chip: Chip, fan_main: u8, fan_16bit: u8, temp456: u8) -> Setup {
    let mut fans = (fan_main >> 4) & 0x07;
    for (bit, fan) in [(4, 3), (5, 4), (2, 5)] {
        if fan_16bit & (1 << bit) != 0 {
            fans |= 1 << fan;
        }
    }
    let mut temps = 0x07;
    for i in 0..3 {
        if (temp456 >> (2 * i)) & 0x03 >= 2 {
            temps |= 1 << (3 + i);
        }
    }
    Setup {
        fans,
        temps,
        avcc3: has_avcc3(chip),
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

fn read1<P: PortIo>(io: &mut P, base: u16, reg: u8) -> Result<u8, P::Error> {
    io.outb(base + ADDR_PORT, reg)?;
    io.inb(base + DATA_PORT)
}

fn write1<P: PortIo>(io: &mut P, base: u16, reg: u8, value: u8) -> Result<(), P::Error> {
    io.outb(base + ADDR_PORT, reg)?;
    io.outb(base + DATA_PORT, value)
}

/// Reads `regs` (all in bank 0). On a chip with banks a bank another program left selected is
/// switched to bank 0 for the read and put back afterwards, also when a read failed; a bank
/// register reading all ones is nothing answering, and is left alone. Call with the ISA-bus mutex
/// held.
pub fn read<P: PortIo>(
    io: &mut P,
    base: u16,
    banks: bool,
    regs: &[u8],
) -> Result<Vec<(u8, u8)>, P::Error> {
    let other = if banks {
        Some(read1(io, base, BANK)?).filter(|&b| b & BANK_BITS != 0 && b != 0xFF)
    } else {
        None
    };
    if let Some(b) = other {
        write1(io, base, BANK, b & !BANK_BITS)?;
    }
    let out = regs
        .iter()
        .map(|&r| read1(io, base, r).map(|v| (r, v)))
        .collect::<Result<Vec<_>, _>>();
    let back = other.map_or(Ok(()), |b| write1(io, base, BANK, b));
    let out = out?;
    back?;
    Ok(out)
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
            Some((n, f64::from(read(reg)?) * MV_PER_COUNT * halved / 1000.0))
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
        assert_eq!(identify(0x8733), None, "IT8792E: the second chip, not read");
        assert_eq!(identify(0xFFFF), None);
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
    }

    #[test]
    fn registers_cover_what_is_enabled() {
        let s = Setup {
            fans: 0b00_0011,
            temps: 0b00_1111,
            avcc3: false,
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

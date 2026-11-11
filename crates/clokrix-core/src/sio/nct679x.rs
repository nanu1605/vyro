//! Nuvoton NCT6791D to NCT6799D, and the NCT6701D read as a NCT6799D (2.1): the hardware
//! monitor of most ASUS boards and many ASRock ones. Unlike the NCT6687's EC space its registers
//! are banked: the bank goes to register 0x4E, then the register's address to the monitor base + 5
//! and its value comes back at + 6.
//! The register tables and scales are the Linux `nct6775` driver's. Pure: the port I/O is behind
//! [`PortIo`], so the access sequence is tested without hardware.
//!
//! Temperatures are read the way that driver reads them: each temperature register reports the
//! source its source register names (the firmware decides which sensor feeds which register), so
//! a reading is keyed by its source, not by the register it came from.

use super::{Chip, Decoded, PortIo};

/// Address and data ports, from the monitor base.
pub const ADDR_PORT: u16 = 5;
pub const DATA_PORT: u16 = 6;
/// Bank select, present in every bank. Bit 7 set reads the high byte of the vendor ID.
pub const BANK_SELECT: u8 = 0x4E;
/// Vendor ID, 0x5CA3: the high byte with bit 7 of the bank select set, the low byte without.
pub const VENDOR_ID: u8 = 0x4F;
pub const NUVOTON: u16 = 0x5CA3;
/// Super-I/O global option register; bit 4 set locks the monitor's I/O ports (firmware sets it on
/// some boards; reads then return nothing).
pub const CR_GLOBAL_OPTION: u8 = 0x28;
pub const HM_IO_LOCK: u8 = 0x10;
/// The selected logical device's enable register, bit 0.
pub const CR_ENABLE: u8 = 0x30;
/// Bit 0 set: the battery input is sampled.
pub const VBAT_MONITOR: u16 = 0x5D;

/// One chip's monitor registers.
#[derive(Debug)]
pub struct Tables {
    /// Voltage inputs, in input order (8-bit counts).
    pub volts: &'static [u16],
    /// Millivolts per count for each input (16 where the chip halves the input inside).
    pub scale_mv: &'static [u16],
    /// Fan speeds: 16-bit RPM, high byte first.
    pub fans: &'static [u16],
    /// Temperature registers with the register naming their source, and whether a low byte
    /// with the half degree in bit 7 follows the register.
    pub temps: &'static [(u16, u16, bool)],
    /// Temperatures the fan controls use, with their source-select registers.
    pub mon: &'static [(u16, u16, bool)],
    /// Sources this chip has.
    pub temp_mask: u32,
    /// Virtual sources (a fan curve's own mix, not a sensor): never shown.
    pub virt_mask: u32,
}

const VOLTS_15: [u16; 15] = [
    0x480, 0x481, 0x482, 0x483, 0x484, 0x485, 0x486, 0x487, 0x488, 0x489, 0x48A, 0x48B, 0x48C,
    0x48D, 0x48E,
];
const VOLTS_18: [u16; 18] = [
    0x480, 0x481, 0x482, 0x483, 0x484, 0x485, 0x486, 0x487, 0x488, 0x489, 0x48A, 0x48B, 0x48C,
    0x48D, 0x48E, 0x48F, 0x470, 0x471,
];
/// Linux `scale_in`: AVSB, 3VCC, 3VSB and VBAT are halved inside the chip.
const SCALE: [u16; 15] = [8, 8, 16, 16, 8, 8, 8, 16, 16, 8, 8, 8, 8, 8, 8];
/// Linux `scale_in_6798`: VTT and VIN5, VIN6 and VHIF halved too.
const SCALE_6798: [u16; 18] = [8, 8, 16, 16, 8, 8, 8, 16, 16, 16, 16, 16, 8, 8, 8, 8, 16, 8];
const FANS_6: [u16; 6] = [0x4C0, 0x4C2, 0x4C4, 0x4C6, 0x4C8, 0x4CA];
const FANS_7: [u16; 7] = [0x4C0, 0x4C2, 0x4C4, 0x4C6, 0x4C8, 0x4CA, 0x4CE];
const TEMPS_6779: [(u16, u16, bool); 2] = [(0x027, 0x621, false), (0x150, 0x622, true)];
const TEMPS_6798: [(u16, u16, bool); 8] = [
    (0x027, 0x621, false),
    (0x150, 0x622, true),
    (0x670, 0xC26, false),
    (0x672, 0xC27, false),
    (0x674, 0xC28, false),
    (0x676, 0xC29, false),
    (0x678, 0xC2A, false),
    (0x67A, 0xC2B, false),
];
const MON_5: [(u16, u16, bool); 5] = [
    (0x073, 0x100, true),
    (0x075, 0x200, true),
    (0x077, 0x300, true),
    (0x079, 0x800, true),
    (0x07B, 0x900, true),
];
const MON_6: [(u16, u16, bool); 6] = [
    (0x073, 0x100, true),
    (0x075, 0x200, true),
    (0x077, 0x300, true),
    (0x079, 0x800, true),
    (0x07B, 0x900, true),
    (0x07D, 0xA00, true),
];
const MON_7: [(u16, u16, bool); 7] = [
    (0x073, 0x100, true),
    (0x075, 0x200, true),
    (0x077, 0x300, true),
    (0x079, 0x800, true),
    (0x07B, 0x900, true),
    (0x07D, 0xA00, true),
    (0x4A0, 0xB00, false),
];

const fn table(
    volts: &'static [u16],
    scale_mv: &'static [u16],
    fans: &'static [u16],
    temps: &'static [(u16, u16, bool)],
    mon: &'static [(u16, u16, bool)],
    temp_mask: u32,
    virt_mask: u32,
) -> Tables {
    Tables {
        volts,
        scale_mv,
        fans,
        temps,
        mon,
        temp_mask,
        virt_mask,
    }
}

static NCT6791: Tables = table(
    &VOLTS_15,
    &SCALE,
    &FANS_6,
    &TEMPS_6779,
    &MON_5,
    0x87FF_FF7E,
    0x8000_0000,
);
static NCT6792: Tables = table(
    &VOLTS_15,
    &SCALE,
    &FANS_6,
    &TEMPS_6779,
    &MON_6,
    0x9FFF_FF7E,
    0x8000_0000,
);
static NCT6793: Tables = table(
    &VOLTS_15,
    &SCALE,
    &FANS_6,
    &TEMPS_6779,
    &MON_6,
    0xBFFF_037E,
    0x8000_0000,
);
static NCT6795: Tables = table(
    &VOLTS_15,
    &SCALE,
    &FANS_6,
    &TEMPS_6779,
    &MON_6,
    0xBFFF_FF7E,
    0x8000_0000,
);
static NCT6796: Tables = table(
    &VOLTS_15,
    &SCALE,
    &FANS_7,
    &TEMPS_6779,
    &MON_6,
    0xBFFF_0FFE,
    0x8000_0C00,
);
static NCT6797: Tables = table(
    &VOLTS_15,
    &SCALE,
    &FANS_7,
    &TEMPS_6779,
    &MON_6,
    0xBFFF_FF7E,
    0x8000_0000,
);
static NCT6798: Tables = table(
    &VOLTS_15,
    &SCALE_6798,
    &FANS_7,
    &TEMPS_6798,
    &MON_7,
    0xBFFF_0FFE,
    0x8000_0C00,
);
static NCT6799: Tables = table(
    &VOLTS_18,
    &SCALE_6798,
    &FANS_7,
    &TEMPS_6798,
    &MON_7,
    0xBFFF_2FFE,
    0x8000_0C00,
);

/// The register tables of a banked Nuvoton chip; `None` for others.
pub const fn tables(chip: Chip) -> Option<&'static Tables> {
    match chip {
        Chip::Nct6791 => Some(&NCT6791),
        Chip::Nct6792 => Some(&NCT6792),
        Chip::Nct6793 => Some(&NCT6793),
        Chip::Nct6795 => Some(&NCT6795),
        Chip::Nct6796 => Some(&NCT6796),
        Chip::Nct6797 => Some(&NCT6797),
        Chip::Nct6798 => Some(&NCT6798),
        Chip::Nct6799 | Chip::Nct6701 => Some(&NCT6799),
        _ => None,
    }
}

/// Every register [`decode`] reads, sorted (so by bank: one bank change each).
pub fn registers(t: &Tables) -> Vec<u16> {
    let mut regs: Vec<u16> = t.volts.to_vec();
    regs.push(VBAT_MONITOR);
    for &f in t.fans {
        regs.extend([f, f + 1]);
    }
    for &(reg, src, word) in t.temps.iter().chain(t.mon) {
        regs.extend([reg, src]);
        if word {
            regs.push(reg + 1);
        }
    }
    regs.sort_unstable();
    regs.dedup();
    regs
}

/// Bank-switching register reads; tracks the bank so each switch is written once.
struct Banked<'a, P: PortIo> {
    io: &'a mut P,
    base: u16,
    bank: Option<u8>,
}

impl<P: PortIo> Banked<'_, P> {
    fn select(&mut self, bank: u8) -> Result<(), P::Error> {
        if self.bank != Some(bank) {
            self.io.outb(self.base + ADDR_PORT, BANK_SELECT)?;
            self.io.outb(self.base + DATA_PORT, bank)?;
            self.bank = Some(bank);
        }
        Ok(())
    }

    fn read(&mut self, addr: u16) -> Result<u8, P::Error> {
        let [bank, reg] = addr.to_be_bytes();
        self.select(bank)?;
        self.io.outb(self.base + ADDR_PORT, reg)?;
        self.io.inb(self.base + DATA_PORT)
    }

    fn write(&mut self, addr: u16, value: u8) -> Result<(), P::Error> {
        let [bank, reg] = addr.to_be_bytes();
        self.select(bank)?;
        self.io.outb(self.base + ADDR_PORT, reg)?;
        self.io.outb(self.base + DATA_PORT, value)
    }

    /// Leaves bank 0 selected, as the firmware does, for tools that assume it.
    fn done(mut self) -> Result<(), P::Error> {
        self.select(0)
    }
}

/// Reads one register (bank << 8 | register), then selects bank 0 again. Call with the ISA-bus
/// mutex held.
pub fn read_reg<P: PortIo>(io: &mut P, base: u16, addr: u16) -> Result<u8, P::Error> {
    let mut b = Banked {
        io,
        base,
        bank: None,
    };
    let value = b.read(addr);
    let done = b.done();
    let value = value?;
    done?;
    Ok(value)
}

/// Writes one register, then selects bank 0 again: fan control's write path
/// ([`super::control679x`]) and nothing else. Call with the ISA-bus mutex held.
pub fn write_reg<P: PortIo>(io: &mut P, base: u16, addr: u16, value: u8) -> Result<(), P::Error> {
    let mut b = Banked {
        io,
        base,
        bank: None,
    };
    let wrote = b.write(addr, value);
    let done = b.done();
    wrote?;
    done
}

/// Reads `regs` (bank << 8 | register). Call with the ISA-bus mutex held.
pub fn read<P: PortIo>(io: &mut P, base: u16, regs: &[u16]) -> Result<Vec<(u16, u8)>, P::Error> {
    let mut b = Banked {
        io,
        base,
        bank: None,
    };
    let out = regs
        .iter()
        .map(|&r| b.read(r).map(|v| (r, v)))
        .collect::<Result<Vec<_>, _>>();
    // Back to bank 0 even when a read failed, then report the failure.
    let done = b.done();
    let out = out?;
    done?;
    Ok(out)
}

/// The vendor ID as the monitor reports it: 0x5CA3 when its ports answer.
pub fn vendor<P: PortIo>(io: &mut P, base: u16) -> Result<u16, P::Error> {
    let mut b = Banked {
        io,
        base,
        bank: None,
    };
    b.select(0x80)?;
    b.io.outb(base + ADDR_PORT, VENDOR_ID)?;
    let high = b.io.inb(base + DATA_PORT)?;
    b.select(0)?;
    b.io.outb(base + ADDR_PORT, VENDOR_ID)?;
    let low = b.io.inb(base + DATA_PORT)?;
    Ok(u16::from_be_bytes([high, low]))
}

/// °C from a temperature register, plus its low byte where it has one; `None` outside what a
/// board sensor reads (an open input reads -128 or 127).
fn temperature(high: u8, low: Option<u8>) -> Option<f64> {
    #[allow(clippy::cast_possible_wrap)]
    let t = f64::from(high as i8) + low.map_or(0.0, |l| if l & 0x80 != 0 { 0.5 } else { 0.0 });
    (-40.0..=125.0).contains(&t).then_some(t)
}

/// Decodes a register dump (`read` gives each register's value). Temperatures are keyed by
/// source and come first from the temperature registers, then from the fan controls' inputs,
/// each source once; voltages are the pin's in V (the board's dividers are the layout's); a
/// battery input that is not sampled reads 0.
pub fn decode(t: &Tables, read: impl Fn(u16) -> Option<u8>) -> Decoded {
    let mut seen = 0u32;
    let mut temps = Vec::new();
    for &(reg, src_reg, word) in t.temps.iter().chain(t.mon) {
        let Some(src) = read(src_reg).map(|s| s & 0x1F) else {
            continue;
        };
        let bit = 1u32 << src;
        if src == 0 || t.temp_mask & bit == 0 || t.virt_mask & bit != 0 || seen & bit != 0 {
            continue;
        }
        let Some(high) = read(reg) else {
            continue;
        };
        let low = if word { read(reg + 1) } else { None };
        seen |= bit;
        temps.push((u16::from(src), temperature(high, low)));
    }
    let vbat_on = read(VBAT_MONITOR).is_some_and(|v| v & 1 != 0);
    let volts = t
        .volts
        .iter()
        .zip(t.scale_mv)
        .enumerate()
        .filter_map(|(n, (&reg, &mv))| {
            let n = u16::try_from(n).ok()?;
            let v = f64::from(read(reg)?) * f64::from(mv) / 1000.0;
            // Input 8 is VBAT, sampled only while its monitor bit is set.
            Some((n, if n == 8 && !vbat_on { 0.0 } else { v }))
        })
        .collect();
    let fans = t
        .fans
        .iter()
        .enumerate()
        .filter_map(|(n, &reg)| {
            let n = u16::try_from(n).ok()?;
            let rpm = u16::from_be_bytes([read(reg)?, read(reg + 1)?]);
            Some((n, (rpm != 0xFFFF).then_some(f64::from(rpm))))
        })
        .collect();
    Decoded { temps, volts, fans }
}

/// Name of a temperature source, as the chip's datasheet and Linux call it.
pub fn source_name(src: u16) -> String {
    match src {
        1 => "SYSTIN".into(),
        2 => "CPUTIN".into(),
        3..=7 => format!("AUXTIN{}", src - 3),
        13 => "AUXTIN5".into(),
        8..=15 => format!("SMBus master {}", src - 8),
        16 | 17 => format!("PECI agent {}", src - 16),
        18 => "PCH CPU max".into(),
        19 => "PCH".into(),
        20 => "PCH CPU".into(),
        21 => "PCH MCH".into(),
        22..=25 => format!("DIMM {}", src - 22),
        26 | 27 => format!("Byte temperature {}", src - 26),
        28 | 29 => format!("PECI agent {} calibrated", src - 28),
        n => format!("Source {n}"),
    }
}

/// Name of a voltage input before a board names it: the chip's own rails by what they are,
/// the others by pin (Linux's labels: CPUVC, VIN1, AVSB, 3VCC, VIN0, VIN8, VIN4, 3VSB, VBAT,
/// VTT, VIN5, VIN6, VIN2, VIN3, VIN7, VIN9, VHIF, VIN10).
pub fn input_name(n: u16) -> String {
    match n {
        0 => "Vcore",
        1 => "VIN1",
        2 => "AVSB",
        3 => "+3.3V",
        4 => "VIN0",
        5 => "VIN8",
        6 => "VIN4",
        7 => "+3.3V Standby",
        8 => "CMOS Battery",
        9 => "VTT",
        10 => "VIN5",
        11 => "VIN6",
        12 => "VIN2",
        13 => "VIN3",
        14 => "VIN7",
        15 => "VIN9",
        16 => "VHIF",
        17 => "VIN10",
        _ => return format!("VIN#{n}"),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A banked Nuvoton monitor: bank select at 0x4E, address then data ports.
    struct FakeMonitor {
        regs: HashMap<u16, u8>,
        bank: u8,
        addr: u8,
        writes: Vec<(u16, u8)>,
    }

    impl FakeMonitor {
        fn new(regs: HashMap<u16, u8>) -> Self {
            Self {
                regs,
                bank: 0,
                addr: 0,
                writes: Vec::new(),
            }
        }
    }

    const BASE: u16 = 0x290;

    impl PortIo for FakeMonitor {
        type Error = ();
        fn inb(&mut self, port: u16) -> Result<u8, ()> {
            match port - BASE {
                DATA_PORT if self.addr == BANK_SELECT => Ok(self.bank),
                DATA_PORT if self.addr == VENDOR_ID => {
                    Ok(if self.bank & 0x80 != 0 { 0x5C } else { 0xA3 })
                }
                DATA_PORT => Ok(*self
                    .regs
                    .get(&u16::from_be_bytes([self.bank & 0x7F, self.addr]))
                    .unwrap_or(&0)),
                _ => Err(()),
            }
        }
        fn outb(&mut self, port: u16, value: u8) -> Result<(), ()> {
            self.writes.push((port - BASE, value));
            match port - BASE {
                ADDR_PORT => self.addr = value,
                DATA_PORT if self.addr == BANK_SELECT => self.bank = value,
                DATA_PORT => return Err(()), // the monitor reader never writes a register
                _ => return Err(()),
            }
            Ok(())
        }
    }

    #[test]
    fn reads_across_banks_switching_each_once() {
        let mut m = FakeMonitor::new(HashMap::from([
            (0x073, 40),
            (0x150, 33),
            (0x480, 0xA8),
            (0x4C0, 0x04),
        ]));
        let got = read(&mut m, BASE, &[0x073, 0x150, 0x480, 0x4C0]).expect("read");
        assert_eq!(
            got,
            [(0x073, 40), (0x150, 33), (0x480, 0xA8), (0x4C0, 0x04)]
        );
        let switches: Vec<u8> = m
            .writes
            .windows(2)
            .filter(|w| w[0] == (ADDR_PORT, BANK_SELECT))
            .map(|w| w[1].1)
            .collect();
        // Banks 0, 1, 4 in turn, then back to 0.
        assert_eq!(switches, [0, 1, 4, 0]);
        assert_eq!(m.bank, 0);
    }

    #[test]
    fn reads_the_vendor_id_from_both_halves() {
        let mut m = FakeMonitor::new(HashMap::new());
        assert_eq!(vendor(&mut m, BASE), Ok(NUVOTON));
        assert_eq!(m.bank, 0);
    }

    #[test]
    fn registers_cover_every_decoded_value_once() {
        let t = tables(Chip::Nct6798).expect("6798");
        let regs = registers(t);
        assert!(regs.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
        for r in [
            0x480,
            0x48E,
            VBAT_MONITOR,
            0x4CE,
            0x4CF,
            0x150,
            0x151,
            0x622,
            0x4A0,
            0xB00,
        ] {
            assert!(regs.contains(&r), "{r:#x}");
        }
        assert!(!regs.contains(&0x4A1), "0x4A0 is a single byte");
        assert_eq!(
            registers(tables(Chip::Nct6799).expect("6799")).len(),
            regs.len() + 3
        );
        assert!(tables(Chip::Nct6687).is_none());
    }

    /// A constructed NCT6798D dump (no field dump from an ASUS board yet), in the arrangement
    /// LibreHardwareMonitor's ASUS mappings describe: PECI agent 0 on 0x027, SYSTIN on 0x150
    /// (33.5 °C), CPUTIN on the first fan control, the PCH on 0x672, a virtual source on one fan
    /// control, AUXTIN0 open (-128), the CPU fan at 1062 RPM.
    fn asus_dump() -> HashMap<u16, u8> {
        HashMap::from([
            (0x621, 16),
            (0x027, 41),
            (0x622, 1),
            (0x150, 33),
            (0x151, 0x80),
            (0xC27, 19),
            (0x672, 44),
            (0x100, 2),
            (0x073, 36),
            (0x074, 0x80),
            (0x200, 1), // SYSTIN again, already read with its half degree: skipped
            (0x075, 33),
            (0x300, 11), // virtual
            (0x077, 50),
            (0x800, 3),
            (0x079, 0x80),
            // Voltages: Vcore 1.168 V (146 × 8 mV), 3VCC 3.344 V (209 × 16), VIN0 1.000 V
            // (125 × 8, +12V through the board's divider of 12), VBAT 3.072 V (192 × 16).
            (0x480, 146),
            (0x483, 209),
            (0x484, 125),
            (0x488, 192),
            (0x489, 66), // VTT 1.056 V: 16 mV a count on this chip
            (VBAT_MONITOR, 0x01),
            (0x4C2, 0x04),
            (0x4C3, 0x26),
        ])
    }

    #[test]
    fn decodes_temperatures_by_source() {
        let regs = asus_dump();
        let d = decode(tables(Chip::Nct6798).expect("6798"), |a| {
            regs.get(&a).copied().or(Some(0))
        });
        assert_eq!(
            d.temps,
            [
                (16, Some(41.0)),
                (1, Some(33.5)),
                (19, Some(44.0)),
                (2, Some(36.5)),
                (3, None),
            ]
        );
    }

    #[test]
    fn decodes_voltages_with_the_chip_scales_and_fans() {
        let regs = asus_dump();
        let t = tables(Chip::Nct6798).expect("6798");
        let d = decode(t, |a| regs.get(&a).copied().or(Some(0)));
        let v = |n: u16| d.volts.iter().find(|x| x.0 == n).expect("input").1;
        assert!((v(0) - 1.168).abs() < 1e-9);
        assert!((v(3) - 3.344).abs() < 1e-9);
        assert!((v(4) - 1.0).abs() < 1e-9);
        assert!((v(8) - 3.072).abs() < 1e-9);
        assert!((v(9) - 1.056).abs() < 1e-9);
        assert_eq!(d.volts.len(), 15);
        assert_eq!(d.fans[1], (1, Some(1062.0)));
        assert_eq!(d.fans[0], (0, Some(0.0)));
        assert_eq!(d.fans.len(), 7);
        // On a NCT6796D VTT is not halved inside.
        let d96 = decode(tables(Chip::Nct6796).expect("6796"), |a| {
            regs.get(&a).copied().or(Some(0))
        });
        let vtt = d96.volts.iter().find(|x| x.0 == 9).expect("vtt").1;
        assert!((vtt - 0.528).abs() < 1e-9);
    }

    #[test]
    fn battery_reads_nothing_while_unsampled() {
        let mut regs = asus_dump();
        regs.insert(VBAT_MONITOR, 0);
        let d = decode(tables(Chip::Nct6798).expect("6798"), |a| {
            regs.get(&a).copied().or(Some(0))
        });
        assert_eq!(d.volts.iter().find(|x| x.0 == 8).map(|x| x.1), Some(0.0));
    }

    #[test]
    fn sources_outside_the_chip_are_ignored() {
        // Source 13 is AUXTIN5 on a NCT6799D only.
        let regs = HashMap::from([(0x621, 13), (0x027, 30)]);
        let only = |chip| {
            decode(tables(chip).expect("chip"), |a| {
                regs.get(&a).copied().or(Some(0))
            })
            .temps
        };
        assert!(only(Chip::Nct6798).is_empty());
        assert_eq!(only(Chip::Nct6799), [(13, Some(30.0))]);
        assert_eq!(source_name(13), "AUXTIN5");
    }

    #[test]
    fn names_sources_and_inputs() {
        assert_eq!(source_name(1), "SYSTIN");
        assert_eq!(source_name(5), "AUXTIN2");
        assert_eq!(source_name(16), "PECI agent 0");
        assert_eq!(source_name(19), "PCH");
        assert_eq!(source_name(29), "PECI agent 1 calibrated");
        assert_eq!(input_name(0), "Vcore");
        assert_eq!(input_name(4), "VIN0");
        assert_eq!(input_name(8), "CMOS Battery");
        assert_eq!(input_name(17), "VIN10");
    }
}

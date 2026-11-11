//! Memory sensors through PawnIO (Phase 6d): DDR5 module temperatures from the SPD5 hub's
//! thermal sensor and running timings from the AMD memory controller (UMC, `AMDFamily17` SMN
//! reads). From 2.1 also on Intel: the SMBus through `SmbusI801` (AMD's is `SmbusPIIX4`), DDR4
//! modules' own thermal sensors (JEDEC TSE2004av, on DDR4 modules that carry one), and the
//! running timings of the Intel 12th to 14th gen memory controller (`IntelMCHBAR`). Decoders here
//! are pure; `win` does the bus access.
//!
//! Clokrix only reads. The hub's thermal registers sit in its register space, which does not depend
//! on the NVM page (MR11), and a DDR4 module's sensor answers at its own address, so no SPD write
//! is ever needed. The UMC register layout is the one documented by ZenTimings / ZenStates-Core
//! for Zen 4 and Zen 5; the Intel one is memtest86+'s for Alder Lake and Raptor Lake.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{probe_spd, read_imc, read_umc, Bus, Probe, RamProvider, SpdRaw, TsodRaw, UmcRaw};

use crate::snapshot::MemTimings;

/// SMBus addresses of SPD devices (one per DIMM slot).
pub const SPD_FIRST: u8 = 0x50;
pub const SPD_LAST: u8 = 0x57;

/// DDR4 modules' thermal sensors (JEDEC TSE2004av, JC-42.4 registers), one per slot at
/// `0x18 + slot`, beside the slot's SPD at `0x50 + slot`.
pub const TSOD_FIRST: u8 = 0x18;
pub const TSOD_LAST: u8 = 0x1F;
/// JC-42.4 registers, 16 bits each, most significant byte first on the bus.
pub const TS_CAPABILITY: u8 = 0x00;
pub const TS_CONFIG: u8 = 0x01;
pub const TS_TEMPERATURE: u8 = 0x05;
pub const TS_MANUFACTURER: u8 = 0x06;
pub const TS_DEVICE: u8 = 0x07;

/// JEDEC manufacturer IDs of TSE2004av sensors (Linux `jc42`): Atmel, Giantec, IDT, Microchip,
/// ON Semiconductor, Seiko, ST.
const TSE2004_MAKERS: [u16; 7] = [0x1114, 0x1C68, 0x00B3, 0x0054, 0x1B09, 0x1C85, 0x104A];

/// Whether registers read at a sensor address are a DDR4 module's TSE2004av sensor: the checks
/// Linux `jc42` makes before it binds (reserved capability and configuration bits clear, the
/// capabilities a TSE2004av must have, a maker and device ID it knows).
pub fn is_tse2004(capability: u16, config: u16, maker: u16, device: u16) -> bool {
    capability & 0xFF00 == 0
        && config & 0xF820 == 0
        && capability & 0x0062 == 0x0062
        && device & 0xFF00 == 0x2200
        && TSE2004_MAKERS.contains(&maker)
}

/// °C from a JC-42.4 temperature register (most significant byte first): bits 15:13 are alarm
/// flags, bits 12:0 a two's-complement count of 1/16 °C.
pub fn tsod_temp(word: u16) -> Option<f64> {
    let raw = i32::from(word & 0x1FFF);
    let signed = if raw & 0x1000 != 0 { raw - 0x2000 } else { raw };
    let t = f64::from(signed) * 0.0625;
    (-40.0..=125.0).contains(&t).then_some(t)
}

/// SPD5 hub registers (JESD300-5), all in the volatile register space.
pub const MR_DEVICE_TYPE_MSB: u8 = 0x00;
pub const MR_DEVICE_TYPE_LSB: u8 = 0x01;
/// MR5: device capability; bit 1 = has a temperature sensor.
pub const MR_CAPABILITY: u8 = 0x05;
/// MR26: thermal sensor configuration; bit 0 = sensor disabled.
pub const MR_TS_CONFIG: u8 = 0x1A;
/// MR49/MR50: current temperature, read as one little-endian word.
pub const MR_TS_SENSE: u8 = 0x31;
/// Device type of an SPD5118 hub (MR0, MR1).
pub const SPD5118: [u8; 2] = [0x51, 0x18];

/// Slot name for an SPD address (or a DDR4 sensor's, `0x18 + slot`), as boards wire them:
/// `0x50 + 2 × channel + slot`, so 0x51 is the second slot of channel A ("DIMMA2").
pub fn slot_name(addr: u8) -> Option<String> {
    let n = if (SPD_FIRST..=SPD_LAST).contains(&addr) {
        addr - SPD_FIRST
    } else if (TSOD_FIRST..=TSOD_LAST).contains(&addr) {
        addr - TSOD_FIRST
    } else {
        return None;
    };
    Some(format!("DIMM{}{}", char::from(b'A' + n / 2), n % 2 + 1))
}

pub fn is_spd5_hub(mr0: u8, mr1: u8) -> bool {
    [mr0, mr1] == SPD5118
}

/// Whether the hub has a thermal sensor and firmware left it on. Clokrix never turns it on.
pub fn has_sensor(mr5: u8, mr26: u8) -> bool {
    mr5 & 0x02 != 0 && mr26 & 0x01 == 0
}

/// MR49/MR50 as a word: bits 12:0 are a two's-complement count of 1/16 °C (bits 1:0 always 0,
/// so 0.25 °C steps); bits 15:13 are reserved and must read 0.
pub fn spd5_temp(word: u16) -> Option<f64> {
    if word & 0xE000 != 0 {
        return None;
    }
    let raw = i32::from(word);
    let signed = if raw & 0x1000 != 0 { raw - 0x2000 } else { raw };
    let t = f64::from(signed) * 0.0625;
    (-40.0..=125.0).contains(&t).then_some(t)
}

/// SMN base of the first UMC; channel `n` is at `n << 20`.
pub const UMC_BASE: u32 = 0x5_0000;
pub const MAX_CHANNELS: u32 = 12;
/// Chip-select base registers for the channel's two DIMMs; bit 0 = enabled.
pub const UMC_DIMM_CS: [u32; 2] = [0x5_0000, 0x5_0008];
/// UMC configuration; bits 1:0 = DRAM type.
pub const UMC_CONFIG: u32 = 0x5_0100;
/// Channel control; bit 19 set = channel disabled.
pub const UMC_CTRL: u32 = 0x5_0DF0;

pub const fn channel_offset(channel: u32) -> u32 {
    channel << 20
}

pub fn channel_active(ctrl: u32, cs0: u32, cs1: u32) -> bool {
    ctrl & (1 << 19) == 0 && (cs0 & 1 != 0 || cs1 & 1 != 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DramType {
    Ddr4,
    Ddr5,
    Lpddr4,
    Lpddr5,
}

pub const fn dram_type(config: u32) -> DramType {
    match config & 3 {
        0 => DramType::Ddr4,
        1 => DramType::Ddr5,
        2 => DramType::Lpddr4,
        _ => DramType::Lpddr5,
    }
}

/// Refresh control: bit 1 = per-bank refresh, bits 18:16 = fine-granularity mode, bit 28 =
/// power-down enabled.
pub const UMC_REFRESH: u32 = 0x5_012C;
/// Memory clock ratio (bits 15:0, in 1/100) and command-rate / gear-down bits.
pub const UMC_RATIO: u32 = 0x5_0200;
/// tRFC1/tRFC2 (one of four copies, one per rank pair; the first set one counts).
pub const UMC_RFC: [u32; 4] = [0x5_0260, 0x5_0264, 0x5_0268, 0x5_026C];
/// tRFCsb, same arrangement.
pub const UMC_RFC_SB: [u32; 4] = [0x5_02C0, 0x5_02C4, 0x5_02C8, 0x5_02CC];
/// Firmware's placeholder in unused tRFC copies.
const RFC_UNSET: u32 = 0x00C0_0138;

/// Every register [`decode_ddr5`] reads (channel-relative), for dumps and fixtures.
pub const DDR5_REGS: [u32; 20] = [
    UMC_CONFIG,
    UMC_REFRESH,
    UMC_RATIO,
    0x5_0204,
    0x5_0208,
    0x5_020C,
    0x5_0210,
    0x5_0214,
    0x5_0218,
    0x5_0230,
    UMC_RFC[0],
    UMC_RFC[1],
    UMC_RFC[2],
    UMC_RFC[3],
    UMC_RFC_SB[0],
    UMC_RFC_SB[1],
    UMC_RFC_SB[2],
    UMC_RFC_SB[3],
    UMC_DIMM_CS[0],
    UMC_DIMM_CS[1],
];

const fn bits(v: u32, hi: u32, lo: u32) -> u32 {
    (v >> lo) & ((1 << (hi - lo + 1)) - 1)
}

/// Decodes one DDR5 channel's timings from its registers (`read` takes a channel-relative
/// address from [`DDR5_REGS`]). Assumes the reference clock is 100 MHz. `None` when the
/// registers do not look like a trained DDR5 controller.
pub fn decode_ddr5(read: impl Fn(u32) -> u32) -> Option<MemTimings> {
    if dram_type(read(UMC_CONFIG)) != DramType::Ddr5 {
        return None;
    }
    let ratio = read(UMC_RATIO);
    let refresh = read(UMC_REFRESH);
    let (t5, t6, t7) = (read(0x5_0204), read(0x5_0208), read(0x5_020C));
    let (t9, t10) = (read(0x5_0214), read(0x5_0218));
    let rfc_reg = UMC_RFC
        .iter()
        .map(|&a| read(a))
        .find(|&v| v != 0 && v != RFC_UNSET)
        .unwrap_or(0);
    let per_bank = refresh & 2 != 0;
    let fgr = bits(refresh, 18, 16);
    let t = MemTimings {
        mts: f64::from(bits(ratio, 15, 0)) / 100.0 * 200.0,
        cl: bits(t5, 5, 0),
        ras: bits(t5, 14, 8),
        rcd_rd: bits(t5, 21, 16),
        rcd_wr: bits(t5, 29, 24),
        rc: bits(t6, 7, 0),
        rp: bits(t6, 21, 16),
        rrd_s: bits(t7, 4, 0),
        rrd_l: bits(t7, 12, 8),
        rtp: bits(t7, 28, 24),
        faw: bits(read(0x5_0210), 7, 0),
        cwl: bits(t9, 5, 0),
        wtr_s: bits(t9, 12, 8),
        wtr_l: bits(t9, 22, 16),
        wr: bits(t10, 7, 0),
        refi: bits(read(0x5_0230), 15, 0),
        rfc: if !per_bank && fgr == 0 {
            bits(rfc_reg, 15, 0)
        } else {
            bits(rfc_reg, 31, 16)
        },
        rfc_sb: UMC_RFC_SB
            .iter()
            .map(|&a| bits(read(a), 10, 0))
            .find(|&v| v != 0)
            .unwrap_or(0),
        command_rate: if ratio & (1 << 17) != 0 { 2 } else { 1 },
        gear_down: ratio & (1 << 18) != 0,
        power_down: refresh & (1 << 28) != 0,
    };
    let sane = (1000.0..=16000.0).contains(&t.mts)
        && (8..=80).contains(&t.cl)
        && t.rcd_rd != 0
        && t.rp != 0
        && t.ras != 0;
    sane.then_some(t)
}

/// Intel memory controller registers (MCHBAR offsets; the second controller at +0x10000), as
/// memtest86+ reads them on Alder Lake and Raptor Lake.
pub const IMC_MC1: u32 = 0x1_0000;
/// DDR type in bits 2:0 (0 DDR4, 1 DDR5, 2 LPDDR5, 3 LPDDR4).
pub const IMC_DECODE: u32 = 0xD800;
/// Channel 0's DIMMs: sizes in bits 6:0 and 22:16; all ones when the controller is off.
pub const IMC_DIMM_CH0: u32 = 0xD80C;
/// tRP in bits 7:0; the next word has tRAS in bits 18:10 and tRCD in bits 26:19.
pub const IMC_TC_PRE: u32 = 0xE000;
pub const IMC_TC_PRE_HIGH: u32 = 0xE004;
/// tCL in bits 22:16.
pub const IMC_ODT_TCL: u32 = 0xE070;
/// The memory clock's ratio in bits 9:2.
pub const IMC_SA_PERF: u32 = 0x5918;
/// Bits 13:12 the gear shift; bits 11:8 zero on the 133 MHz reference.
pub const IMC_BIOS_DATA: u32 = 0x5E04;
/// The base clock in kHz.
pub const IMC_BCLK: u32 = 0x5F60;

/// Every register [`decode_intel`] reads, for dumps and fixtures.
pub const INTEL_REGS: [u32; 13] = [
    IMC_DECODE,
    IMC_DIMM_CH0,
    IMC_TC_PRE,
    IMC_TC_PRE_HIGH,
    IMC_ODT_TCL,
    IMC_MC1 + IMC_DECODE,
    IMC_MC1 + IMC_DIMM_CH0,
    IMC_MC1 + IMC_TC_PRE,
    IMC_MC1 + IMC_TC_PRE_HIGH,
    IMC_MC1 + IMC_ODT_TCL,
    IMC_SA_PERF,
    IMC_BIOS_DATA,
    IMC_BCLK,
];

/// Intel CPUs whose memory controller [`decode_intel`] reads: Alder Lake (12th gen) and Raptor
/// Lake (13th and 14th).
pub const fn intel_imc(family: u32, model: u32) -> bool {
    family == 6 && matches!(model, 0x97 | 0x9A | 0xB7 | 0xBA | 0xBF)
}

/// Decodes an Intel 12th to 14th gen memory controller's running timings: the data rate and
/// CL-tRCD-tRP-tRAS, which is what its registers give (the others stay 0). `None` when the
/// registers do not look like a trained DDR4 or DDR5 controller.
pub fn decode_intel(read: impl Fn(u32) -> Option<u32>) -> Option<MemTimings> {
    let populated = |v: u32| v != u32::MAX && bits(v, 6, 0) + bits(v, 22, 16) > 0;
    let mc = [0, IMC_MC1]
        .into_iter()
        .find(|&mc| read(mc + IMC_DIMM_CH0).is_some_and(populated))?;
    if read(mc + IMC_DECODE)? & 7 > 3 {
        return None;
    }
    let bclk = f64::from(read(IMC_BCLK)?) / 1000.0;
    if !(50.0..=200.0).contains(&bclk) {
        return None;
    }
    let bios = read(IMC_BIOS_DATA)?;
    let mut mts =
        f64::from(bits(read(IMC_SA_PERF)?, 9, 2)) * bclk * f64::from(1 << bits(bios, 13, 12));
    if bios & 0xF00 == 0 {
        mts *= 4.0 / 3.0;
    }
    let (pre, high) = (read(mc + IMC_TC_PRE)?, read(mc + IMC_TC_PRE_HIGH)?);
    let t = MemTimings {
        mts: mts.round(),
        cl: bits(read(mc + IMC_ODT_TCL)?, 22, 16),
        rcd_rd: bits(high, 26, 19),
        rp: bits(pre, 7, 0),
        ras: bits(high, 18, 10),
        ..MemTimings::default()
    };
    let sane = (1000.0..=10000.0).contains(&t.mts)
        && (8..=80).contains(&t.cl)
        && t.rcd_rd != 0
        && t.rp != 0
        && t.ras != 0;
    sane.then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn names_slots_by_spd_address() {
        assert_eq!(slot_name(0x50).as_deref(), Some("DIMMA1"));
        assert_eq!(slot_name(0x51).as_deref(), Some("DIMMA2"));
        assert_eq!(slot_name(0x53).as_deref(), Some("DIMMB2"));
        assert_eq!(slot_name(0x57).as_deref(), Some("DIMMD2"));
        assert_eq!(slot_name(0x48), None);
        // A DDR4 module's sensor sits at 0x18 + the same slot number.
        assert_eq!(slot_name(0x1B).as_deref(), Some("DIMMB2"));
        assert_eq!(slot_name(0x20), None);
    }

    #[test]
    fn recognises_ddr4_module_sensors() {
        // Constructed values: a Microchip sensor (maker 0x0054) with the TSE2004av device ID and
        // capabilities, then the same from ST (0x104A).
        assert!(is_tse2004(0x006F, 0x0000, 0x0054, 0x2201));
        assert!(is_tse2004(0x006F, 0x0000, 0x104A, 0x2201));
        assert!(
            !is_tse2004(0x006F, 0x0000, 0x0054, 0x0400),
            "MCP9808: no DDR4 sensor"
        );
        assert!(
            !is_tse2004(0x0001, 0x0000, 0x0054, 0x2201),
            "missing TSE2004 capabilities"
        );
        assert!(
            !is_tse2004(0x006F, 0x0800, 0x0054, 0x2201),
            "reserved config bit"
        );
        assert!(!is_tse2004(0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF), "nothing there");
        assert!(!is_tse2004(0x006F, 0x0000, 0x1234, 0x2201), "unknown maker");
    }

    #[test]
    fn decodes_ddr4_sensor_temperatures() {
        // 0x01D4 = 29.25 °C; alarm flags in bits 15:13 do not change the reading.
        assert_eq!(tsod_temp(0x01D4), Some(29.25));
        assert_eq!(tsod_temp(0xE1D4), Some(29.25));
        // -2.5 °C: 13-bit two's complement of 40 sixteenths.
        assert_eq!(tsod_temp(0x2000 - 40), Some(-2.5));
        assert_eq!(tsod_temp(0x0FFF), None, "255.9 °C: nonsense");
    }

    /// Constructed registers, in memtest86+'s layout: DDR4-3200 16-18-18-38 on the second
    /// controller, the data rate from the formula (ratio 24 × 100 MHz on the 133 MHz reference,
    /// × 4/3). Not a field dump: none from an Intel PC yet.
    fn adl_ddr4() -> HashMap<u32, u32> {
        HashMap::from([
            (IMC_DIMM_CH0, u32::MAX),
            (IMC_MC1 + IMC_DIMM_CH0, 0x0000_0010),
            (IMC_MC1 + IMC_DECODE, 0),
            (IMC_BCLK, 100_000),
            (IMC_SA_PERF, 24 << 2),
            (IMC_BIOS_DATA, 0x0000_0000),
            (IMC_MC1 + IMC_TC_PRE, 18),
            (IMC_MC1 + IMC_TC_PRE_HIGH, 38 << 10 | 18 << 19),
            (IMC_MC1 + IMC_ODT_TCL, 16 << 16),
        ])
    }

    #[test]
    fn decodes_intel_timings_from_the_controller_in_use() {
        let regs = adl_ddr4();
        let t = decode_intel(|a| regs.get(&a).copied()).expect("timings");
        assert_eq!(t.mts, 3200.0);
        assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (16, 18, 18, 38));
        assert_eq!(
            (t.rc, t.rfc, t.command_rate),
            (0, 0, 0),
            "not read on Intel"
        );
        // DDR5-6000 in gear 2 on the 100 MHz reference: ratio 30, shift 1.
        let mut ddr5 = regs.clone();
        ddr5.insert(IMC_MC1 + IMC_DECODE, 1);
        ddr5.insert(IMC_SA_PERF, 30 << 2);
        ddr5.insert(IMC_BIOS_DATA, 1 << 12 | 0x100);
        assert_eq!(
            decode_intel(|a| ddr5.get(&a).copied()).expect("ddr5").mts,
            6000.0
        );
        assert!(intel_imc(6, 0x97));
        assert!(intel_imc(6, 0xB7));
        assert!(!intel_imc(6, 0xC6), "Arrow Lake: another layout");
    }

    #[test]
    fn rejects_an_intel_controller_without_memory_or_with_nonsense() {
        let mut regs = adl_ddr4();
        regs.insert(IMC_MC1 + IMC_DIMM_CH0, u32::MAX);
        assert_eq!(
            decode_intel(|a| regs.get(&a).copied()),
            None,
            "no controller in use"
        );
        let mut regs = adl_ddr4();
        regs.insert(IMC_BCLK, 0);
        assert_eq!(
            decode_intel(|a| regs.get(&a).copied()),
            None,
            "no base clock"
        );
        let mut regs = adl_ddr4();
        regs.insert(IMC_MC1 + IMC_ODT_TCL, 0);
        assert_eq!(decode_intel(|a| regs.get(&a).copied()), None, "CL 0");
        assert_eq!(decode_intel(|_| None), None);
    }

    #[test]
    fn recognises_spd5_hubs_with_a_live_sensor() {
        assert!(is_spd5_hub(0x51, 0x18));
        assert!(!is_spd5_hub(0x51, 0x08));
        assert!(has_sensor(0x02, 0x00));
        assert!(!has_sensor(0x00, 0x00), "no sensor");
        assert!(!has_sensor(0x02, 0x01), "sensor disabled");
    }

    #[test]
    fn decodes_spd5_temperatures() {
        // 41.25 °C = 660 sixteenths; 0.25 °C steps leave bits 1:0 clear.
        assert_eq!(spd5_temp(660), Some(41.25));
        assert_eq!(spd5_temp(0x0000), Some(0.0));
        // -10 °C: 13-bit two's complement of 160.
        assert_eq!(spd5_temp(0x2000 - 160), Some(-10.0));
        assert_eq!(spd5_temp(0xFFFF), None, "floating bus");
        assert_eq!(spd5_temp(16 * 150), None, "implausible");
    }

    #[test]
    fn reads_channel_presence_and_type() {
        assert_eq!(channel_offset(1), 0x10_0000);
        assert!(channel_active(0, 0, 1));
        assert!(!channel_active(1 << 19, 1, 1), "channel disabled");
        assert!(!channel_active(0, 0, 0), "no DIMM");
        assert_eq!(dram_type(0x0000_1201), DramType::Ddr5);
        assert_eq!(dram_type(0x0000_1200), DramType::Ddr4);
    }

    /// Registers encoding Corsair CMK16GX5M1B5600C40's XMP profile (40-40-40-77, 5600 MT/s,
    /// as HWMonitor lists it) in the ZenTimings field layout.
    fn xmp_5600() -> HashMap<u32, u32> {
        HashMap::from([
            (UMC_CONFIG, 0x0000_1201),
            (UMC_REFRESH, 1 << 28),
            (UMC_RATIO, 2800 | 1 << 16),
            (0x5_0204, 40 | 77 << 8 | 40 << 16 | 40 << 24),
            (0x5_0208, 117 | 40 << 16),
            (0x5_020C, 8 | 14 << 8 | 21 << 24),
            (0x5_0210, 32),
            (0x5_0214, 38 | 7 << 8 | 28 << 16),
            (0x5_0218, 84),
            (0x5_0230, 10920),
            (UMC_RFC[0], RFC_UNSET),
            (UMC_RFC[1], 868 | 588 << 16),
            (UMC_RFC_SB[0], 364),
        ])
    }

    #[test]
    fn decodes_ddr5_timings() {
        let regs = xmp_5600();
        let t = decode_ddr5(|a| regs.get(&a).copied().unwrap_or(0)).unwrap();
        assert_eq!(t.mts, 5600.0);
        assert_eq!(
            (t.cl, t.rcd_rd, t.rcd_wr, t.rp, t.ras, t.rc),
            (40, 40, 40, 40, 77, 117)
        );
        assert_eq!((t.rrd_s, t.rrd_l, t.faw, t.rtp), (8, 14, 32, 21));
        assert_eq!((t.cwl, t.wtr_s, t.wtr_l, t.wr), (38, 7, 28, 84));
        assert_eq!((t.rfc, t.rfc_sb, t.refi), (868, 364, 10920));
        assert_eq!(t.command_rate, 1);
        assert!(!t.gear_down);
        assert!(t.power_down);
    }

    /// UMC channel 0 on the Ryzen 7 9700X with Corsair CMK16GX5M1B5600C40 at XMP (field dump).
    const FIELD_UMC: [(u32, u32); 20] = [
        (0x5_0000, 0x0000_0000),
        (0x5_0008, 0x0000_0001),
        (0x5_0100, 0x8000_0701),
        (0x5_012C, 0x0538_1148),
        (0x5_0200, 0x8005_0AF0),
        (0x5_0204, 0x2828_4D28),
        (0x5_0208, 0x0028_0075),
        (0x5_020C, 0x1500_0E08),
        (0x5_0210, 0x0000_0020),
        (0x5_0214, 0x001C_0726),
        (0x5_0218, 0x0000_0054),
        (0x5_0230, 0x00C0_2A8C),
        (0x5_0260, 0x00C0_0138),
        (0x5_0264, 0x00C0_0138),
        (0x5_0268, 0x01BF_0338),
        (0x5_026C, 0x00C0_0138),
        (0x5_02C0, 0x0000_0000),
        (0x5_02C4, 0x0000_0000),
        (0x5_02C8, 0x0054_016C),
        (0x5_02CC, 0x0000_0000),
    ];

    #[test]
    fn decodes_the_field_dump() {
        let read = |a| FIELD_UMC.iter().find(|r| r.0 == a).map_or(0, |r| r.1);
        assert!(channel_active(
            0,
            read(UMC_DIMM_CS[0]),
            read(UMC_DIMM_CS[1])
        ));
        let t = decode_ddr5(read).unwrap();
        assert_eq!(t.mts, 5600.0);
        assert_eq!(
            (t.cl, t.rcd_rd, t.rcd_wr, t.rp, t.ras, t.rc),
            (40, 40, 40, 40, 77, 117)
        );
        assert_eq!((t.rrd_s, t.rrd_l, t.faw, t.rtp), (8, 14, 32, 21));
        assert_eq!((t.cwl, t.wtr_s, t.wtr_l, t.wr), (38, 7, 28, 84));
        // tRFC1 824 clocks = 294 ns at 2800 MHz, the 16 Gb die value; the third copy is set.
        assert_eq!((t.rfc, t.rfc_sb, t.refi), (824, 364, 10892));
        assert_eq!(t.command_rate, 1);
        assert!(t.gear_down);
        assert!(!t.power_down);
        // Hub temperature words from the same run (HWMonitor's PMIC sensors: 39 / 38 °C).
        assert_eq!(spd5_temp(0x0268), Some(38.5));
        assert_eq!(spd5_temp(0x025C), Some(37.75));
    }

    #[test]
    fn uses_trfc2_with_fine_granularity_refresh() {
        let mut regs = xmp_5600();
        regs.insert(UMC_REFRESH, 1 << 16);
        let t = decode_ddr5(|a| regs.get(&a).copied().unwrap_or(0)).unwrap();
        assert_eq!(t.rfc, 588);
    }

    #[test]
    fn rejects_untrained_or_other_controllers() {
        assert_eq!(decode_ddr5(|_| 0), None, "DDR4 type bits");
        let mut regs = xmp_5600();
        regs.insert(UMC_RATIO, 0);
        assert_eq!(decode_ddr5(|a| regs.get(&a).copied().unwrap_or(0)), None);
        assert_eq!(decode_ddr5(|_| u32::MAX), None, "all ones");
    }
}

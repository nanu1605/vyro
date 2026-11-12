//! Memory sensors through PawnIO (Phase 6d): DDR5 module temperatures from the SPD5 hub's
//! thermal sensor and running timings from the AMD memory controller (UMC, `AMDFamily17` SMN
//! reads). From 2.1 also on Intel: the SMBus through `SmbusI801` (AMD's is `SmbusPIIX4`), DDR4
//! modules' own thermal sensors (JEDEC TSE2004av, on DDR4 modules that carry one), and the
//! running timings of the Intel 12th to 14th gen memory controller (`IntelMCHBAR`). From 2.3 the
//! DDR4 timings of AMD's controller from Zen to Zen 3 (AM4). Decoders here are pure; `win` does
//! the bus access.
//!
//! Clokrix only reads. The hub's thermal registers sit in its register space, which does not depend
//! on the NVM page (MR11), and a DDR4 module's sensor answers at its own address, so no SPD write
//! is ever needed. The UMC register layout is the one documented by ZenTimings / ZenStates-Core
//! for DDR4 (Zen to Zen 3) and DDR5 (Zen 4 and Zen 5), with the DDR4 data rate as memtest86+
//! reads it; the Intel one is memtest86+'s for Alder Lake and Raptor Lake.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{
    probe_spd, read_imc, read_imc_regs, read_umc, Bus, Probe, RamProvider, SpdRaw, TsodRaw, UmcRaw,
};

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

/// Which timing layout an AMD CPU's memory controller has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UmcLayout {
    /// Zen to Zen 3 with DDR4 (AM4, sTRX4, and the APUs up to Cezanne): no type field, so the
    /// CPU's model says it, as Linux `amd64_edac` decides it.
    Ddr4,
    /// Zen 4 and later, and Rembrandt: [`UMC_CONFIG`]'s type field says DDR5.
    Ddr5,
}

/// The layout for an AMD CPU (family 0x17 to 0x1A).
pub const fn umc_layout(family: u32, model: u32) -> UmcLayout {
    match (family, model) {
        // Zen, Zen+ and Zen 2 up to Renoir and Matisse (Van Gogh and Mendocino, 0x90 and 0xA0,
        // run LPDDR5); Zen 3's Milan and Chagall, Vermeer and Cezanne.
        (0x17, 0x00..=0x8F) | (0x19, 0x00..=0x0F | 0x20..=0x2F | 0x50..=0x5F) => UmcLayout::Ddr4,
        _ => UmcLayout::Ddr5,
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
/// The same on DDR4, where there are two copies (ZenStates-Core).
const DDR4_RFC_UNSET: u32 = 0x2106_0138;

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

/// Every register [`decode_ddr4`] reads (channel-relative), for dumps and fixtures.
pub const DDR4_REGS: [u32; 14] = [
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
    UMC_DIMM_CS[0],
    UMC_DIMM_CS[1],
    UMC_CTRL,
];

/// Decodes one DDR4 channel's timings from its registers (`read` takes a channel-relative
/// address from [`DDR4_REGS`]): most fields where DDR5 has them, tFAW and tWR one bit narrower,
/// the memory clock ratio in thirds of the reference clock, 2T and gear-down mode in the ratio
/// register's bits 10 and 11, and tRFC, tRFC2 and tRFC4 side by side, the one for the refresh
/// mode in use counting. Assumes the reference clock is 100 MHz. `None` when the registers do not
/// look like a trained DDR4 controller.
pub fn decode_ddr4(read: impl Fn(u32) -> u32) -> Option<MemTimings> {
    let ratio = read(UMC_RATIO);
    let refresh = read(UMC_REFRESH);
    let (t5, t6, t7) = (read(0x5_0204), read(0x5_0208), read(0x5_020C));
    let (t9, t10) = (read(0x5_0214), read(0x5_0218));
    let (rfc0, rfc1) = (read(UMC_RFC[0]), read(UMC_RFC[1]));
    let rfc_reg = if rfc0 != rfc1 && rfc0 == DDR4_RFC_UNSET {
        rfc1
    } else {
        rfc0
    };
    // Fine-granularity refresh, coded as DDR4's MR3: 1 and 5 refresh at 2x, 2 and 6 at 4x.
    let rfc = match bits(refresh, 18, 16) {
        1 | 5 => bits(rfc_reg, 21, 11),
        2 | 6 => bits(rfc_reg, 31, 22),
        _ => bits(rfc_reg, 10, 0),
    };
    let t = MemTimings {
        // 48 thirds of 100 MHz × 2 = 3200 MT/s; 52 and 56 give DDR4-3466 and -3733 (rounded down,
        // as they are sold).
        mts: (f64::from(bits(ratio, 6, 0)) * 200.0 / 3.0).floor(),
        cl: bits(t5, 5, 0),
        ras: bits(t5, 14, 8),
        rcd_rd: bits(t5, 21, 16),
        rcd_wr: bits(t5, 29, 24),
        rc: bits(t6, 7, 0),
        rp: bits(t6, 21, 16),
        rrd_s: bits(t7, 4, 0),
        rrd_l: bits(t7, 12, 8),
        rtp: bits(t7, 28, 24),
        faw: bits(read(0x5_0210), 6, 0),
        cwl: bits(t9, 5, 0),
        wtr_s: bits(t9, 12, 8),
        wtr_l: bits(t9, 22, 16),
        wr: bits(t10, 6, 0),
        refi: bits(read(0x5_0230), 15, 0),
        rfc,
        rfc_sb: 0,
        command_rate: if ratio & (1 << 10) != 0 { 2 } else { 1 },
        gear_down: ratio & (1 << 11) != 0,
        power_down: refresh & (1 << 28) != 0,
    };
    let sane = (1000.0..=6000.0).contains(&t.mts)
        && (8..=60).contains(&t.cl)
        && t.rcd_rd != 0
        && t.rp != 0
        && t.ras != 0;
    sane.then_some(t)
}

/// Intel memory controller registers (MCHBAR offsets; the second controller at +0x10000), as
/// memtest86+ reads them on Alder Lake and Raptor Lake; Meteor Lake and Arrow Lake share most of
/// them.
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

/// Every register the Alder Lake decoder reads, for dumps and fixtures.
const ALDER_REGS: [u32; 13] = [
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

/// The older controllers (2.3): two channels in one controller, the second at +0x400, with each
/// channel's DIMM sizes in its own register; tRP and tRCD share bits 5:0 of the timing register.
/// Comet Lake keeps Skylake's layout, Rocket Lake Ice Lake's (memtest86+'s readers of both).
pub const SKL_MAD_CH0: u32 = 0x500C;
pub const SKL_MAD_CH1: u32 = 0x5010;
pub const SKL_CH1: u32 = 0x400;
/// tRP and tRCD in bits 5:0; tRAS in bits 14:8 (Skylake) or 15:9 (Ice Lake).
pub const SKL_TC_PRE: u32 = 0x4000;
/// Bits 1:0 zero on DDR4 (Skylake's controller also runs DDR3).
pub const SKL_SCHED: u32 = 0x401C;
/// tCL in bits 20:16.
pub const SKL_TC_CAS: u32 = 0x4070;

/// Meteor Lake and Arrow Lake (2.3), memtest86+'s reader of both: channel 1 of a controller at
/// +0x800, tRCD in its own register, and the memory clock in one register for both controllers.
pub const MTL_DIMM_CH1: u32 = 0xD810;
pub const MTL_CH1: u32 = 0x800;
/// tRCD in bits 29:22.
pub const MTL_TC_ACT: u32 = 0xE138;
/// Bits 23:20 the reference clock, 19:12 the ratio, 24 gear 2.
pub const MTL_PTGRAM: u32 = 0x1_3D98;

const SKL_REGS: [u32; 8] = [
    SKL_MAD_CH0,
    SKL_MAD_CH1,
    SKL_TC_PRE,
    SKL_SCHED,
    SKL_TC_CAS,
    SKL_CH1 + SKL_TC_PRE,
    SKL_CH1 + SKL_SCHED,
    SKL_CH1 + SKL_TC_CAS,
];
const ICL_REGS: [u32; 8] = [
    SKL_MAD_CH0,
    SKL_MAD_CH1,
    SKL_TC_PRE,
    SKL_TC_CAS,
    SKL_CH1 + SKL_TC_PRE,
    SKL_CH1 + SKL_TC_CAS,
    IMC_BIOS_DATA,
    IMC_BCLK,
];
const MTL_REGS: [u32; 21] = [
    IMC_DIMM_CH0,
    MTL_DIMM_CH1,
    IMC_MC1 + IMC_DIMM_CH0,
    IMC_MC1 + MTL_DIMM_CH1,
    IMC_TC_PRE,
    IMC_TC_PRE_HIGH,
    IMC_ODT_TCL,
    MTL_TC_ACT,
    MTL_CH1 + IMC_TC_PRE,
    MTL_CH1 + IMC_TC_PRE_HIGH,
    MTL_CH1 + IMC_ODT_TCL,
    MTL_CH1 + MTL_TC_ACT,
    IMC_MC1 + IMC_TC_PRE,
    IMC_MC1 + IMC_TC_PRE_HIGH,
    IMC_MC1 + IMC_ODT_TCL,
    IMC_MC1 + MTL_TC_ACT,
    IMC_MC1 + MTL_CH1 + IMC_TC_PRE,
    IMC_MC1 + MTL_CH1 + IMC_TC_PRE_HIGH,
    IMC_MC1 + MTL_CH1 + IMC_ODT_TCL,
    IMC_MC1 + MTL_CH1 + MTL_TC_ACT,
    MTL_PTGRAM,
];

/// Which Intel memory controller a CPU has, as memtest86+ tells them apart (Tiger Lake from
/// Intel's datasheet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImcLayout {
    /// Comet Lake-S (10th gen desktop): Skylake's controller, DDR4.
    Skylake,
    /// Rocket Lake (11th gen desktop): Ice Lake's controller, DDR4.
    IceLake,
    /// Alder Lake and Raptor Lake (12th to 14th gen).
    AlderLake,
    /// Meteor Lake and Arrow Lake (Core Ultra 100 and 200S): DDR5.
    MeteorLake,
    /// Tiger Lake UP3, UP4 and H35 (11th gen laptops), from Intel's datasheet 631122.
    TigerLake,
    /// Tiger Lake H (11th gen laptops, H45), from Intel's datasheet 643524.
    TigerLakeH,
}

/// The controller of an Intel CPU Clokrix reads (family 6): `None` for the others.
pub const fn intel_layout(family: u32, model: u32) -> Option<ImcLayout> {
    if family != 6 {
        return None;
    }
    match model {
        0x8C => Some(ImcLayout::TigerLake),
        0x8D => Some(ImcLayout::TigerLakeH),
        0xA5 => Some(ImcLayout::Skylake),
        0xA7 => Some(ImcLayout::IceLake),
        0x97 | 0x9A | 0xB7 | 0xBA | 0xBF => Some(ImcLayout::AlderLake),
        0xAA | 0xC6 => Some(ImcLayout::MeteorLake),
        _ => None,
    }
}

/// Intel CPUs whose memory controller [`decode_intel`] reads.
pub const fn intel_imc(family: u32, model: u32) -> bool {
    intel_layout(family, model).is_some()
}

/// Tiger Lake (11th gen laptops, 2.4), from Intel's 11th Generation Intel Core Processors
/// datasheets, revision 003: volume 2a (document 631122) for UP3, UP4 and H35 (CPUID 806C1h and
/// 806C2h, model 0x8C, per volume 1, document 631121, table 15-1) and volume 2b (document 643524)
/// for H (806D1h, model 0x8D). Both have two controllers, the second 0x10000 above the first, and
/// the same fields at the same places within each register block; the blocks sit apart. DDR4 uses
/// one channel per controller, LPDDR4x two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TglMap {
    /// The shared registers (MAD_INTER_CHANNEL first).
    pub shared: u32,
    /// Channel 0's registers (TC_PRE first).
    pub ch0: u32,
    /// Channel 1's registers.
    pub ch1: u32,
}
/// Volume 2a, section 3.2: channel 0 at 0x4000, channel 1 at 0x4400, shared at 0x5000.
pub const TGL_UP: TglMap = TglMap {
    shared: 0x5000,
    ch0: 0x4000,
    ch1: 0x4400,
};
/// Volume 2b, section 3.2: channel 0 at 0xE000, channel 1 at 0xE800, shared at 0xD800.
pub const TGL_H: TglMap = TglMap {
    shared: 0xD800,
    ch0: 0xE000,
    ch1: 0xE800,
};
pub const TGL_MC1: u32 = 0x1_0000;
/// TC_PRE (64-bit): tRP in bits 6:0; the upper word has tRAS in bits 8:1 (40:33) and tRCD in bits
/// 15:9 (47:41). In tCK. Offsets from a channel's block.
pub const TGL_TC_PRE: u32 = 0x00;
pub const TGL_TC_PRE_HIGH: u32 = 0x04;
/// TC_ODT: tCL in bits 22:16, in tCK.
pub const TGL_TC_ODT: u32 = 0x70;
/// SC_GS_CFG: bit 31 gear 2; bit 30 NO_GEAR2_PARAM_DIVIDE (in gear 2, the timings in DCLKs, not
/// tCK).
pub const TGL_SC_GS_CFG: u32 = 0x88;
/// MAD_INTER_CHANNEL: DDR_TYPE in bits 2:0 (0 DDR4, 1 DDR5, 2 LPDDR5, 3 LPDDR4). Offsets from the
/// shared block.
pub const TGL_MAD_INTER: u32 = 0x00;
/// MAD_DIMM_CH0 and MAD_DIMM_CH1: DIMM L's size in bits 6:0, DIMM S's in bits 22:16 (0.5 GB
/// steps).
pub const TGL_MAD_DIMM_CH0: u32 = 0x0C;
pub const TGL_MAD_DIMM_CH1: u32 = 0x10;

/// Every register the Tiger Lake decoder reads: per controller its type and DIMM maps, per
/// channel its four timing registers; then SA_PERF_STATUS and BCLK_FREQ.
const fn tgl_regs(map: TglMap) -> [u32; 24] {
    let mut r = [0; 24];
    let mut i = 0;
    let mut mc = 0;
    while mc < 2 {
        let base = mc * TGL_MC1;
        r[i] = base + map.shared + TGL_MAD_INTER;
        r[i + 1] = base + map.shared + TGL_MAD_DIMM_CH0;
        r[i + 2] = base + map.shared + TGL_MAD_DIMM_CH1;
        i += 3;
        let chs = [map.ch0, map.ch1];
        let mut ch = 0;
        while ch < 2 {
            let at = base + chs[ch];
            r[i] = at + TGL_TC_PRE;
            r[i + 1] = at + TGL_TC_PRE_HIGH;
            r[i + 2] = at + TGL_TC_ODT;
            r[i + 3] = at + TGL_SC_GS_CFG;
            i += 4;
            ch += 1;
        }
        mc += 1;
    }
    r[i] = IMC_SA_PERF;
    r[i + 1] = IMC_BCLK;
    r
}
const TGL_UP_REGS: [u32; 24] = tgl_regs(TGL_UP);
const TGL_H_REGS: [u32; 24] = tgl_regs(TGL_H);

/// Tiger Lake's raw registers (models 0x8C and 0x8D), which `clokrix-live` writes beside the
/// decoded ones so a mismatch in a field check can be fixed from one file: in both controllers
/// the first 64 registers of channel 0's, channel 1's and the broadcast block (channel 0 + 0xC00
/// on UP3/UP4, + 0x1000 on H) and the first eight shared registers; then the memory clock's
/// registers (SA_PERF_STATUS, MC_BIOS_REQ, MC_BIOS_DATA, BCLK_FREQ). `None` for other CPUs.
pub fn intel_dump_regs(family: u32, model: u32) -> Option<Vec<u32>> {
    let (map, broadcast) = match (family, model) {
        (6, 0x8C) => (TGL_UP, 0x4C00),
        (6, 0x8D) => (TGL_H, 0xF000),
        _ => return None,
    };
    let mut regs = Vec::new();
    for mc in [0, TGL_MC1] {
        for block in [map.ch0, map.ch1, broadcast] {
            regs.extend((0..64).map(|i| mc + block + i * 4));
        }
        regs.extend((0..8).map(|i| mc + map.shared + i * 4));
    }
    regs.extend([IMC_SA_PERF, 0x5E00, IMC_BIOS_DATA, IMC_BCLK]);
    Some(regs)
}

/// Every register [`decode_intel`] reads for a layout (MCHBAR offsets), for dumps and fixtures.
pub const fn intel_regs(layout: ImcLayout) -> &'static [u32] {
    match layout {
        ImcLayout::Skylake => &SKL_REGS,
        ImcLayout::IceLake => &ICL_REGS,
        ImcLayout::AlderLake => &ALDER_REGS,
        ImcLayout::MeteorLake => &MTL_REGS,
        ImcLayout::TigerLake => &TGL_UP_REGS,
        ImcLayout::TigerLakeH => &TGL_H_REGS,
    }
}

/// Decodes an Intel memory controller's running timings: the data rate and CL-tRCD-tRP-tRAS,
/// which is what its registers give (the others stay 0). `None` when the registers do not look
/// like a trained controller.
pub fn decode_intel(layout: ImcLayout, read: impl Fn(u32) -> Option<u32>) -> Option<MemTimings> {
    let t = match layout {
        ImcLayout::Skylake | ImcLayout::IceLake => decode_skylake(layout, &read)?,
        ImcLayout::AlderLake => decode_alder(&read)?,
        ImcLayout::MeteorLake => decode_meteor(&read)?,
        ImcLayout::TigerLake => decode_tiger(TGL_UP, &read)?,
        ImcLayout::TigerLakeH => decode_tiger(TGL_H, &read)?,
    };
    let sane = (1000.0..=10000.0).contains(&t.mts)
        && (8..=80).contains(&t.cl)
        && t.rcd_rd != 0
        && t.rp != 0
        && t.ras != 0;
    sane.then_some(t)
}

/// Comet Lake and Rocket Lake: the channel in use is the first with a DIMM size.
fn decode_skylake(layout: ImcLayout, read: &impl Fn(u32) -> Option<u32>) -> Option<MemTimings> {
    let populated = |v: u32| v != u32::MAX && v & 0x003F_003F != 0;
    let ch = [(SKL_MAD_CH0, 0), (SKL_MAD_CH1, SKL_CH1)]
        .into_iter()
        .find(|&(mad, _)| read(mad).is_some_and(populated))?
        .1;
    let mts = if layout == ImcLayout::Skylake {
        if read(ch + SKL_SCHED)? & 3 != 0 {
            return None; // DDR3
        }
        // The ratio already counts the data rate, in steps of the 100 MHz reference (bit 7 set)
        // or of the 133 MHz one; the base clock is taken as 100 MHz.
        let perf = read(IMC_SA_PERF)?;
        let ratio = f64::from(perf & 0x7F);
        if perf & 0x80 != 0 {
            ratio * 100.0
        } else {
            ratio * 400.0 / 3.0
        }
    } else {
        let bclk = f64::from(read(IMC_BCLK)?) / 1000.0;
        if !(50.0..=200.0).contains(&bclk) {
            return None;
        }
        let bios = read(IMC_BIOS_DATA)?;
        let mut mts = f64::from(bios & 0xFF) * bclk;
        if bios & 0x1_0000 != 0 {
            mts *= 2.0;
        }
        if bios & 0xF00 == 0 {
            mts *= 4.0 / 3.0;
        }
        mts
    };
    let pre = read(ch + SKL_TC_PRE)?;
    Some(MemTimings {
        mts: mts.round(),
        cl: bits(read(ch + SKL_TC_CAS)?, 20, 16),
        rcd_rd: bits(pre, 5, 0),
        rp: bits(pre, 5, 0),
        ras: if layout == ImcLayout::Skylake {
            bits(pre, 14, 8)
        } else {
            bits(pre, 15, 9)
        },
        ..MemTimings::default()
    })
}

/// Alder Lake and Raptor Lake: the controller in use is the first whose channel 0 has a DIMM.
fn decode_alder(read: &impl Fn(u32) -> Option<u32>) -> Option<MemTimings> {
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
    Some(MemTimings {
        mts: mts.round(),
        cl: bits(read(mc + IMC_ODT_TCL)?, 22, 16),
        rcd_rd: bits(high, 26, 19),
        rp: bits(pre, 7, 0),
        ras: bits(high, 18, 10),
        ..MemTimings::default()
    })
}

/// Meteor Lake and Arrow Lake: the first controller that is on, and in it the channel with a
/// DIMM (a single DIMM may sit on channel 1 only).
fn decode_meteor(read: &impl Fn(u32) -> Option<u32>) -> Option<MemTimings> {
    let size = |v: u32| v & 0x007F_007F != 0;
    let mc = [0, IMC_MC1]
        .into_iter()
        .find(|&mc| read(mc + IMC_DIMM_CH0).is_some_and(|v| v != u32::MAX))?;
    let ch0 = read(mc + IMC_DIMM_CH0)?;
    let ch = if !size(ch0) && read(mc + MTL_DIMM_CH1).is_some_and(size) {
        mc + MTL_CH1
    } else {
        mc
    };
    let clock = read(MTL_PTGRAM)?;
    let reference = match bits(clock, 23, 20) {
        0x1 => 200.0,
        0x2 => 100.0,
        0xA => 400.0 / 3.0,
        0xB => 200.0 / 3.0,
        0xC => 100.0 / 3.0,
        _ => return None,
    };
    let gear = f64::from(bits(clock, 24, 24) + 1);
    let mts = reference * f64::from(bits(clock, 19, 12)) * gear * 2.0;
    Some(MemTimings {
        mts: mts.round(),
        cl: bits(read(ch + IMC_ODT_TCL)?, 22, 16),
        rcd_rd: bits(read(ch + MTL_TC_ACT)?, 29, 22),
        rp: bits(read(ch + IMC_TC_PRE)?, 7, 0),
        ras: bits(read(ch + IMC_TC_PRE_HIGH)?, 21, 13),
        ..MemTimings::default()
    })
}

/// Tiger Lake, from Intel's datasheets 631122 and 643524 (section 3.2 for the controller, 3.3
/// for the clock): the first controller with a DIMM size in MAD_DIMM_CH0 or MAD_DIMM_CH1, and in
/// it the first such channel. DDR4 and LPDDR4x only: LPDDR5's timings count WCK cycles, whose
/// ratio to the data rate the datasheets do not give, and DDR5 is not a Tiger Lake memory. The
/// data rate is SA_PERF_STATUS's QCLK_RATIO (bits 9:2) × BCLK_FREQ (kHz), × 4/3 when
/// QCLK_REFERENCE (bit 10) is 0 (the 133.34 MHz reference); MC_BIOS_REQ's GEAR_TYPE has the DDR
/// bus clock equal to QCLK in gear 1 and twice it in gear 2, and a double data rate carries two
/// transfers per clock. In gear 2 with NO_GEAR2_PARAM_DIVIDE set the timings count DCLKs, not
/// tCK: `None`, as the datasheets give no conversion.
fn decode_tiger(map: TglMap, read: &impl Fn(u32) -> Option<u32>) -> Option<MemTimings> {
    let size = |v: u32| v != u32::MAX && bits(v, 6, 0) + bits(v, 22, 16) > 0;
    let (mc, ch) = [0, TGL_MC1]
        .into_iter()
        .flat_map(|mc| {
            [
                (mc, TGL_MAD_DIMM_CH0, map.ch0),
                (mc, TGL_MAD_DIMM_CH1, map.ch1),
            ]
        })
        .find(|&(mc, mad, _)| read(mc + map.shared + mad).is_some_and(size))
        .map(|(mc, _, ch)| (mc, mc + ch))?;
    if !matches!(read(mc + map.shared + TGL_MAD_INTER)? & 7, 0 | 3) {
        return None;
    }
    let cfg = read(ch + TGL_SC_GS_CFG)?;
    let gear2 = cfg & 1 << 31 != 0;
    if gear2 && cfg & 1 << 30 != 0 {
        return None;
    }
    let bclk = f64::from(read(IMC_BCLK)?) / 1000.0;
    if !(50.0..=200.0).contains(&bclk) {
        return None;
    }
    let perf = read(IMC_SA_PERF)?;
    let mut qclk = f64::from(bits(perf, 9, 2)) * bclk;
    if perf & 1 << 10 == 0 {
        qclk *= 4.0 / 3.0;
    }
    let mts = qclk * if gear2 { 4.0 } else { 2.0 };
    let high = read(ch + TGL_TC_PRE_HIGH)?;
    Some(MemTimings {
        mts: mts.round(),
        cl: bits(read(ch + TGL_TC_ODT)?, 22, 16),
        rcd_rd: bits(high, 15, 9),
        rp: bits(read(ch + TGL_TC_PRE)?, 6, 0),
        ras: bits(high, 8, 1),
        ..MemTimings::default()
    })
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
        let t = decode_intel(ImcLayout::AlderLake, |a| regs.get(&a).copied()).expect("timings");
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
            decode_intel(ImcLayout::AlderLake, |a| ddr5.get(&a).copied())
                .expect("ddr5")
                .mts,
            6000.0
        );
        assert!(intel_imc(6, 0x97));
        assert!(intel_imc(6, 0xB7));
        assert_eq!(
            intel_layout(6, 0xC6),
            Some(ImcLayout::MeteorLake),
            "Arrow Lake"
        );
        assert_eq!(
            intel_layout(6, 0xA5),
            Some(ImcLayout::Skylake),
            "Comet Lake-S"
        );
        assert_eq!(
            intel_layout(6, 0xA7),
            Some(ImcLayout::IceLake),
            "Rocket Lake"
        );
        assert_eq!(intel_layout(6, 0xA6), None, "Comet Lake-U: not read");
        assert_eq!(intel_layout(0x12, 0x01), None, "another family");
    }

    #[test]
    fn rejects_an_intel_controller_without_memory_or_with_nonsense() {
        let mut regs = adl_ddr4();
        regs.insert(IMC_MC1 + IMC_DIMM_CH0, u32::MAX);
        assert_eq!(
            decode_intel(ImcLayout::AlderLake, |a| regs.get(&a).copied()),
            None,
            "no controller in use"
        );
        let mut regs = adl_ddr4();
        regs.insert(IMC_BCLK, 0);
        assert_eq!(
            decode_intel(ImcLayout::AlderLake, |a| regs.get(&a).copied()),
            None,
            "no base clock"
        );
        let mut regs = adl_ddr4();
        regs.insert(IMC_MC1 + IMC_ODT_TCL, 0);
        assert_eq!(
            decode_intel(ImcLayout::AlderLake, |a| regs.get(&a).copied()),
            None,
            "CL 0"
        );
        assert_eq!(decode_intel(ImcLayout::AlderLake, |_| None), None);
    }

    /// Constructed Comet Lake-S registers in memtest86+'s Skylake layout (no field dump from such
    /// a PC): DDR4-2933 16-18-18-36 on channel 1, ratio 22 on the 133 MHz reference.
    #[test]
    fn decodes_comet_lake() {
        let mut regs = HashMap::from([
            (SKL_MAD_CH0, 0),
            (SKL_MAD_CH1, 0x0000_0010),
            (SKL_CH1 + SKL_SCHED, 0),
            (IMC_SA_PERF, 22),
            (SKL_CH1 + SKL_TC_PRE, 18 | 36 << 8),
            (SKL_CH1 + SKL_TC_CAS, 16 << 16),
        ]);
        let read = |r: &HashMap<u32, u32>| decode_intel(ImcLayout::Skylake, |a| r.get(&a).copied());
        let t = read(&regs).expect("timings");
        assert_eq!(t.mts, 2933.0);
        assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (16, 18, 18, 36));
        // Bit 7: the 100 MHz reference, ratio 32 = DDR4-3200.
        regs.insert(IMC_SA_PERF, 0x80 | 32);
        assert_eq!(read(&regs).map(|t| t.mts), Some(3200.0));
        regs.insert(SKL_CH1 + SKL_SCHED, 1);
        assert_eq!(read(&regs), None, "DDR3");
        regs.insert(SKL_CH1 + SKL_SCHED, 0);
        regs.insert(SKL_MAD_CH1, 0);
        assert_eq!(read(&regs), None, "no channel in use");
        assert_eq!(intel_regs(ImcLayout::Skylake).len(), 8);
        assert_eq!(intel_dump_regs(6, 0xA5), None, "decoded, not dumped");
    }

    /// Constructed Rocket Lake registers in memtest86+'s Ice Lake layout: DDR4-3200 16-16-16-36
    /// in gear 1 on the 133 MHz reference, then DDR4-3600 in gear 2 on the 100 MHz one.
    #[test]
    fn decodes_rocket_lake() {
        let mut regs = HashMap::from([
            (SKL_MAD_CH0, 0x0000_0010),
            (SKL_MAD_CH1, 0x0000_0010),
            (IMC_BCLK, 100_000),
            (IMC_BIOS_DATA, 24),
            (SKL_TC_PRE, 16 | 36 << 9),
            (SKL_TC_CAS, 16 << 16),
        ]);
        let read = |r: &HashMap<u32, u32>| decode_intel(ImcLayout::IceLake, |a| r.get(&a).copied());
        let t = read(&regs).expect("timings");
        assert_eq!(t.mts, 3200.0);
        assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (16, 16, 16, 36));
        regs.insert(IMC_BIOS_DATA, 0x1_0000 | 0x100 | 18);
        assert_eq!(read(&regs).map(|t| t.mts), Some(3600.0));
        regs.insert(IMC_BCLK, 0);
        assert_eq!(read(&regs), None, "no base clock");
    }

    /// Constructed Arrow Lake registers in memtest86+'s Meteor Lake layout: the first controller
    /// off, one DIMM on channel 1 of the second, DDR5-6400 32-39-39-102 (ratio 48 of the
    /// 33.3 MHz reference, gear 2).
    #[test]
    fn decodes_arrow_lake() {
        let mut regs = HashMap::from([
            (IMC_DIMM_CH0, u32::MAX),
            (IMC_MC1 + IMC_DIMM_CH0, 0),
            (IMC_MC1 + MTL_DIMM_CH1, 0x0000_0020),
            (MTL_PTGRAM, 0xC << 20 | 48 << 12 | 1 << 24),
            (IMC_MC1 + MTL_CH1 + IMC_ODT_TCL, 32 << 16),
            (IMC_MC1 + MTL_CH1 + MTL_TC_ACT, 39 << 22),
            (IMC_MC1 + MTL_CH1 + IMC_TC_PRE, 39),
            (IMC_MC1 + MTL_CH1 + IMC_TC_PRE_HIGH, 102 << 13),
        ]);
        let read =
            |r: &HashMap<u32, u32>| decode_intel(ImcLayout::MeteorLake, |a| r.get(&a).copied());
        let t = read(&regs).expect("timings");
        assert_eq!(t.mts, 6400.0);
        assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (32, 39, 39, 102));
        assert!(intel_regs(ImcLayout::MeteorLake).contains(&(IMC_MC1 + MTL_CH1 + MTL_TC_ACT)));
        regs.insert(MTL_PTGRAM, 0x5 << 20 | 48 << 12);
        assert_eq!(read(&regs), None, "an unknown reference clock");
        regs.insert(IMC_MC1 + IMC_DIMM_CH0, u32::MAX);
        assert_eq!(read(&regs), None, "both controllers off");
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
    fn amd_cpus_get_their_controller_layout() {
        assert_eq!(umc_layout(0x17, 0x71), UmcLayout::Ddr4, "Matisse");
        assert_eq!(umc_layout(0x17, 0x08), UmcLayout::Ddr4, "Pinnacle Ridge");
        assert_eq!(umc_layout(0x19, 0x21), UmcLayout::Ddr4, "Vermeer");
        assert_eq!(umc_layout(0x19, 0x50), UmcLayout::Ddr4, "Cezanne");
        assert_eq!(umc_layout(0x19, 0x44), UmcLayout::Ddr5, "Rembrandt");
        assert_eq!(
            umc_layout(0x17, 0xA0),
            UmcLayout::Ddr5,
            "Mendocino: LPDDR5, never read as DDR4"
        );
        assert_eq!(umc_layout(0x19, 0x61), UmcLayout::Ddr5, "Raphael");
        assert_eq!(umc_layout(0x1A, 0x44), UmcLayout::Ddr5, "Granite Ridge");
    }

    /// Constructed registers in the ZenStates-Core DDR4 layout for a Ryzen 5000 with DDR4-3600
    /// 16-19-19-39 (not a field dump: none from an AM4 PC yet): ratio 54 thirds, 1T, gear-down
    /// on, tRFC 630 in the second copy behind the placeholder, power-down off.
    fn ddr4_3600() -> HashMap<u32, u32> {
        HashMap::from([
            (UMC_DIMM_CS[0], 1),
            (UMC_REFRESH, 0),
            (UMC_RATIO, 54 | 1 << 11),
            (0x5_0204, 16 | 39 << 8 | 19 << 16 | 19 << 24),
            (0x5_0208, 58 | 19 << 16),
            (0x5_020C, 4 | 6 << 8 | 12 << 24),
            (0x5_0210, 16),
            (0x5_0214, 16 | 4 << 8 | 12 << 16),
            (0x5_0218, 26),
            (0x5_0230, 14029),
            (UMC_RFC[0], DDR4_RFC_UNSET),
            (UMC_RFC[1], 630 | 468 << 11 | 288 << 22),
        ])
    }

    #[test]
    fn decodes_ddr4_timings() {
        let regs = ddr4_3600();
        let t = decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)).expect("timings");
        assert_eq!(t.mts, 3600.0);
        assert_eq!(
            (t.cl, t.rcd_rd, t.rcd_wr, t.rp, t.ras, t.rc),
            (16, 19, 19, 19, 39, 58)
        );
        assert_eq!((t.rrd_s, t.rrd_l, t.faw, t.rtp), (4, 6, 16, 12));
        assert_eq!((t.cwl, t.wtr_s, t.wtr_l, t.wr), (16, 4, 12, 26));
        assert_eq!((t.rfc, t.rfc_sb, t.refi), (630, 0, 14029));
        assert_eq!(t.command_rate, 1);
        assert!(t.gear_down);
        assert!(!t.power_down);
    }

    #[test]
    fn ddr4_refresh_modes_and_odd_ratios() {
        let mut regs = ddr4_3600();
        regs.insert(UMC_REFRESH, 1 << 16 | 1 << 28);
        let t = decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)).expect("2x");
        assert_eq!(t.rfc, 468, "fine-granularity 2x: tRFC2");
        assert!(t.power_down);
        regs.insert(UMC_REFRESH, 6 << 16);
        let t = decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)).expect("4x");
        assert_eq!(t.rfc, 288, "on the fly 1x/4x: tRFC4");
        // The first copy set: it counts.
        regs.insert(UMC_REFRESH, 0);
        regs.insert(UMC_RFC[0], 560);
        let t = decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)).expect("first");
        assert_eq!(t.rfc, 560);
        // DDR4-3466 and 3733: thirds of the reference, rounded down; 2T.
        regs.insert(UMC_RATIO, 52 | 1 << 10);
        let t = decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)).expect("3466");
        assert_eq!((t.mts, t.command_rate, t.gear_down), (3466.0, 2, false));
        regs.insert(UMC_RATIO, 56);
        assert_eq!(
            decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)).map(|t| t.mts),
            Some(3733.0)
        );
    }

    #[test]
    fn rejects_an_untrained_ddr4_controller() {
        assert_eq!(decode_ddr4(|_| 0), None);
        assert_eq!(
            decode_ddr4(|_| u32::MAX),
            None,
            "all ones: ratio 127, 8466 MT/s"
        );
        let mut regs = ddr4_3600();
        regs.insert(0x5_0204, 0);
        assert_eq!(decode_ddr4(|a| regs.get(&a).copied().unwrap_or(0)), None);
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

    /// One Tiger Lake channel's constructed registers (from the fields in Intel's datasheets
    /// 631122 and 643524; no field dump from such a laptop yet): DIMM size in `mad`, the DDR type,
    /// SC_GS_CFG and CL-tRCD-tRP-tRAS.
    fn tgl_channel(
        regs: &mut HashMap<u32, u32>,
        map: TglMap,
        mc: u32,
        ch1: bool,
        ddr_type: u32,
        cfg: u32,
        (cl, rcd, rp, ras): (u32, u32, u32, u32),
    ) {
        let shared = mc + map.shared;
        let (mad, ch) = if ch1 {
            (TGL_MAD_DIMM_CH1, mc + map.ch1)
        } else {
            (TGL_MAD_DIMM_CH0, mc + map.ch0)
        };
        regs.insert(shared + TGL_MAD_INTER, ddr_type);
        regs.insert(shared + mad, 16);
        regs.insert(ch + TGL_SC_GS_CFG, cfg);
        regs.insert(ch + TGL_TC_PRE, rp);
        regs.insert(ch + TGL_TC_PRE_HIGH, rcd << 9 | ras << 1);
        regs.insert(ch + TGL_TC_ODT, cl << 16);
    }

    /// Constructed: DDR4-3200 22-22-22-52 in gear 1 on channel 0 of the first controller, QCLK
    /// ratio 12 on the 133 MHz reference (12 × 100 MHz × 4/3 = 1600 MHz QCLK).
    fn tgl_ddr4(map: TglMap) -> HashMap<u32, u32> {
        let mut regs = HashMap::from([(IMC_BCLK, 100_000), (IMC_SA_PERF, 12 << 2)]);
        tgl_channel(&mut regs, map, 0, false, 0, 0, (22, 22, 22, 52));
        regs
    }

    fn read_tiger(layout: ImcLayout, r: &HashMap<u32, u32>) -> Option<MemTimings> {
        decode_intel(layout, |a| r.get(&a).copied())
    }

    #[test]
    fn decodes_tiger_lake_ddr4() {
        for (layout, map) in [
            (ImcLayout::TigerLake, TGL_UP),
            (ImcLayout::TigerLakeH, TGL_H),
        ] {
            let mut regs = tgl_ddr4(map);
            let t = read_tiger(layout, &regs).expect("timings");
            assert_eq!(t.mts, 3200.0);
            assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (22, 22, 22, 52));
            assert_eq!((t.rc, t.rfc), (0, 0), "not read on Intel");
            // The 100 MHz reference (bit 10): ratio 16 is the same DDR4-3200.
            regs.insert(IMC_SA_PERF, 1 << 10 | 16 << 2);
            assert_eq!(read_tiger(layout, &regs).map(|t| t.mts), Some(3200.0));
            // The fields' neighbours (tRPab_ext and up; tCWL) are not part of the timings.
            regs.insert(map.ch0 + TGL_TC_PRE, 22 | 0xFFFF_FF80);
            regs.insert(map.ch0 + TGL_TC_ODT, 22 << 16 | 6 << 23);
            let t = read_tiger(layout, &regs).expect("timings");
            assert_eq!((t.cl, t.rp), (22, 22));
        }
        // UP3's registers mean nothing to H's decoder.
        assert_eq!(read_tiger(ImcLayout::TigerLakeH, &tgl_ddr4(TGL_UP)), None);
        assert_eq!(intel_layout(6, 0x8C), Some(ImcLayout::TigerLake));
        assert_eq!(intel_layout(6, 0x8D), Some(ImcLayout::TigerLakeH));
        assert!(intel_imc(6, 0x8D));
    }

    /// Constructed: LPDDR4x-4267 36-39-39-90 in gear 2 on both channels of both controllers,
    /// ratio 8 on the 133 MHz reference (1066.7 MHz QCLK, a 2133.3 MHz bus).
    #[test]
    fn decodes_tiger_lake_lpddr4x() {
        let mut regs = HashMap::from([(IMC_BCLK, 100_000), (IMC_SA_PERF, 8 << 2)]);
        for mc in [0, TGL_MC1] {
            for ch1 in [false, true] {
                tgl_channel(&mut regs, TGL_UP, mc, ch1, 3, 1 << 31, (36, 39, 39, 90));
            }
        }
        let layout = ImcLayout::TigerLake;
        let t = read_tiger(layout, &regs).expect("timings");
        assert_eq!(t.mts, 4267.0);
        assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (36, 39, 39, 90));
        regs.insert(TGL_UP.ch0 + TGL_SC_GS_CFG, 1 << 31 | 1 << 30);
        assert_eq!(read_tiger(layout, &regs), None, "gear 2 timings in DCLKs");
        regs.insert(TGL_UP.ch0 + TGL_SC_GS_CFG, 1 << 31);
        regs.insert(TGL_UP.shared + TGL_MAD_INTER, 2);
        assert_eq!(read_tiger(layout, &regs), None, "LPDDR5: WCK cycles");
    }

    /// Constructed: one SO-DIMM on the second controller (the first's maps empty), DDR4-2933
    /// 21-21-21-47, ratio 11 on the 133 MHz reference; then on that controller's channel 1.
    #[test]
    fn decodes_tiger_lake_second_controller_and_channel() {
        for (layout, map) in [
            (ImcLayout::TigerLake, TGL_UP),
            (ImcLayout::TigerLakeH, TGL_H),
        ] {
            let mut regs = HashMap::from([
                (IMC_BCLK, 100_000),
                (IMC_SA_PERF, 11 << 2),
                (map.shared + TGL_MAD_INTER, 0),
                (map.shared + TGL_MAD_DIMM_CH0, 0),
                (map.shared + TGL_MAD_DIMM_CH1, 0),
            ]);
            tgl_channel(&mut regs, map, TGL_MC1, false, 0, 0, (21, 21, 21, 47));
            let t = read_tiger(layout, &regs).expect("timings");
            assert_eq!(t.mts, 2933.0);
            assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (21, 21, 21, 47));
            regs.insert(TGL_MC1 + map.shared + TGL_MAD_DIMM_CH0, 0);
            tgl_channel(&mut regs, map, TGL_MC1, true, 0, 0, (20, 20, 20, 46));
            let t = read_tiger(layout, &regs).expect("channel 1");
            assert_eq!((t.cl, t.rcd_rd, t.rp, t.ras), (20, 20, 20, 46));
        }
    }

    #[test]
    fn rejects_an_untrained_or_switched_off_tiger_lake() {
        let layout = ImcLayout::TigerLake;
        let mad = TGL_UP.shared + TGL_MAD_DIMM_CH0;
        let mut regs = tgl_ddr4(TGL_UP);
        regs.insert(mad, 0);
        assert_eq!(read_tiger(layout, &regs), None, "no DIMM mapped: untrained");
        regs.insert(mad, u32::MAX);
        assert_eq!(read_tiger(layout, &regs), None, "all ones: switched off");
        let mut regs = tgl_ddr4(TGL_UP);
        regs.insert(IMC_SA_PERF, 0);
        assert_eq!(
            read_tiger(layout, &regs),
            None,
            "QCLK ratio 0: the PLL is off"
        );
        let mut regs = tgl_ddr4(TGL_UP);
        regs.insert(TGL_UP.ch0 + TGL_TC_ODT, 0);
        assert_eq!(read_tiger(layout, &regs), None, "CL 0");
        let mut regs = tgl_ddr4(TGL_UP);
        regs.insert(IMC_BCLK, 0);
        assert_eq!(read_tiger(layout, &regs), None, "no base clock");
        assert_eq!(decode_intel(layout, |_| None), None);
        assert_eq!(decode_intel(ImcLayout::TigerLakeH, |_| None), None);
    }

    #[test]
    fn tiger_lake_is_decoded_and_dumped() {
        for (model, layout, expected) in [
            (
                0x8C,
                ImcLayout::TigerLake,
                [0x4004, 0x4088, 0x4470, 0x5010, 0x1_4404, 0x1_5000, 0x4C00],
            ),
            (
                0x8D,
                ImcLayout::TigerLakeH,
                [0xE004, 0xE088, 0xE870, 0xD810, 0x1_E804, 0x1_D800, 0xF000],
            ),
        ] {
            let decoded = intel_regs(layout);
            assert_eq!(decoded.len(), 24);
            let regs = intel_dump_regs(6, model).expect("Tiger Lake");
            assert_eq!(regs.len(), 2 * (3 * 64 + 8) + 4);
            assert!(
                decoded.iter().all(|r| regs.contains(r)),
                "the dump holds every decoded register"
            );
            let (dumped_only, decoded_too) = expected.split_last().expect("registers");
            for r in decoded_too {
                assert!(decoded.contains(r) && regs.contains(r), "{r:#x}");
            }
            assert!(regs.contains(dumped_only), "the broadcast block");
            for r in [0x5918, 0x5E00, 0x5E04, 0x5F60] {
                assert!(regs.contains(&r), "{r:#x}");
            }
            let mut sorted = regs.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), regs.len(), "no register twice");
            assert!(
                regs.iter().all(|&r| r % 4 == 0 && r < 0x2_0000),
                "PawnIO's 128 KB window"
            );
        }
        assert!(
            !intel_dump_regs(6, 0x8C)
                .expect("Tiger Lake")
                .contains(&0x4100),
            "the first 64 registers of each block only"
        );
        assert_eq!(intel_dump_regs(6, 0xA7), None);
        assert_eq!(intel_dump_regs(0x19, 0x8C), None);
    }
}

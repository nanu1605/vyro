//! Memory sensors through PawnIO (Phase 6d): DDR5 module temperatures from the SPD5 hub's
//! thermal sensor (`SmbusPIIX4`) and running timings from the AMD memory controller (UMC,
//! `AMDFamily17` SMN reads). Decoders here are pure; `win` does the bus access.
//!
//! Vyro only reads. The hub's thermal registers sit in its register space, which does not depend
//! on the NVM page (MR11), so no SPD write is ever needed. The UMC register layout is the one
//! documented by ZenTimings / ZenStates-Core for Zen 4 and Zen 5.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{probe_spd, read_umc, RamProvider, SpdRaw, UmcRaw};

use crate::snapshot::MemTimings;

/// SMBus addresses of SPD devices (one per DIMM slot).
pub const SPD_FIRST: u8 = 0x50;
pub const SPD_LAST: u8 = 0x57;

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

/// Slot name for an SPD address, as boards wire them: `0x50 + 2 × channel + slot`, so 0x51 is
/// the second slot of channel A ("DIMMA2").
pub fn slot_name(addr: u8) -> Option<String> {
    if !(SPD_FIRST..=SPD_LAST).contains(&addr) {
        return None;
    }
    let n = addr - SPD_FIRST;
    Some(format!("DIMM{}{}", char::from(b'A' + n / 2), n % 2 + 1))
}

pub fn is_spd5_hub(mr0: u8, mr1: u8) -> bool {
    [mr0, mr1] == SPD5118
}

/// Whether the hub has a thermal sensor and firmware left it on. Vyro never turns it on.
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

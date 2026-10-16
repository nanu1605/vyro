//! SMBIOS / DMI table parser (DMTF DSP0134).
//!
//! Input is the buffer Windows returns from `GetSystemFirmwareTable('RSMB')`: an 8-byte
//! `RawSMBIOSData` header followed by the structure table. Only the fields Vyro displays are
//! decoded; every accessor bounds-checks against the structure's declared length, so short or
//! older-version structures yield `None` instead of reading garbage.

use crate::info::{BoardId, DimmInfo, FormFactor, MemoryId};

pub struct Smbios<'a> {
    pub major: u8,
    pub minor: u8,
    table: &'a [u8],
}

#[derive(Debug)]
pub struct Structure<'a> {
    pub kind: u8,
    pub handle: u16,
    formatted: &'a [u8],
    strings: Vec<&'a [u8]>,
}

impl<'a> Smbios<'a> {
    /// Parses a `RawSMBIOSData` buffer.
    pub fn from_raw(raw: &'a [u8]) -> Option<Self> {
        let header = raw.get(..8)?;
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let table = raw.get(8..8 + len).or_else(|| raw.get(8..))?;
        Some(Self {
            major: header[1],
            minor: header[2],
            table,
        })
    }

    pub fn version(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    pub fn structures(&self) -> Vec<Structure<'a>> {
        let mut out = Vec::new();
        let mut rest = self.table;
        while rest.len() >= 4 {
            let kind = rest[0];
            let len = rest[1] as usize;
            if len < 4 || len > rest.len() {
                break;
            }
            let formatted = &rest[..len];
            let handle = u16::from_le_bytes([rest[2], rest[3]]);

            // String set: NUL-terminated strings, ended by an empty string. A structure with no
            // strings is followed by two NULs.
            let mut strings = Vec::new();
            let mut i = len;
            if rest.get(i) == Some(&0) {
                i += 1;
                if rest.get(i) == Some(&0) {
                    i += 1;
                }
            } else {
                loop {
                    let Some(end) = rest[i..].iter().position(|&b| b == 0) else {
                        i = rest.len();
                        break;
                    };
                    if end == 0 {
                        i += 1;
                        break;
                    }
                    strings.push(&rest[i..i + end]);
                    i += end + 1;
                }
            }
            out.push(Structure {
                kind,
                handle,
                formatted,
                strings,
            });
            if kind == 127 {
                break;
            }
            rest = rest.get(i..).unwrap_or_default();
        }
        out
    }

    fn of_kind(&self, kind: u8) -> impl Iterator<Item = Structure<'a>> {
        self.structures()
            .into_iter()
            .filter(move |s| s.kind == kind)
    }
}

/// Firmware placeholders that mean "no value".
const PLACEHOLDERS: &[&str] = &[
    "to be filled by o.e.m.",
    "default string",
    "not specified",
    "not applicable",
    "not available",
    "system product name",
    "system manufacturer",
    "system version",
    "system serial number",
    "base board serial number",
    "chassis serial number",
    "none",
    "n/a",
    "unknown",
    "undefined",
    "oem",
    "0",
    "123456789",
    "0123456789",
];

pub fn clean(s: &str) -> Option<String> {
    let t = s.trim().trim_matches(char::from(0)).trim();
    let lower = t.to_ascii_lowercase();
    if t.is_empty()
        || PLACEHOLDERS.contains(&lower.as_str())
        || t.chars().all(|c| c == 'F' || c == 'f')
    {
        None
    } else {
        Some(t.to_owned())
    }
}

impl Structure<'_> {
    pub fn byte(&self, off: usize) -> Option<u8> {
        self.formatted.get(off).copied()
    }

    pub fn word(&self, off: usize) -> Option<u16> {
        let b = self.formatted.get(off..off + 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn dword(&self, off: usize) -> Option<u32> {
        let b = self.formatted.get(off..off + 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn qword(&self, off: usize) -> Option<u64> {
        let b = self.formatted.get(off..off + 8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Some(u64::from_le_bytes(a))
    }

    /// String referenced by the index byte at `off`, with placeholders filtered out.
    pub fn string(&self, off: usize) -> Option<String> {
        let idx = self.byte(off)? as usize;
        let raw = self.strings.get(idx.checked_sub(1)?)?;
        clean(&String::from_utf8_lossy(raw))
    }
}

// ---------------------------------------------------------------------------------------------
// Typed extraction
// ---------------------------------------------------------------------------------------------

/// Type 0 (BIOS), 1 (System), 2 (Baseboard) and 3 (the chassis: desktop or laptop). Chipset
/// is not in SMBIOS.
pub fn board(smbios: &Smbios) -> BoardId {
    let mut b = BoardId::default();
    if let Some(s) = smbios.of_kind(0).next() {
        b.bios_vendor = s.string(0x04);
        b.bios_version = s.string(0x05);
        b.bios_date = s.string(0x08);
    }
    if let Some(s) = smbios.of_kind(1).next() {
        b.system_manufacturer = s.string(0x04);
        b.system_product = s.string(0x05);
    }
    if let Some(s) = smbios.of_kind(2).next() {
        b.manufacturer = s.string(0x04);
        b.product = s.string(0x05);
        b.version = s.string(0x06);
        b.serial = s.string(0x07);
    }
    if let Some(code) = smbios.of_kind(3).next().and_then(|s| s.byte(0x05)) {
        b.form_factor = FormFactor::from_chassis(code);
    }
    b
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcessorInfo {
    pub socket: Option<String>,
    pub max_mhz: Option<u32>,
    pub current_mhz: Option<u32>,
    pub cores: Option<u32>,
    pub threads: Option<u32>,
    pub populated_sockets: u32,
}

/// Type 4 (Processor): first populated central processor, plus the populated socket count.
pub fn processor(smbios: &Smbios) -> ProcessorInfo {
    let mut out = ProcessorInfo::default();
    for s in smbios.of_kind(4) {
        let central = s.byte(0x05) == Some(3);
        let populated = s.byte(0x18).is_some_and(|st| st & 0x40 != 0);
        if !central || !populated {
            continue;
        }
        out.populated_sockets += 1;
        if out.populated_sockets > 1 {
            continue;
        }
        let nz = |v: Option<u16>| v.filter(|&x| x != 0 && x != 0xFFFF).map(u32::from);
        out.socket = s.string(0x04);
        out.max_mhz = nz(s.word(0x14));
        out.current_mhz = nz(s.word(0x16));
        // 0xFF in the byte fields means "see the 3.0+ word fields".
        out.cores = match s.byte(0x23) {
            Some(0xFF) => nz(s.word(0x2A)),
            b => b.filter(|&x| x != 0).map(u32::from),
        };
        out.threads = match s.byte(0x25) {
            Some(0xFF) => nz(s.word(0x2E)),
            b => b.filter(|&x| x != 0).map(u32::from),
        };
    }
    out
}

pub fn memory_type_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x12 => "DDR",
        0x13 => "DDR2",
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x20 => "HBM",
        0x21 => "HBM2",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        0x24 => "HBM3",
        _ => return None,
    })
}

pub fn form_factor_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x05 => "Chip",
        0x09 => "DIMM",
        0x0C => "RIMM",
        0x0D => "SODIMM",
        0x0F => "FB-DIMM",
        0x10 => "Die",
        0x11 => "CAMM",
        _ => return None,
    })
}

/// JEDEC manufacturer codes as firmware commonly prints them (continuation byte + id, in
/// either byte order).
const JEDEC: &[(&str, &str)] = &[
    ("80CE", "Samsung"),
    ("80AD", "SK hynix"),
    ("802C", "Micron"),
    ("859B", "Crucial"),
    ("0198", "Kingston"),
    ("04CD", "G.Skill"),
    ("029E", "Corsair"),
    ("014F", "Transcend"),
    ("04CB", "ADATA"),
    ("04EF", "Team Group"),
];

/// Maps a raw SMBIOS manufacturer (a name or a JEDEC hex code) to a readable name.
pub fn dimm_vendor(raw: &str) -> Option<String> {
    let s = clean(raw)?;
    let hex = s
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .to_ascii_uppercase();
    if hex.len() >= 4 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let head = &hex[..4];
        let swapped = format!("{}{}", &head[2..4], &head[0..2]);
        if let Some((_, name)) = JEDEC
            .iter()
            .find(|(code, _)| *code == head || *code == swapped)
        {
            return Some((*name).to_owned());
        }
    }
    Some(s)
}

/// Module vendor inferred from a part-number prefix, for firmware that leaves the manufacturer
/// blank or "Unknown".
pub fn vendor_from_part_number(part: &str) -> Option<&'static str> {
    const PREFIXES: &[(&str, &str)] = &[
        ("CMK", "Corsair"),
        ("CMH", "Corsair"),
        ("CMT", "Corsair"),
        ("CMW", "Corsair"),
        ("CMS", "Corsair"),
        ("CMP", "Corsair"),
        ("KF", "Kingston"),
        ("KVR", "Kingston"),
        ("KCP", "Kingston"),
        ("F5-", "G.Skill"),
        ("F4-", "G.Skill"),
        ("CT", "Crucial"),
        ("BL", "Crucial"),
        ("M32", "Samsung"),
        ("M42", "Samsung"),
        ("M47", "Samsung"),
        ("HMC", "SK hynix"),
        ("HMA", "SK hynix"),
        ("MTC", "Micron"),
        ("MTA", "Micron"),
        ("TED", "Team Group"),
        ("FF3", "Team Group"),
        ("AX5", "ADATA"),
        ("AD5", "ADATA"),
        ("PVV", "Patriot"),
        ("PSD", "Patriot"),
    ];
    let up = part.trim().to_ascii_uppercase();
    PREFIXES
        .iter()
        .find(|(p, _)| up.starts_with(p))
        .map(|(_, v)| *v)
}

/// Channel letter from a slot or bank label, e.g. "P0 CHANNEL A", "DIMM_B2",
/// "Controller0-ChannelA-DIMM1".
fn channel_of(label: &str) -> Option<char> {
    let up = label.to_ascii_uppercase();
    if let Some(i) = up.find("CHANNEL") {
        return up[i + 7..]
            .chars()
            .find(|c| !matches!(c, ' ' | '_' | '-'))
            .filter(char::is_ascii_alphabetic);
    }
    let i = up.find("DIMM")?;
    let mut rest = up[i + 4..]
        .chars()
        .skip_while(|c| matches!(c, ' ' | '_' | '-'));
    let letter = rest.next().filter(char::is_ascii_alphabetic)?;
    rest.next().filter(char::is_ascii_digit).map(|_| letter)
}

/// Distinct populated channels, if every populated DIMM's labels name one.
pub fn channel_count(dimms: &[DimmInfo]) -> Option<u32> {
    let mut seen: Vec<char> = Vec::new();
    for d in dimms {
        let ch = d
            .bank
            .as_deref()
            .and_then(channel_of)
            .or_else(|| channel_of(&d.slot))?;
        if !seen.contains(&ch) {
            seen.push(ch);
        }
    }
    (!seen.is_empty()).then_some(seen.len() as u32)
}

/// Type 16 (Physical Memory Array) and 17 (Memory Device).
pub fn memory(smbios: &Smbios) -> MemoryId {
    let mut m = MemoryId::default();
    let mut max_kb: u64 = 0;
    let mut slots: u32 = 0;
    for a in smbios.of_kind(16) {
        if a.byte(0x05) != Some(3) {
            continue; // not system memory
        }
        slots += u32::from(a.word(0x0D).unwrap_or(0));
        max_kb += match a.dword(0x07) {
            Some(0x8000_0000) => a.qword(0x0F).map_or(0, |bytes| bytes / 1024),
            Some(kb) => u64::from(kb),
            None => 0,
        };
    }
    m.slots_total = (slots > 0).then_some(slots);
    m.max_capacity_mb = (max_kb > 0).then(|| max_kb as f64 / 1024.0);

    for d in smbios.of_kind(17) {
        let size_mb = match d.word(0x0C) {
            None | Some(0) | Some(0xFFFF) => continue, // empty or unknown slot
            Some(0x7FFF) => d.dword(0x1C).map_or(0.0, |mb| f64::from(mb & 0x7FFF_FFFF)),
            Some(w) if w & 0x8000 != 0 => f64::from(w & 0x7FFF) / 1024.0,
            Some(w) => f64::from(w),
        };
        if size_mb <= 0.0 {
            continue;
        }
        let speed = |word_off: usize, ext_off: usize| match d.word(word_off) {
            Some(0xFFFF) => d.dword(ext_off).filter(|&v| v != 0),
            Some(0) | None => None,
            Some(v) => Some(u32::from(v)),
        };
        m.dimms.push(DimmInfo {
            slot: d
                .string(0x10)
                .unwrap_or_else(|| format!("Slot {}", m.dimms.len())),
            bank: d.string(0x11),
            manufacturer: d.string(0x17).and_then(|s| dimm_vendor(&s)).or_else(|| {
                d.string(0x1A)
                    .as_deref()
                    .and_then(vendor_from_part_number)
                    .map(str::to_owned)
            }),
            part_number: d.string(0x1A),
            size_mb,
            memory_type: d.byte(0x12).and_then(memory_type_name).map(str::to_owned),
            form_factor: d.byte(0x0E).and_then(form_factor_name).map(str::to_owned),
            speed_mts: speed(0x15, 0x54),
            configured_speed_mts: speed(0x20, 0x58),
            configured_voltage_mv: d.word(0x26).filter(|&v| v != 0).map(u32::from),
            rank: d
                .byte(0x1B)
                .map(|a| u32::from(a & 0x0F))
                .filter(|&r| r != 0),
        });
    }
    m.slots_used = m.dimms.len() as u32;
    let total: f64 = m.dimms.iter().map(|d| d.size_mb).sum();
    m.total_mb = (total > 0.0).then_some(total);
    m.channels = channel_count(&m.dimms);
    m
}

// ---------------------------------------------------------------------------------------------
// Test fixture builder (also used by other modules' tests)
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod fixture {
    /// Builds a `RawSMBIOSData` buffer from `(type, formatted-bytes-after-header, strings)`.
    pub fn raw(major: u8, minor: u8, structs: &[(u8, Vec<u8>, &[&str])]) -> Vec<u8> {
        let mut table = Vec::new();
        for (i, (kind, body, strings)) in structs.iter().enumerate() {
            table.push(*kind);
            table.push(u8::try_from(4 + body.len()).unwrap());
            table.extend_from_slice(&u16::try_from(i).unwrap().to_le_bytes());
            table.extend_from_slice(body);
            if strings.is_empty() {
                table.extend_from_slice(&[0, 0]);
            } else {
                for s in *strings {
                    table.extend_from_slice(s.as_bytes());
                    table.push(0);
                }
                table.push(0);
            }
        }
        table.extend_from_slice(&[127, 4, 0xFF, 0xFF, 0, 0]);
        let mut raw = vec![0, major, minor, 0];
        raw.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
        raw.extend_from_slice(&table);
        raw
    }

    /// Formatted area (after the 4-byte header) of `len` total bytes with values placed at
    /// absolute structure offsets.
    pub fn body(len: usize, fields: &[(usize, &[u8])]) -> Vec<u8> {
        let mut b = vec![0u8; len - 4];
        for (off, bytes) in fields {
            b[off - 4..off - 4 + bytes.len()].copy_from_slice(bytes);
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{body, raw};
    use super::*;

    fn dimm(size: u16, speed: u16, extra: &[(usize, &[u8])]) -> Vec<u8> {
        let mut fields: Vec<(usize, &[u8])> = vec![
            (0x0E, &[0x09]),
            (0x10, &[1]),
            (0x11, &[2]),
            (0x12, &[0x22]),
            (0x17, &[3]),
            (0x1A, &[4]),
            (0x1B, &[0x01]),
            (0x26, &[0x46, 0x05]), // 1350 mV
        ];
        let size = size.to_le_bytes();
        let speed = speed.to_le_bytes();
        let configured = 6000u16.to_le_bytes();
        fields.push((0x0C, &size));
        fields.push((0x15, &speed));
        fields.push((0x20, &configured));
        fields.extend_from_slice(extra);
        body(0x5C, &fields)
    }

    fn sample() -> Vec<u8> {
        raw(
            3,
            5,
            &[
                (
                    0,
                    body(0x18, &[(0x04, &[1]), (0x05, &[2]), (0x08, &[3])]),
                    &[
                        "American Megatrends International, LLC.",
                        "1801",
                        "07/12/2024",
                    ],
                ),
                (
                    1,
                    body(0x1B, &[(0x04, &[1]), (0x05, &[2]), (0x07, &[3])]),
                    &["ASUS", "System Product Name", "System Serial Number"],
                ),
                (
                    2,
                    body(
                        0x0F,
                        &[(0x04, &[1]), (0x05, &[2]), (0x06, &[3]), (0x07, &[4])],
                    ),
                    &[
                        "ASUSTeK COMPUTER INC.",
                        "ROG STRIX Z790-E GAMING WIFI",
                        "Rev 1.xx",
                        "240912345678901",
                    ],
                ),
                (3, body(0x16, &[(0x05, &[0x03])]), &[]),
                (
                    4,
                    body(
                        0x30,
                        &[
                            (0x04, &[1]),
                            (0x05, &[3]),
                            (0x14, &5400u16.to_le_bytes()),
                            (0x16, &3400u16.to_le_bytes()),
                            (0x18, &[0x41]),
                            (0x23, &[16]),
                            (0x25, &[24]),
                        ],
                    ),
                    &["LGA1700"],
                ),
                (
                    16,
                    body(
                        0x17,
                        &[
                            (0x05, &[3]),
                            (0x07, &0x8000_0000u32.to_le_bytes()),
                            (0x0F, &(192u64 << 30).to_le_bytes()),
                            (0x0D, &4u16.to_le_bytes()),
                        ],
                    ),
                    &[],
                ),
                (
                    17,
                    dimm(16384, 6400, &[]),
                    &[
                        "Controller0-ChannelA-DIMM1",
                        "BANK 0",
                        "80CE",
                        "M323R2GA3DB0-CWM",
                    ],
                ),
                (17, body(0x5C, &[]), &[]), // empty slot
                (
                    17,
                    // Extended size (32 GB) and extended speed (6000 MT/s).
                    dimm(
                        0x7FFF,
                        0xFFFF,
                        &[
                            (0x1C, &32768u32.to_le_bytes()),
                            (0x54, &6000u32.to_le_bytes()),
                        ],
                    ),
                    &[
                        "Controller1-ChannelB-DIMM1",
                        "BANK 0",
                        "Kingston",
                        "KF560C40-32",
                    ],
                ),
            ],
        )
    }

    #[test]
    fn parses_header_and_walks_every_structure() {
        let buf = sample();
        let s = Smbios::from_raw(&buf).unwrap();
        assert_eq!(s.version(), "3.5");
        let kinds: Vec<u8> = s.structures().iter().map(|x| x.kind).collect();
        assert_eq!(kinds, [0, 1, 2, 3, 4, 16, 17, 17, 17, 127]);
    }

    #[test]
    fn board_fields_and_placeholders() {
        let buf = sample();
        let b = board(&Smbios::from_raw(&buf).unwrap());
        assert_eq!(
            b.bios_vendor.as_deref(),
            Some("American Megatrends International, LLC.")
        );
        assert_eq!(b.bios_version.as_deref(), Some("1801"));
        assert_eq!(b.bios_date.as_deref(), Some("07/12/2024"));
        assert_eq!(b.system_manufacturer.as_deref(), Some("ASUS"));
        assert_eq!(b.system_product, None, "placeholder filtered");
        assert_eq!(b.product.as_deref(), Some("ROG STRIX Z790-E GAMING WIFI"));
        assert_eq!(b.serial.as_deref(), Some("240912345678901"));
        assert_eq!(b.form_factor, FormFactor::Desktop, "chassis type 3");
    }

    #[test]
    fn a_laptop_chassis_reads_as_a_laptop() {
        let buf = raw(3, 5, &[(3, body(0x16, &[(0x05, &[0x0A])]), &[])]);
        let b = board(&Smbios::from_raw(&buf).unwrap());
        assert_eq!(b.form_factor, FormFactor::Laptop);
        // No chassis structure at all.
        let none = raw(3, 5, &[(0, body(0x18, &[]), &[])]);
        assert_eq!(
            board(&Smbios::from_raw(&none).unwrap()).form_factor,
            FormFactor::Unknown
        );
    }

    #[test]
    fn processor_fields() {
        let buf = sample();
        let p = processor(&Smbios::from_raw(&buf).unwrap());
        assert_eq!(p.socket.as_deref(), Some("LGA1700"));
        assert_eq!((p.max_mhz, p.current_mhz), (Some(5400), Some(3400)));
        assert_eq!((p.cores, p.threads), (Some(16), Some(24)));
        assert_eq!(p.populated_sockets, 1);
    }

    #[test]
    fn memory_devices_skip_empty_slots_and_decode_extended_fields() {
        let buf = sample();
        let m = memory(&Smbios::from_raw(&buf).unwrap());
        assert_eq!(m.slots_total, Some(4));
        assert_eq!(m.slots_used, 2);
        assert_eq!(m.max_capacity_mb, Some(192.0 * 1024.0));
        assert_eq!(m.total_mb, Some(16384.0 + 32768.0));
        assert_eq!(m.channels, Some(2));

        let a = &m.dimms[0];
        assert_eq!(a.slot, "Controller0-ChannelA-DIMM1");
        assert_eq!(a.manufacturer.as_deref(), Some("Samsung"));
        assert_eq!(a.part_number.as_deref(), Some("M323R2GA3DB0-CWM"));
        assert_eq!(a.memory_type.as_deref(), Some("DDR5"));
        assert_eq!(a.form_factor.as_deref(), Some("DIMM"));
        assert_eq!(
            (a.speed_mts, a.configured_speed_mts),
            (Some(6400), Some(6000))
        );
        assert_eq!((a.configured_voltage_mv, a.rank), (Some(1350), Some(1)));

        let b = &m.dimms[1];
        assert_eq!(b.size_mb, 32768.0);
        assert_eq!(b.speed_mts, Some(6000));
        assert_eq!(b.manufacturer.as_deref(), Some("Kingston"));
    }

    #[test]
    fn truncated_buffers_do_not_panic() {
        let buf = sample();
        for cut in 0..buf.len() {
            if let Some(s) = Smbios::from_raw(&buf[..cut]) {
                let _ = board(&s);
                let _ = memory(&s);
                let _ = processor(&s);
            }
        }
    }

    #[test]
    fn jedec_codes_map_in_both_byte_orders() {
        assert_eq!(dimm_vendor("80CE").as_deref(), Some("Samsung"));
        assert_eq!(dimm_vendor("CE80").as_deref(), Some("Samsung"));
        assert_eq!(dimm_vendor("80AD000080AD").as_deref(), Some("SK hynix"));
        assert_eq!(dimm_vendor("0x04CD").as_deref(), Some("G.Skill"));
        assert_eq!(
            dimm_vendor("Micron Technology").as_deref(),
            Some("Micron Technology")
        );
        assert_eq!(dimm_vendor("Unknown"), None);
    }

    #[test]
    fn vendor_falls_back_to_part_number() {
        // Real module from a field report (manufacturer string was blank).
        assert_eq!(
            vendor_from_part_number("CMK16GX5M1B5600C40"),
            Some("Corsair")
        );
        assert_eq!(vendor_from_part_number("F5-6000J3038F16G"), Some("G.Skill"));
        assert_eq!(vendor_from_part_number("KF560C40-16"), Some("Kingston"));
        assert_eq!(vendor_from_part_number("XYZ123"), None);
    }

    #[test]
    fn channel_labels_from_common_vendors() {
        let d = |slot: &str, bank: Option<&str>| DimmInfo {
            slot: slot.into(),
            bank: bank.map(Into::into),
            ..Default::default()
        };
        // AMD AGESA style
        assert_eq!(
            channel_count(&[
                d("DIMM 1", Some("P0 CHANNEL A")),
                d("DIMM 1", Some("P0 CHANNEL B"))
            ]),
            Some(2)
        );
        // MSI / Gigabyte style
        assert_eq!(
            channel_count(&[d("DIMMA2", Some("BANK 0")), d("DIMM_B2", Some("BANK 1"))]),
            Some(2)
        );
        // Two DIMMs on one channel
        assert_eq!(
            channel_count(&[d("DIMM_A1", None), d("DIMM_A2", None)]),
            Some(1)
        );
        // Unrecognised label → unknown rather than a guess
        assert_eq!(
            channel_count(&[d("DIMM_A1", None), d("SLOT 3", None)]),
            None
        );
    }
}

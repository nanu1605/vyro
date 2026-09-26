//! EDID 1.3/1.4 base block + CTA-861 and DisplayID 1.x/2.x extension decoder. Pure: takes the
//! bytes Windows stores under `Device Parameters\EDID` and returns what the Displays panel
//! shows. Unknown or malformed parts are skipped, never fatal.

/// Timing descriptor, from any block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f64,
    /// Image size carried by base-block DTDs, mm.
    pub size_mm: Option<(u32, u32)>,
    pub preferred: bool,
}

/// Where a VRR range was advertised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VrrSource {
    /// AMD FreeSync vendor block (CTA).
    FreeSync,
    /// HDMI Forum VSDB/SCDB VRRmin/VRRmax (HDMI 2.1 VRR).
    HdmiVrr,
    /// DisplayID 2.x Adaptive-Sync data block (VESA Adaptive-Sync).
    DisplayIdAdaptiveSync,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VrrRange {
    pub source: VrrSource,
    pub min_hz: Option<f64>,
    pub max_hz: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeLimits {
    pub min_v_hz: u32,
    pub max_v_hz: u32,
    pub max_pixel_clock_mhz: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hdr {
    /// EOTF names: "SDR", "HDR (gamma)", "PQ (ST 2084)", "HLG".
    pub eotfs: Vec<&'static str>,
    pub max_nits: Option<f64>,
    pub max_frame_avg_nits: Option<f64>,
    pub min_nits: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edid {
    pub version: String,
    /// Every present block sums to 0 mod 256.
    pub checksum_ok: bool,
    /// Three-letter PNP id, e.g. "GSM".
    pub manufacturer_id: String,
    pub product_code: u16,
    /// Serial from the 0xFF descriptor, else the numeric field. Local display only.
    pub serial: Option<String>,
    /// "Week 3, 2021" / "2021" / "Model year 2022".
    pub manufactured: Option<String>,
    pub name: Option<String>,
    pub digital: bool,
    pub bits_per_color: Option<u32>,
    /// Declared input: "DisplayPort", "HDMI", "DVI"; EDID 1.4 only.
    pub interface: Option<&'static str>,
    pub size_mm: Option<(u32, u32)>,
    pub gamma: Option<f64>,
    pub continuous_frequency: bool,
    pub range: Option<RangeLimits>,
    pub timings: Vec<Timing>,
    /// e.g. ["CTA-861 r3", "DisplayID 2.0"].
    pub extensions: Vec<String>,
    pub hdr: Option<Hdr>,
    /// "BT.2020", "DCI-P3".
    pub gamuts: Vec<&'static str>,
    pub vrr: Vec<VrrRange>,
    /// Has an NVIDIA vendor block (G-SYNC module monitors carry one).
    pub nvidia_vsdb: bool,
}

impl Edid {
    /// Preferred (native) mode: first timing flagged preferred, else the first base DTD.
    pub fn native(&self) -> Option<&Timing> {
        self.timings
            .iter()
            .find(|t| t.preferred)
            .or_else(|| self.timings.first())
    }

    /// Highest refresh any detailed timing at native resolution offers.
    pub fn max_refresh_hz(&self) -> Option<f64> {
        let n = self.native()?;
        self.timings
            .iter()
            .filter(|t| t.width == n.width && t.height == n.height)
            .map(|t| t.refresh_hz)
            .fold(None, |m: Option<f64>, r| Some(m.map_or(r, |m| m.max(r))))
    }

    /// Best explicit VRR range (FreeSync, HDMI VRR, DisplayID Adaptive-Sync), falling back to
    /// the range-limits descriptor for a continuous-frequency DisplayPort EDID, which is how
    /// DP Adaptive-Sync monitors without vendor blocks advertise it.
    pub fn vrr_range(&self) -> Option<VrrRange> {
        let explicit = self
            .vrr
            .iter()
            .find(|v| v.min_hz.is_some() && v.max_hz.is_some())
            .or_else(|| self.vrr.first())
            .copied();
        match explicit {
            Some(mut v) => {
                if v.min_hz.is_none() || v.max_hz.is_none() {
                    if let Some(r) = self.range {
                        v.min_hz = v.min_hz.or(Some(f64::from(r.min_v_hz)));
                        v.max_hz = v.max_hz.or(Some(f64::from(r.max_v_hz)));
                    }
                }
                Some(v)
            }
            None => None,
        }
    }

    /// Range-limits descriptor plausibly describing DP Adaptive-Sync (not proof on its own).
    pub fn dp_range_hint(&self) -> Option<(f64, f64)> {
        let r = self.range?;
        let dp = self.interface == Some("DisplayPort");
        (dp && self.continuous_frequency && r.max_v_hz >= r.min_v_hz + 20 && r.min_v_hz >= 24)
            .then_some((f64::from(r.min_v_hz), f64::from(r.max_v_hz)))
    }
}

pub fn parse(bytes: &[u8]) -> Option<Edid> {
    const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    if bytes.len() < 128 || bytes[..8] != HEADER {
        return None;
    }
    let b = &bytes[..128];
    let mut e = Edid {
        version: format!("{}.{}", b[18], b[19]),
        manufacturer_id: pnp_id(u16::from_be_bytes([b[8], b[9]])),
        product_code: u16::from_le_bytes([b[10], b[11]]),
        ..Default::default()
    };
    let v14 = b[18] == 1 && b[19] >= 4;

    let numeric_serial = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
    e.manufactured = match (b[16], b[17]) {
        (_, 0) => None,
        (0xFF, y) => Some(format!("Model year {}", 1990 + u32::from(y))),
        (w @ 1..=54, y) => Some(format!("Week {w}, {}", 1990 + u32::from(y))),
        (_, y) => Some((1990 + u32::from(y)).to_string()),
    };

    let input = b[20];
    e.digital = input & 0x80 != 0;
    if e.digital && v14 {
        e.bits_per_color = match (input >> 4) & 7 {
            d @ 1..=6 => Some(4 + 2 * u32::from(d)),
            _ => None,
        };
        e.interface = match input & 0x0F {
            1 => Some("DVI"),
            2 | 3 => Some("HDMI"),
            5 => Some("DisplayPort"),
            _ => None,
        };
    }
    if b[21] > 0 && b[22] > 0 {
        e.size_mm = Some((u32::from(b[21]) * 10, u32::from(b[22]) * 10));
    }
    if b[23] != 0xFF {
        e.gamma = Some((f64::from(b[23]) + 100.0) / 100.0);
    }
    e.continuous_frequency = v14 && b[24] & 0x01 != 0;

    let mut text_serial = None;
    for (i, d) in b[54..126].as_chunks::<18>().0.iter().enumerate() {
        if d[0] != 0 || d[1] != 0 {
            if let Some(mut t) = dtd(d) {
                t.preferred = i == 0;
                e.timings.push(t);
            }
            continue;
        }
        match d[3] {
            0xFC => e.name = descriptor_text(d),
            0xFF => text_serial = descriptor_text(d),
            0xFD => e.range = range_limits(d),
            _ => {}
        }
    }
    // A DTD's image size (mm) is more precise than the cm field.
    if let Some((w, h)) = e.timings.first().and_then(|t| t.size_mm) {
        if w > 0 && h > 0 {
            e.size_mm = Some((w, h));
        }
    }
    e.serial = text_serial.or_else(|| {
        (numeric_serial != 0 && numeric_serial != 0x0101_0101).then(|| numeric_serial.to_string())
    });

    let count = usize::from(b[126]);
    let mut ok = checksum(b);
    for i in 1..=count {
        let Some(ext) = bytes.get(i * 128..(i + 1) * 128) else {
            break;
        };
        ok &= checksum(ext);
        match ext[0] {
            0x02 => cta(ext, &mut e),
            0x70 => displayid(&ext[1..], &mut e),
            _ => {}
        }
    }
    e.checksum_ok = ok;
    e.gamuts.dedup();
    Some(e)
}

/// Replaces the serial fields so an EDID can be shown or exported without identifying the
/// unit; checksums are fixed up so the result still parses.
pub fn scrub(bytes: &[u8]) -> Vec<u8> {
    let mut b = bytes.to_vec();
    if b.len() < 128 {
        return b;
    }
    b[12..16].fill(0);
    for o in [54, 72, 90, 108] {
        if b[o..o + 3] == [0, 0, 0] && b[o + 3] == 0xFF {
            b[o + 5..o + 18].copy_from_slice(b"\n            ");
        }
    }
    let s: u8 = b[..127].iter().fold(0u8, |a, &x| a.wrapping_add(x));
    b[127] = 0u8.wrapping_sub(s);
    b
}

fn checksum(block: &[u8]) -> bool {
    block.iter().fold(0u8, |a, &x| a.wrapping_add(x)) == 0
}

fn pnp_id(v: u16) -> String {
    [(v >> 10) & 0x1F, (v >> 5) & 0x1F, v & 0x1F]
        .iter()
        .map(|&c| char::from(b'A' - 1 + c as u8))
        .collect()
}

fn descriptor_text(d: &[u8]) -> Option<String> {
    let raw = &d[5..18];
    let end = raw.iter().position(|&c| c == 0x0A).unwrap_or(raw.len());
    let s: String = raw[..end]
        .iter()
        .map(|&c| {
            if c.is_ascii_graphic() || c == b' ' {
                char::from(c)
            } else {
                ' '
            }
        })
        .collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

fn range_limits(d: &[u8]) -> Option<RangeLimits> {
    let off = d[4];
    let min_v = u32::from(d[5]) + if off & 0x03 == 0x03 { 255 } else { 0 };
    let max_v = u32::from(d[6]) + if off & 0x02 != 0 { 255 } else { 0 };
    (min_v > 0 && max_v >= min_v).then_some(RangeLimits {
        min_v_hz: min_v,
        max_v_hz: max_v,
        max_pixel_clock_mhz: (d[9] > 0).then(|| u32::from(d[9]) * 10),
    })
}

/// 18-byte Detailed Timing Descriptor.
fn dtd(d: &[u8]) -> Option<Timing> {
    let clock_hz = f64::from(u16::from_le_bytes([d[0], d[1]])) * 10_000.0;
    let ha = u32::from(d[2]) | (u32::from(d[4] >> 4) << 8);
    let hb = u32::from(d[3]) | (u32::from(d[4] & 0x0F) << 8);
    let va = u32::from(d[5]) | (u32::from(d[7] >> 4) << 8);
    let vb = u32::from(d[6]) | (u32::from(d[7] & 0x0F) << 8);
    let wmm = u32::from(d[12]) | (u32::from(d[14] >> 4) << 8);
    let hmm = u32::from(d[13]) | (u32::from(d[14] & 0x0F) << 8);
    let interlaced = d[17] & 0x80 != 0;
    let total = f64::from((ha + hb) * (va + vb));
    if ha == 0 || va == 0 || total == 0.0 {
        return None;
    }
    let mut refresh = clock_hz / total;
    let mut height = va;
    if interlaced {
        refresh *= 2.0;
        height *= 2;
    }
    Some(Timing {
        width: ha,
        height,
        refresh_hz: refresh,
        size_mm: (wmm > 0 && hmm > 0).then_some((wmm, hmm)),
        preferred: false,
    })
}

fn cta(ext: &[u8], e: &mut Edid) {
    e.extensions.push(format!("CTA-861 r{}", ext[1]));
    let dtd_start = usize::from(ext[2]).clamp(4, 127);
    let mut i = 4;
    while i < dtd_start {
        let tag = ext[i] >> 5;
        let len = usize::from(ext[i] & 0x1F);
        let Some(p) = ext
            .get(i + 1..i + 1 + len)
            .filter(|_| i + 1 + len <= dtd_start)
        else {
            break;
        };
        match tag {
            3 if len >= 3 => vendor_block(u32::from_le_bytes([p[0], p[1], p[2], 0]), p, e),
            7 if len >= 1 => match p[0] {
                0x05 if len >= 2 => {
                    if p[1] & 0xE0 != 0 {
                        e.gamuts.push("BT.2020");
                    }
                    if len >= 3 && p[2] & 0x80 != 0 {
                        e.gamuts.push("DCI-P3");
                    }
                }
                0x06 if len >= 3 => e.hdr = Some(hdr_static(&p[1..])),
                // HDMI Forum SCDB: same layout as the VSDB with the OUI replaced.
                0x79 if len >= 4 => hdmi_forum(p, e),
                _ => {}
            },
            _ => {}
        }
        i += 1 + len;
    }
    let mut o = dtd_start;
    while o + 18 <= 127 && (ext[o] != 0 || ext[o + 1] != 0) {
        if let Some(mut t) = dtd(&ext[o..o + 18]) {
            t.size_mm = None;
            e.timings.push(t);
        }
        o += 18;
    }
}

fn vendor_block(oui: u32, p: &[u8], e: &mut Edid) {
    match oui {
        0x00_001A => {
            // AMD FreeSync. v1/v2: [oui3][major][minor][min Hz][max Hz]; v3 carries no range.
            let v = if p.len() >= 7 && p[3] < 3 && p[5] > 0 && p[6] > p[5] {
                VrrRange {
                    source: VrrSource::FreeSync,
                    min_hz: Some(f64::from(p[5])),
                    max_hz: Some(f64::from(p[6])),
                }
            } else {
                VrrRange {
                    source: VrrSource::FreeSync,
                    min_hz: None,
                    max_hz: None,
                }
            };
            e.vrr.push(v);
        }
        0xC4_5DD8 => hdmi_forum(p, e),
        0x00_044B => e.nvidia_vsdb = true,
        _ => {}
    }
}

/// HDMI Forum VSDB / SCDB payload (index 0..3 = OUI or ext-tag + reserved).
fn hdmi_forum(p: &[u8], e: &mut Edid) {
    if p.len() < 10 {
        return;
    }
    let min = u32::from(p[8] & 0x3F);
    let max = (u32::from(p[8] >> 6) << 8) | u32::from(p[9]);
    if max > 0 && max > min {
        e.vrr.push(VrrRange {
            source: VrrSource::HdmiVrr,
            min_hz: (min > 0).then_some(f64::from(min)),
            max_hz: Some(f64::from(max)),
        });
    }
}

/// CTA HDR Static Metadata payload after the extended tag.
fn hdr_static(p: &[u8]) -> Hdr {
    const EOTF: [&str; 4] = ["SDR", "HDR (gamma)", "PQ (ST 2084)", "HLG"];
    let eotfs = (0..4)
        .filter(|b| p[0] & (1 << b) != 0)
        .map(|b| EOTF[b])
        .collect();
    let lum = |cv: u8| 50.0 * 2f64.powf(f64::from(cv) / 32.0);
    let max = p.get(2).filter(|&&v| v > 0).map(|&v| lum(v));
    let avg = p.get(3).filter(|&&v| v > 0).map(|&v| lum(v));
    let min = match (max, p.get(4)) {
        (Some(m), Some(&cv)) if cv > 0 => Some(m * (f64::from(cv) / 255.0).powi(2) / 100.0),
        _ => None,
    };
    Hdr {
        eotfs,
        max_nits: max,
        max_frame_avg_nits: avg,
        min_nits: min,
    }
}

/// DisplayID section (extension byte 1 onward): version, length, product type, ext count,
/// then data blocks of `[tag][revision][len][payload]`.
fn displayid(s: &[u8], e: &mut Edid) {
    if s.len() < 5 {
        return;
    }
    let version = s[0];
    e.extensions
        .push(format!("DisplayID {}.{}", version >> 4, version & 0x0F));
    let end = (4 + usize::from(s[1])).min(s.len());
    let mut i = 4;
    while i + 3 <= end {
        let (tag, rev, len) = (s[i], s[i + 1], usize::from(s[i + 2]));
        if tag == 0 && len == 0 {
            break;
        }
        let Some(p) = s.get(i + 3..i + 3 + len) else {
            break;
        };
        match tag {
            // Type I (DisplayID 1.x, 10 kHz clock) / Type VII (2.x, 1 kHz clock) timings.
            0x03 | 0x22 => {
                let unit = if tag == 0x03 { 10_000.0 } else { 1_000.0 };
                for d in p.as_chunks::<20>().0 {
                    if let Some(t) = displayid_timing(d, unit) {
                        e.timings.push(t);
                    }
                }
            }
            0x2B => {
                let size = 6 + usize::from((rev >> 4) & 7);
                for d in p.chunks_exact(size) {
                    let max = 1 + (u32::from(d[4] & 3) << 8 | u32::from(d[3]));
                    if d[2] > 0 && max > u32::from(d[2]) {
                        e.vrr.push(VrrRange {
                            source: VrrSource::DisplayIdAdaptiveSync,
                            min_hz: Some(f64::from(d[2])),
                            max_hz: Some(f64::from(max)),
                        });
                    }
                }
            }
            _ => {}
        }
        i += 3 + len;
    }
}

fn displayid_timing(d: &[u8], clock_unit_hz: f64) -> Option<Timing> {
    let le16 = |o: usize| u32::from(u16::from_le_bytes([d[o], d[o + 1]]) & 0x7FFF) + 1;
    let clock = f64::from(u32::from_le_bytes([d[0], d[1], d[2], 0]) + 1) * clock_unit_hz;
    let (ha, hb, va, vb) = (le16(4), le16(6), le16(12), le16(14));
    let mut refresh = clock / (f64::from(ha + hb) * f64::from(va + vb));
    let interlaced = d[3] & 0x10 != 0;
    if interlaced {
        refresh *= 2.0;
    }
    Some(Timing {
        width: ha,
        height: va,
        refresh_hz: refresh,
        size_mm: None,
        preferred: d[3] & 0x80 != 0,
    })
}

/// Parses the whitespace-separated hex fixture format (`#` lines are comments).
pub(crate) fn hex_fixture(text: &str) -> Vec<u8> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .flat_map(str::split_whitespace)
        .filter_map(|h| u8::from_str_radix(h, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lg() -> Vec<u8> {
        hex_fixture(include_str!("fixtures/lg_ultragear_dp.hex"))
    }
    fn aoc() -> Vec<u8> {
        hex_fixture(include_str!("fixtures/aoc_hdmi21_freesync.hex"))
    }
    fn auo() -> Vec<u8> {
        hex_fixture(include_str!("fixtures/auo_laptop_displayid2.hex"))
    }
    fn close(a: Option<f64>, b: f64, tol: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < tol)
    }

    #[test]
    fn lg_ultragear_displayport_hdr400() {
        let e = parse(&lg()).unwrap();
        assert!(e.checksum_ok);
        assert_eq!(e.version, "1.4");
        assert_eq!(e.manufacturer_id, "GSM");
        assert_eq!(e.product_code, 23423);
        assert_eq!(e.name.as_deref(), Some("LG ULTRAGEAR"));
        assert_eq!(e.serial.as_deref(), Some("TESTSERIAL01"));
        assert_eq!(e.manufactured.as_deref(), Some("Week 3, 2021"));
        assert_eq!(e.interface, Some("DisplayPort"));
        assert_eq!(e.bits_per_color, Some(10));
        assert_eq!(e.size_mm, Some((597, 336)));
        assert!(close(e.gamma, 2.2, 1e-9));
        assert!(e.continuous_frequency);
        let n = e.native().unwrap();
        assert_eq!((n.width, n.height), (2560, 1440));
        assert!(close(Some(n.refresh_hz), 143.973, 0.001));
        assert!(close(e.max_refresh_hz(), 143.973, 0.001));
        assert_eq!(e.range.map(|r| (r.min_v_hz, r.max_v_hz)), Some((48, 144)));
        assert_eq!(e.extensions, ["CTA-861 r3"]);
        assert_eq!(e.gamuts, ["BT.2020"]);
        let hdr = e.hdr.as_ref().unwrap();
        assert_eq!(hdr.eotfs, ["SDR", "PQ (ST 2084)"]);
        assert!(close(hdr.max_nits, 400.0, 0.01));
        assert!(close(hdr.max_frame_avg_nits, 343.724, 0.01));
        assert!(close(hdr.min_nits, 0.098, 0.001));
        // No vendor block: DP Adaptive-Sync only hinted by the continuous range.
        assert!(e.vrr_range().is_none());
        assert_eq!(e.dp_range_hint(), Some((48.0, 144.0)));
    }

    #[test]
    fn aoc_hdmi21_freesync_and_hdmi_vrr() {
        let e = parse(&aoc()).unwrap();
        assert!(e.checksum_ok);
        assert_eq!(e.version, "1.3");
        assert_eq!(e.manufacturer_id, "AOC");
        assert_eq!(e.name.as_deref(), Some("AG324UWS4R4B"));
        assert_eq!(e.manufactured.as_deref(), Some("Week 1, 2020"));
        assert_eq!(e.interface, None, "EDID 1.3 has no interface field");
        assert_eq!(e.extensions, ["CTA-861 r3", "DisplayID 1.2"]);
        assert_eq!(e.gamuts, ["BT.2020"]);
        let hdr = e.hdr.as_ref().unwrap();
        assert_eq!(hdr.eotfs, ["SDR", "HDR (gamma)", "PQ (ST 2084)"]);
        assert!(close(hdr.max_nits, 417.71, 0.01));
        assert!(e.vrr.contains(&VrrRange {
            source: VrrSource::HdmiVrr,
            min_hz: Some(48.0),
            max_hz: Some(120.0)
        }));
        assert!(e.vrr.contains(&VrrRange {
            source: VrrSource::FreeSync,
            min_hz: Some(48.0),
            max_hz: Some(144.0)
        }));
        let n = e.native().unwrap();
        assert_eq!((n.width, n.height), (3840, 2160));
        assert_eq!(e.dp_range_hint(), None);
    }

    #[test]
    fn auo_laptop_displayid2_adaptive_sync() {
        let e = parse(&auo()).unwrap();
        assert!(e.checksum_ok);
        assert_eq!(e.manufacturer_id, "AUO");
        assert_eq!(e.name.as_deref(), Some("B173HAN04.9"));
        assert_eq!(e.serial, None);
        assert_eq!(e.size_mm, Some((382, 215)));
        assert_eq!(e.extensions, ["DisplayID 2.0"]);
        assert_eq!(
            e.vrr_range(),
            Some(VrrRange {
                source: VrrSource::DisplayIdAdaptiveSync,
                min_hz: Some(60.0),
                max_hz: Some(144.0)
            })
        );
        // Base DTD is 60 Hz; the DisplayID Type VII preferred timing is 144 Hz.
        let n = e.native().unwrap();
        assert_eq!((n.width, n.height), (1920, 1080));
        assert!(close(e.max_refresh_hz(), 144.028, 0.001));
        // Week 229 is invalid: year only.
        assert_eq!(e.manufactured.as_deref(), Some("2022"));
    }

    #[test]
    fn scrub_removes_serials_and_keeps_checksum() {
        let s = scrub(&lg());
        let e = parse(&s).unwrap();
        assert!(e.checksum_ok);
        assert_eq!(e.serial, None);
        assert_eq!(e.name.as_deref(), Some("LG ULTRAGEAR"));
    }

    #[test]
    fn rejects_garbage_and_survives_truncation() {
        assert!(parse(&[0u8; 128]).is_none());
        assert!(parse(&[]).is_none());
        let mut t = aoc();
        t.truncate(200);
        let e = parse(&t).unwrap();
        assert!(e.extensions.is_empty());
        // Random extension payloads never panic.
        let mut rng = fastrand::Rng::with_seed(7);
        for _ in 0..500 {
            let mut b = lg();
            for x in &mut b[128..] {
                *x = rng.u8(..);
            }
            b[128] = if rng.bool() { 0x02 } else { 0x70 };
            let _ = parse(&b);
        }
    }
}

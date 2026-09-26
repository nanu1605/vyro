//! Displays (spec §2 Displays, Phase 3). Windows APIs fill a [`RawDisplay`] per active output;
//! [`build`] merges it with the parsed EDID into the [`DisplayInfo`] the panel shows. Merging
//! is pure so every precedence rule is tested here.

pub mod edid;
mod pnp;
#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{collect, watch};

use crate::info::{DisplayInfo, EdidInfo, HdrInfo, LocalOnly, VrrInfo};
use edid::{Edid, VrrSource};

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO(_2)`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AdvancedColor {
    pub supported: bool,
    pub enabled: bool,
    pub wide_color_enforced: bool,
    /// Only from the Windows 11 24H2 `_2` query.
    pub hdr_supported: Option<bool>,
    /// `_2` active mode: 0 SDR, 1 WCG, 2 HDR.
    pub active_mode: Option<u32>,
    pub encoding: u32,
    pub bits_per_color: u32,
}

/// `DXGI_OUTPUT_DESC1`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DxgiOutput {
    pub bits_per_color: u32,
    pub color_space: u32,
    pub min_nits: f32,
    pub max_nits: f32,
    pub max_full_frame_nits: f32,
}

/// VRR answer from a GPU vendor API.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VendorVrr {
    /// "NVAPI" or "ADL".
    pub api: &'static str,
    pub possible: Option<bool>,
    pub enabled: Option<bool>,
    pub min_hz: Option<f64>,
    pub max_hz: Option<f64>,
}

/// Everything the Windows side gathers about one active output.
#[derive(Debug, Clone, Default)]
pub struct RawDisplay {
    pub key: String,
    pub friendly_name: Option<String>,
    pub gdi_name: Option<String>,
    pub adapter: Option<String>,
    /// PCI vendor of the adapter (0x10DE NVIDIA, 0x1002 AMD, 0x8086 Intel).
    pub adapter_vendor: Option<u32>,
    pub output_technology: Option<i32>,
    pub rotation: Option<i32>,
    pub refresh_hz: Option<f64>,
    /// Source mode: `[x, y, width, height]`.
    pub desktop: Option<[i32; 4]>,
    pub color: Option<AdvancedColor>,
    /// `SDRWhiteLevel` (1000 = 80 nits).
    pub sdr_white_level: Option<u32>,
    pub dxgi: Option<DxgiOutput>,
    pub dpi: Option<u32>,
    pub brightness_pct: Option<u32>,
    pub edid: Option<Vec<u8>>,
    pub vendor_vrr: Option<VendorVrr>,
}

/// `DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY` → (label, internal).
pub fn connection(tech: i32) -> (Option<&'static str>, bool) {
    match tech {
        0 => (Some("VGA"), false),
        1 => (Some("S-Video"), false),
        2 => (Some("Composite"), false),
        3 => (Some("Component"), false),
        4 => (Some("DVI"), false),
        5 => (Some("HDMI"), false),
        6 => (Some("Internal (LVDS)"), true),
        8 => (Some("D-Jpn"), false),
        9 => (Some("SDI"), false),
        10 => (Some("DisplayPort"), false),
        11 => (Some("Internal (eDP)"), true),
        12 => (Some("UDI"), false),
        13 => (Some("Internal (UDI)"), true),
        14 => (Some("SDTV dongle"), false),
        15 => (Some("Miracast"), false),
        16 => (Some("Indirect (wired)"), false),
        17 => (Some("Indirect (virtual)"), false),
        18 => (Some("DisplayPort (USB-C)"), false),
        // DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL (0x80000000).
        i32::MIN => (Some("Internal"), true),
        _ => (None, false),
    }
}

/// `DISPLAYCONFIG_ROTATION` → degrees clockwise.
pub fn orientation(rotation: i32) -> Option<u32> {
    match rotation {
        1 => Some(0),
        2 => Some(90),
        3 => Some(180),
        4 => Some(270),
        _ => None,
    }
}

pub fn color_encoding(v: u32) -> Option<&'static str> {
    match v {
        0 => Some("RGB"),
        1 => Some("YCbCr 4:4:4"),
        2 => Some("YCbCr 4:2:2"),
        3 => Some("YCbCr 4:2:0"),
        4 => Some("Intensity"),
        _ => None,
    }
}

/// `DXGI_COLOR_SPACE_TYPE` of an output.
pub fn color_space(v: u32) -> String {
    match v {
        0 => "sRGB".into(),
        1 => "scRGB (linear)".into(),
        12 => "BT.2100 PQ (HDR10)".into(),
        17 => "BT.2020 (gamma 2.2)".into(),
        other => format!("DXGI color space {other}"),
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn vrr(raw: &RawDisplay, e: Option<&Edid>) -> VrrInfo {
    let explicit = e.and_then(Edid::vrr_range);
    let hint = e.and_then(Edid::dp_range_hint);
    let vendor = raw.vendor_vrr.as_ref();
    let mut v = VrrInfo::default();

    let vendor_yes = vendor.and_then(|x| x.possible) == Some(true);
    let vendor_no = vendor.and_then(|x| x.possible) == Some(false);
    v.supported = if vendor_yes || explicit.is_some() {
        Some(true)
    } else if vendor_no {
        Some(false)
    } else {
        None
    };
    v.enabled = vendor.and_then(|x| x.enabled);
    v.source = vendor
        .filter(|x| x.possible.is_some() || x.enabled.is_some())
        .map(|x| x.api.to_owned())
        .or_else(|| explicit.map(|_| "EDID".to_owned()));

    let (min, max) = match (explicit, vendor) {
        (Some(x), _) if x.min_hz.is_some() && x.max_hz.is_some() => (x.min_hz, x.max_hz),
        (_, Some(x)) if x.min_hz.is_some() && x.max_hz.is_some() => (x.min_hz, x.max_hz),
        _ if v.supported == Some(true) => hint.map_or((None, None), |(a, b)| (Some(a), Some(b))),
        _ => (None, None),
    };
    v.min_hz = min;
    v.max_hz = max;

    if v.supported == Some(true) {
        v.technology = Some(
            match (raw.adapter_vendor, vendor.map(|x| x.api)) {
                (Some(0x10DE), _) | (_, Some("NVAPI")) => {
                    if e.is_some_and(|e| e.nvidia_vsdb) {
                        "G-SYNC"
                    } else {
                        "G-SYNC Compatible"
                    }
                }
                (Some(0x1002), _) | (_, Some("ADL")) => "FreeSync",
                _ => match explicit.map(|x| x.source) {
                    Some(VrrSource::FreeSync) => "FreeSync",
                    Some(VrrSource::HdmiVrr) => "HDMI VRR",
                    _ => "Adaptive-Sync",
                },
            }
            .to_owned(),
        );
    }
    v
}

fn hdr(raw: &RawDisplay, e: Option<&Edid>) -> HdrInfo {
    let ehdr = e.and_then(|e| e.hdr.as_ref());
    let pq = ehdr.is_some_and(|h| h.eotfs.contains(&"PQ (ST 2084)"));
    let c = raw.color;
    let dxgi_pq = raw.dxgi.map(|d| d.color_space == 12);

    let supported = c
        .and_then(|c| c.hdr_supported)
        .or(pq.then_some(true))
        .or(c.map(|c| c.supported))
        .or(e.map(|_| false));
    let enabled = c
        .and_then(|c| c.active_mode.map(|m| m == 2))
        .or(c.map(|c| c.enabled && !c.wide_color_enforced))
        .or(dxgi_pq);

    let positive = |v: f32| (v > 0.0).then(|| round1(f64::from(v)));
    let dxgi = raw.dxgi;
    HdrInfo {
        supported,
        enabled,
        sdr_white_nits: raw
            .sdr_white_level
            .filter(|_| enabled == Some(true))
            .map(|l| round1(f64::from(l) / 1000.0 * 80.0)),
        max_nits: dxgi
            .and_then(|d| positive(d.max_nits))
            .or(ehdr.and_then(|h| h.max_nits.map(round1))),
        max_full_frame_nits: dxgi
            .and_then(|d| positive(d.max_full_frame_nits))
            .or(ehdr.and_then(|h| h.max_frame_avg_nits.map(round1))),
        min_nits: dxgi
            .and_then(|d| (d.min_nits > 0.0).then(|| f64::from(d.min_nits)))
            .or(ehdr.and_then(|h| h.min_nits))
            .map(|n| (n * 10_000.0).round() / 10_000.0),
        eotfs: ehdr
            .map(|h| h.eotfs.iter().map(|&s| s.to_owned()).collect())
            .unwrap_or_default(),
        gamuts: e
            .map(|e| e.gamuts.iter().map(|&s| s.to_owned()).collect())
            .unwrap_or_default(),
    }
}

pub fn build(raw: &RawDisplay) -> DisplayInfo {
    let e = raw.edid.as_deref().and_then(edid::parse);
    let e = e.as_ref();
    let (conn, internal) = raw.output_technology.map_or((None, false), connection);
    let native = e.and_then(Edid::native);
    let size = e.and_then(|e| e.size_mm);
    let model = e.and_then(|e| e.name.clone());
    let manufacturer_id = e.map(|e| e.manufacturer_id.clone());

    let name = model
        .clone()
        .or_else(|| raw.friendly_name.clone().filter(|s| !s.is_empty()))
        .or_else(|| internal.then(|| "Built-in display".to_owned()))
        .unwrap_or_else(|| "Display".to_owned());

    DisplayInfo {
        id: raw.key.clone(),
        name,
        manufacturer: manufacturer_id
            .as_deref()
            .map(|id| pnp::vendor(id).map_or_else(|| id.to_owned(), str::to_owned)),
        manufacturer_id,
        model,
        product_code: e.map(|e| format!("{:04X}", e.product_code)),
        serial: e.and_then(|e| e.serial.clone()).map(LocalOnly),
        manufactured: e.and_then(|e| e.manufactured.clone()),
        primary: raw.desktop.is_some_and(|r| r[0] == 0 && r[1] == 0),
        gdi_name: raw.gdi_name.clone(),
        adapter: raw.adapter.clone(),
        connection: conn.map(str::to_owned),
        internal,
        width: raw.desktop.and_then(|r| u32::try_from(r[2]).ok()),
        height: raw.desktop.and_then(|r| u32::try_from(r[3]).ok()),
        native_width: native.map(|t| t.width),
        native_height: native.map(|t| t.height),
        refresh_hz: raw
            .refresh_hz
            .filter(|&r| r > 0.0)
            .map(|r| (r * 100.0).round() / 100.0),
        max_refresh_hz: e
            .and_then(Edid::max_refresh_hz)
            .map(|r| (r * 100.0).round() / 100.0),
        bits_per_color: raw
            .color
            .map(|c| c.bits_per_color)
            .filter(|&b| b > 0)
            .or(raw.dxgi.map(|d| d.bits_per_color).filter(|&b| b > 0)),
        color_encoding: raw
            .color
            .and_then(|c| color_encoding(c.encoding))
            .map(str::to_owned),
        color_space: raw.dxgi.map(|d| color_space(d.color_space)),
        hdr: hdr(raw, e),
        vrr: vrr(raw, e),
        dpi: raw.dpi,
        scale_pct: raw.dpi.map(|d| (d * 100 + 48) / 96),
        orientation: raw.rotation.and_then(orientation),
        width_mm: size.map(|s| s.0),
        height_mm: size.map(|s| s.1),
        diagonal_in: size.map(|(w, h)| round1(f64::from(w).hypot(f64::from(h)) / 25.4)),
        brightness_pct: raw.brightness_pct,
        rect: raw.desktop,
        edid: raw.edid.as_deref().zip(e).map(|(bytes, e)| EdidInfo {
            version: e.version.clone(),
            checksum_ok: e.checksum_ok,
            interface: e.interface.map(str::to_owned),
            bits_per_color: e.bits_per_color,
            gamma: e.gamma,
            extensions: e.extensions.clone(),
            range_hz: e.range.map(|r| [r.min_v_hz, r.max_v_hz]),
            hex: edid::scrub(bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        }),
    }
}

/// Two plausible displays for the simulator and non-Windows builds: a 27" DP monitor as
/// primary and a 4K HDMI monitor to its right.
pub fn sample() -> Vec<DisplayInfo> {
    let lg = RawDisplay {
        key: "sim:1".into(),
        gdi_name: Some(r"\\.\DISPLAY1".into()),
        adapter: Some("Simulated GPU".into()),
        adapter_vendor: Some(0x10DE),
        output_technology: Some(10),
        rotation: Some(1),
        refresh_hz: Some(143.97),
        desktop: Some([0, 0, 2560, 1440]),
        color: Some(AdvancedColor {
            supported: true,
            bits_per_color: 10,
            ..Default::default()
        }),
        dxgi: Some(DxgiOutput {
            bits_per_color: 10,
            max_nits: 400.0,
            ..Default::default()
        }),
        dpi: Some(96),
        edid: Some(edid::hex_fixture(include_str!(
            "fixtures/lg_ultragear_dp.hex"
        ))),
        vendor_vrr: Some(VendorVrr {
            api: "NVAPI",
            possible: Some(true),
            enabled: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let aoc = RawDisplay {
        key: "sim:2".into(),
        gdi_name: Some(r"\\.\DISPLAY2".into()),
        output_technology: Some(5),
        rotation: Some(1),
        refresh_hz: Some(60.0),
        desktop: Some([2560, 0, 3840, 2160]),
        color: Some(AdvancedColor {
            supported: true,
            enabled: true,
            bits_per_color: 10,
            ..Default::default()
        }),
        sdr_white_level: Some(2500),
        dxgi: Some(DxgiOutput {
            bits_per_color: 10,
            color_space: 12,
            max_nits: 417.7,
            ..Default::default()
        }),
        dpi: Some(144),
        brightness_pct: Some(70),
        edid: Some(edid::hex_fixture(include_str!(
            "fixtures/aoc_hdmi21_freesync.hex"
        ))),
        ..lg.clone()
    };
    let mut v = vec![build(&lg), build(&aoc)];
    for d in &mut v {
        d.serial = None;
    }
    v
}

/// Monitor device-interface path → PnP instance id (the `Enum` registry key holding the EDID):
/// `\\?\DISPLAY#GSM5B7F#5&1b2b&0&UID4352#{e6f07b5f-…}` → `DISPLAY\GSM5B7F\5&1b2b&0&UID4352`.
pub fn device_instance(path: &str) -> Option<String> {
    let p = path.strip_prefix(r"\\?\").unwrap_or(path);
    let p = match p.rfind("#{") {
        Some(i) => &p[..i],
        None => p,
    };
    let parts: Vec<&str> = p.split('#').collect();
    (parts.len() == 3 && parts.iter().all(|s| !s.is_empty())).then(|| parts.join("\\"))
}

/// Primary first, then left-to-right, top-to-bottom.
pub fn sort(list: &mut [DisplayInfo]) {
    list.sort_by_key(|d| {
        let r = d.rect.unwrap_or([i32::MAX; 4]);
        (!d.primary, r[0], r[1])
    });
}

#[cfg(test)]
mod tests {
    use super::edid::hex_fixture;
    use super::*;

    fn lg_raw() -> RawDisplay {
        RawDisplay {
            key: "0:1234:4352".into(),
            friendly_name: Some("LG ULTRAGEAR".into()),
            gdi_name: Some(r"\\.\DISPLAY1".into()),
            adapter: Some("NVIDIA GeForce RTX 5060 Ti".into()),
            adapter_vendor: Some(0x10DE),
            output_technology: Some(10),
            rotation: Some(1),
            refresh_hz: Some(143.973),
            desktop: Some([0, 0, 2560, 1440]),
            color: Some(AdvancedColor {
                supported: true,
                enabled: false,
                encoding: 0,
                bits_per_color: 10,
                ..Default::default()
            }),
            sdr_white_level: Some(1000),
            dxgi: Some(DxgiOutput {
                bits_per_color: 10,
                color_space: 0,
                min_nits: 0.0984,
                max_nits: 400.0,
                max_full_frame_nits: 343.7,
            }),
            dpi: Some(120),
            brightness_pct: None,
            edid: Some(hex_fixture(include_str!("fixtures/lg_ultragear_dp.hex"))),
            vendor_vrr: Some(VendorVrr {
                api: "NVAPI",
                possible: Some(true),
                enabled: Some(true),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn lg_on_nvidia_merges_every_source() {
        let d = build(&lg_raw());
        assert_eq!(d.name, "LG ULTRAGEAR");
        assert_eq!(d.manufacturer.as_deref(), Some("LG Electronics"));
        assert_eq!(d.manufacturer_id.as_deref(), Some("GSM"));
        assert_eq!(d.product_code.as_deref(), Some("5B7F"));
        assert_eq!(d.serial, Some(LocalOnly("TESTSERIAL01".into())));
        assert!(d.primary);
        assert_eq!(d.connection.as_deref(), Some("DisplayPort"));
        assert!(!d.internal);
        assert_eq!((d.width, d.height), (Some(2560), Some(1440)));
        assert_eq!((d.native_width, d.native_height), (Some(2560), Some(1440)));
        assert_eq!(d.refresh_hz, Some(143.97));
        assert_eq!(d.max_refresh_hz, Some(143.97));
        assert_eq!(d.bits_per_color, Some(10));
        assert_eq!(d.color_encoding.as_deref(), Some("RGB"));
        assert_eq!(d.color_space.as_deref(), Some("sRGB"));
        assert_eq!((d.dpi, d.scale_pct), (Some(120), Some(125)));
        assert_eq!(d.orientation, Some(0));
        assert_eq!((d.width_mm, d.height_mm), (Some(597), Some(336)));
        assert_eq!(d.diagonal_in, Some(27.0));

        assert_eq!(d.hdr.supported, Some(true), "EDID declares PQ");
        assert_eq!(d.hdr.enabled, Some(false));
        assert_eq!(d.hdr.sdr_white_nits, None, "only meaningful with HDR on");
        assert_eq!(d.hdr.max_nits, Some(400.0));
        assert_eq!(d.hdr.eotfs, ["SDR", "PQ (ST 2084)"]);

        assert_eq!(d.vrr.supported, Some(true));
        assert_eq!(d.vrr.enabled, Some(true));
        assert_eq!(d.vrr.technology.as_deref(), Some("G-SYNC Compatible"));
        assert_eq!(d.vrr.source.as_deref(), Some("NVAPI"));
        assert_eq!(
            (d.vrr.min_hz, d.vrr.max_hz),
            (Some(48.0), Some(144.0)),
            "DP range hint"
        );

        let edid = d.edid.unwrap();
        assert!(edid.checksum_ok);
        assert_eq!(edid.range_hz, Some([48, 144]));
        assert!(
            !edid.hex.contains(&hex_of(b"TESTSERIAL01")),
            "hex is scrubbed"
        );
        assert_eq!(edid.hex.len(), 512);
    }

    fn hex_of(b: &[u8]) -> String {
        b.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn serial_never_reaches_debug_output() {
        let d = build(&lg_raw());
        assert!(!format!("{d:?}").contains("TESTSERIAL01"));
        assert!(serde_json::to_string(&d).unwrap().contains("TESTSERIAL01"));
    }

    #[test]
    fn hdr_on_reports_white_level_and_windows_11_24h2_flags_win() {
        let mut raw = lg_raw();
        raw.color = Some(AdvancedColor {
            supported: true,
            enabled: true,
            hdr_supported: Some(true),
            active_mode: Some(2),
            bits_per_color: 10,
            ..Default::default()
        });
        raw.sdr_white_level = Some(2500);
        raw.dxgi.as_mut().unwrap().color_space = 12;
        let d = build(&raw);
        assert_eq!(d.hdr.enabled, Some(true));
        assert_eq!(d.hdr.sdr_white_nits, Some(200.0));
        assert_eq!(d.color_space.as_deref(), Some("BT.2100 PQ (HDR10)"));

        // Auto Color Management on an SDR panel: advanced color on, but not HDR.
        raw.color = Some(AdvancedColor {
            supported: true,
            enabled: true,
            hdr_supported: Some(false),
            active_mode: Some(1),
            ..Default::default()
        });
        raw.edid = None;
        let d = build(&raw);
        assert_eq!((d.hdr.supported, d.hdr.enabled), (Some(false), Some(false)));
    }

    #[test]
    fn vrr_without_vendor_api_uses_edid_only() {
        let aoc = hex_fixture(include_str!("fixtures/aoc_hdmi21_freesync.hex"));
        let raw = RawDisplay {
            edid: Some(aoc),
            output_technology: Some(5),
            adapter_vendor: Some(0x8086),
            ..Default::default()
        };
        let d = build(&raw);
        assert_eq!(d.vrr.supported, Some(true));
        assert_eq!(d.vrr.enabled, None, "state needs a vendor API");
        assert_eq!(d.vrr.source.as_deref(), Some("EDID"));
        assert_eq!(d.vrr.technology.as_deref(), Some("HDMI VRR"));
        assert_eq!((d.vrr.min_hz, d.vrr.max_hz), (Some(48.0), Some(120.0)));
        assert_eq!(d.manufacturer.as_deref(), Some("AOC"));
        assert!(!d.primary, "no desktop rect");

        // DP range alone, no vendor API: capability unknown, not "no".
        let lg = RawDisplay {
            edid: lg_raw().edid,
            ..Default::default()
        };
        let d = build(&lg);
        assert_eq!(d.vrr.supported, None);
        assert_eq!(d.vrr.min_hz, None);

        // Vendor says not supported and no EDID block: a definite no.
        let mut raw = lg_raw();
        raw.vendor_vrr = Some(VendorVrr {
            api: "ADL",
            possible: Some(false),
            ..Default::default()
        });
        let d = build(&raw);
        assert_eq!(d.vrr.supported, Some(false));
        assert_eq!(d.vrr.technology, None);
    }

    #[test]
    fn laptop_panel_is_internal_and_rotated() {
        let raw = RawDisplay {
            edid: Some(hex_fixture(include_str!(
                "fixtures/auo_laptop_displayid2.hex"
            ))),
            output_technology: Some(i32::MIN),
            rotation: Some(2),
            desktop: Some([-1920, 0, 1080, 1920]),
            adapter_vendor: Some(0x1002),
            vendor_vrr: Some(VendorVrr {
                api: "ADL",
                possible: Some(true),
                enabled: Some(false),
                min_hz: Some(60.0),
                max_hz: Some(144.0),
            }),
            ..Default::default()
        };
        let d = build(&raw);
        assert!(d.internal);
        assert_eq!(d.connection.as_deref(), Some("Internal"));
        assert_eq!(d.orientation, Some(90));
        assert_eq!(d.manufacturer.as_deref(), Some("AU Optronics"));
        assert_eq!(d.vrr.technology.as_deref(), Some("FreeSync"));
        assert_eq!(d.vrr.enabled, Some(false));
        assert_eq!(d.max_refresh_hz, Some(144.03));
        assert_eq!(d.diagonal_in, Some(17.3));
        assert_eq!(d.hdr.supported, Some(false), "EDID without HDR block");
    }

    #[test]
    fn no_edid_falls_back_to_friendly_name() {
        let d = build(&RawDisplay {
            friendly_name: Some("Generic PnP Monitor".into()),
            ..Default::default()
        });
        assert_eq!(d.name, "Generic PnP Monitor");
        assert_eq!(d.hdr.supported, None);
        assert_eq!(d.vrr.supported, None);
        assert!(d.edid.is_none());
    }

    #[test]
    fn device_path_maps_to_registry_instance() {
        assert_eq!(
            device_instance(
                r"\\?\DISPLAY#GSM5B7F#5&1b2b3c&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}"
            )
            .as_deref(),
            Some(r"DISPLAY\GSM5B7F\5&1b2b3c&0&UID4352")
        );
        assert_eq!(device_instance(""), None);
        assert_eq!(device_instance(r"\\?\DISPLAY#X"), None);
    }

    #[test]
    fn sort_puts_primary_first_then_left_to_right() {
        let mk = |x: i32, y: i32| DisplayInfo {
            primary: x == 0 && y == 0,
            rect: Some([x, y, 100, 100]),
            ..Default::default()
        };
        let mut v = vec![mk(1920, 0), mk(-1080, 0), mk(0, 0)];
        sort(&mut v);
        let xs: Vec<i32> = v.iter().map(|d| d.rect.unwrap()[0]).collect();
        assert_eq!(xs, [0, -1080, 1920]);
    }
}

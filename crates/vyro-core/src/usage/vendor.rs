//! Decoders for the GPU vendor libraries' raw data: NVIDIA NVAPI, AMD ADL (PMLog) and Intel
//! IGCL (Control Library). Offsets and sizes were checked against the vendors' headers compiled
//! for x64 Windows; the FFI side in `live` only fetches the bytes.

// ---------------------------------------------------------------------------------------------
// NVIDIA NVAPI
// ---------------------------------------------------------------------------------------------

/// NVAPI struct version: size in the low 16 bits, version above.
pub const fn nvapi_version(size: usize, ver: u32) -> u32 {
    size as u32 | (ver << 16)
}

/// Hotspot and memory-junction temperatures from `NvAPI_GPU_GetThermalSensors` (1/256 °C
/// units). The channel layout differs by generation; RTX 50 no longer exposes a hotspot here
/// (see [`nvidia_hotspot`]).
pub fn nvapi_thermal(name: &str, temps: &[i32]) -> (Option<f64>, Option<f64>) {
    let at = |i: usize| {
        temps
            .get(i)
            .map(|&t| f64::from(t) / 256.0)
            .filter(|t| *t > 0.0 && *t < 150.0)
    };
    let n = name.to_ascii_uppercase();
    if n.contains("RTX 50") {
        (None, at(2))
    } else if n.contains("RTX 40") {
        (at(1), at(7))
    } else {
        (at(1), at(9))
    }
}

/// GPU hotspot from the raw GB20x thermal registers that PawnIO's `Nvidia` module returns
/// (`ioctl_read_thermal_registers`, BAR0 + 0xAD0A90): channel 0 is the hotspot, valid when
/// bit 30 is set, in 1/256 °C in the low 16 bits.
pub fn nvidia_hotspot(raw: &[u64]) -> Option<f64> {
    let r = *raw.first()?;
    if r & (1 << 30) == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let t = (r & 0xFFFF) as f64 / 256.0;
    (t > 0.0 && t < 150.0).then_some(t)
}

/// Core voltage from `NvAPI_GPU_ClientVoltRailsGetStatus` (µV at offset 0x28 of a 0x4C-byte
/// struct).
pub fn nvapi_core_volts(buf: &[u8]) -> Option<f64> {
    let uv = u32::from_le_bytes(buf.get(0x28..0x2C)?.try_into().ok()?);
    (uv > 0 && uv < 3_000_000).then(|| f64::from(uv) / 1e6)
}

// ---------------------------------------------------------------------------------------------
// AMD ADL
// ---------------------------------------------------------------------------------------------

/// `sizeof(AdapterInfo)` on Windows.
pub const ADL_ADAPTER_INFO_SIZE: usize = 1572;
const ADL_INDEX: usize = 4;
const ADL_BUS: usize = 264;
const ADL_VENDOR: usize = 276;
const ADL_PRESENT: usize = 792;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdlAdapter {
    pub index: i32,
    pub bus: u32,
    /// `iPresent`; a headless iGPU reports 0 but still answers sensor queries.
    pub present: bool,
}

fn i32_at(b: &[u8], off: usize) -> Option<i32> {
    Some(i32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

/// One raw `AdapterInfo` record (for diagnostics and matching).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdlRecord {
    pub index: i32,
    pub bus: i32,
    pub vendor: i32,
    pub present: bool,
    pub name: String,
    /// GDI source this logical adapter drives, e.g. `\\.\DISPLAY1`; empty when none.
    pub display_name: String,
}

const ADL_NAME: usize = 280;
const ADL_DISPLAY_NAME: usize = 536;
const ADL_VENDOR_AMD: i32 = 1002;
const ADL_MAX_PATH: usize = 256;

/// Every record in an `AdapterInfo` array.
pub fn parse_adl_records(buf: &[u8]) -> Vec<AdlRecord> {
    buf.as_chunks::<ADL_ADAPTER_INFO_SIZE>()
        .0
        .iter()
        .filter_map(|rec| {
            let text = |off: usize| {
                let raw = &rec[off..off + ADL_MAX_PATH];
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                String::from_utf8_lossy(&raw[..end]).trim().to_owned()
            };
            Some(AdlRecord {
                index: i32_at(rec, ADL_INDEX)?,
                bus: i32_at(rec, ADL_BUS)?,
                vendor: i32_at(rec, ADL_VENDOR)?,
                present: i32_at(rec, ADL_PRESENT)? != 0,
                name: text(ADL_NAME),
                display_name: text(ADL_DISPLAY_NAME),
            })
        })
        .collect()
}

/// AMD adapters, one per PCI bus (ADL lists a logical adapter per display output).
pub fn adl_adapters(records: &[AdlRecord]) -> Vec<AdlAdapter> {
    let mut out: Vec<AdlAdapter> = Vec::new();
    for r in records {
        let Ok(bus) = u32::try_from(r.bus) else {
            continue;
        };
        // ADL stores AMD's PCI vendor id as the *decimal* number 1002 (0x3EA), not 0x1002.
        if r.vendor != ADL_VENDOR_AMD && r.vendor != 0x1002 {
            continue;
        }
        let a = AdlAdapter {
            index: r.index,
            bus,
            present: r.present,
        };
        match out.iter_mut().find(|x| x.bus == bus) {
            // Prefer a present logical adapter over a not-present one on the same device.
            Some(x) if a.present && !x.present => *x = a,
            Some(_) => {}
            None => out.push(a),
        }
    }
    out
}

/// [`adl_adapters`] straight from the raw buffer.
pub fn parse_adl_adapters(buf: &[u8]) -> Vec<AdlAdapter> {
    adl_adapters(&parse_adl_records(buf))
}

/// `sizeof(ADLDisplayInfo)`.
pub const ADL_DISPLAY_INFO_SIZE: usize = 552;

/// Active display index on ADL logical adapter `adapter`, from an `ADLDisplayInfo` array:
/// the first display mapped to that adapter and both connected and mapped.
pub fn adl_active_display(buf: &[u8], adapter: i32) -> Option<i32> {
    const CONNECTED_MAPPED: i32 = 0x3;
    buf.as_chunks::<ADL_DISPLAY_INFO_SIZE>()
        .0
        .iter()
        .find(|d| {
            i32_at(&d[..], 8) == Some(adapter)
                && i32_at(&d[..], 548).is_some_and(|v| v & CONNECTED_MAPPED == CONNECTED_MAPPED)
        })
        .and_then(|d| i32_at(&d[..], 0))
}

/// `ADL_PMLOG_*` sensor ids used here.
pub mod pmlog {
    pub const CLK_GFX: usize = 1;
    pub const CLK_MEM: usize = 2;
    pub const TEMP_EDGE: usize = 8;
    pub const TEMP_MEM: usize = 9;
    pub const FAN_RPM: usize = 14;
    pub const FAN_PCT: usize = 15;
    pub const ACTIVITY_GFX: usize = 19;
    pub const GFX_VOLTAGE: usize = 21;
    pub const ASIC_POWER: usize = 23;
    pub const TEMP_HOTSPOT: usize = 27;
    pub const TEMP_GFX: usize = 28;
    pub const BOARD_POWER: usize = 73;
    pub const MAX_SENSORS: usize = 256;
}

/// Readings from one vendor library, applied over what WDDM / PDH already provide.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VendorReadings {
    pub mhz_core: Option<f64>,
    pub mhz_mem: Option<f64>,
    pub temp_edge: Option<f64>,
    pub temp_hotspot: Option<f64>,
    pub temp_mem: Option<f64>,
    pub fan_rpm: Option<f64>,
    pub fan_pct: Option<f64>,
    pub voltage_v: Option<f64>,
    pub power_w: Option<f64>,
    pub throttle_power: Option<bool>,
    pub throttle_thermal: Option<bool>,
}

/// Decodes `ADLPMLogDataOutput.sensors` as `(supported, value)` pairs.
pub fn decode_pmlog(sensors: &[[i32; 2]]) -> VendorReadings {
    let get = |i: usize| {
        sensors
            .get(i)
            .filter(|s| s[0] != 0)
            .map(|s| f64::from(s[1]))
    };
    let pos = |i: usize| get(i).filter(|v| *v > 0.0);
    VendorReadings {
        mhz_core: pos(pmlog::CLK_GFX),
        mhz_mem: pos(pmlog::CLK_MEM),
        temp_edge: pos(pmlog::TEMP_EDGE).or(pos(pmlog::TEMP_GFX)),
        temp_hotspot: pos(pmlog::TEMP_HOTSPOT),
        temp_mem: pos(pmlog::TEMP_MEM),
        fan_rpm: get(pmlog::FAN_RPM),
        fan_pct: get(pmlog::FAN_PCT),
        voltage_v: pos(pmlog::GFX_VOLTAGE).map(|mv| mv / 1000.0),
        power_w: pos(pmlog::BOARD_POWER).or(pos(pmlog::ASIC_POWER)),
        ..Default::default()
    }
}

/// On an APU, ADL's power and voltage sensors describe the whole package (CPU cores included:
/// a field test read 42–87 W and 1.2 V on an idle Radeon iGPU next to a busy CPU), so they are
/// dropped rather than shown as GPU readings.
pub fn integrated_only(mut r: VendorReadings) -> VendorReadings {
    r.power_w = None;
    r.voltage_v = None;
    r
}

// ---------------------------------------------------------------------------------------------
// Intel IGCL
// ---------------------------------------------------------------------------------------------

pub const CTL_INIT_ARGS_SIZE: usize = 36;
pub const CTL_DEVICE_PROPS_SIZE: usize = 320;
pub const CTL_DEVICE_PROPS_DEVICE_ID: usize = 8;
pub const CTL_DEVICE_PROPS_DEVICE_ID_SIZE: usize = 16;
pub const CTL_DEVICE_PROPS_VENDOR: usize = 64;
pub const CTL_TELEMETRY_SIZE: usize = 1024;

mod ctl {
    pub const ITEM: usize = 24;
    pub const TIMESTAMP: usize = 8;
    pub const GPU_ENERGY: usize = 32;
    pub const GPU_VOLTAGE: usize = 56;
    pub const GPU_CLOCK: usize = 80;
    pub const GPU_TEMP: usize = 104;
    pub const POWER_LIMITED: usize = 200;
    pub const TEMP_LIMITED: usize = 201;
    pub const CURRENT_LIMITED: usize = 202;
    pub const VRAM_CLOCK: usize = 256;
    pub const VRAM_TEMP: usize = 352;
    pub const FAN_SPEED: usize = 688;

    pub const UNITS_VOLTS: u32 = 3;
    pub const UNITS_RPM: u32 = 9;
    pub const UNITS_PERCENT: u32 = 11;
    pub const UNITS_MILLIVOLTS: u32 = 13;
}

/// One `ctl_oc_telemetry_item_t` as `(units, value)`, when supported.
fn ctl_item(buf: &[u8], off: usize) -> Option<(u32, f64)> {
    let rec = buf.get(off..off + ctl::ITEM)?;
    if rec[0] == 0 {
        return None;
    }
    let units = u32::from_le_bytes(rec[4..8].try_into().ok()?);
    let ty = u32::from_le_bytes(rec[8..12].try_into().ok()?);
    let v: [u8; 8] = rec[16..24].try_into().ok()?;
    let value = match ty {
        0 => f64::from(v[0] as i8),
        1 => f64::from(v[0]),
        2 => f64::from(i16::from_le_bytes([v[0], v[1]])),
        3 => f64::from(u16::from_le_bytes([v[0], v[1]])),
        4 => f64::from(i32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        5 => f64::from(u32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        6 => i64::from_le_bytes(v) as f64,
        7 => u64::from_le_bytes(v) as f64,
        8 => f64::from(f32::from_le_bytes([v[0], v[1], v[2], v[3]])),
        9 => f64::from_le_bytes(v),
        _ => return None,
    };
    value.is_finite().then_some((units, value))
}

/// A decoded `ctl_power_telemetry_t`. Power needs two samples (energy counter / timestamp).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CtlSample {
    pub readings: VendorReadings,
    pub energy_j: Option<f64>,
    pub time_s: Option<f64>,
}

pub fn decode_ctl_telemetry(buf: &[u8]) -> CtlSample {
    let val = |off| ctl_item(buf, off).map(|(_, v)| v);
    let flag = |off: usize| buf.get(off).map(|&b| b != 0);
    let voltage_v = ctl_item(buf, ctl::GPU_VOLTAGE).and_then(|(u, v)| match u {
        ctl::UNITS_VOLTS => Some(v),
        ctl::UNITS_MILLIVOLTS => Some(v / 1000.0),
        _ => None,
    });
    let fan = ctl_item(buf, ctl::FAN_SPEED);
    let power = flag(ctl::POWER_LIMITED)
        .zip(flag(ctl::CURRENT_LIMITED))
        .map(|(a, b)| a || b);
    CtlSample {
        readings: VendorReadings {
            mhz_core: val(ctl::GPU_CLOCK).filter(|v| *v > 0.0),
            mhz_mem: val(ctl::VRAM_CLOCK).filter(|v| *v > 0.0),
            temp_edge: val(ctl::GPU_TEMP).filter(|v| *v > 0.0),
            temp_mem: val(ctl::VRAM_TEMP).filter(|v| *v > 0.0),
            fan_rpm: fan.filter(|(u, _)| *u == ctl::UNITS_RPM).map(|(_, v)| v),
            fan_pct: fan
                .filter(|(u, _)| *u == ctl::UNITS_PERCENT)
                .map(|(_, v)| v),
            voltage_v: voltage_v.filter(|v| *v > 0.0),
            throttle_power: power,
            throttle_thermal: flag(ctl::TEMP_LIMITED),
            ..Default::default()
        },
        energy_j: val(ctl::GPU_ENERGY),
        time_s: val(ctl::TIMESTAMP),
    }
}

/// Average power between two samples, W.
pub fn ctl_power(prev: &CtlSample, cur: &CtlSample) -> Option<f64> {
    let dj = cur.energy_j? - prev.energy_j?;
    let dt = cur.time_s? - prev.time_s?;
    (dt > 0.0 && dj >= 0.0).then(|| dj / dt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvapi_versions_and_channels() {
        assert_eq!(nvapi_version(168, 2), 0x0002_00A8);
        let mut t = [0i32; 32];
        t[1] = 70 * 256;
        t[2] = 62 * 256;
        t[7] = 80 * 256;
        t[9] = 90 * 256;
        assert_eq!(
            nvapi_thermal("NVIDIA GeForce RTX 5060 Ti", &t),
            (None, Some(62.0))
        );
        assert_eq!(
            nvapi_thermal("NVIDIA GeForce RTX 4090", &t),
            (Some(70.0), Some(80.0))
        );
        assert_eq!(
            nvapi_thermal("NVIDIA GeForce RTX 3080", &t),
            (Some(70.0), Some(90.0))
        );
        assert_eq!(
            nvapi_thermal("NVIDIA GeForce GTX 1080", &[0; 32]),
            (None, None)
        );
        let mut v = [0u8; 0x4C];
        v[0x28..0x2C].copy_from_slice(&875_000u32.to_le_bytes());
        assert_eq!(nvapi_core_volts(&v), Some(0.875));
        assert_eq!(nvapi_core_volts(&[0; 0x4C]), None);
    }

    #[test]
    fn decodes_the_gb20x_hotspot_register() {
        // 43 °C (HWMonitor's "Hot Spot" on the field RTX 5060 Ti) with the valid bit.
        assert_eq!(nvidia_hotspot(&[0x4000_2B00, 0, 0, 0, 0, 0]), Some(43.0));
        assert_eq!(nvidia_hotspot(&[0x4000_2B80]), Some(43.5));
        assert_eq!(nvidia_hotspot(&[0x0000_2B00]), None, "no valid bit");
        assert_eq!(nvidia_hotspot(&[0x4000_0000]), None, "zero");
        assert_eq!(nvidia_hotspot(&[]), None);
    }

    #[test]
    fn adl_display_names_and_active_display() {
        let mut r = adl_rec(3, 1, 1002, 1);
        r[ADL_DISPLAY_NAME..ADL_DISPLAY_NAME + 13].copy_from_slice(b"\\\\.\\DISPLAY2\0");
        assert_eq!(parse_adl_records(&r)[0].display_name, r"\\.\DISPLAY2");

        let info = |logical: i32, adapter: i32, value: i32| {
            let mut d = vec![0u8; ADL_DISPLAY_INFO_SIZE];
            d[0..4].copy_from_slice(&logical.to_le_bytes());
            d[8..12].copy_from_slice(&adapter.to_le_bytes());
            d[548..552].copy_from_slice(&value.to_le_bytes());
            d
        };
        let buf = [info(0, 3, 0x1), info(1, 4, 0x3), info(2, 3, 0x3)].concat();
        assert_eq!(adl_active_display(&buf, 3), Some(2));
        assert_eq!(adl_active_display(&buf, 5), None);
    }

    fn adl_rec(index: i32, bus: i32, vendor: i32, present: i32) -> Vec<u8> {
        let mut r = vec![0u8; ADL_ADAPTER_INFO_SIZE];
        r[ADL_NAME..ADL_NAME + 9].copy_from_slice(b"Radeon X\0");
        r[ADL_INDEX..ADL_INDEX + 4].copy_from_slice(&index.to_le_bytes());
        r[ADL_BUS..ADL_BUS + 4].copy_from_slice(&bus.to_le_bytes());
        r[ADL_VENDOR..ADL_VENDOR + 4].copy_from_slice(&vendor.to_le_bytes());
        r[ADL_PRESENT..ADL_PRESENT + 4].copy_from_slice(&present.to_le_bytes());
        r
    }

    #[test]
    fn adl_adapters_dedupe_by_bus() {
        let buf = [
            adl_rec(0, 14, 0x1002, 1),
            adl_rec(1, 14, 0x1002, 1),
            adl_rec(2, 3, 0x1002, 0),
            adl_rec(3, 5, 0x10DE, 1),
            adl_rec(4, 3, 0x1002, 1),
        ]
        .concat();
        assert_eq!(
            parse_adl_adapters(&buf),
            vec![
                AdlAdapter {
                    index: 0,
                    bus: 14,
                    present: true
                },
                AdlAdapter {
                    index: 4,
                    bus: 3,
                    present: true
                },
            ],
            "a present adapter replaces a not-present one on the same bus"
        );
        // As reported by a real Radeon iGPU next to an NVIDIA card (vendor ids 1002 and 10
        // are ADL's decimal-looking encodings).
        let real = [
            adl_rec(0, 20, 1002, 1),
            adl_rec(1, 20, 1002, 1),
            adl_rec(5, 1, 10, 1),
        ]
        .concat();
        assert_eq!(
            parse_adl_adapters(&real),
            vec![AdlAdapter {
                index: 0,
                bus: 20,
                present: true
            }]
        );
        let headless = adl_rec(7, 14, 0x1002, 0);
        assert_eq!(
            parse_adl_adapters(&headless),
            vec![AdlAdapter {
                index: 7,
                bus: 14,
                present: false
            }],
            "headless iGPU kept"
        );
    }

    #[test]
    fn pmlog_decoding() {
        let mut s = [[0i32; 2]; pmlog::MAX_SENSORS];
        s[pmlog::CLK_GFX] = [1, 2450];
        s[pmlog::TEMP_EDGE] = [1, 55];
        s[pmlog::TEMP_HOTSPOT] = [1, 71];
        s[pmlog::GFX_VOLTAGE] = [1, 1050];
        s[pmlog::ASIC_POWER] = [1, 180];
        s[pmlog::FAN_PCT] = [1, 0];
        s[pmlog::TEMP_MEM] = [0, 99]; // unsupported → ignored
        let r = decode_pmlog(&s);
        assert_eq!(r.mhz_core, Some(2450.0));
        assert_eq!(r.temp_edge, Some(55.0));
        assert_eq!(r.temp_hotspot, Some(71.0));
        assert_eq!(r.voltage_v, Some(1.05));
        assert_eq!(r.power_w, Some(180.0));
        assert_eq!(r.fan_pct, Some(0.0), "0 % is a real reading (fan stop)");
        assert_eq!(r.temp_mem, None);
        s[pmlog::BOARD_POWER] = [1, 210];
        s[pmlog::TEMP_EDGE] = [0, 0];
        s[pmlog::TEMP_GFX] = [1, 48];
        let r = decode_pmlog(&s);
        assert_eq!(r.power_w, Some(210.0), "board power preferred");
        assert_eq!(r.temp_edge, Some(48.0), "GFX temp as edge fallback (APUs)");
    }

    #[test]
    fn apu_package_readings_are_not_gpu_readings() {
        let r = integrated_only(VendorReadings {
            temp_edge: Some(46.0),
            power_w: Some(64.8),
            voltage_v: Some(1.22),
            ..Default::default()
        });
        assert_eq!(
            (r.temp_edge, r.power_w, r.voltage_v),
            (Some(46.0), None, None)
        );
    }

    fn put_item(buf: &mut [u8], off: usize, units: u32, ty: u32, value: [u8; 8]) {
        buf[off] = 1;
        buf[off + 4..off + 8].copy_from_slice(&units.to_le_bytes());
        buf[off + 8..off + 12].copy_from_slice(&ty.to_le_bytes());
        buf[off + 16..off + 24].copy_from_slice(&value);
    }

    #[test]
    fn ctl_telemetry_decoding() {
        let mut b = vec![0u8; CTL_TELEMETRY_SIZE];
        put_item(&mut b, ctl::TIMESTAMP, 7, 9, 10.0f64.to_le_bytes());
        put_item(&mut b, ctl::GPU_ENERGY, 6, 9, 1000.0f64.to_le_bytes());
        put_item(&mut b, ctl::GPU_VOLTAGE, 3, 9, 0.95f64.to_le_bytes());
        put_item(&mut b, ctl::GPU_CLOCK, 0, 9, 2400.0f64.to_le_bytes());
        put_item(&mut b, ctl::GPU_TEMP, 5, 9, 61.5f64.to_le_bytes());
        let mut f = [0u8; 8];
        f[..4].copy_from_slice(&1450u32.to_le_bytes());
        put_item(&mut b, ctl::FAN_SPEED, ctl::UNITS_RPM, 5, f);
        b[ctl::TEMP_LIMITED] = 1;
        let first = decode_ctl_telemetry(&b);
        let r = &first.readings;
        assert_eq!(r.mhz_core, Some(2400.0));
        assert_eq!(r.temp_edge, Some(61.5));
        assert_eq!(r.voltage_v, Some(0.95));
        assert_eq!(r.fan_rpm, Some(1450.0));
        assert_eq!(r.fan_pct, None);
        assert_eq!(r.throttle_thermal, Some(true));
        assert_eq!(r.throttle_power, Some(false));
        assert_eq!(r.temp_mem, None, "unsupported item");

        put_item(&mut b, ctl::TIMESTAMP, 7, 9, 11.0f64.to_le_bytes());
        put_item(&mut b, ctl::GPU_ENERGY, 6, 9, 1150.0f64.to_le_bytes());
        let second = decode_ctl_telemetry(&b);
        assert_eq!(ctl_power(&first, &second), Some(150.0));
        assert_eq!(ctl_power(&second, &first), None);
    }
}

//! Windows display collection. `QueryDisplayConfig` is the backbone (one path per active
//! output); DXGI outputs, the registry EDID, per-monitor DPI, DDC/CI or WMI brightness and the
//! GPU vendor VRR APIs are joined onto it. Every piece is optional: a failing call leaves its
//! fields empty.

use std::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;

use windows::core::{w, Interface, HSTRING};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
    GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_DEVICE_INFO_TYPE, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, PHYSICAL_MONITOR,
    QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HWND, LPARAM, LRESULT, LUID, WPARAM,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6, DXGI_ADAPTER_FLAG_SOFTWARE,
};
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, RegisterClassW,
    SetTimer, MSG, WINDOW_EX_STYLE, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSW, WS_OVERLAPPED,
};

use super::{build, sort, AdvancedColor, DxgiOutput, RawDisplay, VendorVrr};
use crate::info::DisplayInfo;
use crate::live::dll::Library;
use crate::usage::vendor::{
    adl_active_display, parse_adl_records, ADL_ADAPTER_INFO_SIZE, ADL_DISPLAY_INFO_SIZE,
};
use crate::winutil::{wide_array, Key, HKEY_LOCAL_MACHINE};

/// All active displays, primary first.
pub fn collect() -> Vec<DisplayInfo> {
    let t = std::time::Instant::now();
    let mut raws = paths();
    if raws.is_empty() {
        log::info!("display: QueryDisplayConfig returned no active paths");
        return Vec::new();
    }
    let outputs = dxgi_outputs();
    let nv = NvDisplay::load();
    let adl = AdlDisplay::load();
    let wmi_brightness = wmi_brightness();

    for (raw, instance) in &mut raws {
        let out = raw
            .gdi_name
            .as_ref()
            .and_then(|g| outputs.iter().find(|o| &o.gdi == g));
        let monitor = out.map(|o| o.monitor);
        if let Some(o) = out {
            raw.dxgi = Some(o.desc);
            raw.adapter = Some(o.adapter.clone());
            raw.adapter_vendor = Some(o.vendor);
        }
        if let Some(inst) = instance.as_deref() {
            raw.edid = registry_edid(inst);
        }
        if let Some(m) = monitor {
            raw.dpi = dpi(m);
            raw.brightness_pct = ddc_brightness(m);
        }
        if raw.brightness_pct.is_none() {
            raw.brightness_pct = instance.as_deref().and_then(|inst| {
                let inst = inst.to_ascii_uppercase();
                wmi_brightness
                    .iter()
                    .find(|(name, _)| name.to_ascii_uppercase().starts_with(&inst))
                    .map(|&(_, b)| b)
            });
        }
        if let Some(gdi) = raw.gdi_name.as_deref() {
            raw.vendor_vrr = match raw.adapter_vendor {
                Some(0x10DE) => nv.as_ref().and_then(|n| n.vrr(gdi)),
                Some(0x1002) => adl.as_ref().and_then(|a| a.freesync(gdi)),
                _ => None,
            };
        }
        log::info!(
            "display: {} {:?} instance={:?} tech={:?} edid={}B dxgi={} dpi={:?} brightness={:?} color={:?} sdr={:?} vrr={:?}",
            raw.key,
            raw.gdi_name,
            instance,
            raw.output_technology,
            raw.edid.as_ref().map_or(0, Vec::len),
            raw.dxgi.is_some(),
            raw.dpi,
            raw.brightness_pct,
            raw.color,
            raw.sdr_white_level,
            raw.vendor_vrr
        );
    }
    let mut list: Vec<DisplayInfo> = raws.iter().map(|(r, _)| build(r)).collect();
    sort(&mut list);
    log::info!(
        "display: {} display(s) collected in {:?}",
        list.len(),
        t.elapsed()
    );
    list
}

// ---------------------------------------------------------------------------------------------
// DisplayConfig
// ---------------------------------------------------------------------------------------------

fn device_info<T>(kind: DISPLAYCONFIG_DEVICE_INFO_TYPE, adapter: LUID, id: u32) -> Option<T> {
    let mut buf = std::mem::MaybeUninit::<T>::zeroed();
    let header = buf.as_mut_ptr().cast::<DISPLAYCONFIG_DEVICE_INFO_HEADER>();
    // SAFETY: every DISPLAYCONFIG_* request struct starts with the header; `T` is one of them.
    unsafe {
        (*header).r#type = kind;
        (*header).size = size_of::<T>() as u32;
        (*header).adapterId = adapter;
        (*header).id = id;
        (DisplayConfigGetDeviceInfo(header) == ERROR_SUCCESS.0 as i32).then(|| buf.assume_init())
    }
}

/// `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2` (Windows 11 24H2; not yet in the bindings).
#[repr(C)]
struct AdvancedColorInfo2 {
    header: DISPLAYCONFIG_DEVICE_INFO_HEADER,
    value: u32,
    color_encoding: u32,
    bits_per_color_channel: u32,
    active_color_mode: u32,
}
const GET_ADVANCED_COLOR_INFO_2: DISPLAYCONFIG_DEVICE_INFO_TYPE =
    DISPLAYCONFIG_DEVICE_INFO_TYPE(15);

fn advanced_color(adapter: LUID, id: u32) -> Option<AdvancedColor> {
    if let Some(c) = device_info::<AdvancedColorInfo2>(GET_ADVANCED_COLOR_INFO_2, adapter, id) {
        let v = c.value;
        return Some(AdvancedColor {
            supported: v & 0x1 != 0,
            enabled: v & 0x2 != 0,
            wide_color_enforced: false,
            hdr_supported: Some(v & 0x10 != 0),
            active_mode: Some(c.active_color_mode),
            encoding: c.color_encoding,
            bits_per_color: c.bits_per_color_channel,
        });
    }
    let c: DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO = device_info(
        DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
        adapter,
        id,
    )?;
    // SAFETY: plain bitfield union read.
    let v = unsafe { c.Anonymous.value };
    Some(AdvancedColor {
        supported: v & 0x1 != 0,
        enabled: v & 0x2 != 0,
        wide_color_enforced: v & 0x4 != 0,
        hdr_supported: None,
        active_mode: None,
        encoding: c.colorEncoding.0 as u32,
        bits_per_color: c.bitsPerColorChannel,
    })
}

/// Active paths with the monitor's PnP instance id (for the registry EDID).
fn paths() -> Vec<(RawDisplay, Option<String>)> {
    let (mut np, mut nm) = (0u32, 0u32);
    let mut paths = Vec::new();
    let mut modes = Vec::new();
    for _ in 0..3 {
        // SAFETY: out-pointers valid.
        if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) }
            != ERROR_SUCCESS
        {
            return Vec::new();
        }
        paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        // SAFETY: arrays sized as reported; counts updated in place.
        let rc = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                None,
            )
        };
        if rc == ERROR_SUCCESS {
            paths.truncate(np as usize);
            modes.truncate(nm as usize);
            break;
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            log::info!("display: QueryDisplayConfig failed ({rc:?})");
            return Vec::new();
        }
    }

    paths
        .iter()
        .map(|p| {
            let s = &p.sourceInfo;
            let t = &p.targetInfo;
            let mut raw = RawDisplay {
                key: format!(
                    "{:x}:{:x}:{}",
                    t.adapterId.HighPart, t.adapterId.LowPart, t.id
                ),
                output_technology: Some(t.outputTechnology.0),
                rotation: Some(t.rotation.0),
                ..Default::default()
            };
            let r = t.refreshRate;
            if r.Denominator != 0 && r.Numerator != 0 {
                raw.refresh_hz = Some(f64::from(r.Numerator) / f64::from(r.Denominator));
            }
            // SAFETY: without QDC_VIRTUAL_MODE_AWARE the union holds a plain index.
            let idx = unsafe { s.Anonymous.modeInfoIdx } as usize;
            if let Some(m) = modes
                .get(idx)
                .filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE)
            {
                // SAFETY: infoType says the union holds a source mode.
                let sm = unsafe { m.Anonymous.sourceMode };
                raw.desktop = Some([
                    sm.position.x,
                    sm.position.y,
                    sm.width as i32,
                    sm.height as i32,
                ]);
            }
            if let Some(n) = device_info::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                s.adapterId,
                s.id,
            ) {
                raw.gdi_name = Some(wide_array(&n.viewGdiDeviceName));
            }
            let mut instance = None;
            if let Some(n) = device_info::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                t.adapterId,
                t.id,
            ) {
                raw.friendly_name = Some(wide_array(&n.monitorFriendlyDeviceName));
                instance = super::device_instance(&wide_array(&n.monitorDevicePath));
            }
            raw.color = advanced_color(t.adapterId, t.id);
            raw.sdr_white_level = device_info::<DISPLAYCONFIG_SDR_WHITE_LEVEL>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                t.adapterId,
                t.id,
            )
            .map(|w| w.SDRWhiteLevel);
            (raw, instance)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// DXGI, registry, DPI, brightness
// ---------------------------------------------------------------------------------------------

struct Output {
    gdi: String,
    monitor: HMONITOR,
    adapter: String,
    vendor: u32,
    desc: DxgiOutput,
}

fn dxgi_outputs() -> Vec<Output> {
    let mut out = Vec::new();
    // SAFETY: plain factory creation.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return out;
    };
    for i in 0.. {
        // SAFETY: enumeration ends with an error.
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        // SAFETY: valid adapter.
        let Ok(ad) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if ad.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        for j in 0.. {
            // SAFETY: enumeration ends with an error.
            let Ok(o) = (unsafe { adapter.EnumOutputs(j) }) else {
                break;
            };
            let Ok(o6) = o.cast::<IDXGIOutput6>() else {
                continue;
            };
            // SAFETY: valid output.
            let Ok(d) = (unsafe { o6.GetDesc1() }) else {
                continue;
            };
            out.push(Output {
                gdi: wide_array(&d.DeviceName),
                monitor: d.Monitor,
                adapter: wide_array(&ad.Description),
                vendor: ad.VendorId,
                desc: DxgiOutput {
                    bits_per_color: d.BitsPerColor,
                    color_space: d.ColorSpace.0 as u32,
                    min_nits: d.MinLuminance,
                    max_nits: d.MaxLuminance,
                    max_full_frame_nits: d.MaxFullFrameLuminance,
                },
            });
        }
    }
    out
}

fn registry_edid(instance: &str) -> Option<Vec<u8>> {
    let key = Key::open(
        HKEY_LOCAL_MACHINE,
        &format!(r"SYSTEM\CurrentControlSet\Enum\{instance}\Device Parameters"),
    )?;
    let v = key.value("EDID")?;
    (v.data.len() >= 128).then_some(v.data)
}

fn dpi(m: HMONITOR) -> Option<u32> {
    // SAFETY: thread-local awareness switch, restored below; a DPI-unaware thread would
    // always read 96.
    let prev = unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let (mut x, mut y) = (0u32, 0u32);
    // SAFETY: valid monitor handle from DXGI and out-pointers.
    let ok = unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut x, &mut y) }.is_ok();
    // SAFETY: restoring the previous context.
    unsafe { SetThreadDpiAwarenessContext(prev) };
    (ok && x > 0).then_some(x)
}

/// External monitors over DDC/CI (MCCS brightness). Laptop panels fail here and use WMI.
fn ddc_brightness(m: HMONITOR) -> Option<u32> {
    let mut n = 0u32;
    // SAFETY: valid monitor and out-pointer.
    unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut n) }.ok()?;
    if n == 0 {
        return None;
    }
    let mut phys = vec![PHYSICAL_MONITOR::default(); n as usize];
    // SAFETY: array sized to `n`.
    unsafe { GetPhysicalMonitorsFromHMONITOR(m, &mut phys) }.ok()?;
    let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
    // SAFETY: handle from GetPhysicalMonitorsFromHMONITOR.
    let ok =
        unsafe { GetMonitorBrightness(phys[0].hPhysicalMonitor, &mut min, &mut cur, &mut max) }
            != 0;
    // SAFETY: releasing what GetPhysicalMonitorsFromHMONITOR returned.
    let _ = unsafe { DestroyPhysicalMonitors(&phys) };
    (ok && max > min).then(|| (cur.saturating_sub(min) * 100 + (max - min) / 2) / (max - min))
}

#[derive(serde::Deserialize)]
#[serde(rename = "WmiMonitorBrightness", rename_all = "PascalCase")]
struct WmiMonitorBrightness {
    instance_name: String,
    current_brightness: u8,
    active: bool,
}

/// Laptop panel backlight: `(PnP instance name, percent)`.
fn wmi_brightness() -> Vec<(String, u32)> {
    let Ok(con) = wmi::WMIConnection::with_namespace_path(r"ROOT\WMI") else {
        return Vec::new();
    };
    let rows: Vec<WmiMonitorBrightness> = con.query().unwrap_or_default();
    rows.into_iter()
        .filter(|r| r.active)
        .map(|r| (r.instance_name, u32::from(r.current_brightness)))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Vendor VRR: NVAPI (G-SYNC) and ADL (FreeSync)
// ---------------------------------------------------------------------------------------------

type NvQueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;
type NvInitialize = unsafe extern "C" fn() -> i32;
type NvDisplayIdByName = unsafe extern "C" fn(*const u8, *mut u32) -> i32;
type NvGetVrrInfo = unsafe extern "C" fn(u32, *mut NvVrrInfo) -> i32;

#[repr(C)]
struct NvVrrInfo {
    version: u32,
    flags: u32,
    reserved: [u32; 4],
}
const NV_VRR_INFO_VER: u32 = size_of::<NvVrrInfo>() as u32 | (1 << 16);

struct NvDisplay {
    by_name: NvDisplayIdByName,
    vrr_info: NvGetVrrInfo,
}

impl NvDisplay {
    fn load() -> Option<Self> {
        let lib = Library::system32("nvapi64.dll")?;
        // SAFETY: documented export signature.
        let qi: NvQueryInterface = unsafe { lib.get(c"nvapi_QueryInterface") }?;
        let get = |id: u32| {
            // SAFETY: lookup by id; null when unknown.
            let p = unsafe { qi(id) };
            (!p.is_null()).then_some(p)
        };
        // SAFETY (transmutes): each id maps to the declared function type (nvapi_interface.h).
        let init = unsafe { std::mem::transmute::<*mut c_void, NvInitialize>(get(0x0150_E828)?) };
        if unsafe { init() } != 0 {
            return None;
        }
        Some(Self {
            by_name: unsafe {
                std::mem::transmute::<*mut c_void, NvDisplayIdByName>(get(0xAE45_7190)?)
            },
            vrr_info: unsafe {
                std::mem::transmute::<*mut c_void, NvGetVrrInfo>(get(0xDF8F_DA57)?)
            },
        })
    }

    fn vrr(&self, gdi: &str) -> Option<VendorVrr> {
        let name = format!("{gdi}\0");
        let mut id = 0u32;
        // SAFETY: NUL-terminated ASCII name and out-pointer.
        let rc = unsafe { (self.by_name)(name.as_ptr(), &mut id) };
        if rc != 0 {
            log::info!("display: NvAPI_DISP_GetDisplayIdByDisplayName({gdi}) failed ({rc})");
            return None;
        }
        let mut info = NvVrrInfo {
            version: NV_VRR_INFO_VER,
            flags: 0,
            reserved: [0; 4],
        };
        // SAFETY: versioned struct of the declared size.
        let rc = unsafe { (self.vrr_info)(id, &mut info) };
        if rc != 0 {
            log::info!("display: NvAPI_Disp_GetVRRInfo({id:#x}) failed ({rc})");
            return None;
        }
        let f = info.flags;
        log::info!(
            "display: NVAPI VRR {gdi}: enabled={} possible={} requested={} indicator={} inVrrMode={}",
            f & 1 != 0,
            f & 2 != 0,
            f & 4 != 0,
            f & 8 != 0,
            f & 16 != 0
        );
        Some(VendorVrr {
            api: "NVAPI",
            // A clear flag may just mean G-SYNC is switched off: unknown, not "unsupported".
            possible: (f & 0x13 != 0).then_some(true),
            // Field PC (G-SYNC Compatible on, desktop idle): bIsVRREnabled=0 while
            // bIsDisplayInVRRMode=1. The display being in VRR mode is the setting; the
            // "enabled" bit only rises while an application drives variable refresh.
            enabled: Some(f & 0x11 != 0),
            active: Some(f & 0x1 != 0),
            min_hz: None,
            max_hz: None,
        })
    }
}

type AdlContext = *mut c_void;
type AdlMalloc = unsafe extern "system" fn(i32) -> *mut c_void;
type AdlCreate = unsafe extern "system" fn(AdlMalloc, i32, *mut AdlContext) -> i32;
type AdlCount = unsafe extern "system" fn(AdlContext, *mut i32) -> i32;
type AdlAdapterInfo = unsafe extern "system" fn(AdlContext, *mut u8, i32) -> i32;
type AdlDisplayInfo =
    unsafe extern "system" fn(AdlContext, i32, *mut i32, *mut *mut u8, i32) -> i32;
type AdlFreeSync =
    unsafe extern "system" fn(AdlContext, i32, i32, *mut i32, *mut i32, *mut i32, *mut i32) -> i32;

unsafe extern "system" fn adl_malloc(size: i32) -> *mut c_void {
    // SAFETY: plain allocation; freed with CoTaskMemFree.
    unsafe { windows::Win32::System::Com::CoTaskMemAlloc(usize::try_from(size).unwrap_or(0)) }
}

struct AdlDisplay {
    ctx: AdlContext,
    records: Vec<crate::usage::vendor::AdlRecord>,
    display_info: AdlDisplayInfo,
    freesync: AdlFreeSync,
}

impl AdlDisplay {
    /// The context is intentionally never destroyed: this runs rarely, and ADL tolerates it.
    fn load() -> Option<Self> {
        let lib = Library::system32("atiadlxx.dll")?;
        // SAFETY (lookups): signatures from the ADL SDK.
        let create: AdlCreate = unsafe { lib.get(c"ADL2_Main_Control_Create") }?;
        let count: AdlCount = unsafe { lib.get(c"ADL2_Adapter_NumberOfAdapters_Get") }?;
        let info: AdlAdapterInfo = unsafe { lib.get(c"ADL2_Adapter_AdapterInfo_Get") }?;
        let display_info: AdlDisplayInfo = unsafe { lib.get(c"ADL2_Display_DisplayInfo_Get") }?;
        let freesync: AdlFreeSync = unsafe { lib.get(c"ADL2_Display_FreeSyncState_Get") }?;
        let mut ctx: AdlContext = std::ptr::null_mut();
        // SAFETY: valid callback and out-pointer; 1 = connected adapters only.
        if unsafe { create(adl_malloc, 1, &mut ctx) } < 0 || ctx.is_null() {
            return None;
        }
        let mut n = 0i32;
        // SAFETY: valid context.
        if unsafe { count(ctx, &mut n) } != 0 || n <= 0 {
            return None;
        }
        let mut buf = vec![0u8; ADL_ADAPTER_INFO_SIZE * n as usize];
        // SAFETY: buffer sized for `n` records.
        if unsafe { info(ctx, buf.as_mut_ptr(), buf.len() as i32) } != 0 {
            return None;
        }
        Some(Self {
            ctx,
            records: parse_adl_records(&buf),
            display_info,
            freesync,
        })
    }

    fn freesync(&self, gdi: &str) -> Option<VendorVrr> {
        let adapter = self
            .records
            .iter()
            .find(|r| r.display_name.eq_ignore_ascii_case(gdi))?
            .index;
        let (mut n, mut ptr) = (0i32, std::ptr::null_mut::<u8>());
        // SAFETY: ADL allocates `n` ADLDisplayInfo records through adl_malloc.
        let rc = unsafe { (self.display_info)(self.ctx, adapter, &mut n, &mut ptr, 0) };
        if rc != 0 || ptr.is_null() || n <= 0 {
            return None;
        }
        // SAFETY: ADL wrote `n` records at `ptr`.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, n as usize * ADL_DISPLAY_INFO_SIZE) };
        let display = adl_active_display(bytes, adapter);
        // SAFETY: allocated by adl_malloc (CoTaskMemAlloc).
        unsafe { windows::Win32::System::Com::CoTaskMemFree(Some(ptr as *const c_void)) };
        let display = display?;
        let (mut cur, mut def, mut min, mut max) = (0i32, 0i32, 0i32, 0i32);
        // SAFETY: valid context, indices and out-pointers.
        let rc = unsafe {
            (self.freesync)(
                self.ctx, adapter, display, &mut cur, &mut def, &mut min, &mut max,
            )
        };
        log::info!(
            "display: ADL FreeSync {gdi} (adapter {adapter}, display {display}): rc={rc} current={cur} default={def} range={min}..{max} uHz"
        );
        let hz = |u: i32| (u > 0).then(|| (f64::from(u) / 1e6).round());
        match rc {
            0 => Some(VendorVrr {
                api: "ADL",
                possible: Some(true),
                enabled: Some(cur != 0),
                active: None,
                min_hz: hz(min),
                max_hz: hz(max),
            }),
            // ADL_ERR_NOT_SUPPORTED.
            -8 => Some(VendorVrr {
                api: "ADL",
                possible: Some(false),
                ..Default::default()
            }),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Hot-plug
// ---------------------------------------------------------------------------------------------

static ON_CHANGE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
const DEBOUNCE_TIMER: usize = 1;

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DEVICECHANGE => {
            // SAFETY: timer on our own window; re-arming restarts the debounce.
            unsafe { SetTimer(Some(hwnd), DEBOUNCE_TIMER, 750, None) };
            LRESULT(0)
        }
        WM_TIMER if wp.0 == DEBOUNCE_TIMER => {
            // SAFETY: our own timer.
            let _ = unsafe { KillTimer(Some(hwnd), DEBOUNCE_TIMER) };
            if let Some(f) = ON_CHANGE.get() {
                f();
            }
            LRESULT(0)
        }
        // SAFETY: default handling for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Calls `on_change` (debounced) after displays are added, removed or reconfigured. Uses a
/// hidden top-level window: message-only windows do not receive the broadcasts. Only the
/// first call per process installs a watcher.
pub fn watch(on_change: impl Fn() + Send + Sync + 'static) {
    if ON_CHANGE.set(Box::new(on_change)).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("display-watch".into())
        .spawn(|| {
            // SAFETY: standard window class registration and message loop on this thread.
            unsafe {
                let Ok(module) = GetModuleHandleW(None) else {
                    return;
                };
                let class = w!("VyroDisplayWatch");
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: module.into(),
                    lpszClassName: class,
                    ..Default::default()
                };
                if RegisterClassW(&wc) == 0 {
                    log::info!("display: watcher class registration failed");
                    return;
                }
                let Ok(_hwnd) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    class,
                    &HSTRING::from("Vyro display watcher"),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(module.into()),
                    None,
                ) else {
                    log::info!("display: watcher window creation failed");
                    return;
                };
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    DispatchMessageW(&msg);
                }
            }
        });
    if let Err(e) = spawned {
        log::info!("display: watcher thread failed to start: {e}");
    }
}

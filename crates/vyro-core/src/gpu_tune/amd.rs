//! AMD Radeon cards through ADLX (2.0), the interface of AMD's own driver software, as AMD Software
//! does it: `amdadlx64.dll` from System32, installed with the driver. RX 6000, 7000 and 9000
//! cards, desktop and laptop; integrated Radeon graphics are not tuned.
//!
//! ADLX sets the core's and the memory's highest clocks as clocks, not offsets: Vyro's offset is
//! the change from the card's own (its default, which ADLX gives), and a setting whose default is
//! not given is left alone. The power limit is a percentage above or below the card's own, which
//! ADLX gives as a change too. Stock is ADLX's "reset to factory".

use std::ffi::{c_char, c_void, CStr};

use super::controller::TuneDevice;
use super::ffi::adlx::{self, IntRange};
use super::{is_laptop, Clocks, Family, GpuInfo, Limits, Range, Settings, TuneCapability, Vendor};
use crate::live::dll::Library;

type Obj = *mut c_void;

/// Method `index` of the object `obj`, as the function type `F`.
///
/// # Safety
/// `obj` is a live ADLX object whose table has `F` at `index` ([`super::ffi::adlx`]).
unsafe fn method<F: Copy>(obj: Obj, index: usize) -> F {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    // SAFETY: an ADLX object begins with a pointer to its table of methods.
    unsafe {
        let table = *obj.cast::<*const usize>();
        std::mem::transmute_copy(&*table.add(index))
    }
}

type GetInt = unsafe extern "system" fn(Obj, *mut i32) -> i32;
type SetInt = unsafe extern "system" fn(Obj, i32) -> i32;
type GetRange = unsafe extern "system" fn(Obj, *mut IntRange) -> i32;
type GetStr = unsafe extern "system" fn(Obj, *mut *const c_char) -> i32;
type GetObj = unsafe extern "system" fn(Obj, *mut Obj) -> i32;
type ForGpu = unsafe extern "system" fn(Obj, Obj) -> i32;
type ForGpuBool = unsafe extern "system" fn(Obj, Obj, *mut u8) -> i32;
type ForGpuObj = unsafe extern "system" fn(Obj, Obj, *mut Obj) -> i32;
type Release = unsafe extern "system" fn(Obj) -> i32;
type Query = unsafe extern "system" fn(Obj, *const u16, *mut Obj) -> i32;
type Size = unsafe extern "system" fn(Obj) -> u32;
type At = unsafe extern "system" fn(Obj, u32, *mut Obj) -> i32;
type Init2 = unsafe extern "C" fn(u64, *mut Obj, *mut Obj) -> i32;
type Init = unsafe extern "C" fn(u64, *mut Obj) -> i32;

/// An ADLX object Vyro holds, released when dropped.
struct Held(Obj);

impl Held {
    fn get(&self) -> Obj {
        self.0
    }

    /// The same object as interface `name`, if it is one.
    fn query(&self, name: &str) -> Option<Held> {
        let iid = adlx::iid(name);
        let mut out = std::ptr::null_mut();
        // SAFETY: live object; `iid` is a NUL-terminated UTF-16 name.
        let r = unsafe {
            method::<Query>(self.0, adlx::object::QUERY_INTERFACE)(self.0, iid.as_ptr(), &mut out)
        };
        (r == adlx::OK && !out.is_null()).then_some(Held(out))
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: an object ADLX gave Vyro, released once.
        unsafe {
            method::<Release>(self.0, adlx::object::RELEASE)(self.0);
        }
    }
}

fn ok(r: i32) -> Result<(), String> {
    match r {
        adlx::OK | 1 | 2 => Ok(()),
        adlx::NOT_SUPPORTED => Err("the driver does not allow it".into()),
        code => Err(format!("AMD's driver interface said no (ADLX code {code})")),
    }
}

fn int(obj: Obj, index: usize) -> Option<i32> {
    let mut v = 0;
    // SAFETY: live object with an `(int*)` getter at `index`.
    (unsafe { method::<GetInt>(obj, index)(obj, &mut v) } == adlx::OK).then_some(v)
}

fn range(obj: Obj, index: usize) -> Option<IntRange> {
    let mut r = IntRange::default();
    // SAFETY: live object with an `(ADLX_IntRange*)` getter at `index`.
    (unsafe { method::<GetRange>(obj, index)(obj, &mut r) } == adlx::OK && r.max > r.min)
        .then_some(r)
}

fn text(obj: Obj, index: usize) -> Option<String> {
    let mut p: *const c_char = std::ptr::null();
    // SAFETY: live object with a `(const char**)` getter at `index`; ADLX keeps the text.
    if unsafe { method::<GetStr>(obj, index)(obj, &mut p) } != adlx::OK || p.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated string ADLX owns while the object lives.
    Some(
        unsafe { CStr::from_ptr(p) }
            .to_string_lossy()
            .trim()
            .to_owned(),
    )
}

/// ADLX's system, started once for the life of the process (its objects belong to it).
struct System(Obj);
// SAFETY: ADLX's system services are thread-safe and live as long as the process.
unsafe impl Send for System {}
unsafe impl Sync for System {}

fn system() -> Result<Obj, String> {
    static SYSTEM: std::sync::OnceLock<Result<System, String>> = std::sync::OnceLock::new();
    match SYSTEM.get_or_init(|| {
        let lib = Library::system32(adlx::DLL).ok_or("no AMD driver (ADLX) found")?;
        let mut sys = std::ptr::null_mut();
        // SAFETY (both): the exports' signatures in ADLX.h.
        let r = if let Some(init) = unsafe { lib.get::<Init2>(c"ADLXInitialize2") } {
            let mut mapping = std::ptr::null_mut();
            unsafe { init(adlx::FULL_VERSION, &mut sys, &mut mapping) }
        } else if let Some(init) = unsafe { lib.get::<Init>(c"ADLXInitialize") } {
            unsafe { init(adlx::FULL_VERSION, &mut sys) }
        } else {
            return Err("AMD's driver has no ADLX".into());
        };
        if r != adlx::OK || sys.is_null() {
            return Err(format!(
                "AMD's driver interface (ADLX) did not start (code {r})"
            ));
        }
        Ok(System(sys))
    }) {
        Ok(s) => Ok(s.0),
        Err(e) => Err(e.clone()),
    }
}

/// One setting ADLX sets as a clock: its interface, its range and the card's own value.
struct Clock {
    obj: Held,
    get: usize,
    set: usize,
    range: IntRange,
    default: i32,
}

impl Clock {
    fn open(
        tuning: Obj,
        gpu: Obj,
        supported: usize,
        get_iface: usize,
        iid: &str,
        m: [usize; 4],
    ) -> Option<Self> {
        let [get_range, get, set, get_default] = m;
        let mut yes = 0u8;
        // SAFETY: live tuning services and GPU; the method's signature in IGPUTuning.h.
        let r = unsafe { method::<ForGpuBool>(tuning, supported)(tuning, gpu, &mut yes) };
        if r != adlx::OK || yes == 0 {
            return None;
        }
        let mut iface = std::ptr::null_mut();
        // SAFETY: as above; ADLX gives an acquired object.
        if unsafe { method::<ForGpuObj>(tuning, get_iface)(tuning, gpu, &mut iface) } != adlx::OK
            || iface.is_null()
        {
            return None;
        }
        let obj = Held(iface).query(iid)?;
        let range = range(obj.get(), get_range)?;
        let default = int(obj.get(), get_default)?;
        Some(Self {
            obj,
            get,
            set,
            range,
            default,
        })
    }

    /// The offsets allowed: the range less the card's own, no further down than `floor`.
    fn offsets(&self, floor: i32) -> Option<Range> {
        let r = Range {
            min: (self.range.min - self.default).max(floor),
            max: self.range.max - self.default,
        };
        (r.max > 0 && r.min <= 0).then_some(r)
    }

    fn read(&self) -> Result<i32, String> {
        int(self.obj.get(), self.get)
            .map(|v| v - self.default)
            .ok_or_else(|| "the driver did not give its clock".into())
    }

    /// Sets the clock `offset` above the card's own, on the driver's step.
    fn write(&self, offset: i32) -> Result<(), String> {
        let step = self.range.step.max(1);
        let v = (self.default + offset).clamp(self.range.min, self.range.max);
        let v = self.range.min + (v - self.range.min) / step * step;
        // SAFETY: live interface with an `(int)` setter at `set`.
        ok(unsafe { method::<SetInt>(self.obj.get(), self.set)(self.obj.get(), v) })
    }
}

/// The power limit: % above or below the card's own.
struct Power {
    obj: Held,
    range: IntRange,
    default: i32,
}

struct AmdCard {
    tuning: Held,
    gpu: Held,
    core: Option<Clock>,
    mem: Option<Clock>,
    power: Option<Power>,
    info: GpuInfo,
    limits: Limits,
}

// SAFETY: ADLX objects may be called from any thread; the controller uses the card from one.
unsafe impl Send for AmdCard {}

/// Lowest the core's highest clock goes below the card's own: further only makes it slow.
const CORE_FLOOR: i32 = -500;
const MEM_FLOOR: i32 = -200;

/// The first AMD card Vyro can tune.
pub fn open_device() -> Result<Box<dyn TuneDevice>, TuneCapability> {
    let unavailable = |reason: String| TuneCapability::Unavailable { reason };
    let sys = system().map_err(unavailable)?;
    let mut list = std::ptr::null_mut();
    let mut services = std::ptr::null_mut();
    // SAFETY (both): the system's methods in ISystem.h; ADLX gives acquired objects.
    unsafe {
        if method::<GetObj>(sys, adlx::system::GET_GPUS)(sys, &mut list) != adlx::OK
            || list.is_null()
        {
            return Err(unavailable("AMD's driver listed no GPU".into()));
        }
    }
    let list = Held(list);
    unsafe {
        if method::<GetObj>(sys, adlx::system::GET_GPU_TUNING_SERVICES)(sys, &mut services)
            != adlx::OK
            || services.is_null()
        {
            return Err(unavailable("AMD's driver offers no GPU tuning".into()));
        }
    }
    let tuning = Held(services);
    // SAFETY: live list.
    let count = unsafe { method::<Size>(list.get(), adlx::gpu_list::SIZE)(list.get()) };
    let mut seen = None;
    for i in 0..count {
        let mut g = std::ptr::null_mut();
        // SAFETY: an index within the list's size; ADLX gives an acquired GPU.
        if unsafe { method::<At>(list.get(), adlx::gpu_list::AT_GPU_LIST)(list.get(), i, &mut g) }
            != adlx::OK
            || g.is_null()
        {
            continue;
        }
        let gpu = Held(g);
        let name = text(gpu.get(), adlx::gpu::NAME).unwrap_or_else(|| "AMD GPU".into());
        let discrete = int(gpu.get(), adlx::gpu::TYPE) == Some(adlx::GPUTYPE_DISCRETE);
        let family = Family::from_radeon(&name).filter(|_| discrete);
        if family.is_none() {
            seen.get_or_insert(name);
            continue;
        }
        let core = Clock::open(
            tuning.get(),
            gpu.get(),
            adlx::tuning::IS_SUPPORTED_MANUAL_GFX,
            adlx::tuning::GET_MANUAL_GFX,
            adlx::gfx::IID,
            [
                adlx::gfx::GET_MAX_RANGE,
                adlx::gfx::GET_MAX,
                adlx::gfx::SET_MAX,
                adlx::gfx::GET_MAX_DEFAULT,
            ],
        );
        let mem = Clock::open(
            tuning.get(),
            gpu.get(),
            adlx::tuning::IS_SUPPORTED_MANUAL_VRAM,
            adlx::tuning::GET_MANUAL_VRAM,
            adlx::vram::IID,
            [
                adlx::vram::GET_MAX_RANGE,
                adlx::vram::GET_MAX,
                adlx::vram::SET_MAX,
                adlx::vram::GET_MAX_DEFAULT,
            ],
        );
        let power = open_power(tuning.get(), gpu.get());
        let limits = Limits {
            core: core.as_ref().and_then(|c| c.offsets(CORE_FLOOR)),
            mem: mem.as_ref().and_then(|c| c.offsets(MEM_FLOOR)),
            power: power.as_ref().and_then(|p| {
                let r = Range {
                    min: 100 + p.range.min - p.default,
                    max: 100 + p.range.max - p.default,
                };
                (r.min < r.max && r.contains(100)).then_some(r)
            }),
            power_default_w: None,
        };
        let id = text(gpu.get(), adlx::gpu::DEVICE_ID).unwrap_or_default();
        let unique = int(gpu.get(), adlx::gpu::UNIQUE_ID).unwrap_or(i as i32);
        let driver = gpu
            .query(adlx::gpu2::IID)
            .and_then(|g2| text(g2.get(), adlx::gpu2::AMD_SOFTWARE_VERSION))
            .unwrap_or_default();
        let info = GpuInfo {
            vendor: Vendor::Amd,
            laptop: is_laptop(&name),
            uuid: format!("AMD-{id}-{unique}"),
            name,
            family,
            driver,
        };
        log::info!(
            "gpu tuning: {} ({:?}, AMD Software {}): core {:?}, memory {:?}, power {:?} %",
            info.name,
            info.family,
            info.driver,
            limits.core,
            limits.mem,
            limits.power
        );
        return Ok(Box::new(AmdCard {
            tuning,
            gpu,
            core,
            mem,
            power,
            info,
            limits,
        }));
    }
    Err(match seen {
        Some(name) => TuneCapability::ReadOnly {
            reason: format!(
                "Vyro tunes AMD Radeon RX 6000 to RX 9000 cards; the {name} is not one"
            ),
        },
        None => unavailable("no AMD Radeon card found".into()),
    })
}

fn open_power(tuning: Obj, gpu: Obj) -> Option<Power> {
    let mut yes = 0u8;
    // SAFETY: live tuning services and GPU; signatures in IGPUTuning.h.
    let r = unsafe {
        method::<ForGpuBool>(tuning, adlx::tuning::IS_SUPPORTED_MANUAL_POWER)(tuning, gpu, &mut yes)
    };
    if r != adlx::OK || yes == 0 {
        return None;
    }
    let mut iface = std::ptr::null_mut();
    // SAFETY: as above.
    if unsafe {
        method::<ForGpuObj>(tuning, adlx::tuning::GET_MANUAL_POWER)(tuning, gpu, &mut iface)
    } != adlx::OK
        || iface.is_null()
    {
        return None;
    }
    let any = Held(iface);
    // The default comes with the newer interface; the older one's is the card's own (0).
    let (obj, default) = match any.query(adlx::power::IID1) {
        Some(p1) => {
            let d = int(p1.get(), adlx::power::GET_DEFAULT).unwrap_or(0);
            (p1, d)
        }
        None => (any.query(adlx::power::IID)?, 0),
    };
    let range = range(obj.get(), adlx::power::GET_RANGE)?;
    Some(Power {
        obj,
        range,
        default,
    })
}

impl AmdCard {
    fn set_power(&self, pct: i32) -> Result<(), String> {
        let Some(p) = &self.power else {
            return Ok(());
        };
        let v = (pct - 100 + p.default).clamp(p.range.min, p.range.max);
        // SAFETY: live power interface; `SetPowerLimit(int)`.
        ok(unsafe { method::<SetInt>(p.obj.get(), adlx::power::SET)(p.obj.get(), v) })
            .map_err(|e| format!("power limit: {e}"))
    }

    fn reset(&self) -> Result<(), String> {
        let (t, g) = (self.tuning.get(), self.gpu.get());
        // SAFETY: live tuning services and GPU; `ResetToFactory(IADLXGPU*)`.
        ok(unsafe { method::<ForGpu>(t, adlx::tuning::RESET_TO_FACTORY)(t, g) })
    }
}

impl TuneDevice for AmdCard {
    fn info(&self) -> GpuInfo {
        self.info.clone()
    }

    fn limits(&self) -> Limits {
        self.limits
    }

    fn read(&mut self) -> Result<Settings, String> {
        let power_pct = match &self.power {
            Some(p) if self.limits.power.is_some() => {
                let v = int(p.obj.get(), adlx::power::GET)
                    .ok_or("the driver did not give its power limit")?;
                100 + v - p.default
            }
            _ => 100,
        };
        Ok(Settings {
            core_mhz: match &self.core {
                Some(c) if self.limits.core.is_some() => c.read()?,
                _ => 0,
            },
            mem_mhz: match &self.mem {
                Some(c) if self.limits.mem.is_some() => c.read()?,
                _ => 0,
            },
            power_pct,
        })
    }

    fn clocks(&mut self) -> Clocks {
        let abs = |c: &Option<Clock>| {
            c.as_ref()
                .and_then(|c| int(c.obj.get(), c.get))
                .map(f64::from)
        };
        Clocks {
            core_max_mhz: abs(&self.core),
            mem_max_mhz: abs(&self.mem),
            power_limit_w: None,
        }
    }

    /// Stock is AMD's own reset. Otherwise, raising the power limit: it goes first, then the
    /// clocks; lowering it: the clocks first.
    fn apply(&mut self, s: &Settings) -> Result<(), String> {
        if s.is_stock() && self.reset().is_ok() {
            return Ok(());
        }
        let now = self.read().unwrap_or_default();
        let power_first = s.power_pct >= now.power_pct;
        if power_first && self.limits.power.is_some() && s.power_pct != now.power_pct {
            self.set_power(s.power_pct)?;
        }
        if let Some(c) = self
            .core
            .as_ref()
            .filter(|_| self.limits.core.is_some() && s.core_mhz != now.core_mhz)
        {
            c.write(s.core_mhz)
                .map_err(|e| format!("core clock: {e}"))?;
        }
        if let Some(c) = self
            .mem
            .as_ref()
            .filter(|_| self.limits.mem.is_some() && s.mem_mhz != now.mem_mhz)
        {
            c.write(s.mem_mhz)
                .map_err(|e| format!("memory clock: {e}"))?;
        }
        if !power_first && self.limits.power.is_some() && s.power_pct != now.power_pct {
            self.set_power(s.power_pct)?;
        }
        Ok(())
    }
}

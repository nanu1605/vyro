//! The makers' C interfaces GPU tuning calls (2.0), as their headers lay them out: AMD's ADLX (each
//! object a table of methods, called by their place in it) and Intel's control library (plain
//! functions and structures). Apart from the calls ([`super::amd`], [`super::intel`]), so the
//! layouts are checked on every platform. Every value comes from the makers' headers compiled for
//! 64-bit Windows: ADLX SDK 2.0.0.125 (`ISystem.h`, `ISystem2.h`, `IGPUTuning.h`,
//! `IGPUManual*Tuning.h`) and IGCL 1.1 (`igcl_api.h`), offsets read with `offsetof`.

/// AMD's ADLX.
pub mod adlx {
    pub const DLL: &str = "amdadlx64.dll";
    /// `ADLX_FULL_VERSION` of the headers these come from (2.0.0.125): the version Vyro asks for.
    pub const FULL_VERSION: u64 = (2 << 48) | 125;
    pub const OK: i32 = 0;
    pub const NOT_SUPPORTED: i32 = 12;
    /// `ADLX_GPU_TYPE`.
    pub const GPUTYPE_DISCRETE: i32 = 2;

    /// `ADLX_IntRange`.
    #[repr(C)]
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub struct IntRange {
        pub min: i32,
        pub max: i32,
        pub step: i32,
    }

    /// `IADLXInterface`: every object but the system begins with these.
    pub mod object {
        pub const RELEASE: usize = 1;
        pub const QUERY_INTERFACE: usize = 2;
    }
    /// `IADLXSystem` (not counted: it is never released).
    pub mod system {
        pub const GET_GPUS: usize = 1;
        pub const GET_GPU_TUNING_SERVICES: usize = 8;
    }
    /// `IADLXGPUList`.
    pub mod gpu_list {
        pub const SIZE: usize = 3;
        pub const AT_GPU_LIST: usize = 11;
    }
    /// `IADLXGPU`.
    pub mod gpu {
        pub const TYPE: usize = 5;
        pub const NAME: usize = 7;
        pub const DEVICE_ID: usize = 14;
        pub const UNIQUE_ID: usize = 18;
    }
    /// `IADLXGPU2` (drivers from 2024).
    pub mod gpu2 {
        pub const IID: &str = "IADLXGPU2";
        pub const AMD_SOFTWARE_VERSION: usize = 31;
    }
    /// `IADLXGPUTuningServices`.
    pub mod tuning {
        pub const IS_AT_FACTORY: usize = 4;
        pub const RESET_TO_FACTORY: usize = 5;
        pub const IS_SUPPORTED_MANUAL_GFX: usize = 8;
        pub const IS_SUPPORTED_MANUAL_VRAM: usize = 9;
        pub const IS_SUPPORTED_MANUAL_POWER: usize = 11;
        pub const GET_MANUAL_GFX: usize = 14;
        pub const GET_MANUAL_VRAM: usize = 15;
        pub const GET_MANUAL_POWER: usize = 17;
    }
    /// `IADLXManualGraphicsTuning2_1` (RX 6000 and later): the core's highest clock, absolute.
    pub mod gfx {
        pub const IID: &str = "IADLXManualGraphicsTuning2_1";
        pub const GET_MAX_RANGE: usize = 6;
        pub const GET_MAX: usize = 7;
        pub const SET_MAX: usize = 8;
        pub const GET_MAX_DEFAULT: usize = 13;
    }
    /// `IADLXManualVRAMTuning2_1`: the memory's highest clock, absolute.
    pub mod vram {
        pub const IID: &str = "IADLXManualVRAMTuning2_1";
        pub const GET_MAX_RANGE: usize = 7;
        pub const GET_MAX: usize = 8;
        pub const SET_MAX: usize = 9;
        pub const GET_MAX_DEFAULT: usize = 10;
    }
    /// `IADLXManualPowerTuning1` (with its default) or `IADLXManualPowerTuning`: the power limit,
    /// in % above or below the card's own.
    pub mod power {
        pub const IID1: &str = "IADLXManualPowerTuning1";
        pub const IID: &str = "IADLXManualPowerTuning";
        pub const GET_RANGE: usize = 3;
        pub const GET: usize = 4;
        pub const SET: usize = 5;
        /// `IADLXManualPowerTuning1` only.
        pub const GET_DEFAULT: usize = 10;
    }

    /// An interface name as `QueryInterface` takes it: UTF-16, ending in 0.
    pub fn iid(name: &str) -> Vec<u16> {
        name.encode_utf16().chain([0]).collect()
    }
}

/// Intel's control library (IGCL).
pub mod igcl {
    use std::ffi::c_void;

    pub const DLL: &str = "ControlLib.dll";
    /// `CTL_MAKE_VERSION(1, 1)`.
    pub const IMPL_VERSION: u32 = (1 << 16) | 1;
    pub const INIT_FLAG_USE_LEVEL_ZERO: u32 = 1;
    pub const SUCCESS: u32 = 0;
    pub const DEVICE_TYPE_GRAPHICS: u32 = 1;
    pub const ADAPTER_INTEGRATED: u32 = 1;
    /// `ctl_units_t`.
    pub const UNITS_MHZ: u32 = 0;
    pub const UNITS_WATTS: u32 = 4;
    pub const UNITS_MILLIWATTS: u32 = 10;
    pub const UNITS_PERCENT: u32 = 11;

    /// `ctl_init_args_t`.
    #[repr(C)]
    pub struct InitArgs {
        pub size: u32,
        pub version: u8,
        pub app_version: u32,
        pub flags: u32,
        pub supported_version: u32,
        pub application_uid: [u8; 16],
    }

    /// `ctl_device_adapter_properties_t`, version 2.
    #[repr(C)]
    pub struct DeviceProps {
        pub size: u32,
        pub version: u8,
        pub device_id: *mut c_void,
        pub device_id_size: u32,
        pub device_type: u32,
        pub supported_subfunction_flags: u32,
        pub driver_version: u64,
        pub firmware_version: [u64; 3],
        pub pci_vendor_id: u32,
        pub pci_device_id: u32,
        pub rev_id: u32,
        pub num_eus_per_sub_slice: u32,
        pub num_sub_slices_per_slice: u32,
        pub num_slices: u32,
        pub name: [u8; 100],
        pub graphics_adapter_properties: u32,
        pub frequency: u32,
        pub pci_subsys_id: u16,
        pub pci_subsys_vendor_id: u16,
        pub bdf: [u8; 3],
        pub num_xe_cores: u32,
        pub reserved: [u8; 108],
    }

    impl DeviceProps {
        /// Asking for version 2, with room for the adapter's LUID at `luid`.
        pub fn new(luid: *mut c_void, luid_size: u32) -> Self {
            Self {
                size: size_of::<Self>() as u32,
                version: 2,
                device_id: luid,
                device_id_size: luid_size,
                device_type: 0,
                supported_subfunction_flags: 0,
                driver_version: 0,
                firmware_version: [0; 3],
                pci_vendor_id: 0,
                pci_device_id: 0,
                rev_id: 0,
                num_eus_per_sub_slice: 0,
                num_sub_slices_per_slice: 0,
                num_slices: 0,
                name: [0; 100],
                graphics_adapter_properties: 0,
                frequency: 0,
                pci_subsys_id: 0,
                pci_subsys_vendor_id: 0,
                bdf: [0; 3],
                num_xe_cores: 0,
                reserved: [0; 108],
            }
        }

        pub fn name(&self) -> String {
            let end = self
                .name
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(self.name.len());
            String::from_utf8_lossy(&self.name[..end]).trim().to_owned()
        }

        /// "32.0.101.6913": the four 16-bit parts, highest first.
        pub fn driver(&self) -> String {
            let v = self.driver_version;
            format!(
                "{}.{}.{}.{}",
                v >> 48,
                (v >> 32) & 0xffff,
                (v >> 16) & 0xffff,
                v & 0xffff
            )
        }
    }

    /// `ctl_oc_control_info_t`. The flags are read as bytes: whatever the driver writes.
    #[repr(C)]
    #[derive(Debug, Default, Clone, Copy, PartialEq)]
    pub struct ControlInfo {
        pub supported: u8,
        pub relative: u8,
        pub has_reference: u8,
        pub units: u32,
        pub min: f64,
        pub max: f64,
        pub step: f64,
        pub default: f64,
        pub reference: f64,
    }

    /// `ctl_oc_properties_t`, version 1.
    #[repr(C)]
    #[derive(Debug, Default, Clone, Copy, PartialEq)]
    pub struct OcProperties {
        pub size: u32,
        pub version: u8,
        pub supported: u8,
        pub gpu_frequency_offset: ControlInfo,
        pub gpu_voltage_offset: ControlInfo,
        pub vram_frequency_offset: ControlInfo,
        pub vram_voltage_offset: ControlInfo,
        pub power_limit: ControlInfo,
        pub temperature_limit: ControlInfo,
        pub vram_mem_speed_limit: ControlInfo,
        pub vf_curve_voltage_limit: ControlInfo,
        pub vf_curve_frequency_limit: ControlInfo,
    }

    impl OcProperties {
        pub fn asked() -> Self {
            Self {
                size: size_of::<Self>() as u32,
                version: 1,
                ..Self::default()
            }
        }
    }

    /// Whole numbers inside `[min, max]`, a rounding error at either end let off (115 / 100 * 100
    /// is 114.999…).
    fn inside(min: f64, max: f64) -> crate::gpu_tune::Range {
        const SLACK: f64 = 1e-6;
        crate::gpu_tune::Range {
            min: (min - SLACK).ceil() as i32,
            max: (max + SLACK).floor() as i32,
        }
    }

    /// How the power limit is counted: the card's own, and in what.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct PowerScale {
        pub default: f64,
        pub units: u32,
    }

    /// The core offsets the library allows, in whole MHz, when they are MHz and include stock.
    pub fn core_range(p: &OcProperties) -> Option<crate::gpu_tune::Range> {
        let f = p.gpu_frequency_offset;
        if f.supported == 0 || f.units != UNITS_MHZ || f.max <= f.min {
            return None;
        }
        let r = inside(f.min - f.default, f.max - f.default);
        (r.max > 0 && r.min <= 0).then_some(r)
    }

    /// The power limits the library allows, as % of the card's own, when it gives its own: watts
    /// on the A-series, a percentage on the B-series.
    pub fn power_range(p: &OcProperties) -> Option<(crate::gpu_tune::Range, PowerScale)> {
        let l = p.power_limit;
        let known = matches!(l.units, UNITS_WATTS | UNITS_MILLIWATTS | UNITS_PERCENT);
        if l.supported == 0 || !known || l.default <= 0.0 || l.max <= l.min {
            return None;
        }
        let r = inside(l.min / l.default * 100.0, l.max / l.default * 100.0);
        (r.min < r.max && r.contains(100)).then_some((
            r,
            PowerScale {
                default: l.default,
                units: l.units,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::mem::offset_of;

    use super::*;

    /// The offsets `offsetof` gives for the headers, compiled for 64-bit Windows.
    #[test]
    fn igcl_structures_are_laid_out_as_its_header() {
        use igcl::*;
        assert_eq!(size_of::<InitArgs>(), 36);
        assert_eq!(offset_of!(InitArgs, app_version), 8);
        assert_eq!(offset_of!(InitArgs, flags), 12);
        assert_eq!(offset_of!(InitArgs, supported_version), 16);
        assert_eq!(offset_of!(InitArgs, application_uid), 20);

        assert_eq!(size_of::<DeviceProps>(), 320);
        assert_eq!(offset_of!(DeviceProps, device_id), 8);
        assert_eq!(offset_of!(DeviceProps, device_id_size), 16);
        assert_eq!(offset_of!(DeviceProps, device_type), 20);
        assert_eq!(offset_of!(DeviceProps, pci_vendor_id), 64);
        assert_eq!(offset_of!(DeviceProps, pci_device_id), 68);
        assert_eq!(offset_of!(DeviceProps, name), 88);
        assert_eq!(offset_of!(DeviceProps, graphics_adapter_properties), 188);
        // As the sensors' reading of the same structure has it.
        assert_eq!(
            size_of::<DeviceProps>(),
            crate::usage::vendor::CTL_DEVICE_PROPS_SIZE
        );
        assert_eq!(
            offset_of!(DeviceProps, pci_vendor_id),
            crate::usage::vendor::CTL_DEVICE_PROPS_VENDOR
        );

        assert_eq!(size_of::<ControlInfo>(), 48);
        assert_eq!(offset_of!(ControlInfo, units), 4);
        assert_eq!(offset_of!(ControlInfo, min), 8);
        assert_eq!(offset_of!(ControlInfo, default), 32);
        assert_eq!(offset_of!(ControlInfo, reference), 40);

        assert_eq!(size_of::<OcProperties>(), 440);
        assert_eq!(offset_of!(OcProperties, supported), 5);
        assert_eq!(offset_of!(OcProperties, gpu_frequency_offset), 8);
        assert_eq!(offset_of!(OcProperties, power_limit), 200);
        assert_eq!(offset_of!(OcProperties, vram_mem_speed_limit), 296);
        assert_eq!(offset_of!(OcProperties, vf_curve_frequency_limit), 392);
    }

    #[test]
    fn adlx_asks_for_its_headers_version_by_name() {
        assert_eq!(adlx::FULL_VERSION, 562_949_953_421_437);
        assert_eq!(size_of::<adlx::IntRange>(), 12);
        assert_eq!(
            adlx::iid("IADLXGPU2"),
            [73, 65, 68, 76, 88, 71, 80, 85, 50, 0]
        );
    }

    fn info(supported: bool, units: u32, min: f64, max: f64, default: f64) -> igcl::ControlInfo {
        igcl::ControlInfo {
            supported: u8::from(supported),
            units,
            min,
            max,
            default,
            ..igcl::ControlInfo::default()
        }
    }

    #[test]
    fn intel_ranges_from_what_the_library_gives() {
        use crate::gpu_tune::Range;
        use igcl::*;
        let mut p = OcProperties::asked();
        p.gpu_frequency_offset = info(true, UNITS_MHZ, 0.0, 250.5, 0.0);
        assert_eq!(core_range(&p), Some(Range { min: 0, max: 250 }));
        p.gpu_frequency_offset.units = UNITS_PERCENT;
        assert_eq!(core_range(&p), None, "not MHz");
        // The A-series: watts.
        p.power_limit = info(true, UNITS_WATTS, 95.0, 228.0, 190.0);
        let (r, s) = power_range(&p).unwrap();
        assert_eq!(r, Range { min: 50, max: 120 });
        assert_eq!(s.default, 190.0);
        // The B-series: percent.
        p.power_limit = info(true, UNITS_PERCENT, 75.0, 115.0, 100.0);
        assert_eq!(power_range(&p).unwrap().0, Range { min: 75, max: 115 });
        p.power_limit.default = 0.0;
        assert!(power_range(&p).is_none(), "no default, no percentage");
        p.power_limit = info(false, UNITS_WATTS, 95.0, 228.0, 190.0);
        assert!(power_range(&p).is_none());
    }

    #[test]
    fn intel_names_and_versions_read() {
        let mut p = igcl::DeviceProps::new(std::ptr::null_mut(), 0);
        p.name[..30].copy_from_slice(b"Intel(R) Arc(TM) A770 Graphics");
        assert_eq!(p.name(), "Intel(R) Arc(TM) A770 Graphics");
        p.driver_version = (32 << 48) | (101 << 16) | 6913;
        assert_eq!(p.driver(), "32.0.101.6913");
    }
}

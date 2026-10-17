//! Windows side of the device list: device containers and nodes from the device query API,
//! Bluetooth LE Battery Service reads, XInput and Core Audio endpoints. Nothing here needs
//! administrator rights. Bluetooth and XInput are loaded at run time, so a PC without them
//! still starts.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::{Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Devices::Bluetooth::{
    BLUETOOTH_GATT_FLAG_NONE, BTH_LE_GATT_CHARACTERISTIC, BTH_LE_GATT_CHARACTERISTIC_VALUE,
    BTH_LE_GATT_SERVICE, BTH_LE_UUID,
};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_PropertyW, CM_Register_Notification,
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_NOTIFY_ACTION, CM_NOTIFY_EVENT_DATA, CM_NOTIFY_FILTER,
    CM_NOTIFY_FILTER_FLAG_ALL_INTERFACE_CLASSES, CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE, CR_SUCCESS,
    HCMNOTIFICATION,
};
use windows::Win32::Devices::DeviceQuery::{
    DevCreateObjectQuery, DevFreeObjects, DevGetObjects, DevObjectTypeAEP, DevObjectTypeDevice,
    DevObjectTypeDeviceContainer, DevQueryFlagLocalize, DevQueryResultStateChange,
    DevQueryStateEnumCompleted, DEVPROP_FILTER_EXPRESSION, DEVPROP_OPERATOR_EQUALS, DEV_OBJECT,
    DEV_OBJECT_TYPE, DEV_QUERY_RESULT_ACTION_DATA, HDEVQUERY,
};
use windows::Win32::Devices::FunctionDiscovery::{
    PKEY_Device_ContainerId, PKEY_Device_FriendlyName,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_DeviceContainer_Category, DEVPKEY_DeviceContainer_FriendlyName,
    DEVPKEY_DeviceContainer_IsConnected, DEVPKEY_DeviceContainer_IsLocalMachine,
    DEVPKEY_DeviceContainer_IsNetworkDevice, DEVPKEY_DeviceContainer_IsNotInterestingForDisplay,
    DEVPKEY_DeviceContainer_Manufacturer, DEVPKEY_Device_BusReportedDeviceDesc,
    DEVPKEY_Device_Class, DEVPKEY_Device_CompatibleIds, DEVPKEY_Device_ContainerId,
    DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_EnumeratorName, DEVPKEY_Device_FriendlyName,
    DEVPKEY_Device_HardwareIds, DEVPKEY_Device_InstanceId, DEVPKEY_Device_IsPresent,
    DEVPKEY_Device_ProblemCode, DEVPKEY_NAME, DEVPROPCOMPKEY, DEVPROPERTY, DEVPROPTYPE,
    DEVPROP_STORE_SYSTEM, DEVPROP_TRUE, DEVPROP_TYPE_BOOLEAN, DEVPROP_TYPE_BYTE, DEVPROP_TYPE_GUID,
    DEVPROP_TYPE_STRING, DEVPROP_TYPE_STRING_INDIRECT, DEVPROP_TYPE_STRING_LIST,
    DEVPROP_TYPE_UINT16, DEVPROP_TYPE_UINT32,
};
use windows::Win32::Foundation::{CloseHandle, DEVPROPKEY, GENERIC_READ, HANDLE, PROPERTYKEY};
use windows::Win32::Media::Audio::{
    eAll, eCapture, eConsole, eRender, EDataFlow, IMMDevice, IMMDeviceEnumerator, IMMEndpoint,
    MMDeviceEnumerator, PKEY_AudioEndpoint_FormFactor, DEVICE_STATE, DEVICE_STATE_ACTIVE,
    DEVICE_STATE_UNPLUGGED,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PROPVARIANT, PROPVARIANT_0_0,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Variant::{VT_CLSID, VT_LPWSTR, VT_UI4};
use windows::Win32::UI::Input::XboxController::{
    XINPUT_BATTERY_INFORMATION, XINPUT_CAPABILITIES, XINPUT_CAPS_WIRELESS,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

use super::{
    build, gatt_wanted, norm_guid, plain_text, Devices, Raw, RawAep, RawContainer, RawEndpoint,
    RawNode, RawPad,
};
use crate::live::dll::Library;

/// Windows' Bluetooth battery reading on a device node (what Settings shows), a `BYTE`.
const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104e_a319_6ee2_4701_bd47_8ddb_f425_bbe5),
    pid: 2,
};
/// System.Devices.Aep.ContainerId, .IsPaired and .ProtocolId: a paired entry's device, and
/// whether it is Bluetooth.
const AEP_CONTAINER: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0xe7c3_fb29_caa7_4f47_8c8b_be59_b330_d4c5),
    pid: 2,
};
const AEP_PAIRED: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0xa359_96ab_11cf_4935_8b61_a676_1081_ecdf),
    pid: 16,
};
const AEP_PROTOCOL: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x3b2c_e006_5e61_4fde_bab8_9b8a_ac9b_26df),
    pid: 5,
};
/// Bluetooth and Bluetooth LE association protocols.
const AEP_BLUETOOTH: [GUID; 2] = [
    GUID::from_u128(0xe0cb_f06c_cd8b_4647_bb8a_263b_43f0_f974),
    GUID::from_u128(0xbb7b_b05e_5972_42b5_94fc_76ea_a708_4d49),
];
/// The Battery Service: Windows gives each GATT service an interface class of its UUID.
const BATTERY_SERVICE: GUID = GUID::from_u128(0x0000_180f_0000_1000_8000_0080_5f9b_34fb);
const BATTERY_LEVEL: u16 = 0x2A19;
const BATTERY_LEVEL_LONG: GUID = GUID::from_u128(0x0000_2a19_0000_1000_8000_0080_5f9b_34fb);
/// How long a Battery Service reading is reused.
const GATT_FRESH: Duration = Duration::from_secs(55);
/// Battery Service reads stop after this much time in one collection.
const GATT_BUDGET: Duration = Duration::from_secs(3);

/// The current list. Blocking (well under 100 ms, more when a Battery Service read has to
/// reach the device); call it off the UI thread.
pub fn collect() -> Devices {
    let containers = containers();
    let (nodes, instances) = nodes();
    let mut raw = Raw {
        containers,
        nodes,
        aeps: aeps(),
        ..Raw::default()
    };
    raw.gatt = gatt_batteries(&gatt_wanted(&raw), &instances);
    raw.hid = super::hid_win::batteries(&instances);
    raw.pads = pads();
    raw.endpoints = endpoints(&instances);
    build(&raw)
}

// ---------------------------------------------------------------------------------------------
// Device query
// ---------------------------------------------------------------------------------------------

fn comp(key: DEVPROPKEY) -> DEVPROPCOMPKEY {
    DEVPROPCOMPKEY {
        Key: key,
        Store: DEVPROP_STORE_SYSTEM,
        LocaleName: PCWSTR::null(),
    }
}

fn same(a: &DEVPROPKEY, b: &DEVPROPKEY) -> bool {
    a.fmtid == b.fmtid && a.pid == b.pid
}

/// One object's properties as the query returned them.
struct Props<'a>(&'a [DEVPROPERTY]);

impl Props<'_> {
    fn raw(&self, key: &DEVPROPKEY, ty: DEVPROPTYPE) -> Option<&[u8]> {
        let p = self.0.iter().find(|p| same(&p.CompKey.Key, key))?;
        if p.Type != ty || p.Buffer.is_null() || p.BufferSize == 0 {
            return None;
        }
        // SAFETY: the query owns `BufferSize` bytes at `Buffer` until the objects are freed,
        // which happens after this borrow ends.
        Some(unsafe { std::slice::from_raw_parts(p.Buffer.cast::<u8>(), p.BufferSize as usize) })
    }

    fn wide(&self, key: &DEVPROPKEY, ty: DEVPROPTYPE) -> Option<Vec<u16>> {
        let b = self.raw(key, ty)?;
        Some(
            b.as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect(),
        )
    }

    /// Text, and names Windows keeps as indirect strings (resolved by the localized query, or
    /// their fallback text).
    fn string(&self, key: &DEVPROPKEY) -> Option<String> {
        let w = self
            .wide(key, DEVPROP_TYPE_STRING)
            .or_else(|| self.wide(key, DEVPROP_TYPE_STRING_INDIRECT))?;
        plain_text(&String::from_utf16_lossy(
            w.split(|c| *c == 0).next().unwrap_or(&[]),
        ))
    }

    fn strings(&self, key: &DEVPROPKEY) -> Vec<String> {
        self.wide(key, DEVPROP_TYPE_STRING_LIST)
            .map(|w| {
                w.split(|c| *c == 0)
                    .filter(|s| !s.is_empty())
                    .map(String::from_utf16_lossy)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn bool(&self, key: &DEVPROPKEY) -> Option<bool> {
        self.raw(key, DEVPROP_TYPE_BOOLEAN).map(|b| b[0] != 0)
    }

    fn u32(&self, key: &DEVPROPKEY) -> Option<u32> {
        let b = self.raw(key, DEVPROP_TYPE_UINT32)?;
        Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
    }

    /// A small number stored as a byte, or a wider integer.
    fn small(&self, key: &DEVPROPKEY) -> Option<u8> {
        if let Some(b) = self.raw(key, DEVPROP_TYPE_BYTE) {
            return Some(b[0]);
        }
        let wide = self
            .raw(key, DEVPROP_TYPE_UINT16)
            .and_then(|b| Some(u32::from(u16::from_le_bytes(b.get(..2)?.try_into().ok()?))))
            .or_else(|| self.u32(key))?;
        u8::try_from(wide).ok()
    }

    fn guid(&self, key: &DEVPROPKEY) -> Option<String> {
        let b = self.raw(key, DEVPROP_TYPE_GUID)?;
        let b: [u8; 16] = b.get(..16)?.try_into().ok()?;
        let g = GUID::from_values(
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u16::from_le_bytes([b[4], b[5]]),
            u16::from_le_bytes([b[6], b[7]]),
            [b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]],
        );
        Some(format!("{g:?}"))
    }
}

fn wide_str(p: PCWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: a NUL-terminated string owned by the query result.
    unsafe { p.to_string() }.unwrap_or_default()
}

/// Runs one device query; `false` if it failed.
fn query(
    kind: DEV_OBJECT_TYPE,
    keys: &[DEVPROPKEY],
    filter: Option<&[DEVPROP_FILTER_EXPRESSION]>,
    mut each: impl FnMut(&str, &Props),
) -> bool {
    let wanted: Vec<DEVPROPCOMPKEY> = keys.iter().copied().map(comp).collect();
    let mut count = 0u32;
    let mut objects: *mut DEV_OBJECT = std::ptr::null_mut();
    // SAFETY: valid key and filter arrays; the result is freed below.
    if let Err(e) = unsafe {
        DevGetObjects(
            kind,
            DevQueryFlagLocalize.0.unsigned_abs(),
            Some(&wanted),
            filter,
            &mut count,
            &mut objects,
        )
    } {
        log::debug!("devices: query {} failed: {e}", kind.0);
        return false;
    }
    if objects.is_null() {
        return true;
    }
    // SAFETY: the query returned `count` objects at `objects`.
    let list = unsafe { std::slice::from_raw_parts(objects, count as usize) };
    for o in list {
        let props: &[DEVPROPERTY] = if o.pProperties.is_null() {
            &[]
        } else {
            // SAFETY: `cPropertyCount` properties at `pProperties`, alive until freed.
            unsafe { std::slice::from_raw_parts(o.pProperties, o.cPropertyCount as usize) }
        };
        each(&wide_str(o.pszObjectId), &Props(props));
    }
    // SAFETY: the array DevGetObjects returned, freed once.
    unsafe { DevFreeObjects(list) };
    true
}

fn containers() -> Vec<RawContainer> {
    let keys = [
        DEVPKEY_DeviceContainer_FriendlyName,
        DEVPKEY_DeviceContainer_Manufacturer,
        DEVPKEY_DeviceContainer_IsConnected,
        DEVPKEY_DeviceContainer_IsLocalMachine,
        DEVPKEY_DeviceContainer_IsNetworkDevice,
        DEVPKEY_DeviceContainer_IsNotInterestingForDisplay,
        DEVPKEY_DeviceContainer_Category,
        DEVPKEY_NAME,
        DEVPKEY_BLUETOOTH_BATTERY,
    ];
    let mut out = Vec::new();
    query(DevObjectTypeDeviceContainer, &keys, None, |id, p| {
        out.push(RawContainer {
            id: id.to_owned(),
            name: p
                .string(&DEVPKEY_DeviceContainer_FriendlyName)
                .or_else(|| p.string(&DEVPKEY_NAME)),
            manufacturer: p.string(&DEVPKEY_DeviceContainer_Manufacturer),
            connected: p.bool(&DEVPKEY_DeviceContainer_IsConnected),
            local_machine: p.bool(&DEVPKEY_DeviceContainer_IsLocalMachine) == Some(true),
            network: p.bool(&DEVPKEY_DeviceContainer_IsNetworkDevice) == Some(true),
            hidden: p.bool(&DEVPKEY_DeviceContainer_IsNotInterestingForDisplay) == Some(true),
            categories: p.strings(&DEVPKEY_DeviceContainer_Category),
            battery: p.small(&DEVPKEY_BLUETOOTH_BATTERY),
        });
    });
    out
}

/// Paired Bluetooth devices as Settings lists them: their names, also while they are off.
fn aeps() -> Vec<RawAep> {
    let keys = [
        DEVPKEY_NAME,
        AEP_CONTAINER,
        AEP_PAIRED,
        AEP_PROTOCOL,
        DEVPKEY_BLUETOOTH_BATTERY,
    ];
    let paired = DEVPROP_TRUE;
    let mut out = Vec::new();
    // One query per protocol, filtered as Windows' own Bluetooth selectors are: an unfiltered
    // endpoint query would also search the network.
    for protocol in AEP_BLUETOOTH {
        let filter = [
            DEVPROP_FILTER_EXPRESSION {
                Operator: DEVPROP_OPERATOR_EQUALS,
                Property: DEVPROPERTY {
                    CompKey: comp(AEP_PROTOCOL),
                    Type: DEVPROP_TYPE_GUID,
                    BufferSize: u32::try_from(std::mem::size_of::<GUID>()).unwrap_or(16),
                    Buffer: std::ptr::from_ref(&protocol).cast_mut().cast::<c_void>(),
                },
            },
            DEVPROP_FILTER_EXPRESSION {
                Operator: DEVPROP_OPERATOR_EQUALS,
                Property: DEVPROPERTY {
                    CompKey: comp(AEP_PAIRED),
                    Type: DEVPROP_TYPE_BOOLEAN,
                    BufferSize: 1,
                    Buffer: std::ptr::from_ref(&paired).cast_mut().cast::<c_void>(),
                },
            },
        ];
        query(DevObjectTypeAEP, &keys, Some(&filter), |_, p| {
            if p.bool(&AEP_PAIRED) == Some(false) {
                return;
            }
            out.push(RawAep {
                container: p.guid(&AEP_CONTAINER),
                name: p.string(&DEVPKEY_NAME),
                battery: p.small(&DEVPKEY_BLUETOOTH_BATTERY),
            });
        });
    }
    out
}

/// Present device nodes, and each one's container by instance id (uppercase), which stays here.
pub(super) fn nodes() -> (Vec<RawNode>, HashMap<String, String>) {
    let keys = [
        DEVPKEY_Device_ContainerId,
        DEVPKEY_Device_Class,
        DEVPKEY_Device_EnumeratorName,
        DEVPKEY_Device_FriendlyName,
        DEVPKEY_Device_DeviceDesc,
        DEVPKEY_NAME,
        DEVPKEY_Device_BusReportedDeviceDesc,
        DEVPKEY_Device_HardwareIds,
        DEVPKEY_Device_CompatibleIds,
        DEVPKEY_Device_ProblemCode,
        DEVPKEY_Device_IsPresent,
        DEVPKEY_BLUETOOTH_BATTERY,
    ];
    let present = DEVPROP_TRUE;
    let filter = [DEVPROP_FILTER_EXPRESSION {
        Operator: DEVPROP_OPERATOR_EQUALS,
        Property: DEVPROPERTY {
            CompKey: comp(DEVPKEY_Device_IsPresent),
            Type: DEVPROP_TYPE_BOOLEAN,
            BufferSize: 1,
            Buffer: std::ptr::from_ref(&present).cast_mut().cast::<c_void>(),
        },
    }];
    let run = |filter: Option<&[DEVPROP_FILTER_EXPRESSION]>| {
        let mut nodes = Vec::new();
        let mut instances = HashMap::new();
        let ok = query(DevObjectTypeDevice, &keys, filter, |id, p| {
            if p.bool(&DEVPKEY_Device_IsPresent) == Some(false) {
                return;
            }
            let container = p.guid(&DEVPKEY_Device_ContainerId);
            if let Some(c) = &container {
                instances.insert(id.to_ascii_uppercase(), c.clone());
            }
            nodes.push(RawNode {
                container,
                class: p.string(&DEVPKEY_Device_Class),
                enumerator: p.string(&DEVPKEY_Device_EnumeratorName),
                name: p
                    .string(&DEVPKEY_Device_FriendlyName)
                    .or_else(|| p.string(&DEVPKEY_Device_DeviceDesc))
                    .or_else(|| p.string(&DEVPKEY_NAME)),
                bus_name: p.string(&DEVPKEY_Device_BusReportedDeviceDesc),
                hardware_ids: p.strings(&DEVPKEY_Device_HardwareIds),
                compatible_ids: p.strings(&DEVPKEY_Device_CompatibleIds),
                problem: p.u32(&DEVPKEY_Device_ProblemCode),
                battery: p.small(&DEVPKEY_BLUETOOTH_BATTERY),
            });
        });
        (ok, nodes, instances)
    };
    // A PC always has present devices: nothing back means the filter was not understood.
    let (ok, nodes, instances) = run(Some(&filter));
    if ok && !nodes.is_empty() {
        return (nodes, instances);
    }
    log::info!("devices: present-only query returned nothing; reading all nodes");
    let (_, nodes, instances) = run(None);
    (nodes, instances)
}

// ---------------------------------------------------------------------------------------------
// Bluetooth LE Battery Service
// ---------------------------------------------------------------------------------------------

type GetCharacteristics = unsafe extern "system" fn(
    HANDLE,
    *const BTH_LE_GATT_SERVICE,
    u16,
    *mut BTH_LE_GATT_CHARACTERISTIC,
    *mut u16,
    u32,
) -> HRESULT;
type GetCharacteristicValue = unsafe extern "system" fn(
    HANDLE,
    *const BTH_LE_GATT_CHARACTERISTIC,
    u32,
    *mut BTH_LE_GATT_CHARACTERISTIC_VALUE,
    *mut u16,
    u32,
) -> HRESULT;

#[derive(Clone, Copy)]
struct Gatt {
    characteristics: GetCharacteristics,
    value: GetCharacteristicValue,
}

impl Gatt {
    fn get() -> Option<Self> {
        static API: OnceLock<Option<Gatt>> = OnceLock::new();
        *API.get_or_init(|| {
            let lib = Library::system32("bluetoothapis.dll")?;
            // SAFETY: the types match the documented BluetoothApis signatures.
            unsafe {
                Some(Self {
                    characteristics: lib.get(c"BluetoothGATTGetCharacteristics")?,
                    value: lib.get(c"BluetoothGATTGetCharacteristicValue")?,
                })
            }
        })
    }

    /// The Battery Level (0x2A19) of the service at `path`.
    fn read(self, path: &str) -> Option<u8> {
        // SAFETY: plain open of a device interface path; closed by the guard.
        let handle = unsafe {
            CreateFileW(
                &HSTRING::from(path),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .ok()?;
        let _close = Closer(handle);
        let mut chars = [BTH_LE_GATT_CHARACTERISTIC::default(); 8];
        let mut n = 0u16;
        // SAFETY: a service handle (the parent service is implied), an 8-entry buffer.
        unsafe {
            (self.characteristics)(
                handle,
                std::ptr::null(),
                8,
                chars.as_mut_ptr(),
                &mut n,
                BLUETOOTH_GATT_FLAG_NONE,
            )
        }
        .ok()
        .ok()?;
        let ch = chars
            .iter()
            .take(usize::from(n))
            .find(|c| c.IsReadable && is_battery_level(&c.CharacteristicUuid))?;
        // The value: a u32 size, then the bytes. 32 bytes, u32-aligned.
        let mut buf = [0u32; 8];
        let mut need = 0u16;
        // SAFETY: `buf` is 32 writable bytes, aligned for the value header.
        unsafe {
            (self.value)(
                handle,
                ch,
                32,
                buf.as_mut_ptr().cast(),
                &mut need,
                BLUETOOTH_GATT_FLAG_NONE,
            )
        }
        .ok()
        .ok()?;
        let size = buf[0];
        let level = buf[1].to_le_bytes()[0];
        (size >= 1 && level <= 100).then_some(level)
    }
}

fn is_battery_level(u: &BTH_LE_UUID) -> bool {
    // SAFETY: the union member read is the one `IsShortUuid` selects.
    unsafe {
        if u.IsShortUuid {
            u.Value.ShortUuid == BATTERY_LEVEL
        } else {
            u.Value.LongUuid == BATTERY_LEVEL_LONG
        }
    }
}

struct Closer(HANDLE);

impl Drop for Closer {
    fn drop(&mut self) {
        // SAFETY: a handle this module opened, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Present interfaces of a class.
pub(super) fn interfaces(class: &GUID) -> Vec<String> {
    for _ in 0..3 {
        let mut len = 0u32;
        // SAFETY: size query for the whole system's present interfaces of `class`.
        let cr = unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut len,
                class,
                PCWSTR::null(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if cr != CR_SUCCESS || len <= 1 {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        // SAFETY: `buf` holds `len` characters, as just asked for.
        let cr = unsafe {
            CM_Get_Device_Interface_ListW(
                class,
                PCWSTR::null(),
                &mut buf,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if cr == CR_SUCCESS {
            return buf
                .split(|c| *c == 0)
                .filter(|s| !s.is_empty())
                .map(String::from_utf16_lossy)
                .collect();
        }
        // The list grew between the two calls: ask again.
    }
    Vec::new()
}

/// The device instance an interface belongs to.
pub(super) fn interface_instance(path: &str) -> Option<String> {
    let mut ty = DEVPROPTYPE::default();
    let mut buf = [0u16; 512];
    let mut size = u32::try_from(std::mem::size_of_val(&buf)).ok()?;
    // SAFETY: `buf` is `size` writable bytes.
    let cr = unsafe {
        CM_Get_Device_Interface_PropertyW(
            &HSTRING::from(path),
            &DEVPKEY_Device_InstanceId,
            &mut ty,
            Some(buf.as_mut_ptr().cast()),
            &mut size,
            0,
        )
    };
    if cr != CR_SUCCESS || ty != DEVPROP_TYPE_STRING {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).to_ascii_uppercase())
}

/// Battery Service readings for the wanted containers, each read at most once a minute.
fn gatt_batteries(wanted: &[String], instances: &HashMap<String, String>) -> Vec<(String, u8)> {
    /// Container → when read, and what.
    type Cache = Mutex<HashMap<String, (Instant, Option<u8>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    if wanted.is_empty() {
        return Vec::new();
    }
    let Some(gatt) = Gatt::get() else {
        return Vec::new();
    };
    let wanted: HashSet<&str> = wanted.iter().map(String::as_str).collect();
    let mut cache = crate::lock(CACHE.get_or_init(Mutex::default));
    let start = Instant::now();
    let mut out = Vec::new();
    let mut done = HashSet::new();
    for path in interfaces(&BATTERY_SERVICE) {
        let Some(container) = interface_instance(&path)
            .and_then(|i| instances.get(&i))
            .map(|c| norm_guid(c))
        else {
            continue;
        };
        if !wanted.contains(container.as_str()) || !done.insert(container.clone()) {
            continue;
        }
        let level = match cache.get(&container) {
            Some((at, v)) if at.elapsed() < GATT_FRESH => *v,
            _ if start.elapsed() > GATT_BUDGET => continue,
            _ => {
                let v = gatt.read(&path);
                cache.insert(container.clone(), (Instant::now(), v));
                v
            }
        };
        if let Some(v) = level {
            out.push((container, v));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// XInput
// ---------------------------------------------------------------------------------------------

type GetCapabilities = unsafe extern "system" fn(u32, u32, *mut XINPUT_CAPABILITIES) -> u32;
type GetBattery = unsafe extern "system" fn(u32, u8, *mut XINPUT_BATTERY_INFORMATION) -> u32;
type GetCapabilitiesEx = unsafe extern "system" fn(u32, u32, u32, *mut CapabilitiesEx) -> u32;

/// XInput 1.4's capabilities with the controller's vendor and product, from the export it has
/// by number only (108), as SDL reads it.
#[repr(C)]
#[derive(Default)]
struct CapabilitiesEx {
    caps: XINPUT_CAPABILITIES,
    vendor: u16,
    product: u16,
    version: u16,
    unknown1: u16,
    unknown2: u32,
}

struct XInput {
    caps: GetCapabilities,
    battery: GetBattery,
    caps_ex: Option<GetCapabilitiesEx>,
}

fn pads() -> Vec<RawPad> {
    static API: OnceLock<Option<XInput>> = OnceLock::new();
    let Some(api) = API.get_or_init(|| {
        let lib = Library::system32("xinput1_4.dll")?;
        // SAFETY: the types match the documented XInput 1.4 signatures, and ordinal 108's as
        // SDL uses it.
        unsafe {
            Some(XInput {
                caps: lib.get(c"XInputGetCapabilities")?,
                battery: lib.get(c"XInputGetBatteryInformation")?,
                caps_ex: lib.ordinal(108),
            })
        }
    }) else {
        return Vec::new();
    };
    (0..4)
        .filter_map(|slot| {
            let mut c = XINPUT_CAPABILITIES::default();
            // SAFETY: valid slot and out pointer; non-zero means nothing in the slot.
            if unsafe { (api.caps)(slot, 0, &mut c) } != 0 {
                return None;
            }
            let ids = api.caps_ex.and_then(|ex| {
                let mut x = CapabilitiesEx::default();
                // SAFETY: valid slot and out pointer; the first argument must be 1.
                (unsafe { ex(1, slot, 0, &mut x) } == 0 && x.vendor != 0)
                    .then_some((x.vendor, x.product))
            });
            let mut b = XINPUT_BATTERY_INFORMATION::default();
            // SAFETY: as above; device type 0 is the gamepad itself.
            let ok = unsafe { (api.battery)(slot, 0, &mut b) } == 0;
            Some(RawPad {
                slot,
                ids,
                wireless: c.Flags.contains(XINPUT_CAPS_WIRELESS),
                battery_type: if ok { b.BatteryType.0 } else { 0xFF },
                battery_level: b.BatteryLevel.0,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Core Audio
// ---------------------------------------------------------------------------------------------

fn endpoint_id(d: &IMMDevice) -> Option<String> {
    // SAFETY: COM call; the returned string is ours to free.
    let p = unsafe { d.GetId() }.ok()?;
    // SAFETY: a NUL-terminated string from GetId, freed right after.
    let s = unsafe { p.to_string() }.ok();
    // SAFETY: allocated by COM for us.
    unsafe { CoTaskMemFree(Some(p.0.cast_const().cast())) };
    s
}

fn prop<T>(
    store: &IPropertyStore,
    key: &PROPERTYKEY,
    read: impl FnOnce(&PROPVARIANT_0_0) -> Option<T>,
) -> Option<T> {
    // SAFETY: COM call; the variant is cleared below.
    let mut v: PROPVARIANT = unsafe { store.GetValue(key) }.ok()?;
    // SAFETY: reading the variant's header and the member its type selects (in `read`).
    let out = read(unsafe { &v.Anonymous.Anonymous });
    // SAFETY: frees what GetValue allocated.
    let _ = unsafe { PropVariantClear(&mut v) };
    out
}

fn endpoints(instances: &HashMap<String, String>) -> Vec<RawEndpoint> {
    // SAFETY: COM for this thread (the multithreaded apartment, as the other COM readers).
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // SAFETY: standard Core Audio entry point.
    let Ok(en) = (unsafe {
        CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }) else {
        return Vec::new();
    };
    let default = |flow: EDataFlow| {
        // SAFETY: COM call.
        unsafe { en.GetDefaultAudioEndpoint(flow, eConsole) }
            .ok()
            .and_then(|d| endpoint_id(&d))
    };
    let defaults = [default(eRender), default(eCapture)];
    let states = DEVICE_STATE(DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_UNPLUGGED.0);
    // SAFETY: COM call.
    let Ok(list) = (unsafe { en.EnumAudioEndpoints(eAll, states) }) else {
        return Vec::new();
    };
    // SAFETY: COM call.
    let count = unsafe { list.GetCount() }.unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..count {
        // SAFETY: COM call with an index below the count.
        let Ok(dev) = (unsafe { list.Item(i) }) else {
            continue;
        };
        let Some(id) = endpoint_id(&dev) else {
            continue;
        };
        // SAFETY: COM calls.
        let state = unsafe { dev.GetState() }.map_or(0, |s| s.0);
        let output = dev
            .cast::<IMMEndpoint>()
            .ok()
            // SAFETY: COM call.
            .and_then(|e| unsafe { e.GetDataFlow() }.ok())
            == Some(eRender);
        // SAFETY: COM call.
        let store = unsafe { dev.OpenPropertyStore(STGM_READ) }.ok();
        let name = store.as_ref().and_then(|s| {
            prop(s, &PKEY_Device_FriendlyName, |v| {
                // SAFETY: `pwszVal` is the member for VT_LPWSTR.
                (v.vt == VT_LPWSTR)
                    .then(|| unsafe { v.Anonymous.pwszVal.to_string() }.ok())
                    .flatten()
            })
        });
        let form_factor = store.as_ref().and_then(|s| {
            // SAFETY: any bits are a valid u32; it is used only for VT_UI4.
            prop(s, &PKEY_AudioEndpoint_FormFactor, |v| {
                (v.vt == VT_UI4).then_some(unsafe { v.Anonymous.ulVal })
            })
        });
        let container = store
            .as_ref()
            .and_then(|s| {
                prop(s, &PKEY_Device_ContainerId, |v| {
                    // SAFETY: `puuid` is the member for VT_CLSID; checked for null.
                    let g = unsafe { v.Anonymous.puuid.as_ref() };
                    (v.vt == VT_CLSID)
                        .then(|| g.map(|g| format!("{g:?}")))
                        .flatten()
                })
            })
            .or_else(|| {
                instances
                    .get(&format!("SWD\\MMDEVAPI\\{id}").to_ascii_uppercase())
                    .cloned()
            });
        out.push(RawEndpoint {
            default: defaults.iter().any(|d| d.as_deref() == Some(id.as_str())),
            name: name.unwrap_or_else(|| "Audio device".into()),
            id,
            output,
            state,
            form_factor,
            container,
        });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Hot add / remove
// ---------------------------------------------------------------------------------------------

static POKE: OnceLock<mpsc::SyncSender<()>> = OnceLock::new();
/// The container query has finished its first listing; later results are changes.
static LISTED: AtomicBool = AtomicBool::new(false);

fn poke() {
    if let Some(tx) = POKE.get() {
        // Full means a change is already waiting: nothing to add.
        let _ = tx.try_send(());
    }
}

unsafe extern "system" fn on_interface(
    _: HCMNOTIFICATION,
    _: *const c_void,
    _: CM_NOTIFY_ACTION,
    _: *const CM_NOTIFY_EVENT_DATA,
    _: u32,
) -> u32 {
    poke();
    0
}

unsafe extern "system" fn on_container(
    _: HDEVQUERY,
    _: *const c_void,
    data: *const DEV_QUERY_RESULT_ACTION_DATA,
) {
    // SAFETY: Windows passes a valid action for the duration of the call.
    let Some(d) = (unsafe { data.as_ref() }) else {
        return;
    };
    if d.Action == DevQueryResultStateChange {
        // SAFETY: `State` is the member for a state change.
        if unsafe { d.Data.State } == DevQueryStateEnumCompleted {
            LISTED.store(true, Ordering::SeqCst);
        }
    } else if LISTED.load(Ordering::SeqCst) {
        poke();
    }
}

/// Calls `on_change` (from its own thread) once things settle after a device arrives, leaves,
/// connects or disconnects. Interface arrivals and removals cover plugging in and out (and a
/// Bluetooth device's functions starting); the container query covers Bluetooth connection.
/// Starts once; later calls do nothing.
pub fn watch(on_change: impl Fn() + Send + 'static) {
    let (tx, rx) = mpsc::sync_channel::<()>(1);
    if POKE.set(tx).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("device-watch".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                // A plug-in is a burst of interfaces: wait for a quiet moment (at most 3 s).
                let start = Instant::now();
                while rx.recv_timeout(Duration::from_millis(600)).is_ok()
                    && start.elapsed() < Duration::from_secs(3)
                {}
                on_change();
            }
        });
    if let Err(e) = spawned {
        log::warn!("devices: watcher thread failed: {e}");
        return;
    }

    let filter = CM_NOTIFY_FILTER {
        cbSize: u32::try_from(std::mem::size_of::<CM_NOTIFY_FILTER>()).unwrap_or(0),
        Flags: CM_NOTIFY_FILTER_FLAG_ALL_INTERFACE_CLASSES,
        FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
        ..Default::default()
    };
    let mut handle = HCMNOTIFICATION::default();
    // SAFETY: valid filter; the callback is a plain function; the registration lives for the
    // process.
    let cr = unsafe { CM_Register_Notification(&filter, None, Some(on_interface), &mut handle) };
    if cr != CR_SUCCESS {
        log::info!("devices: interface notifications unavailable ({})", cr.0);
    }

    let keys = [comp(DEVPKEY_DeviceContainer_IsConnected)];
    // SAFETY: the callback is a plain function; the query lives for the process.
    match unsafe {
        DevCreateObjectQuery(
            DevObjectTypeDeviceContainer,
            windows::Win32::Devices::DeviceQuery::DevQueryFlagUpdateResults
                .0
                .unsigned_abs(),
            Some(&keys),
            None,
            Some(on_container),
            None,
        )
    } {
        Ok(_) => {}
        Err(e) => log::info!("devices: connection watch unavailable: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On a real Windows machine (CI): the queries work, the present-only filter is understood,
    /// and a whole read finishes quickly.
    #[test]
    fn reads_this_pc() {
        let containers = containers();
        assert!(
            containers.iter().any(|c| c.local_machine),
            "{} containers, none is the PC",
            containers.len()
        );
        let start = Instant::now();
        let mut filtered = 0;
        let present = DEVPROP_TRUE;
        let filter = [DEVPROP_FILTER_EXPRESSION {
            Operator: DEVPROP_OPERATOR_EQUALS,
            Property: DEVPROPERTY {
                CompKey: comp(DEVPKEY_Device_IsPresent),
                Type: DEVPROP_TYPE_BOOLEAN,
                BufferSize: 1,
                Buffer: std::ptr::from_ref(&present).cast_mut().cast::<c_void>(),
            },
        }];
        assert!(query(
            DevObjectTypeDevice,
            &[DEVPKEY_Device_IsPresent],
            Some(&filter),
            |_, p| {
                assert_eq!(p.bool(&DEVPKEY_Device_IsPresent), Some(true));
                filtered += 1;
            }
        ));
        assert!(filtered > 0);
        let (nodes, instances) = nodes();
        assert!(nodes
            .iter()
            .any(|n| n.container.is_some() && n.class.is_some()));
        assert!(!instances.is_empty());
        let d = collect();
        assert!(start.elapsed() < Duration::from_secs(10));
        for x in &d.devices {
            assert!(!x.name.is_empty());
        }
        println!("{}", serde_json::to_string_pretty(&d).unwrap_or_default());
    }

    /// A node's name as Device Manager shows it: Configuration Manager returns it localized.
    fn cm_name(instance: &str) -> Option<String> {
        use windows::Win32::Devices::DeviceAndDriverInstallation::{
            CM_Get_DevNode_PropertyW, CM_Locate_DevNodeW, CM_LOCATE_DEVNODE_NORMAL,
        };
        let id = HSTRING::from(instance);
        let mut node = 0u32;
        // SAFETY: valid out pointer and NUL-terminated id.
        if unsafe { CM_Locate_DevNodeW(&mut node, PCWSTR(id.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) }
            != CR_SUCCESS
        {
            return None;
        }
        [DEVPKEY_Device_FriendlyName, DEVPKEY_Device_DeviceDesc]
            .iter()
            .find_map(|key| {
                let mut ty = DEVPROPTYPE::default();
                let mut buf = [0u16; 512];
                let mut size = u32::try_from(std::mem::size_of_val(&buf)).unwrap_or(0);
                // SAFETY: the buffer holds `size` bytes.
                let cr = unsafe {
                    CM_Get_DevNode_PropertyW(
                        node,
                        key,
                        &mut ty,
                        Some(buf.as_mut_ptr().cast()),
                        &mut size,
                        0,
                    )
                };
                (cr == CR_SUCCESS && ty == DEVPROP_TYPE_STRING).then(|| {
                    String::from_utf16_lossy(buf.split(|c| *c == 0).next().unwrap_or(&[]))
                        .trim()
                        .to_owned()
                })
            })
            .filter(|s| !s.is_empty())
    }

    /// Every present node's name is the one Device Manager shows, so a name the query leaves out
    /// or unresolved fails here (1.3: "Unknown device" where Settings names the device).
    #[test]
    fn names_match_device_manager() {
        let keys = [
            DEVPKEY_Device_FriendlyName,
            DEVPKEY_Device_DeviceDesc,
            DEVPKEY_Device_IsPresent,
        ];
        let mut checked = 0;
        let mut wrong = Vec::new();
        assert!(query(DevObjectTypeDevice, &keys, None, |id, p| {
            if p.bool(&DEVPKEY_Device_IsPresent) != Some(true) {
                return;
            }
            let Some(want) = cm_name(id) else {
                return;
            };
            checked += 1;
            let got = p
                .string(&DEVPKEY_Device_FriendlyName)
                .or_else(|| p.string(&DEVPKEY_Device_DeviceDesc));
            if got.as_deref() != Some(want.as_str()) {
                let ty = |k: &DEVPROPKEY| {
                    p.0.iter()
                        .find(|x| same(&x.CompKey.Key, k))
                        .map(|x| x.Type.0)
                };
                wrong.push(format!(
                    "{want:?}: query gave {got:?} (types {:?} / {:?})",
                    ty(&DEVPKEY_Device_FriendlyName),
                    ty(&DEVPKEY_Device_DeviceDesc)
                ));
            }
        }));
        let named = containers().iter().filter(|c| c.name.is_some()).count();
        println!("{checked} nodes checked; {named} containers named");
        assert!(checked > 0);
        assert!(
            wrong.is_empty(),
            "{} of {checked} names differ from Device Manager's:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }
}

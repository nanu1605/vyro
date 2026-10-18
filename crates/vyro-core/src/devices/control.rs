//! Connect and Disconnect for paired Bluetooth devices (1.3). An audio device is connected and
//! disconnected through its audio connection, as Windows Settings does, without administrator
//! rights. Any other device: Connect asks Windows to reach it (a fresh service lookup pages the
//! device), and Disconnect drops its link at the radio, which the Vyro service does.
//!
//! Every action names the device by the hashed id the list gives it; the address is looked up
//! here again and never leaves this module.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use windows::core::{Interface, GUID, PCWSTR};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice, BluetoothLEDevice};
use windows::Win32::Devices::Bluetooth::{BLUETOOTH_FIND_RADIO_PARAMS, HBLUETOOTH_RADIO_FIND};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_ContainerId;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_DEVICE_NOT_CONNECTED, ERROR_NOT_FOUND, HANDLE,
};
use windows::Win32::Media::Audio::{
    eAll, IConnector, IDeviceTopology, IMMDevice, IMMDeviceEnumerator, IPart, MMDeviceEnumerator,
    DEVICE_STATE, DEVICE_STATEMASK_ALL,
};
use windows::Win32::Media::KernelStreaming::{
    IKsControl, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Variant::VT_CLSID;
use windows::Win32::System::IO::DeviceIoControl;
use windows_future::{AsyncStatus, IAsyncOperation};

use super::{bt_address, key, norm_guid, LinkState};
use crate::live::dll::Library;

/// The Bluetooth audio driver's one-shot properties: reconnect (0) and disconnect (1).
const KSPROPSETID_BT_AUDIO: GUID = GUID::from_u128(0x7fa0_6c40_b8f6_4c7e_8556_e8c3_3a12_e54d);
const ONESHOT_RECONNECT: u32 = 0;
const ONESHOT_DISCONNECT: u32 = 1;
const KSPROPERTY_TYPE_GET: u32 = 1;
/// `IOCTL_BTH_DISCONNECT_DEVICE` (bthioctl.h): the radio drops a remote device's link.
const IOCTL_BTH_DISCONNECT_DEVICE: u32 = 0x0041_000C;
/// How long Connect waits for Windows to reach a device.
const REACH: Duration = Duration::from_secs(12);
/// How long the check after asking waits for the change to show (1.4.1), and how often it
/// reads.
const SETTLE_CONNECT: Duration = Duration::from_secs(15);
const SETTLE_DISCONNECT: Duration = Duration::from_secs(10);
const SETTLE_EVERY: Duration = Duration::from_millis(500);

/// The paired device's addresses (a dual-mode device has a Classic and an LE node).
fn addresses(id: &str) -> Result<Vec<(u64, bool)>, String> {
    let (_, instances) = super::win::nodes();
    let found: Vec<(u64, bool)> = instances
        .iter()
        .filter(|(_, c)| key(&norm_guid(c)) == id)
        .filter_map(|(instance, _)| bt_address(instance))
        .collect();
    if found.is_empty() {
        Err("Not a paired Bluetooth device".into())
    } else {
        Ok(found)
    }
}

fn com() {
    // SAFETY: COM for this thread (the multithreaded apartment, as the other COM readers).
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// After a Connect or Disconnect was taken: the device's connection once it shows what was
/// asked, or as it is when the wait ends. The answer comes from Windows, not from the request.
pub fn settle(id: &str, connect: bool) -> LinkState {
    let want = if connect {
        LinkState::Connected
    } else {
        LinkState::NotConnected
    };
    let until = Instant::now()
        + if connect {
            SETTLE_CONNECT
        } else {
            SETTLE_DISCONNECT
        };
    let started = Instant::now();
    loop {
        let now = super::win::state_of(id).unwrap_or(LinkState::Unknown);
        if now == want || Instant::now() >= until {
            log::info!(
                "devices: {} {id}: {now:?} after {} ms",
                if connect { "connect" } else { "disconnect" },
                started.elapsed().as_millis()
            );
            return now;
        }
        std::thread::sleep(SETTLE_EVERY);
    }
}

// ---------------------------------------------------------------------------------------------
// Audio: what Settings does
// ---------------------------------------------------------------------------------------------

fn container_of(dev: &IMMDevice) -> Option<String> {
    // SAFETY: COM calls; the variant is cleared below.
    let store = unsafe { dev.OpenPropertyStore(STGM_READ) }.ok()?;
    let mut v: PROPVARIANT = unsafe { store.GetValue(&PKEY_Device_ContainerId) }.ok()?;
    // SAFETY: `puuid` is the member for VT_CLSID; checked for null.
    let id = unsafe {
        let inner = &v.Anonymous.Anonymous;
        (inner.vt == VT_CLSID)
            .then(|| inner.Anonymous.puuid.as_ref().map(|g| format!("{g:?}")))
            .flatten()
    };
    // SAFETY: frees what GetValue allocated.
    let _ = unsafe { PropVariantClear(&mut v) };
    id
}

/// The kernel-streaming filter behind an audio endpoint (its connector's other end), and the
/// filter's device id, which names it among the device's filters. The id holds the device's
/// address: it is compared here and never logged.
fn filter_of(en: &IMMDeviceEnumerator, endpoint: &IMMDevice) -> Option<(String, IKsControl)> {
    // SAFETY: COM calls on live interfaces; the device id string is freed after use.
    unsafe {
        let topology: IDeviceTopology = endpoint.Activate(CLSCTX_ALL, None).ok()?;
        let connector: IConnector = topology.GetConnector(0).ok()?;
        let other: IConnector = connector.GetConnectedTo().ok()?;
        let part: IPart = other.cast().ok()?;
        let filter = part.GetTopologyObject().ok()?;
        let id = filter.GetDeviceId().ok()?;
        let name = id.to_string().ok();
        let device = en.GetDevice(PCWSTR(id.0));
        CoTaskMemFree(Some(id.0.cast_const().cast()));
        Some((name?, device.ok()?.Activate(CLSCTX_ALL, None).ok()?))
    }
}

/// Connects or disconnects a Bluetooth audio device through its audio connection: every one
/// of its audio profiles (1.4.1). A headset has a stereo and a hands-free profile, each its
/// own filter; asking only the first left the other connected.
pub fn audio(id: &str, connect: bool) -> Result<(), String> {
    com();
    // SAFETY: standard Core Audio entry point.
    let en: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|e| format!("Windows audio is unavailable: {e}"))?;
    // SAFETY: COM call; every state, since a disconnected device's endpoints are not active.
    let list = unsafe { en.EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATEMASK_ALL)) }
        .map_err(|e| e.to_string())?;
    // SAFETY: COM call.
    let count = unsafe { list.GetCount() }.unwrap_or(0);
    let property = KSIDENTIFIER {
        Anonymous: KSIDENTIFIER_0 {
            Anonymous: KSIDENTIFIER_0_0 {
                Set: KSPROPSETID_BT_AUDIO,
                Id: if connect {
                    ONESHOT_RECONNECT
                } else {
                    ONESHOT_DISCONNECT
                },
                Flags: KSPROPERTY_TYPE_GET,
            },
        },
    };
    let verb = if connect { "connect" } else { "disconnect" };
    let mut ours = 0;
    let mut asked: Vec<String> = Vec::new();
    let mut accepted = 0;
    for i in 0..count {
        // SAFETY: COM call with an index below the count.
        let Ok(endpoint) = (unsafe { list.Item(i) }) else {
            continue;
        };
        if container_of(&endpoint).is_none_or(|c| key(&norm_guid(&c)) != id) {
            continue;
        }
        ours += 1;
        let Some((filter, ks)) = filter_of(&en, &endpoint) else {
            log::info!("devices: {verb} {id}: an endpoint has no Bluetooth audio filter");
            continue;
        };
        // A filter serves the endpoints of one profile: ask it once.
        if asked.contains(&filter) {
            continue;
        }
        asked.push(filter);
        let mut returned = 0u32;
        // SAFETY: a property request with no data, as the driver's one-shot expects.
        let done = unsafe {
            ks.KsProperty(
                &property,
                u32::try_from(std::mem::size_of::<KSIDENTIFIER>()).unwrap_or(0),
                std::ptr::null_mut(),
                0,
                &mut returned,
            )
        };
        match done {
            Ok(()) => accepted += 1,
            Err(e) => log::info!("devices: {verb} {id}: profile {} refused: {e}", asked.len()),
        }
    }
    log::info!(
        "devices: {verb} {id}: {ours} endpoints, {} profiles, {accepted} accepted",
        asked.len()
    );
    if accepted > 0 {
        Ok(())
    } else if ours == 0 {
        Err("Windows has no audio connection for this device".into())
    } else {
        Err("Windows' Bluetooth audio driver did not take the request".into())
    }
}

// ---------------------------------------------------------------------------------------------
// Any other device: reach it, or drop its link
// ---------------------------------------------------------------------------------------------

/// Waits for an asynchronous call, at most until `until`; cancels it after that.
fn wait<T: windows::core::RuntimeType + 'static>(
    op: &IAsyncOperation<T>,
    until: Instant,
) -> Option<T> {
    while Instant::now() < until {
        match op.Status().ok()? {
            AsyncStatus::Started => std::thread::sleep(Duration::from_millis(50)),
            AsyncStatus::Completed => return op.GetResults().ok(),
            _ => return None,
        }
    }
    let _ = op.Cancel();
    None
}

/// Asks Windows to reach the device: a fresh (uncached) lookup of its services pages it, and a
/// device that is on and in range connects. Whether it did shows in the next listing.
pub fn reach(id: &str) -> Result<(), String> {
    com();
    let until = Instant::now() + REACH;
    let mut found = false;
    for (address, le) in addresses(id)? {
        if le {
            let Some(d) = BluetoothLEDevice::FromBluetoothAddressAsync(address)
                .ok()
                .and_then(|op| wait(&op, until))
            else {
                continue;
            };
            found = true;
            if let Ok(op) = d.GetGattServicesWithCacheModeAsync(BluetoothCacheMode::Uncached) {
                let _ = wait(&op, until);
            }
        } else {
            let Some(d) = BluetoothDevice::FromBluetoothAddressAsync(address)
                .ok()
                .and_then(|op| wait(&op, until))
            else {
                continue;
            };
            found = true;
            if let Ok(op) = d.GetRfcommServicesWithCacheModeAsync(BluetoothCacheMode::Uncached) {
                let _ = wait(&op, until);
            }
        }
    }
    log::info!("devices: connect {id}: found {found}");
    if found {
        Ok(())
    } else {
        Err("Windows could not find this device: is Bluetooth on?".into())
    }
}

type FindFirst = unsafe extern "system" fn(
    *const BLUETOOTH_FIND_RADIO_PARAMS,
    *mut HANDLE,
) -> HBLUETOOTH_RADIO_FIND;
type FindNext = unsafe extern "system" fn(HBLUETOOTH_RADIO_FIND, *mut HANDLE) -> i32;
type FindClose = unsafe extern "system" fn(HBLUETOOTH_RADIO_FIND) -> i32;

/// This PC's Bluetooth radios, opened.
fn radios() -> Vec<HANDLE> {
    static API: OnceLock<Option<(FindFirst, FindNext, FindClose)>> = OnceLock::new();
    let Some((first, next, close)) = *API.get_or_init(|| {
        let lib = Library::system32("bluetoothapis.dll")?;
        // SAFETY: the types match the documented BluetoothApis signatures.
        unsafe {
            Some((
                lib.get(c"BluetoothFindFirstRadio")?,
                lib.get(c"BluetoothFindNextRadio")?,
                lib.get(c"BluetoothFindRadioClose")?,
            ))
        }
    }) else {
        return Vec::new();
    };
    let params = BLUETOOTH_FIND_RADIO_PARAMS {
        dwSize: u32::try_from(std::mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>()).unwrap_or(4),
    };
    let mut out = Vec::new();
    let mut radio = HANDLE::default();
    // SAFETY: valid parameters and out handle; the search is closed below.
    let find = unsafe { first(&params, &mut radio) };
    if find.is_invalid() {
        return out;
    }
    out.push(radio);
    loop {
        let mut more = HANDLE::default();
        // SAFETY: the search handle from above.
        if unsafe { next(find, &mut more) } == 0 {
            break;
        }
        out.push(more);
    }
    // SAFETY: closes the search once; the radio handles stay ours to close.
    unsafe { close(find) };
    out
}

/// Drops the device's link at the radio (the Vyro service does this; it may need
/// administrator rights). A device may connect again by itself as soon as it is used.
pub fn disconnect(id: &str) -> Result<(), String> {
    let addresses = addresses(id)?;
    let radios = radios();
    if radios.is_empty() {
        return Err("This PC has no Bluetooth radio turned on".into());
    }
    let mut last = None;
    let mut done = false;
    for radio in &radios {
        for (address, _) in &addresses {
            // SAFETY: an 8-byte address in, nothing out, on an open radio handle.
            let r = unsafe {
                DeviceIoControl(
                    *radio,
                    IOCTL_BTH_DISCONNECT_DEVICE,
                    Some(std::ptr::from_ref(address).cast::<c_void>()),
                    8,
                    None,
                    0,
                    None,
                    None,
                )
            };
            match r {
                Ok(()) => done = true,
                Err(e) => last = Some(e),
            }
        }
    }
    for radio in &radios {
        // SAFETY: radio handles opened by the search, closed once.
        let _ = unsafe { CloseHandle(*radio) };
    }
    log::info!(
        "devices: disconnect {id}: {} addresses, {} radios, done {done}, last error {:?}",
        addresses.len(),
        radios.len(),
        last.as_ref().map(windows::core::Error::code)
    );
    if done {
        return Ok(());
    }
    let code = last.map(|e| e.code());
    Err(if code == Some(ERROR_ACCESS_DENIED.to_hresult()) {
        "Disconnecting this device needs the Vyro service (it runs as administrator)".into()
    } else if code == Some(ERROR_DEVICE_NOT_CONNECTED.to_hresult())
        || code == Some(ERROR_NOT_FOUND.to_hresult())
    {
        "It is not connected".into()
    } else {
        format!("Windows did not disconnect it ({})", last_text(code))
    })
}

fn last_text(code: Option<windows::core::HRESULT>) -> String {
    code.map_or_else(|| "no answer".into(), |c| c.message())
}

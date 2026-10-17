//! Windows side of `hid.rs`: finds the HID collections that report a battery and asks them.
//! Nothing here needs administrator rights. Logitech's HID++ collections and a controller's
//! gamepad collection open for reading and writing; the mouse and keyboard collections Windows
//! holds are opened without access, only to read their description and feature reports.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetFeature, HidD_GetHidGuid,
    HidD_GetPreparsedData, HidP_Feature, HidP_GetCaps, HidP_GetSpecificValueCaps,
    HidP_GetUsageValue, HIDD_ATTRIBUTES, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS,
    PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_OVERLAPPED,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

use super::hid::{self, feature, Charge, Reply};
use super::win::{interface_instance, interfaces};
use super::{norm_guid, BatterySource, DeviceKind, RawHid};

/// How long one HID++ answer may take.
const ANSWER: Duration = Duration::from_millis(250);
/// How long a controller's input report may take.
const PAD_REPORT: Duration = Duration::from_millis(200);
/// Readings are reused this long.
const FRESH: Duration = Duration::from_secs(55);
/// Protocol reads stop after this much time in one collection.
const BUDGET: Duration = Duration::from_secs(3);

/// One HID collection's description.
#[derive(Debug, Clone)]
pub struct Collection {
    pub path: String,
    /// Its device instance, uppercase; with "&COLxx" removed it names the interface the
    /// collection belongs to.
    pub instance: String,
    pub vendor: u16,
    pub product: u16,
    pub page: u16,
    pub usage: u16,
    output_len: usize,
    input_len: usize,
    /// A feature report with a Battery Strength value: its id, link collection and range.
    strength: Option<(u8, u16, i32, i32, usize)>,
}

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: a handle this module opened, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn open(path: &str, access: u32, flags: FILE_FLAGS_AND_ATTRIBUTES) -> Option<Owned> {
    // SAFETY: plain open of a device interface path; closed by the guard.
    unsafe {
        CreateFileW(
            &HSTRING::from(path),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .ok()
    .map(Owned)
}

struct Preparsed(PHIDP_PREPARSED_DATA);

impl Drop for Preparsed {
    fn drop(&mut self) {
        // SAFETY: data from HidD_GetPreparsedData, freed once.
        let _ = unsafe { HidD_FreePreparsedData(self.0) };
    }
}

/// Every HID collection present, described. Opening without access works for all of them,
/// including the mouse and keyboard Windows holds.
pub fn collections() -> Vec<Collection> {
    // SAFETY: returns a constant GUID.
    let class = unsafe { HidD_GetHidGuid() };
    interfaces(&class)
        .into_iter()
        .filter_map(describe)
        .collect()
}

fn describe(path: String) -> Option<Collection> {
    let h = open(&path, 0, FILE_FLAGS_AND_ATTRIBUTES(0))?;
    let mut attrs = HIDD_ATTRIBUTES {
        Size: u32::try_from(std::mem::size_of::<HIDD_ATTRIBUTES>()).ok()?,
        ..Default::default()
    };
    // SAFETY: valid handle and out struct with its size set.
    if !unsafe { HidD_GetAttributes(h.0, &mut attrs) } {
        return None;
    }
    let mut pp = PHIDP_PREPARSED_DATA::default();
    // SAFETY: valid handle and out pointer; freed by the guard.
    if !unsafe { HidD_GetPreparsedData(h.0, &mut pp) } {
        return None;
    }
    let pp = Preparsed(pp);
    let mut caps = HIDP_CAPS::default();
    // SAFETY: preparsed data from above.
    if unsafe { HidP_GetCaps(pp.0, &mut caps) } != HIDP_STATUS_SUCCESS {
        return None;
    }
    let mut values = [HIDP_VALUE_CAPS::default(); 4];
    let mut count = u16::try_from(values.len()).ok()?;
    // SAFETY: room for `count` entries.
    let found = unsafe {
        HidP_GetSpecificValueCaps(
            HidP_Feature,
            Some(hid::BATTERY_PAGE),
            None,
            Some(hid::BATTERY_STRENGTH),
            values.as_mut_ptr(),
            &mut count,
            pp.0,
        )
    } == HIDP_STATUS_SUCCESS;
    let strength = (found && count > 0).then(|| {
        let v = values[0];
        (
            v.ReportID,
            v.LinkCollection,
            v.LogicalMin,
            v.LogicalMax,
            usize::from(caps.FeatureReportByteLength),
        )
    });
    Some(Collection {
        instance: interface_instance(&path).unwrap_or_default(),
        path,
        vendor: attrs.VendorID,
        product: attrs.ProductID,
        page: caps.UsagePage,
        usage: caps.Usage,
        output_len: usize::from(caps.OutputReportByteLength),
        input_len: usize::from(caps.InputReportByteLength),
        strength,
    })
}

// ---------------------------------------------------------------------------------------------
// Overlapped reports
// ---------------------------------------------------------------------------------------------

/// A read waiting on one handle. Boxed so its buffer and OVERLAPPED stay put while Windows
/// fills them; dropping it cancels the read and waits for it to end.
struct Pending {
    handle: HANDLE,
    event: Owned,
    ov: Box<OVERLAPPED>,
    buf: Vec<u8>,
    armed: bool,
}

impl Pending {
    fn new(handle: HANDLE, len: usize) -> Option<Self> {
        // SAFETY: an unnamed manual-reset event, closed by the guard.
        let event = Owned(unsafe { CreateEventW(None, true, false, None) }.ok()?);
        Some(Self {
            handle,
            event,
            ov: Box::default(),
            buf: vec![0; len.max(1)],
            armed: false,
        })
    }

    fn arm(&mut self) -> bool {
        if self.armed {
            return true;
        }
        *self.ov = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        // SAFETY: the buffer and OVERLAPPED are boxed and outlive the read (see Drop).
        match unsafe { ReadFile(self.handle, Some(&mut self.buf), None, Some(&mut *self.ov)) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(_) => return false,
        }
        self.armed = true;
        true
    }

    /// The report a signalled read finished with.
    fn take(&mut self) -> Option<Vec<u8>> {
        let mut n = 0u32;
        self.armed = false;
        // SAFETY: the OVERLAPPED of the read just signalled.
        unsafe { GetOverlappedResult(self.handle, &*self.ov, &mut n, false) }.ok()?;
        Some(self.buf[..(n as usize).min(self.buf.len())].to_vec())
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.armed {
            let mut n = 0u32;
            // SAFETY: cancels our own read, then waits for it to end before the buffer goes.
            unsafe {
                let _ = CancelIoEx(self.handle, Some(&*self.ov));
                let _ = GetOverlappedResult(self.handle, &*self.ov, &mut n, true);
            }
        }
    }
}

/// Reads from one or more handles at once (a receiver answers on its long or short
/// collection), and writes to the first.
struct Port {
    handles: Vec<Owned>,
    reads: Vec<Pending>,
    output_len: usize,
}

impl Port {
    fn open(main: &Collection, more: &[&Collection], write: bool) -> Option<Self> {
        let access = GENERIC_READ.0 | if write { GENERIC_WRITE.0 } else { 0 };
        let mut handles = Vec::new();
        let mut lens = Vec::new();
        for c in std::iter::once(main).chain(more.iter().copied()) {
            match open(&c.path, access, FILE_FLAG_OVERLAPPED) {
                Some(h) => {
                    handles.push(h);
                    lens.push(c.input_len);
                }
                None if handles.is_empty() => return None,
                None => {}
            }
        }
        let reads = handles
            .iter()
            .zip(lens)
            .map(|(h, len)| Pending::new(h.0, len))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            handles,
            reads,
            output_len: main.output_len,
        })
    }

    fn write(&self, data: &[u8]) -> bool {
        let mut buf = data.to_vec();
        buf.resize(self.output_len.max(data.len()), 0);
        let Some(mut w) = Pending::new(self.handles[0].0, 0) else {
            return false;
        };
        *w.ov = OVERLAPPED {
            hEvent: w.event.0,
            ..Default::default()
        };
        // SAFETY: `buf` and the boxed OVERLAPPED live until the write has ended below.
        match unsafe { WriteFile(w.handle, Some(&buf), None, Some(&mut *w.ov)) } {
            Ok(()) => return true,
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(_) => return false,
        }
        w.armed = true;
        // SAFETY: waits on our own event.
        let r = unsafe { WaitForMultipleObjects(&[w.event.0], false, 500) };
        if r != WAIT_OBJECT_0 {
            return false; // Drop cancels it.
        }
        w.take().is_some()
    }

    /// The next report from any handle, or `None` after `wait`.
    fn read(&mut self, wait: Duration) -> Option<Vec<u8>> {
        for r in &mut self.reads {
            if !r.arm() {
                return None;
            }
        }
        let events: Vec<HANDLE> = self.reads.iter().map(|r| r.event.0).collect();
        let ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: valid event handles.
        let r = unsafe { WaitForMultipleObjects(&events, false, ms) };
        let i = r.0.checked_sub(WAIT_OBJECT_0.0)? as usize;
        self.reads.get_mut(i)?.take()
    }
}

// ---------------------------------------------------------------------------------------------
// Logitech HID++
// ---------------------------------------------------------------------------------------------

/// Asks `device` one thing: `Ok` with the reply's parameters, `Err` with an error code, `None`
/// with no answer in time.
fn ask(
    port: &mut Port,
    device: u8,
    index: u8,
    function: u8,
    params: &[u8],
    deadline: Instant,
) -> Option<Result<Vec<u8>, u8>> {
    let req = hid::request(device, index, function, params);
    if !port.write(&req) {
        return None;
    }
    let until = (Instant::now() + ANSWER).min(deadline);
    loop {
        let left = until.checked_duration_since(Instant::now())?;
        let got = port.read(left)?;
        match hid::reply(&req, &got) {
            Reply::Ok(p) => return Some(Ok(p.to_vec())),
            Reply::Error(e) => return Some(Err(e)),
            Reply::Other => {}
        }
    }
}

fn feature_index(port: &mut Port, device: u8, id: u16, deadline: Instant) -> Option<u8> {
    let [hi, lo] = id.to_be_bytes();
    match ask(port, device, 0x00, 0, &[hi, lo], deadline)? {
        Ok(p) => p.first().copied().filter(|i| *i != 0),
        Err(_) => None,
    }
}

fn hidpp_charge(port: &mut Port, device: u8, deadline: Instant) -> Option<Charge> {
    type Parse = fn(&[u8]) -> Option<Charge>;
    let ways: [(u16, u8, Parse); 3] = [
        (feature::UNIFIED_BATTERY, 1, hid::unified),
        (feature::BATTERY_STATUS, 0, hid::status),
        (feature::BATTERY_VOLTAGE, 0, hid::voltage),
    ];
    for (id, function, parse) in ways {
        let Some(index) = feature_index(port, device, id, deadline) else {
            continue;
        };
        if let Some(Ok(p)) = ask(port, device, index, function, &[], deadline) {
            return parse(&p);
        }
    }
    None
}

fn hidpp_name(
    port: &mut Port,
    device: u8,
    deadline: Instant,
) -> (Option<String>, Option<DeviceKind>) {
    let Some(index) = feature_index(port, device, feature::DEVICE_NAME, deadline) else {
        return (None, None);
    };
    let len = match ask(port, device, index, 0, &[], deadline) {
        Some(Ok(p)) => usize::from(p.first().copied().unwrap_or(0)),
        _ => 0,
    };
    let mut parts = Vec::new();
    let mut got = 0;
    while got < len {
        let Ok(at) = u8::try_from(got) else { break };
        match ask(port, device, index, 1, &[at], deadline) {
            Some(Ok(p)) => {
                got += p.len().max(1);
                parts.push(p);
            }
            _ => break,
        }
    }
    let kind = match ask(port, device, index, 2, &[], deadline) {
        Some(Ok(p)) => p.first().map(|t| hid::device_kind(*t)),
        _ => None,
    };
    (hid::name(&parts, len), kind)
}

/// A device that answers HID++ 2.0 at `device`.
fn alive(port: &mut Port, device: u8, deadline: Instant) -> bool {
    matches!(
        ask(port, device, 0x00, 1, &[0, 0, 0xAA], deadline),
        Some(Ok(p)) if hid::is_hidpp2(&p)
    )
}

/// Devices paired to a receiver (or the device itself) that answered once this session, by
/// interface and slot: shown as asleep while they don't answer.
type Seen = Mutex<HashMap<(String, u8), (Option<String>, Option<DeviceKind>)>>;

fn logitech(key: &str, container: &str, cols: &[&Collection], deadline: Instant) -> Vec<RawHid> {
    static SEEN: OnceLock<Seen> = OnceLock::new();
    let long = cols
        .iter()
        .find(|c| (c.page == 0xFF00 && c.usage == 2) || (c.page == 0xFF43 && c.usage == 0x0202));
    let Some(long) = long else {
        return Vec::new();
    };
    let short: Vec<&Collection> = cols
        .iter()
        .copied()
        .filter(|c| c.path != long.path && matches!(c.page, 0xFF00 | 0xFF43))
        .collect();
    let Some(mut port) = Port::open(long, &short, true) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let raw = |slot, name, kind, awake, charge| RawHid {
        container: container.to_owned(),
        slot,
        name,
        kind,
        awake,
        charge,
        source: BatterySource::Logitech,
    };
    // Connected directly (cable or Bluetooth): the device answers for itself.
    if alive(&mut port, hid::DIRECT, deadline) {
        if let Some(c) = hidpp_charge(&mut port, hid::DIRECT, deadline) {
            out.push(raw(None, None, None, true, Some(c)));
        }
        return out;
    }
    let mut seen = crate::lock(SEEN.get_or_init(Mutex::default));
    for slot in hid::SLOTS {
        if Instant::now() > deadline {
            break;
        }
        let id = (key.to_owned(), slot);
        if alive(&mut port, slot, deadline) {
            let (name, kind) = hidpp_name(&mut port, slot, deadline);
            let charge = hidpp_charge(&mut port, slot, deadline);
            seen.insert(id, (name.clone(), kind));
            out.push(raw(Some(slot), name, kind, true, charge));
        } else if let Some((name, kind)) = seen.get(&id) {
            out.push(raw(Some(slot), name.clone(), *kind, false, None));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Sony controllers and the standard battery value
// ---------------------------------------------------------------------------------------------

fn sony(c: &Collection, pad: hid::Pad) -> Option<Charge> {
    let mut port = Port::open(c, &[], false)?;
    let until = Instant::now() + PAD_REPORT;
    for _ in 0..4 {
        let left = until.checked_duration_since(Instant::now())?;
        if let Some(charge) = hid::pad_charge(pad, &port.read(left)?) {
            return Some(charge);
        }
    }
    None
}

fn strength(c: &Collection) -> Option<Charge> {
    let (id, link, min, max, len) = c.strength?;
    let h = open(&c.path, 0, FILE_FLAGS_AND_ATTRIBUTES(0))?;
    let mut buf = vec![0u8; len.max(2)];
    buf[0] = id;
    // SAFETY: a buffer of the feature report's length, its id first.
    if !unsafe { HidD_GetFeature(h.0, buf.as_mut_ptr().cast(), u32::try_from(buf.len()).ok()?) } {
        return None;
    }
    let mut pp = PHIDP_PREPARSED_DATA::default();
    // SAFETY: valid handle and out pointer; freed by the guard.
    if !unsafe { HidD_GetPreparsedData(h.0, &mut pp) } {
        return None;
    }
    let pp = Preparsed(pp);
    let mut value = 0u32;
    // SAFETY: the report just read and its collection's preparsed data.
    let ok = unsafe {
        HidP_GetUsageValue(
            HidP_Feature,
            hid::BATTERY_PAGE,
            Some(link),
            hid::BATTERY_STRENGTH,
            &mut value,
            pp.0,
            &buf,
        )
    } == HIDP_STATUS_SUCCESS;
    let pct = hid::strength(
        i64::from(ok.then_some(value)?),
        i64::from(min),
        i64::from(max),
    )?;
    Some(Charge {
        percent: Some(pct),
        level: None,
        charging: None,
    })
}

// ---------------------------------------------------------------------------------------------
// All of it
// ---------------------------------------------------------------------------------------------

/// The interface a collection belongs to: its instance without "&COLxx", and without the
/// collection's own number at the end ("…\\8&2B1&0&0001" → "…\\8&2B1&0").
fn interface_of(instance: &str) -> String {
    let Some(i) = instance.find("&COL") else {
        return instance.to_owned();
    };
    let rest = &instance[i + 4..];
    let tail = rest.find('\\').map_or("", |at| &rest[at..]);
    let tail = tail.rsplit_once('&').map_or(tail, |(t, _)| t);
    format!("{}{tail}", &instance[..i])
}

/// Batteries through makers' protocols, by container. Each interface is asked at most once a
/// minute; the whole read stops after a few seconds.
pub fn batteries(instances: &HashMap<String, String>) -> Vec<RawHid> {
    type Cache = Mutex<HashMap<String, (Instant, Vec<RawHid>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cols = collections();
    let mut groups: HashMap<String, Vec<&Collection>> = HashMap::new();
    for c in &cols {
        groups.entry(interface_of(&c.instance)).or_default().push(c);
    }
    let mut cache = crate::lock(CACHE.get_or_init(Mutex::default));
    let start = Instant::now();
    let deadline = start + BUDGET;
    let mut out = Vec::new();
    for (key, cols) in &groups {
        let Some(container) = cols
            .iter()
            .find_map(|c| instances.get(&c.instance))
            .map(|c| norm_guid(c))
        else {
            continue;
        };
        if let Some((at, v)) = cache.get(key) {
            if at.elapsed() < FRESH {
                out.extend(v.iter().cloned());
                continue;
            }
        }
        if Instant::now() > deadline {
            continue;
        }
        let (vendor, product) = (cols[0].vendor, cols[0].product);
        let direct = |charge: Option<Charge>, source| {
            charge.map(|c| RawHid {
                container: container.clone(),
                slot: None,
                name: None,
                kind: None,
                awake: true,
                charge: Some(c),
                source,
            })
        };
        let found: Vec<RawHid> = if vendor == hid::LOGITECH {
            logitech(key, &container, cols, deadline)
        } else if let Some(pad) = (vendor == hid::SONY)
            .then(|| hid::sony_pad(product))
            .flatten()
        {
            cols.iter()
                .filter(|c| c.page == 1 && c.usage == 5)
                .find_map(|c| sony(c, pad))
                .and_then(|c| direct(Some(c), BatterySource::PlayStation))
                .into_iter()
                .collect()
        } else {
            cols.iter()
                .find_map(|c| strength(c))
                .and_then(|c| direct(Some(c), BatterySource::HidBattery))
                .into_iter()
                .collect()
        };
        cache.insert(key.clone(), (Instant::now(), found.clone()));
        out.extend(found);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_of_one_interface_group_together() {
        let short = interface_of("HID\\VID_046D&PID_C539&MI_02&COL01\\8&2B1&0&0000");
        assert_eq!(short, "HID\\VID_046D&PID_C539&MI_02\\8&2B1&0");
        assert_eq!(
            interface_of("HID\\VID_046D&PID_C539&MI_02&COL02\\8&2B1&0&0001"),
            short
        );
        assert_eq!(
            interface_of("HID\\VID_054C&PID_0CE6\\7&1"),
            "HID\\VID_054C&PID_0CE6\\7&1"
        );
    }

    /// On a real Windows machine (CI): every HID collection is described, and a whole battery
    /// read (with nothing to find on a server) ends quickly.
    #[test]
    fn reads_this_pc() {
        let start = Instant::now();
        let cols = collections();
        println!("{} HID collections", cols.len());
        for c in &cols {
            println!(
                "{:04x}:{:04x} page {:04x} usage {:04x}",
                c.vendor, c.product, c.page, c.usage
            );
        }
        let (_, instances) = super::super::win::nodes();
        let found = batteries(&instances);
        println!("{found:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}

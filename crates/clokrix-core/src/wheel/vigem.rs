//! A virtual Xbox 360 controller through ViGEmBus (2.6), the signed driver (BSD-3, Nefarius
//! Software Solutions; retired in 2023, still signed and in wide use) that the Clokrix installer
//! carries and installs on first use. Clokrix's own client of the bus: the device interface, its
//! control codes and structures are those of ViGEmClient's public headers (checked against the
//! MIT-licensed `vigem-client` crate); no code of either is used.
//!
//! The bus is opened for overlapped I/O; each request waits for its own event. A thread may wait
//! for the game's rumble ([`Pad::rumble`]) while another submits reports. Dropping the pad
//! unplugs the controller.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, ERROR_IO_PENDING, GENERIC_READ,
    GENERIC_WRITE, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED,
    FILE_FLAG_WRITE_THROUGH, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::CreateEventW;
use windows::Win32::System::IO::{CancelIoEx, DeviceIoControl, GetOverlappedResult, OVERLAPPED};

use super::{PadSink, Report};

/// The bus's device interface.
const BUS: GUID = GUID::from_values(
    0x96E4_2B22,
    0xF5E9,
    0x42F8,
    [0xB0, 0x43, 0xED, 0x0F, 0x93, 0x2F, 0x01, 0x4F],
);

// CTL_CODE(FILE_DEVICE_BUS_EXTENDER, 0x801 + n, METHOD_BUFFERED, write or read-write access).
const PLUGIN: u32 = 0x002A_A004;
const UNPLUG: u32 = 0x002A_A008;
const CHECK_VERSION: u32 = 0x002A_A00C;
const WAIT_READY: u32 = 0x002A_A010;
const XUSB_NOTIFICATION: u32 = 0x002A_E804;
const XUSB_REPORT: u32 = 0x002A_A808;
/// The bus's interface version, as its clients say it.
const COMMON_VERSION: u32 = 1;
/// An Xbox 360 wired controller, as Microsoft's own.
const XBOX360: i32 = 0;
const VENDOR: u16 = 0x045E;
const PRODUCT: u16 = 0x028E;

#[repr(C)]
struct CheckVersion {
    size: u32,
    version: u32,
}

#[repr(C)]
struct PluginTarget {
    size: u32,
    serial: u32,
    target: i32,
    vendor: u16,
    product: u16,
}

#[repr(C)]
struct Serial {
    size: u32,
    serial: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XusbReport {
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

#[repr(C)]
struct SubmitReport {
    size: u32,
    serial: u32,
    report: XusbReport,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Notification {
    size: u32,
    serial: u32,
    large: u8,
    small: u8,
    led: u8,
}

fn size<T>() -> u32 {
    std::mem::size_of::<T>() as u32
}

/// Whether ViGEmBus is installed and running: its interface is present.
pub fn installed() -> bool {
    !crate::devices::win::interfaces(&BUS).is_empty()
}

struct Handle(HANDLE);

// SAFETY: a file handle, used with overlapped I/O from more than one thread, as Windows allows.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle this module opened, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// An event, closed when dropped.
struct Event(HANDLE);

impl Event {
    fn new() -> Result<Self, String> {
        // SAFETY: an auto-reset event with no name.
        unsafe { CreateEventW(None, false, false, None) }
            .map(Self)
            .map_err(|e| format!("an event: {e}"))
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: an event this module made, closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// One request to the bus, waiting for it to finish. `Err` carries Windows' error code.
///
/// # Safety
/// `input` and `output` must be valid for `in_len` and `out_len` bytes until it returns.
unsafe fn ioctl(
    bus: HANDLE,
    code: u32,
    input: *const std::ffi::c_void,
    in_len: u32,
    output: *mut std::ffi::c_void,
    out_len: u32,
) -> Result<(), u32> {
    let event = Event::new().map_err(|_| 0u32)?;
    let mut ov = OVERLAPPED {
        hEvent: event.0,
        ..Default::default()
    };
    let mut n = 0u32;
    // SAFETY: the caller's buffers are valid for their lengths until this returns, as is `ov`,
    // waited on before it goes.
    let sent = unsafe {
        DeviceIoControl(
            bus,
            code,
            Some(input),
            in_len,
            (!output.is_null()).then_some(output),
            out_len,
            Some(&mut n),
            Some(&mut ov),
        )
    };
    if let Err(e) = sent {
        if e.code() != ERROR_IO_PENDING.to_hresult() {
            return Err((e.code().0 & 0xFFFF) as u32);
        }
    }
    // SAFETY: the request above, on this handle, with this OVERLAPPED.
    if unsafe { GetOverlappedResult(bus, &ov, &mut n, true) }.is_err() {
        // SAFETY: no arguments.
        return Err(unsafe { GetLastError() }.0);
    }
    Ok(())
}

/// A virtual Xbox 360 controller, plugged in.
pub struct Pad {
    bus: Handle,
    serial: u32,
    /// Set while it is being unplugged: the rumble wait ends.
    closing: AtomicBool,
}

impl Pad {
    /// Opens the bus and plugs a controller in. Fails (saying why) when ViGEmBus is not
    /// installed, is a version it does not know, or has no free place.
    pub fn plug() -> Result<Self, String> {
        let path = crate::devices::win::interfaces(&BUS)
            .into_iter()
            .next()
            .ok_or("ViGEmBus is not installed")?;
        // SAFETY: plain open of the bus's interface path; closed by the guard.
        let bus = unsafe {
            CreateFileW(
                &HSTRING::from(path),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL
                    | FILE_FLAG_NO_BUFFERING
                    | FILE_FLAG_WRITE_THROUGH
                    | FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map(Handle)
        .map_err(|e| format!("ViGEmBus could not be opened: {e}"))?;
        let check = CheckVersion {
            size: size::<CheckVersion>(),
            version: COMMON_VERSION,
        };
        // SAFETY: `check` lives across the call, and is its size.
        unsafe {
            ioctl(
                bus.0,
                CHECK_VERSION,
                std::ptr::from_ref(&check).cast(),
                size::<CheckVersion>(),
                std::ptr::null_mut(),
                0,
            )
        }
        .map_err(|e| format!("ViGEmBus is a version Clokrix does not know (error {e})"))?;
        // The driver takes the first serial number not in use.
        let mut serial = 1;
        loop {
            let plug = PluginTarget {
                size: size::<PluginTarget>(),
                serial,
                target: XBOX360,
                vendor: VENDOR,
                product: PRODUCT,
            };
            // SAFETY: as above.
            let r = unsafe {
                ioctl(
                    bus.0,
                    PLUGIN,
                    std::ptr::from_ref(&plug).cast(),
                    size::<PluginTarget>(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            if r.is_ok() {
                break;
            }
            serial += 1;
            if serial > 16 {
                return Err("ViGEmBus has no free place for a controller".into());
            }
        }
        let wait = Serial {
            size: size::<Serial>(),
            serial,
        };
        // SAFETY: as above. ViGEmBus before 1.17 does not know this request, and needs no wait.
        let ready = unsafe {
            ioctl(
                bus.0,
                WAIT_READY,
                std::ptr::from_ref(&wait).cast(),
                size::<Serial>(),
                std::ptr::null_mut(),
                0,
            )
        };
        let pad = Self {
            bus,
            serial,
            closing: AtomicBool::new(false),
        };
        match ready {
            Ok(()) => {}
            Err(e) if e == ERROR_INVALID_PARAMETER.0 => {}
            Err(e) => return Err(format!("the controller did not come up (error {e})")),
        }
        log::info!("wheel: a virtual Xbox controller is plugged in (place {serial})");
        Ok(pad)
    }

    /// Waits for the game's next rumble: the large and small motors, 0 to 255. `None` once the
    /// pad is being unplugged or the bus refuses.
    pub fn rumble(&self) -> Option<(u8, u8)> {
        if self.closing.load(Ordering::SeqCst) {
            return None;
        }
        let mut n = Notification {
            size: size::<Notification>(),
            serial: self.serial,
            large: 0,
            small: 0,
            led: 0,
        };
        let p = std::ptr::from_mut(&mut n).cast();
        // SAFETY: `n` lives across the call (which waits for it to finish), in and out.
        unsafe {
            ioctl(
                self.bus.0,
                XUSB_NOTIFICATION,
                p,
                size::<Notification>(),
                p,
                size::<Notification>(),
            )
        }
        .ok()?;
        (!self.closing.load(Ordering::SeqCst)).then_some((n.large, n.small))
    }
}

impl Pad {
    /// Gives the controller `r`; from any thread.
    pub fn submit(&self, r: &Report) -> Result<(), String> {
        let s = SubmitReport {
            size: size::<SubmitReport>(),
            serial: self.serial,
            report: XusbReport {
                buttons: r.buttons,
                left_trigger: r.left_trigger,
                right_trigger: r.right_trigger,
                lx: r.thumb_lx,
                ly: r.thumb_ly,
                rx: r.thumb_rx,
                ry: r.thumb_ry,
            },
        };
        // SAFETY: `s` lives across the call, and is its size.
        unsafe {
            ioctl(
                self.bus.0,
                XUSB_REPORT,
                std::ptr::from_ref(&s).cast(),
                size::<SubmitReport>(),
                std::ptr::null_mut(),
                0,
            )
        }
        .map_err(|e| format!("the controller did not take the report (error {e})"))
    }
}

impl PadSink for Pad {
    fn update(&mut self, r: &Report) -> Result<(), String> {
        self.submit(r)
    }
}

impl Pad {
    /// Ends a rumble wait on another thread (before the pad is dropped).
    pub fn wake(&self) {
        self.closing.store(true, Ordering::SeqCst);
        // SAFETY: cancels this handle's pending requests (the rumble wait) from any thread.
        let _ = unsafe { CancelIoEx(self.bus.0, None) };
    }
}

impl Drop for Pad {
    fn drop(&mut self) {
        self.wake();
        let s = Serial {
            size: size::<Serial>(),
            serial: self.serial,
        };
        // SAFETY: as above.
        let _ = unsafe {
            ioctl(
                self.bus.0,
                UNPLUG,
                std::ptr::from_ref(&s).cast(),
                size::<Serial>(),
                std::ptr::null_mut(),
                0,
            )
        };
        log::info!("wheel: the virtual Xbox controller is unplugged");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The structures are laid out as the bus reads them.
    #[test]
    fn structures_are_the_sizes_the_bus_takes() {
        assert_eq!(size::<CheckVersion>(), 8);
        assert_eq!(size::<PluginTarget>(), 16);
        assert_eq!(size::<Serial>(), 8);
        assert_eq!(size::<XusbReport>(), 12);
        assert_eq!(size::<SubmitReport>(), 20);
        assert_eq!(size::<Notification>(), 12);
    }

    /// Where ViGEmBus is installed (Windows CI installs it), a controller plugs in, takes a
    /// report that XInput then reads back, and unplugs; elsewhere it says so.
    #[test]
    fn a_controller_plugs_in_and_xinput_reads_its_report() {
        use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE};
        if !installed() {
            eprintln!("wheel: ViGEmBus is not installed here");
            return;
        }
        let mut pad = match Pad::plug() {
            Ok(p) => p,
            Err(e) => panic!("ViGEmBus is installed, yet: {e}"),
        };
        let r = Report {
            buttons: super::super::packet::buttons::A,
            right_trigger: 200,
            thumb_lx: -12_000,
            ..Report::default()
        };
        pad.update(&r).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let seen = (0..4).any(|i| {
            let mut s = XINPUT_STATE::default();
            // SAFETY: valid out struct.
            let got = unsafe { XInputGetState(i, &mut s) };
            got == 0 && s.Gamepad.sThumbLX == -12_000 && s.Gamepad.bRightTrigger == 200
        });
        assert!(seen, "XInput did not see the report");
        drop(pad);
    }
}

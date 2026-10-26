//! The monitor's picture through DXGI Desktop Duplication (1.7). The app runs it as the signed-in
//! user; the secure desktop (sign-in, lock screen, administrator prompts) cannot be captured and
//! is reported as such. The mouse pointer is not part of the picture: the phone draws its own.
//!
//! With HDR on, Desktop Duplication hands over 16-bit float pictures; [`super::hdr`] converts
//! them to 8-bit on the GPU.
//!
//! On some PCs Desktop Duplication hands over pictures of nothing but zeros. A picture that stays
//! blank is then taken again through GDI, as Chrome falls back to it; when GDI sees the screen,
//! the rest of the session is taken through GDI. So is a session whose pictures Desktop
//! Duplication gives in a form that cannot be read or converted.

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
    IDXGIOutputDuplication, DXGI_ERROR_ACCESS_DENIED, DXGI_ERROR_ACCESS_LOST,
    DXGI_ERROR_SESSION_DISCONNECTED, DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};

use super::frames;
use super::hdr::{self, Hdr};
use super::input::Rect;
pub use super::producer::{CaptureError, Picture};

fn other(what: &str, e: &windows::core::Error) -> CaptureError {
    if e.code() == DXGI_ERROR_ACCESS_DENIED {
        CaptureError::Secure
    } else if e.code() == DXGI_ERROR_ACCESS_LOST || e.code() == DXGI_ERROR_SESSION_DISCONNECTED {
        CaptureError::Lost
    } else if e.code() == DXGI_ERROR_UNSUPPORTED {
        CaptureError::Other(format!("{what}: screen capture is not supported here"))
    } else {
        CaptureError::Other(format!("{what}: {e}"))
    }
}

/// A monitor as the phone can choose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    pub index: usize,
    pub name: String,
    /// Where it is on the desktop, in pixels.
    pub rect: Rect,
}

/// Every monitor attached to the desktop, adapter by adapter.
fn outputs() -> Vec<(IDXGIAdapter1, IDXGIOutput, Monitor)> {
    let mut out = Vec::new();
    // SAFETY: plain DXGI enumeration; results are owned COM references.
    unsafe {
        let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut a = 0;
        while let Ok(adapter) = f.EnumAdapters1(a) {
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                if let Ok(d) = output.GetDesc() {
                    if d.AttachedToDesktop.as_bool() {
                        let len = d
                            .DeviceName
                            .iter()
                            .position(|c| *c == 0)
                            .unwrap_or(d.DeviceName.len());
                        let r = d.DesktopCoordinates;
                        out.push((
                            adapter.clone(),
                            output,
                            Monitor {
                                index: 0,
                                name: String::from_utf16_lossy(&d.DeviceName[..len]),
                                rect: Rect {
                                    left: r.left,
                                    top: r.top,
                                    width: (r.right - r.left).max(1),
                                    height: (r.bottom - r.top).max(1),
                                },
                            },
                        ));
                    }
                }
                o += 1;
            }
            a += 1;
        }
    }
    // The primary monitor (at 0,0) first.
    out.sort_by_key(|(_, _, m)| (m.rect.left != 0 || m.rect.top != 0, m.rect.left, m.rect.top));
    for (i, (_, _, m)) in out.iter_mut().enumerate() {
        m.index = i;
    }
    out
}

pub fn monitors() -> Vec<Monitor> {
    outputs().into_iter().map(|(_, _, m)| m).collect()
}

/// 8-bit BGRA: what Desktop Duplication gives and the conversion reads.
fn readable(f: DXGI_FORMAT) -> bool {
    f == DXGI_FORMAT_B8G8R8A8_UNORM || f == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
}

/// Logs what Windows duplicates: a field report then says what the PC's capture was given.
fn describe(monitor: &Monitor, dup: &IDXGIOutputDuplication) {
    // SAFETY: a live duplication.
    let d = unsafe { dup.GetDesc() };
    log::info!(
        "screen: duplicating {} at {}x{}: {}x{}, pixel format {}{}, rotation {}, {}",
        monitor.name,
        monitor.rect.left,
        monitor.rect.top,
        d.ModeDesc.Width,
        d.ModeDesc.Height,
        d.ModeDesc.Format.0,
        if readable(d.ModeDesc.Format) {
            " (8-bit BGRA)"
        } else if d.ModeDesc.Format == hdr::FORMAT {
            " (16-bit float: HDR)"
        } else {
            ""
        },
        d.Rotation.0,
        if d.DesktopImageInSystemMemory.as_bool() {
            "in system memory"
        } else {
            "in video memory"
        }
    );
}

pub struct Capture {
    ctx: ID3D11DeviceContext,
    device: ID3D11Device,
    output: IDXGIOutput1,
    /// None between letting the duplication go and a new one opening.
    dup: Option<IDXGIOutputDuplication>,
    staging: Option<ID3D11Texture2D>,
    /// Converts an HDR desktop's pictures, once one came.
    hdr: Option<Hdr>,
    /// Why Desktop Duplication's pictures cannot be used, when one could not be.
    unusable: Option<String>,
    /// A picture was taken since the duplication opened (until then every frame is one).
    seen: bool,
    /// GDI instead, once Desktop Duplication's picture stayed blank and GDI could fill it.
    gdi: Option<Gdi>,
    /// Since when Desktop Duplication's latest picture is blank.
    blank_since: Option<Instant>,
    /// When the monitor was last taken through GDI to see whether it really is blank.
    tried: Option<Instant>,
    /// When where the monitor is on the desktop was last looked up.
    followed: Instant,
    pub monitor: Monitor,
}

// SAFETY: the device, duplication and GDI objects are used from one thread at a time.
unsafe impl Send for Capture {}

/// Desktop Duplication's picture blank this long (one blank picture as it starts is not enough):
/// the monitor is taken through GDI to see whether it really is.
const BLANK_FOR: Duration = Duration::from_secs(1);
/// And again at most this often while GDI sees it blank too (a screen that really is black).
const RETRY: Duration = Duration::from_secs(5);
/// Where the monitor is on the desktop is looked up this often, and after it is opened again.
const FOLLOW: Duration = Duration::from_secs(1);

/// The monitor through GDI: a copy of its part of the screen into a bitmap of our own.
struct Gdi {
    mem: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut std::ffi::c_void,
    rect: Rect,
    /// The last picture, and when it was taken: GDI does not say when the screen changes.
    last: Option<(Vec<u8>, Instant)>,
}

impl Gdi {
    fn open(rect: Rect) -> Result<Self, CaptureError> {
        let fail = |what: &str| CaptureError::Other(format!("GDI: {what}"));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: rect.width,
                // Negative: rows top to bottom, as the conversion reads them.
                biHeight: -rect.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: plain GDI calls; the screen DC is released here, the rest in `drop`.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return Err(fail("no screen"));
            }
            let mem = CreateCompatibleDC(Some(screen));
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0);
            ReleaseDC(None, screen);
            let bitmap = match bitmap {
                Ok(b) if !bits.is_null() => b,
                _ => {
                    let _ = DeleteDC(mem);
                    return Err(fail("no bitmap"));
                }
            };
            let old = SelectObject(mem, bitmap.into());
            Ok(Self {
                mem,
                bitmap,
                old,
                bits,
                rect,
                last: None,
            })
        }
    }

    /// The monitor now. A copy that fails is the secure desktop (or no desktop at all).
    fn grab(&mut self) -> Result<Picture, CaptureError> {
        let r = self.rect;
        let len = r.width as usize * r.height as usize * 4;
        // SAFETY: the bitmap is `len` bytes and selected into `mem`; it is read after GdiFlush.
        unsafe {
            let screen = GetDC(None);
            let copied = BitBlt(
                self.mem,
                0,
                0,
                r.width,
                r.height,
                Some(screen),
                r.left,
                r.top,
                SRCCOPY | CAPTUREBLT,
            );
            ReleaseDC(None, screen);
            copied.map_err(|_| CaptureError::Secure)?;
            let _ = GdiFlush();
            Ok(Picture {
                bgra: std::slice::from_raw_parts(self.bits.cast::<u8>(), len).to_vec(),
                width: r.width as u32,
                height: r.height as u32,
            })
        }
    }

    /// The next picture, at most one every `timeout_ms`; `None` when the screen did not change.
    fn next(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError> {
        if let Some((_, at)) = &self.last {
            let due = *at + Duration::from_millis(u64::from(timeout_ms));
            std::thread::sleep(due.saturating_duration_since(Instant::now()));
        }
        let p = self.grab()?;
        let same = self.last.as_ref().is_some_and(|(b, _)| *b == p.bgra);
        self.last = Some((p.bgra.clone(), Instant::now()));
        Ok((!same).then_some(p))
    }
}

impl Drop for Gdi {
    fn drop(&mut self) {
        // SAFETY: the objects made in `open`, put back and deleted once.
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.mem);
        }
    }
}

impl Capture {
    /// Captures monitor `index` (0: the primary).
    pub fn open(index: usize) -> Result<Self, CaptureError> {
        let (adapter, output, monitor) = outputs()
            .into_iter()
            .nth(index)
            .ok_or_else(|| CaptureError::Other("no such monitor".into()))?;
        let mut device = None;
        let mut ctx = None;
        let adapter: IDXGIAdapter = adapter.cast().map_err(|e| other("adapter", &e))?;
        // SAFETY: out pointers are valid; an explicit adapter needs the "unknown" driver type.
        unsafe {
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
        }
        .map_err(|e| other("Direct3D", &e))?;
        let (Some(device), Some(ctx)) = (device, ctx) else {
            return Err(CaptureError::Other("Direct3D: no device".into()));
        };
        let output: IDXGIOutput1 = output.cast().map_err(|e| other("output", &e))?;
        // SAFETY: a live output and device.
        let dup =
            unsafe { output.DuplicateOutput(&device) }.map_err(|e| other("duplication", &e))?;
        describe(&monitor, &dup);
        Ok(Self {
            ctx,
            device,
            output,
            dup: Some(dup),
            staging: None,
            hdr: None,
            unusable: None,
            seen: false,
            gdi: None,
            blank_since: None,
            tried: None,
            followed: Instant::now(),
            monitor,
        })
    }

    /// Opens the duplication again after [`CaptureError::Lost`]. The old one goes first, as in
    /// Microsoft's sample; until a new one opens, [`next`](Self::next) says the picture is lost.
    pub fn reopen(&mut self) -> Result<(), CaptureError> {
        self.dup = None;
        // SAFETY: as in `open`.
        let dup = unsafe { self.output.DuplicateOutput(&self.device) }
            .map_err(|e| other("duplication", &e))?;
        describe(&self.monitor, &dup);
        self.dup = Some(dup);
        self.staging = None;
        self.hdr = None;
        self.blank_since = None;
        self.seen = false;
        self.follow();
        Ok(())
    }

    /// Follows the monitor on the desktop: another resolution, or a monitor added or moved
    /// beside it, changes where it is, and input and GDI go by that. Looked up afresh by name,
    /// as the output kept since it opened may still describe it as it was.
    fn follow(&mut self) {
        self.followed = Instant::now();
        let Some(now) = monitors().into_iter().find(|m| m.name == self.monitor.name) else {
            return;
        };
        if now.rect == self.monitor.rect {
            return;
        }
        let r = now.rect;
        log::info!(
            "screen: {} is now {}x{} at {},{}",
            self.monitor.name,
            r.width,
            r.height,
            r.left,
            r.top
        );
        self.monitor.rect = r;
        if self.gdi.is_some() {
            match Gdi::open(r) {
                Ok(g) => self.gdi = Some(g),
                Err(e) => log::warn!("screen: {e}"),
            }
        }
    }

    /// The next look gives a picture though the screen did not change: the size sent changed
    /// (HD or SD), and a still screen would otherwise go on at the old one. GDI forgets its last
    /// picture; Desktop Duplication opens again, and its first frame is the whole desktop.
    pub fn refresh(&mut self) {
        if let Some(g) = &mut self.gdi {
            g.last = None;
        } else if let Err(e) = self.reopen() {
            // `next` says the picture is lost, and the producer opens it again.
            log::debug!("screen: {e}");
        }
    }

    /// How the pictures are taken: "Desktop Duplication" (with "HDR" when they are converted),
    /// or "GDI" after the fallback.
    pub fn method(&self) -> &'static str {
        if self.gdi.is_some() {
            "GDI"
        } else if self.hdr.is_some() {
            "Desktop Duplication (HDR)"
        } else {
            "Desktop Duplication"
        }
    }

    /// The next picture, waiting at most `timeout_ms`; `None` when the screen did not change.
    pub fn next(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError> {
        if self.followed.elapsed() >= FOLLOW {
            self.follow();
        }
        if let Some(g) = &mut self.gdi {
            return g.next(timeout_ms);
        }
        let picture = self.duplicated(timeout_ms)?;
        if let Some(why) = self.unusable.take() {
            return self.instead_through_gdi(&why);
        }
        match &picture {
            Some(p) if frames::blank(&p.bgra) => {
                self.blank_since.get_or_insert_with(Instant::now);
            }
            Some(_) => self.blank_since = None,
            None => {}
        }
        let blank = self.blank_since.is_some_and(|t| t.elapsed() >= BLANK_FOR);
        if blank && self.tried.is_none_or(|t| t.elapsed() >= RETRY) {
            self.tried = Some(Instant::now());
            if let Some((g, q)) = self.through_gdi() {
                log::warn!(
                    "screen: Desktop Duplication gives {} a blank picture; GDI sees it, so the \
                     session goes on through GDI",
                    self.monitor.name
                );
                self.gdi = Some(g);
                return Ok(Some(q));
            }
        }
        Ok(picture)
    }

    /// Desktop Duplication's pictures cannot be used: GDI takes the monitor for the rest of the
    /// session, or the session ends saying why.
    fn instead_through_gdi(&mut self, why: &str) -> Result<Option<Picture>, CaptureError> {
        let mut g = Gdi::open(self.monitor.rect)
            .map_err(|e| CaptureError::Other(format!("{why}, and {e}")))?;
        let p = g.next(0)?;
        log::warn!(
            "screen: {why}, so {} goes on through GDI",
            self.monitor.name
        );
        self.gdi = Some(g);
        Ok(p)
    }

    /// The converter for an HDR picture this size, made on the first one.
    fn hdr(&mut self, width: u32, height: u32) -> Result<&Hdr, CaptureError> {
        if self
            .hdr
            .as_ref()
            .is_none_or(|h| (h.width, h.height) != (width, height))
        {
            self.hdr = None;
            let white = crate::display::sdr_white_level(&self.monitor.name);
            let h = Hdr::new(&self.device, width, height, white.unwrap_or(hdr::WHITE))?;
            log::info!(
                "screen: {} is HDR; its pictures are converted with SDR white at {} nits{}",
                self.monitor.name,
                hdr::nits(white.unwrap_or(hdr::WHITE)),
                if white.is_some() {
                    ""
                } else {
                    " (Windows does not say)"
                }
            );
            self.hdr = Some(h);
        }
        self.hdr
            .as_ref()
            .ok_or_else(|| CaptureError::Other("HDR: none".into()))
    }

    /// The monitor through GDI, when GDI sees something on it.
    fn through_gdi(&self) -> Option<(Gdi, Picture)> {
        let mut g = match Gdi::open(self.monitor.rect) {
            Ok(g) => g,
            Err(e) => {
                log::debug!("screen: {e}");
                return None;
            }
        };
        let p = g.grab().ok()?;
        if frames::blank(&p.bgra) {
            return None;
        }
        g.last = Some((p.bgra.clone(), Instant::now()));
        Some((g, p))
    }

    /// A texture the CPU can read, for 8-bit pictures like `desc`.
    fn staging(&mut self, desc: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D, CaptureError> {
        if let Some(s) = &self.staging {
            return Ok(s.clone());
        }
        let sd = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            MipLevels: 1,
            ArraySize: 1,
            ..*desc
        };
        let mut s = None;
        // SAFETY: valid descriptor and out pointer.
        unsafe { self.device.CreateTexture2D(&sd, None, Some(&mut s)) }
            .map_err(|e| other("capture", &e))?;
        let s = s.ok_or_else(|| CaptureError::Other("capture: no texture".into()))?;
        self.staging = Some(s.clone());
        Ok(s)
    }

    /// The next picture through Desktop Duplication.
    fn duplicated(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError> {
        let Some(dup) = self.dup.clone() else {
            return Err(CaptureError::Lost);
        };
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res = None;
        // SAFETY: out pointers valid; every acquired frame is released below.
        match unsafe { dup.AcquireNextFrame(timeout_ms, &mut info, &mut res) } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) => return Err(other("capture", &e)),
        }
        let result = (|| {
            let Some(res) = res else {
                return Ok(None);
            };
            if info.LastPresentTime == 0 && self.seen {
                // Only the pointer moved.
                return Ok(None);
            }
            let tex: ID3D11Texture2D = res.cast().map_err(|e| other("capture", &e))?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            // SAFETY: a live texture.
            unsafe { tex.GetDesc(&mut desc) };
            let staging = if readable(desc.Format) {
                let s = self.staging(&desc)?;
                // SAFETY: both textures are live and the same size and format.
                unsafe { self.ctx.CopyResource(&s, &tex) };
                s
            } else if desc.Format == hdr::FORMAT {
                let ctx = self.ctx.clone();
                match self
                    .hdr(desc.Width, desc.Height)
                    .map(|h| h.convert(&ctx, &tex))
                {
                    Ok(s) => s,
                    Err(e) => {
                        self.unusable =
                            Some(format!("the screen is HDR and cannot be converted ({e})"));
                        return Ok(None);
                    }
                }
            } else {
                // Anything else would go out as noise.
                self.unusable = Some(format!(
                    "the screen's pixel format ({}) cannot be read",
                    desc.Format.0
                ));
                return Ok(None);
            };
            let bgra = read(&self.ctx, &staging, desc.Width, desc.Height)?;
            self.seen = true;
            Ok(Some(Picture {
                bgra,
                width: desc.Width,
                height: desc.Height,
            }))
        })();
        // SAFETY: the frame acquired above.
        let _ = unsafe { dup.ReleaseFrame() };
        result
    }
}

/// The rows of a mapped staging texture holding `width` x `height` BGRA pixels, packed.
pub(super) fn read(
    ctx: &ID3D11DeviceContext,
    staging: &ID3D11Texture2D,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, CaptureError> {
    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: a live staging texture of this size, 4 bytes a pixel; the mapped rows are read
    // within the pitch while mapped.
    unsafe {
        ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
            .map_err(|e| other("capture", &e))?;
        let row = width as usize * 4;
        let mut bgra = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            let p = m.pData.cast::<u8>().add(y * m.RowPitch as usize);
            bgra.extend_from_slice(std::slice::from_raw_parts(p, row));
        }
        ctx.Unmap(staging, 0);
        Ok(bgra)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GDI, the fallback, takes the primary monitor too: its size, rows packed, not blank.
    #[test]
    fn gdi_captures_the_primary_monitor_where_there_is_one() {
        let Some(m) = monitors().into_iter().next() else {
            eprintln!("screen: no monitor for GDI");
            return;
        };
        let mut g = match Gdi::open(m.rect) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("screen: no GDI capture here: {e}");
                return;
            }
        };
        match g.next(10) {
            Ok(Some(p)) => {
                assert_eq!(
                    (p.width as i32, p.height as i32),
                    (m.rect.width, m.rect.height)
                );
                assert_eq!(p.bgra.len(), p.width as usize * p.height as usize * 4);
                let mean = p.bgra.iter().map(|&b| u64::from(b)).sum::<u64>() / p.bgra.len() as u64;
                eprintln!(
                    "screen: GDI captured {}x{}, mean byte {mean}{}",
                    p.width,
                    p.height,
                    if frames::blank(&p.bgra) {
                        ", blank"
                    } else {
                        ""
                    }
                );
            }
            Ok(None) => panic!("the first GDI picture is always new"),
            Err(e) => eprintln!("screen: GDI capture failed: {e}"),
        }
    }

    /// Takes a picture where the test runs on a desktop; CI's runner may have none, and says so.
    #[test]
    fn captures_the_primary_monitor_where_there_is_one() {
        eprintln!("screen: monitors {:?}", monitors());
        let mut c = match Capture::open(0) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("screen: no capture here: {e}");
                return;
            }
        };
        for _ in 0..20 {
            match c.next(100) {
                Ok(Some(p)) => {
                    assert_eq!(p.bgra.len(), p.width as usize * p.height as usize * 4);
                    let zero = p.bgra.iter().all(|&b| b == 0);
                    let mean =
                        p.bgra.iter().map(|&b| u64::from(b)).sum::<u64>() / p.bgra.len() as u64;
                    eprintln!(
                        "screen: captured {}x{}, mean byte {mean}{}",
                        p.width,
                        p.height,
                        if zero {
                            ", all zero (no picture yet)"
                        } else {
                            ""
                        }
                    );
                    // Asked again (HD or SD changed the size sent), a still screen gives its
                    // picture once more.
                    c.refresh();
                    let again = (0..20).find_map(|_| c.next(100).ok().flatten());
                    let again = again.expect("no picture in 2 s after asking for one again");
                    assert_eq!((again.width, again.height), (p.width, p.height));
                    eprintln!(
                        "screen: asked again, captured {}x{}",
                        again.width, again.height
                    );
                    return;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("screen: capture failed: {e}");
                    return;
                }
            }
        }
        eprintln!("screen: the screen did not change in 2 s");
    }
}

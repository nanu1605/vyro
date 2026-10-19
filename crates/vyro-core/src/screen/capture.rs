//! The monitor's picture through DXGI Desktop Duplication (1.7). The app runs it as the signed-in
//! user; the secure desktop (sign-in, lock screen, administrator prompts) cannot be captured and
//! is reported as such. The mouse pointer is not part of the picture: the phone draws its own.

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput, IDXGIOutput1,
    IDXGIOutputDuplication, DXGI_ERROR_ACCESS_DENIED, DXGI_ERROR_ACCESS_LOST,
    DXGI_ERROR_SESSION_DISCONNECTED, DXGI_ERROR_UNSUPPORTED, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};

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

pub struct Capture {
    ctx: ID3D11DeviceContext,
    device: ID3D11Device,
    output: IDXGIOutput1,
    dup: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    /// A picture was taken since the duplication opened (until then every frame is one).
    seen: bool,
    pub monitor: Monitor,
}

// SAFETY: the device and duplication are used from one thread at a time (the producer).
unsafe impl Send for Capture {}

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
        Ok(Self {
            ctx,
            device,
            output,
            dup,
            staging: None,
            seen: false,
            monitor,
        })
    }

    /// Opens the duplication again after [`CaptureError::Lost`].
    pub fn reopen(&mut self) -> Result<(), CaptureError> {
        // SAFETY: as in `open`.
        self.dup = unsafe { self.output.DuplicateOutput(&self.device) }
            .map_err(|e| other("duplication", &e))?;
        self.staging = None;
        self.seen = false;
        Ok(())
    }

    /// The next picture, waiting at most `timeout_ms`; `None` when the screen did not change.
    pub fn next(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res = None;
        // SAFETY: out pointers valid; every acquired frame is released below.
        match unsafe { self.dup.AcquireNextFrame(timeout_ms, &mut info, &mut res) } {
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
            let staging = match &self.staging {
                Some(s) => s.clone(),
                None => {
                    let sd = D3D11_TEXTURE2D_DESC {
                        Usage: D3D11_USAGE_STAGING,
                        BindFlags: 0,
                        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                        MiscFlags: 0,
                        MipLevels: 1,
                        ArraySize: 1,
                        ..desc
                    };
                    let mut s = None;
                    // SAFETY: valid descriptor and out pointer.
                    unsafe { self.device.CreateTexture2D(&sd, None, Some(&mut s)) }
                        .map_err(|e| other("capture", &e))?;
                    let s = s.ok_or_else(|| CaptureError::Other("capture: no texture".into()))?;
                    self.staging = Some(s.clone());
                    s
                }
            };
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: both textures are live and the same size; the mapped rows are read within
            // the pitch while mapped.
            unsafe {
                self.ctx.CopyResource(&staging, &tex);
                self.ctx
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
                    .map_err(|e| other("capture", &e))?;
                let row = desc.Width as usize * 4;
                let mut bgra = Vec::with_capacity(row * desc.Height as usize);
                for y in 0..desc.Height as usize {
                    let p = m.pData.cast::<u8>().add(y * m.RowPitch as usize);
                    bgra.extend_from_slice(std::slice::from_raw_parts(p, row));
                }
                self.ctx.Unmap(&staging, 0);
                self.seen = true;
                Ok(Some(Picture {
                    bgra,
                    width: desc.Width,
                    height: desc.Height,
                }))
            }
        })();
        // SAFETY: the frame acquired above.
        let _ = unsafe { self.dup.ReleaseFrame() };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

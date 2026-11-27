//! The screen made small on the GPU, for ambient lighting (2.6, `crate::lighting`): a compute
//! shader averages each of the 64x36 cells of the duplicated desktop, in linear light, and only
//! those (about 36 KB) are read back, not the whole picture. An 8-bit desktop's sRGB is decoded
//! first; an HDR desktop's scRGB is scaled so that SDR white is 1.0 (highlights stay above it,
//! for the picker to ease), and its negative wide-gamut parts count as nothing.

use windows::core::s;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC,
    D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE, D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_SAMPLE_DESC,
};

use super::hdr::{self, bytes, compile};
use super::producer::CaptureError;
use crate::lighting::colour::{Thumb, THUMB_H, THUMB_W};

const SHADER: &str = r"
Texture2D<float4> src : register(t0);
RWTexture2D<float4> dst : register(u0);
cbuffer Params : register(b0) { float scale; uint mode; float2 pad; };
groupshared float3 acc[256];

// One group a cell of the grid, its 256 threads each taking one point of the cell.
[numthreads(16, 16, 1)]
void main(uint3 g : SV_GroupID, uint3 t : SV_GroupThreadID, uint i : SV_GroupIndex) {
    uint w, h;
    src.GetDimensions(w, h);
    float2 cell = float2(w, h) / float2(64.0, 36.0);
    float2 at = (float2(g.xy) + (float2(t.xy) + 0.5) / 16.0) * cell;
    uint2 p = min(uint2(at), uint2(w - 1, h - 1));
    float3 c = src[p].rgb;
    if (mode == 0) {
        // 8-bit sRGB.
        c = lerp(c / 12.92, pow((c + 0.055) / 1.055, 2.4), step(0.04045, c));
    } else if (mode == 2) {
        // HDR scRGB: SDR white to 1.0.
        c = c * scale;
    }
    // Negative (wide-gamut) parts, and not-a-number, as nothing.
    acc[i] = max(c, 0.0);
    GroupMemoryBarrierWithGroupSync();
    for (uint s = 128; s > 0; s >>= 1) {
        if (i < s) {
            acc[i] += acc[i + s];
        }
        GroupMemoryBarrierWithGroupSync();
    }
    if (i == 0) {
        dst[g.xy] = float4(acc[0] / 256.0, 1.0);
    }
}
";

/// The reducer for one picture size and format, on the duplication's device.
pub struct Reduce {
    cs: ID3D11ComputeShader,
    params: ID3D11Buffer,
    /// The duplicated picture is copied here: Desktop Duplication's own may not be readable by
    /// a shader.
    src: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    out: ID3D11Texture2D,
    uav: ID3D11UnorderedAccessView,
    staging: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    pub format: DXGI_FORMAT,
    pub white: u32,
}

fn fail(what: &str, e: &windows::core::Error) -> CaptureError {
    CaptureError::Other(format!("lighting {what}: {e}"))
}

fn none(what: &str) -> CaptureError {
    CaptureError::Other(format!("lighting {what}: none"))
}

impl Reduce {
    /// For pictures `width` x `height` in `format` (8-bit BGRA, or HDR's 16-bit float); `white` is
    /// the display's SDR white level (1000 is 80 nits), for HDR.
    pub fn new(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        white: u32,
    ) -> Result<Self, CaptureError> {
        let code = compile(SHADER, s!("main"), s!("cs_5_0"))?;
        let mut cs = None;
        // SAFETY: the bytecode outlives the call; out pointer valid.
        unsafe { device.CreateComputeShader(bytes(&code), None, Some(&mut cs)) }
            .map_err(|e| fail("shader", &e))?;
        let cs = cs.ok_or_else(|| none("shader"))?;
        // How the shader reads the picture: 0 sRGB to decode, 1 decoded already (an _SRGB
        // format, which the GPU decodes as it reads), 2 HDR.
        let mode: u32 = if format == hdr::FORMAT {
            2
        } else if format == DXGI_FORMAT_B8G8R8A8_UNORM_SRGB {
            1
        } else {
            0
        };
        let p: [u32; 4] = [(1000.0 / white.max(1) as f32).to_bits(), mode, 0, 0];
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: 16,
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: p.as_ptr().cast(),
            ..Default::default()
        };
        let mut params = None;
        // SAFETY: valid descriptor, 16 bytes of initial data, out pointer valid.
        unsafe { device.CreateBuffer(&desc, Some(&init), Some(&mut params)) }
            .map_err(|e| fail("parameters", &e))?;
        let params = params.ok_or_else(|| none("parameters"))?;
        let texture = |w: u32, h: u32, f: DXGI_FORMAT, usage: D3D11_USAGE, bind: u32, cpu: u32| {
            let d = D3D11_TEXTURE2D_DESC {
                Width: w,
                Height: h,
                MipLevels: 1,
                ArraySize: 1,
                Format: f,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: usage,
                BindFlags: bind,
                CPUAccessFlags: cpu,
                MiscFlags: 0,
            };
            let mut t = None;
            // SAFETY: valid descriptor and out pointer.
            unsafe { device.CreateTexture2D(&d, None, Some(&mut t)) }
                .map_err(|e| fail("texture", &e))?;
            t.ok_or_else(|| none("texture"))
        };
        let (tw, th) = (THUMB_W as u32, THUMB_H as u32);
        let src = texture(
            width,
            height,
            format,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_SHADER_RESOURCE.0 as u32,
            0,
        )?;
        let out = texture(
            tw,
            th,
            DXGI_FORMAT_R32G32B32A32_FLOAT,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_UNORDERED_ACCESS.0 as u32,
            0,
        )?;
        let staging = texture(
            tw,
            th,
            DXGI_FORMAT_R32G32B32A32_FLOAT,
            D3D11_USAGE_STAGING,
            0,
            D3D11_CPU_ACCESS_READ.0 as u32,
        )?;
        let mut srv = None;
        let mut uav = None;
        // SAFETY: live textures made for these views; out pointers valid.
        unsafe {
            device
                .CreateShaderResourceView(&src, None, Some(&mut srv))
                .map_err(|e| fail("view", &e))?;
            device
                .CreateUnorderedAccessView(&out, None, Some(&mut uav))
                .map_err(|e| fail("view", &e))?;
        }
        Ok(Self {
            cs,
            params,
            src,
            srv: srv.ok_or_else(|| none("view"))?,
            out,
            uav: uav.ok_or_else(|| none("view"))?,
            staging,
            width,
            height,
            format,
            white,
        })
    }

    /// Queues `tex` (this size and format) made small; [`Self::read`] then gives it. `tex` may be
    /// let go of once this returns: what is read is the copy.
    pub fn queue(&self, ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D) {
        // SAFETY: live resources of matching size and format on `ctx`'s device; views are
        // unbound again before the result is copied out.
        unsafe {
            ctx.CopyResource(&self.src, tex);
            ctx.CSSetShader(&self.cs, None);
            ctx.CSSetShaderResources(0, Some(&[Some(self.srv.clone())]));
            ctx.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.uav.clone())), None);
            ctx.CSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            ctx.Dispatch(THUMB_W as u32, THUMB_H as u32, 1);
            ctx.CSSetShaderResources(0, Some(&[None]));
            ctx.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
            ctx.CopyResource(&self.staging, &self.out);
        }
    }

    /// The picture queued last, small.
    pub fn read(&self, ctx: &ID3D11DeviceContext) -> Result<Thumb, CaptureError> {
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: a live staging texture of THUMB_W x THUMB_H float4s, read within its pitch
        // while mapped.
        unsafe {
            ctx.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
                .map_err(|e| fail("read", &e))?;
            let mut px = Vec::with_capacity(THUMB_W * THUMB_H);
            for y in 0..THUMB_H {
                let row = m
                    .pData
                    .cast::<u8>()
                    .add(y * m.RowPitch as usize)
                    .cast::<f32>();
                for x in 0..THUMB_W {
                    let p = row.add(x * 4);
                    px.push([
                        p.read_unaligned(),
                        p.add(1).read_unaligned(),
                        p.add(2).read_unaligned(),
                    ]);
                }
            }
            ctx.Unmap(&self.staging, 0);
            Ok(Thumb {
                width: THUMB_W,
                height: THUMB_H,
                px,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

    fn warp() -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        let (mut device, mut ctx) = (None, None);
        // SAFETY: out pointers valid; WARP needs no adapter.
        let made = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
        };
        match (made, device, ctx) {
            (Ok(()), Some(d), Some(c)) => Some((d, c)),
            _ => None,
        }
    }

    fn texture(
        device: &ID3D11Device,
        w: u32,
        h: u32,
        format: DXGI_FORMAT,
        pixels: &[u8],
        bpp: u32,
    ) -> ID3D11Texture2D {
        let d = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let init = D3D11_SUBRESOURCE_DATA {
            pSysMem: pixels.as_ptr().cast(),
            SysMemPitch: w * bpp,
            SysMemSlicePitch: 0,
        };
        let mut tex = None;
        // SAFETY: valid descriptor; the initial data is `w * h` pixels of `bpp` bytes.
        unsafe { device.CreateTexture2D(&d, Some(&init), Some(&mut tex)) }.unwrap();
        tex.unwrap()
    }

    /// An 8-bit picture whose size divides by nothing of the grid's: its left half sRGB red, its
    /// right half sRGB 50% grey. Each cell comes out in linear light.
    #[test]
    fn an_8_bit_screen_made_small_in_linear_light() {
        let Some((device, ctx)) = warp() else {
            eprintln!("screen: no Direct3D here");
            return;
        };
        let (w, h) = (1001u32, 563u32);
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                px.extend_from_slice(if x < w / 2 {
                    &[0, 0, 255, 255]
                } else {
                    &[128, 128, 128, 255]
                });
            }
        }
        let tex = texture(&device, w, h, DXGI_FORMAT_B8G8R8A8_UNORM, &px, 4);
        let r = Reduce::new(&device, w, h, DXGI_FORMAT_B8G8R8A8_UNORM, hdr::WHITE).unwrap();
        r.queue(&ctx, &tex);
        let t = r.read(&ctx).unwrap();
        assert_eq!(t.px.len(), THUMB_W * THUMB_H);
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
        assert!(near(t.px[0], [1.0, 0.0, 0.0]), "{:?}", t.px[0]);
        // sRGB 128 is linear 0.216.
        let last = t.px[THUMB_W * THUMB_H - 1];
        assert!(near(last, [0.216, 0.216, 0.216]), "{last:?}");
    }

    /// An HDR picture: SDR white comes out as 1.0, a highlight above it, and negative parts as
    /// nothing.
    #[test]
    fn an_hdr_screen_made_small_with_sdr_white_at_one() {
        let Some((device, ctx)) = warp() else {
            eprintln!("screen: no Direct3D here");
            return;
        };
        const SDR_WHITE: u16 = 0x4100; // 2.5: SDR white at 200 nits
        const BRIGHTER: u16 = 0x4500; // 5.0
        const MINUS: u16 = 0xBC00; // -1.0
        const ONE: u16 = 0x3C00;
        let (w, h) = (128u32, 72u32);
        let mut px = Vec::with_capacity((w * h * 8) as usize);
        for _ in 0..h {
            for x in 0..w {
                let p: [u16; 4] = if x < 64 {
                    [SDR_WHITE, SDR_WHITE, SDR_WHITE, ONE]
                } else {
                    [BRIGHTER, MINUS, 0, ONE]
                };
                for v in p {
                    px.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        let tex = texture(&device, w, h, hdr::FORMAT, &px, 8);
        let r = Reduce::new(&device, w, h, hdr::FORMAT, hdr::WHITE).unwrap();
        r.queue(&ctx, &tex);
        let t = r.read(&ctx).unwrap();
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
        assert!(near(t.px[0], [1.0, 1.0, 1.0]), "{:?}", t.px[0]);
        assert!(
            near(t.px[THUMB_W - 1], [2.0, 0.0, 0.0]),
            "{:?}",
            t.px[THUMB_W - 1]
        );
    }
}

//! Pictures of an HDR desktop (1.7). With HDR on, Desktop Duplication hands over the desktop as
//! Windows composes it: 16-bit float, linear scRGB, where 1.0 is 80 nits and SDR white sits at the
//! display's SDR white level. A compute shader turns it into the 8-bit BGRA the rest of the
//! pipeline reads: SDR white becomes 255, brighter highlights are clipped, and the sRGB curve is
//! applied. Read as 8-bit, the same bytes go out as a black or garbled picture.

use windows::core::{s, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, ID3DInclude};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11ComputeShader, ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11UnorderedAccessView, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS, D3D11_BUFFER_DESC,
    D3D11_CPU_ACCESS_READ, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_IMMUTABLE, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};

use super::producer::CaptureError;

/// What Desktop Duplication gives on an HDR desktop.
pub const FORMAT: DXGI_FORMAT = DXGI_FORMAT_R16G16B16A16_FLOAT;

/// SDR white when Windows does not say: 200 nits (1000 is 80 nits).
pub const WHITE: u32 = 2500;

/// Nits of an SDR white level as Windows gives it.
pub fn nits(white: u32) -> u32 {
    white * 80 / 1000
}

const SHADER: &str = r"
Texture2D<float4> src : register(t0);
RWTexture2D<unorm float4> dst : register(u0);
cbuffer Params : register(b0) { float scale; float3 pad; };

[numthreads(16, 16, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    uint w, h;
    src.GetDimensions(w, h);
    if (id.x >= w || id.y >= h) return;
    float3 c = saturate(src[id.xy].rgb * scale);
    float3 s = lerp(c * 12.92, 1.055 * pow(c, 1.0 / 2.4) - 0.055, step(0.0031308, c));
    // Stored as RGBA, so the bytes come out blue, green, red, alpha: BGRA.
    dst[id.xy] = float4(s.b, s.g, s.r, 1.0);
}
";

/// The converter for one picture size, on the duplication's device.
pub struct Hdr {
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
}

fn fail(what: &str, e: &windows::core::Error) -> CaptureError {
    CaptureError::Other(format!("HDR {what}: {e}"))
}

fn none(what: &str) -> CaptureError {
    CaptureError::Other(format!("HDR {what}: none"))
}

impl Hdr {
    /// `white` is the display's SDR white level (1000 is 80 nits).
    pub fn new(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        white: u32,
    ) -> Result<Self, CaptureError> {
        let code = compile(SHADER, s!("main"), s!("cs_5_0"))?;
        let mut cs = None;
        // SAFETY: the bytecode outlives the call; out pointer valid.
        unsafe { device.CreateComputeShader(bytes(&code), None, Some(&mut cs)) }
            .map_err(|e| fail("shader", &e))?;
        let cs = cs.ok_or_else(|| none("shader"))?;

        let p: [f32; 4] = [1000.0 / white.max(1) as f32, 0.0, 0.0, 0.0];
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

        let texture = |format: DXGI_FORMAT, usage: D3D11_USAGE, bind: u32, cpu: u32| {
            let d = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
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
        let src = texture(
            FORMAT,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_SHADER_RESOURCE.0 as u32,
            0,
        )?;
        let out = texture(
            DXGI_FORMAT_R8G8B8A8_UNORM,
            D3D11_USAGE_DEFAULT,
            D3D11_BIND_UNORDERED_ACCESS.0 as u32,
            0,
        )?;
        let staging = texture(
            DXGI_FORMAT_R8G8B8A8_UNORM,
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
        })
    }

    /// Converts `tex` (this size, [`FORMAT`]); the returned staging texture holds it as BGRA.
    pub fn convert(&self, ctx: &ID3D11DeviceContext, tex: &ID3D11Texture2D) -> ID3D11Texture2D {
        // SAFETY: live resources of matching size and format on `ctx`'s device; views are
        // unbound again before the result is copied out.
        unsafe {
            ctx.CopyResource(&self.src, tex);
            ctx.CSSetShader(&self.cs, None);
            ctx.CSSetShaderResources(0, Some(&[Some(self.srv.clone())]));
            ctx.CSSetUnorderedAccessViews(0, 1, Some(&Some(self.uav.clone())), None);
            ctx.CSSetConstantBuffers(0, Some(&[Some(self.params.clone())]));
            ctx.Dispatch(self.width.div_ceil(16), self.height.div_ceil(16), 1);
            ctx.CSSetShaderResources(0, Some(&[None]));
            ctx.CSSetUnorderedAccessViews(0, 1, Some(&None), None);
            ctx.CopyResource(&self.staging, &self.out);
        }
        self.staging.clone()
    }
}

fn compile(src: &str, entry: PCSTR, target: PCSTR) -> Result<ID3DBlob, CaptureError> {
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source pointer and length describe `src`; out pointers are valid.
    let r = unsafe {
        D3DCompile(
            src.as_ptr().cast(),
            src.len(),
            PCSTR::null(),
            None,
            None::<&ID3DInclude>,
            entry,
            target,
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(e) = r {
        let msg = errors.map(|b| String::from_utf8_lossy(bytes(&b)).into_owned());
        return Err(CaptureError::Other(format!(
            "HDR shader: {e} {}",
            msg.unwrap_or_default()
        )));
    }
    code.ok_or_else(|| none("shader"))
}

fn bytes(b: &ID3DBlob) -> &[u8] {
    // SAFETY: the blob owns this memory for its lifetime, which bounds the slice's.
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer().cast(), b.GetBufferSize()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    };

    /// Half floats, as an HDR desktop holds them.
    const BLACK: u16 = 0x0000;
    const HALF_WHITE: u16 = 0x3D00; // 1.25
    const SDR_WHITE: u16 = 0x4100; // 2.5: SDR white at 200 nits
    const BRIGHTER: u16 = 0x4500; // 5.0: an HDR highlight
    const ONE: u16 = 0x3C00;

    /// Columns of black, half SDR white, SDR white, a highlight and SDR red, in a picture whose
    /// size is no multiple of the shader's groups: each comes out as 8-bit sRGB BGRA.
    #[test]
    fn an_hdr_picture_comes_out_as_8_bit_srgb() {
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
        let (Ok(()), Some(device), Some(ctx)) = (made, device, ctx) else {
            eprintln!("screen: no Direct3D here");
            return;
        };
        let (w, h) = (50u32, 20u32);
        let columns: [[u16; 4]; 5] = [
            [BLACK, BLACK, BLACK, ONE],
            [HALF_WHITE, HALF_WHITE, HALF_WHITE, ONE],
            [SDR_WHITE, SDR_WHITE, SDR_WHITE, ONE],
            [BRIGHTER, BRIGHTER, BRIGHTER, ONE],
            [SDR_WHITE, BLACK, BLACK, ONE],
        ];
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                pixels.extend_from_slice(&columns[(x * 5 / w) as usize]);
            }
        }
        let d = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: FORMAT,
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
            SysMemPitch: w * 8,
            SysMemSlicePitch: 0,
        };
        let mut tex = None;
        // SAFETY: valid descriptor; the initial data is `w * h` pixels of 8 bytes.
        unsafe { device.CreateTexture2D(&d, Some(&init), Some(&mut tex)) }.unwrap();
        let tex = tex.unwrap();

        let hdr = Hdr::new(&device, w, h, WHITE).unwrap();
        let staging = hdr.convert(&ctx, &tex);
        let bgra = super::super::capture::read(&ctx, &staging, w, h).unwrap();
        assert_eq!(bgra.len(), (w * h * 4) as usize);
        let at = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]]
        };
        assert_eq!(at(0, 0), [0, 0, 0, 255], "black");
        // Linear 0.5 is sRGB 0.735: 187.5.
        let half = at(15, 10);
        assert!(
            half[..3].iter().all(|&v| (187..=188).contains(&v)) && half[3] == 255,
            "half SDR white: {half:?}"
        );
        assert_eq!(at(25, 5), [255, 255, 255, 255], "SDR white");
        assert_eq!(at(35, 19), [255, 255, 255, 255], "a highlight is clipped");
        assert_eq!(at(49, 19), [0, 0, 255, 255], "red, in BGRA order");
        assert_eq!(nits(WHITE), 200);
    }
}

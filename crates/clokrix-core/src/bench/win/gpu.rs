//! Direct3D 11 GPU workloads (method v1) on the high-performance adapter: FP32 multiply-add
//! chains, a copy in video memory, an integer hash the CPU checks, and an offscreen scene.
//!
//! Work goes out in batches of about [`BATCH_SECS`] with two in flight, timed from the first
//! batch submitted to the last one finished. Completion is polled with event queries, so a stop
//! waits for at most the batches already submitted.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::core::{Interface, HRESULT, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, ID3DInclude, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Buffer, ID3D11ComputeShader, ID3D11DepthStencilView, ID3D11Device,
    ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader, ID3D11Query, ID3D11RasterizerState,
    ID3D11RenderTargetView, ID3D11Texture2D, ID3D11UnorderedAccessView, ID3D11VertexShader,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_DEPTH_STENCIL, D3D11_BIND_INDEX_BUFFER,
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_UNORDERED_ACCESS, D3D11_BIND_VERTEX_BUFFER,
    D3D11_BUFFER_DESC, D3D11_BUFFER_UAV, D3D11_BUFFER_UAV_FLAG_RAW, D3D11_CLEAR_DEPTH,
    D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_FLAG, D3D11_CULL_NONE,
    D3D11_FILL_SOLID, D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_VERTEX_DATA,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_MAP_WRITE_DISCARD, D3D11_QUERY_DESC,
    D3D11_QUERY_EVENT, D3D11_RASTERIZER_DESC, D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS,
    D3D11_RESOURCE_MISC_BUFFER_STRUCTURED, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_UAV_DIMENSION_BUFFER, D3D11_UNORDERED_ACCESS_VIEW_DESC,
    D3D11_UNORDERED_ACCESS_VIEW_DESC_0, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC,
    D3D11_USAGE_STAGING, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_D32_FLOAT, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32G32B32_FLOAT,
    DXGI_FORMAT_R32_TYPELESS, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIFactory6,
    DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED,
    DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_DRIVER_INTERNAL_ERROR,
    DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
};

use crate::bench::gpu::{check_hash, Gpu, GpuError, BATCH_SECS, COPY_BYTES, SCENE_H, SCENE_W};

/// Threads in the FP32 dispatch (256 per group) and FLOPs per thread per iteration: 16 steps of
/// 4 chains of float4 multiply-adds, 2 operations each.
const FMA_GROUPS: u32 = 1024;
const FMA_THREADS: u64 = FMA_GROUPS as u64 * 256;
const FLOPS_PER_ITER: u64 = 16 * 4 * 4 * 2;
/// Elements the check shader hashes, and every how many the CPU checks.
const CHECK_N: u32 = 1 << 20;
const CHECK_EVERY: usize = 256;
/// A batch never waits longer than this: the GPU has stopped responding.
const HUNG: Duration = Duration::from_secs(10);
/// Scene: draw calls a frame, instances per draw, sphere tessellation.
const DRAWS: u32 = 400;
const INSTANCES: u32 = 16;
const STACKS: u32 = 12;
const SLICES: u32 = 24;

const FMA_HLSL: &str = r"
RWStructuredBuffer<float4> Out : register(u0);
cbuffer P : register(b0) { uint iters; float ka; float kb; uint pad; };
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    float4 x0 = float4(id.x, id.x + 1, id.x + 2, id.x + 3) * 1e-6;
    float4 x1 = x0 + 0.5;
    float4 x2 = x0 + 0.25;
    float4 x3 = x0 + 0.75;
    [loop] for (uint i = 0; i < iters; i++) {
        [unroll] for (int k = 0; k < 16; k++) {
            x0 = mad(x0, ka, kb); x1 = mad(x1, ka, kb);
            x2 = mad(x2, ka, kb); x3 = mad(x3, ka, kb);
        }
    }
    Out[id.x] = x0 + x1 + x2 + x3;
}
";

const COPY_HLSL: &str = r"
RWByteAddressBuffer Src : register(u0);
RWByteAddressBuffer Dst : register(u1);
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    uint o = id.x * 16;
    Dst.Store4(o, Src.Load4(o));
}
";

const CHECK_HLSL: &str = r"
RWStructuredBuffer<uint> Out : register(u0);
[numthreads(256, 1, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    uint x = (id.x * 0x9E3779B9u) ^ 0x56524F21u;
    [loop] for (uint i = 0; i < 64; i++) {
        x ^= x >> 16; x *= 0x7FEB352Du;
        x ^= x >> 15; x *= 0x846CA68Bu;
        x ^= x >> 16;
    }
    Out[id.x] = x;
}
";

/// Video memory pattern (GPU tuning's stability test): every thread writes, then checks, 16
/// words; differences are counted in `Bad`.
const VRAM_HLSL: &str = r"
RWByteAddressBuffer Buf : register(u0);
RWByteAddressBuffer Bad : register(u1);
cbuffer P : register(b0) { uint seed; uint offset; uint pad0; uint pad1; };
uint pattern(uint i) {
    uint x = (i * 0x9E3779B9u) ^ seed;
    x ^= x >> 15; x *= 0x2C1B3C6Du; x ^= x >> 12;
    return x;
}
[numthreads(256, 1, 1)]
void fill(uint3 id : SV_DispatchThreadID) {
    uint w = id.x * 16;
    [unroll] for (uint k = 0; k < 16; k += 4) {
        uint i = offset + w + k;
        Buf.Store4((w + k) * 4, uint4(pattern(i), pattern(i + 1), pattern(i + 2), pattern(i + 3)));
    }
}
[numthreads(256, 1, 1)]
void verify(uint3 id : SV_DispatchThreadID) {
    uint w = id.x * 16;
    uint bad = 0;
    [unroll] for (uint k = 0; k < 16; k += 4) {
        uint i = offset + w + k;
        uint4 v = Buf.Load4((w + k) * 4);
        bad += (v.x != pattern(i) ? 1u : 0u) + (v.y != pattern(i + 1) ? 1u : 0u)
             + (v.z != pattern(i + 2) ? 1u : 0u) + (v.w != pattern(i + 3) ? 1u : 0u);
    }
    if (bad != 0) { uint old; Bad.InterlockedAdd(0, bad, old); }
}
";
/// Video memory is checked in chunks of this many MB (16 words a thread, 16384 groups).
const VRAM_CHUNK_MB: u32 = 256;
/// The fixed frame [`D3d::frame_hash`] renders.
const HASH_FRAME: u64 = 1000;

const SCENE_HLSL: &str = r"
cbuffer Frame : register(b0) { row_major float4x4 viewProj; float4 light; float4 eye; float4 clock; };
cbuffer Draw : register(b1) { float4 base; };
struct VSIn { float3 pos : POSITION; float3 nrm : NORMAL; uint inst : SV_InstanceID; };
struct VSOut { float4 pos : SV_Position; float3 wpos : TEXCOORD0; float3 nrm : NORMAL; float hue : TEXCOORD1; };
VSOut vs(VSIn i) {
    float3 off = float3(i.inst % 4, i.inst / 4, 0) * 2.5;
    float a = clock.x + base.w * 0.37 + i.inst * 0.11;
    float s = sin(a), c = cos(a);
    float3 p = float3(c * i.pos.x - s * i.pos.z, i.pos.y, s * i.pos.x + c * i.pos.z);
    float3 n = float3(c * i.nrm.x - s * i.nrm.z, i.nrm.y, s * i.nrm.x + c * i.nrm.z);
    float3 w = p + base.xyz + off;
    VSOut o;
    o.pos = mul(viewProj, float4(w, 1));
    o.wpos = w;
    o.nrm = n;
    o.hue = frac(base.w * 0.618 + i.inst * 0.05);
    return o;
}
float h3(float3 p) { return frac(sin(dot(p, float3(12.9898, 78.233, 37.719))) * 43758.5453); }
float noise(float3 p) {
    float3 i = floor(p); float3 f = frac(p); f = f * f * (3 - 2 * f);
    return lerp(lerp(lerp(h3(i), h3(i + float3(1, 0, 0)), f.x),
                     lerp(h3(i + float3(0, 1, 0)), h3(i + float3(1, 1, 0)), f.x), f.y),
                lerp(lerp(h3(i + float3(0, 0, 1)), h3(i + float3(1, 0, 1)), f.x),
                     lerp(h3(i + float3(0, 1, 1)), h3(i + float3(1, 1, 1)), f.x), f.y), f.z);
}
float4 ps(VSOut i) : SV_Target {
    float3 n = normalize(i.nrm);
    float t = 0; float amp = 0.5; float3 q = i.wpos * 2.0;
    [unroll] for (int k = 0; k < 6; k++) { t += noise(q) * amp; q *= 2.03; amp *= 0.5; }
    float3 col = 0.5 + 0.5 * cos(6.2831 * (i.hue + float3(0, 0.33, 0.67)));
    float3 l = normalize(light.xyz - i.wpos);
    float3 v = normalize(eye.xyz - i.wpos);
    float d = saturate(dot(n, l));
    float sp = pow(saturate(dot(reflect(-l, n), v)), 32);
    return float4(col * (0.15 + 0.85 * d) * (0.6 + 0.4 * t) + sp, 1);
}
";

fn other(what: &str, e: &windows::core::Error) -> GpuError {
    GpuError::Other(format!("{what}: {e}"))
}

fn is_reset(hr: HRESULT) -> bool {
    [
        DXGI_ERROR_DEVICE_REMOVED,
        DXGI_ERROR_DEVICE_HUNG,
        DXGI_ERROR_DEVICE_RESET,
        DXGI_ERROR_DRIVER_INTERNAL_ERROR,
    ]
    .contains(&hr)
}

/// The high-performance hardware adapter and its name.
fn high_performance() -> Option<(IDXGIAdapter1, String)> {
    // SAFETY: plain DXGI factory and adapter queries; the results are owned COM references.
    unsafe {
        let f: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let f6: Option<IDXGIFactory6> = f.cast().ok();
        for i in 0..16 {
            let a: IDXGIAdapter1 = match &f6 {
                Some(f6) => f6.EnumAdapterByGpuPreference(i, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE),
                None => f.EnumAdapters1(i),
            }
            .ok()?;
            let d = a.GetDesc1().ok()?;
            // Software adapters, and the Basic Render Driver (Microsoft's vendor id).
            if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 || d.VendorId == 0x1414 {
                continue;
            }
            let len = d
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(d.Description.len());
            let name = String::from_utf16_lossy(&d.Description[..len])
                .trim()
                .to_owned();
            return Some((a, name));
        }
        None
    }
}

pub fn adapter_name() -> Option<String> {
    high_performance().map(|(_, n)| n)
}

fn compile(src: &str, entry: PCSTR, target: PCSTR) -> Result<ID3DBlob, GpuError> {
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
        return Err(GpuError::Other(format!(
            "shader: {e} {}",
            msg.unwrap_or_default()
        )));
    }
    code.ok_or_else(|| GpuError::Other("shader: no code".into()))
}

fn bytes(b: &ID3DBlob) -> &[u8] {
    // SAFETY: the blob owns this memory for its lifetime, which bounds the slice's.
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer().cast(), b.GetBufferSize()) }
}

struct Fma {
    cs: ID3D11ComputeShader,
    out: ID3D11UnorderedAccessView,
    params: ID3D11Buffer,
    iters: u32,
}

struct CopyKernel {
    cs: ID3D11ComputeShader,
    views: [Option<ID3D11UnorderedAccessView>; 2],
    groups: u32,
    per_batch: u32,
}

struct Scene {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    vb: ID3D11Buffer,
    ib: ID3D11Buffer,
    indices: u32,
    frame_cb: ID3D11Buffer,
    draw_cb: ID3D11Buffer,
    color: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    dsv: ID3D11DepthStencilView,
    raster: ID3D11RasterizerState,
    frame: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrameConsts {
    view_proj: [[f32; 4]; 4],
    light: [f32; 4],
    eye: [f32; 4],
    clock: [f32; 4],
}

pub struct D3d {
    name: String,
    dev: ID3D11Device,
    ctx: ID3D11DeviceContext,
    size: (u32, u32),
    draws: u32,
    fma: Option<Fma>,
    copy: Option<CopyKernel>,
    scene: Option<Scene>,
    vram: Option<Vram>,
}

/// The pattern kernels, the chunks of video memory they use, and the difference counter.
struct Vram {
    fill: ID3D11ComputeShader,
    verify: ID3D11ComputeShader,
    params: ID3D11Buffer,
    chunks: Vec<ID3D11UnorderedAccessView>,
    bad: ID3D11Buffer,
    bad_view: ID3D11UnorderedAccessView,
    bad_read: ID3D11Buffer,
}

impl D3d {
    /// A device on the high-performance GPU, or on WARP (Windows' software rasteriser, for
    /// tests) with a small scene.
    pub fn open(warp: bool) -> Result<Self, GpuError> {
        let (adapter, name) = if warp {
            (None, "WARP".to_owned())
        } else {
            let (a, n) = high_performance()
                .ok_or_else(|| GpuError::Other("No Direct3D 11 GPU found.".into()))?;
            (Some(a), n)
        };
        let mut dev = None;
        let mut ctx = None;
        let adapter: Option<IDXGIAdapter> = adapter.and_then(|a| a.cast().ok());
        // SAFETY: out pointers are valid; an adapter needs the "unknown" driver type.
        unsafe {
            D3D11CreateDevice(
                adapter.as_ref(),
                if warp {
                    D3D_DRIVER_TYPE_WARP
                } else {
                    D3D_DRIVER_TYPE_UNKNOWN
                },
                HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                Some(&mut ctx),
            )
        }
        .map_err(|e| other("Direct3D 11 device", &e))?;
        let (Some(dev), Some(ctx)) = (dev, ctx) else {
            return Err(GpuError::Other("Direct3D 11 device: none".into()));
        };
        let (size, draws) = if warp {
            ((64, 64), 4)
        } else {
            ((SCENE_W, SCENE_H), DRAWS)
        };
        Ok(Self {
            name,
            dev,
            ctx,
            size,
            draws,
            fma: None,
            copy: None,
            scene: None,
            vram: None,
        })
    }

    /// A Direct3D failure: a reset when the device is gone.
    fn fail(&self, what: &str, e: &windows::core::Error) -> GpuError {
        // SAFETY: a plain query on a live device.
        let removed = unsafe { self.dev.GetDeviceRemovedReason() }.err();
        match removed {
            Some(r) => GpuError::Reset(format!("{what}: {:?}", r.code())),
            None if is_reset(e.code()) => GpuError::Reset(format!("{what}: {:?}", e.code())),
            None => other(what, e),
        }
    }

    fn event(&self) -> Result<ID3D11Query, GpuError> {
        let mut q = None;
        let desc = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        // SAFETY: valid descriptor and out pointer.
        unsafe { self.dev.CreateQuery(&desc, Some(&mut q)) }.map_err(|e| self.fail("query", &e))?;
        q.ok_or_else(|| GpuError::Other("query: none".into()))
    }

    /// Whether the GPU has passed `q`. Called through the vtable: the wrapper folds "not yet"
    /// (S_FALSE) into success.
    fn passed(&self, q: &ID3D11Query) -> Result<bool, GpuError> {
        let mut data = 0u32;
        // SAFETY: both interfaces are live; an event query writes a 4-byte BOOL into `data`.
        let hr: HRESULT = unsafe {
            (Interface::vtable(&self.ctx).GetData)(
                Interface::as_raw(&self.ctx),
                q.as_raw(),
                std::ptr::from_mut(&mut data).cast(),
                4,
                0,
            )
        };
        if hr.is_err() {
            return Err(self.fail("waiting", &windows::core::Error::from_hresult(hr)));
        }
        Ok(hr.0 == 0)
    }

    fn wait(&self, q: &ID3D11Query) -> Result<(), GpuError> {
        let start = Instant::now();
        while !self.passed(q)? {
            if start.elapsed() > HUNG {
                return Err(GpuError::Reset("the GPU stopped responding".into()));
            }
            std::thread::yield_now();
        }
        Ok(())
    }

    fn end(&self, q: &ID3D11Query) {
        // SAFETY: a live query on the immediate context.
        unsafe {
            self.ctx.End(q);
            self.ctx.Flush();
        }
    }

    fn buffer(
        &self,
        desc: &D3D11_BUFFER_DESC,
        init: Option<&[u8]>,
    ) -> Result<ID3D11Buffer, GpuError> {
        let data = init.map(|d| D3D11_SUBRESOURCE_DATA {
            pSysMem: d.as_ptr().cast(),
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        });
        let mut b = None;
        // SAFETY: the descriptor and the initial data outlive the call.
        unsafe {
            self.dev
                .CreateBuffer(desc, data.as_ref().map(std::ptr::from_ref), Some(&mut b))
        }
        .map_err(|e| self.fail("buffer", &e))?;
        b.ok_or_else(|| GpuError::Other("buffer: none".into()))
    }

    fn uav(
        &self,
        buf: &ID3D11Buffer,
        format: DXGI_FORMAT,
        elements: u32,
        raw: bool,
    ) -> Result<ID3D11UnorderedAccessView, GpuError> {
        let desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
            Format: format,
            ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
            Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                Buffer: D3D11_BUFFER_UAV {
                    FirstElement: 0,
                    NumElements: elements,
                    Flags: if raw {
                        D3D11_BUFFER_UAV_FLAG_RAW.0 as u32
                    } else {
                        0
                    },
                },
            },
        };
        let mut v = None;
        // SAFETY: valid descriptor and out pointer.
        unsafe {
            self.dev
                .CreateUnorderedAccessView(buf, Some(&desc), Some(&mut v))
        }
        .map_err(|e| self.fail("view", &e))?;
        v.ok_or_else(|| GpuError::Other("view: none".into()))
    }

    fn compute_shader(&self, src: &str) -> Result<ID3D11ComputeShader, GpuError> {
        let code = compile(src, windows::core::s!("main"), windows::core::s!("cs_5_0"))?;
        let mut cs = None;
        // SAFETY: the bytecode outlives the call.
        unsafe {
            self.dev
                .CreateComputeShader(bytes(&code), None, Some(&mut cs))
        }
        .map_err(|e| self.fail("compute shader", &e))?;
        cs.ok_or_else(|| GpuError::Other("compute shader: none".into()))
    }

    fn structured(&self, elements: u32, stride: u32) -> Result<ID3D11Buffer, GpuError> {
        self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: elements * stride,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
                StructureByteStride: stride,
            },
            None,
        )
    }

    fn constants(&self, bytes: u32, dynamic: bool) -> Result<ID3D11Buffer, GpuError> {
        self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: bytes,
                Usage: if dynamic {
                    D3D11_USAGE_DYNAMIC
                } else {
                    D3D11_USAGE_DEFAULT
                },
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: if dynamic {
                    D3D11_CPU_ACCESS_WRITE.0 as u32
                } else {
                    0
                },
                MiscFlags: 0,
                StructureByteStride: 0,
            },
            None,
        )
    }

    /// Runs `submit` (one batch) again and again for `secs` with two batches in flight;
    /// returns the batches done and the seconds they took.
    fn batches(
        &self,
        secs: f64,
        stop: &AtomicBool,
        submit: &dyn Fn(&ID3D11DeviceContext),
    ) -> Result<(u64, f64), GpuError> {
        let q = [self.event()?, self.event()?];
        let start = Instant::now();
        let until = start + Duration::from_secs_f64(secs);
        let mut n = 0u64;
        loop {
            submit(&self.ctx);
            self.end(&q[(n % 2) as usize]);
            n += 1;
            if n >= 2 {
                self.wait(&q[(n % 2) as usize])?;
            }
            if Instant::now() >= until || stop.load(Ordering::Relaxed) {
                break;
            }
        }
        self.wait(&q[((n - 1) % 2) as usize])?;
        Ok((n, start.elapsed().as_secs_f64()))
    }

    /// Times one batch, for sizing the next ones.
    fn time_one(&self, submit: &dyn Fn(&ID3D11DeviceContext)) -> Result<f64, GpuError> {
        let q = self.event()?;
        let start = Instant::now();
        submit(&self.ctx);
        self.end(&q);
        self.wait(&q)?;
        Ok(start.elapsed().as_secs_f64())
    }

    /// Makes the FP32 kernel and sizes it so one dispatch takes about a batch.
    fn fma_setup(&mut self) -> Result<(), GpuError> {
        if self.fma.is_none() {
            let cs = self.compute_shader(FMA_HLSL)?;
            let out = self.structured(FMA_THREADS as u32, 16)?;
            let out = self.uav(&out, DXGI_FORMAT_UNKNOWN, FMA_THREADS as u32, false)?;
            let params = self.constants(16, false)?;
            let mut f = Fma {
                cs,
                out,
                params,
                iters: 16,
            };
            // Twice: the first guess is skewed by the submit overhead of a tiny dispatch.
            for _ in 0..2 {
                self.set_iters(&f);
                let t = self.time_one(&|c| dispatch_fma(c, &f))?;
                let want = (f64::from(f.iters) * BATCH_SECS / t.max(1e-6)).clamp(16.0, 65_536.0);
                f.iters = want as u32;
            }
            self.set_iters(&f);
            self.fma = Some(f);
        }
        Ok(())
    }

    fn set_iters(&self, f: &Fma) {
        let p: [u32; 4] = [f.iters, 0.9999f32.to_bits(), 0.0001f32.to_bits(), 0];
        // SAFETY: the 16-byte source matches the 16-byte constant buffer.
        unsafe {
            self.ctx
                .UpdateSubresource(&f.params, 0, None, p.as_ptr().cast(), 0, 0);
        }
    }

    /// Makes the copy kernel and sizes a batch to about [`BATCH_SECS`].
    fn copy_setup(&mut self) -> Result<(), GpuError> {
        if self.copy.is_none() {
            let half = (COPY_BYTES / 2) as u32;
            let desc = D3D11_BUFFER_DESC {
                ByteWidth: half,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
                StructureByteStride: 0,
            };
            let src = self.buffer(&desc, None)?;
            let dst = self.buffer(&desc, None)?;
            let views = [
                Some(self.uav(&src, DXGI_FORMAT_R32_TYPELESS, half / 4, true)?),
                Some(self.uav(&dst, DXGI_FORMAT_R32_TYPELESS, half / 4, true)?),
            ];
            let mut c = CopyKernel {
                cs: self.compute_shader(COPY_HLSL)?,
                views,
                groups: half / 16 / 256,
                per_batch: 1,
            };
            for _ in 0..2 {
                let t = self.time_one(&|ctx| {
                    for _ in 0..c.per_batch {
                        dispatch_copy(ctx, &c);
                    }
                })?;
                let want = f64::from(c.per_batch) * BATCH_SECS / t.max(1e-6);
                c.per_batch = want.clamp(1.0, 64.0) as u32;
            }
            self.copy = Some(c);
        }
        Ok(())
    }

    fn scene_setup(&mut self) -> Result<(), GpuError> {
        if self.scene.is_some() {
            return Ok(());
        }
        let vs_code = compile(
            SCENE_HLSL,
            windows::core::s!("vs"),
            windows::core::s!("vs_5_0"),
        )?;
        let ps_code = compile(
            SCENE_HLSL,
            windows::core::s!("ps"),
            windows::core::s!("ps_5_0"),
        )?;
        let (mut vs, mut ps, mut layout) = (None, None, None);
        let elems = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: windows::core::s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: windows::core::s!("NORMAL"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 12,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];
        // SAFETY: bytecode and descriptors outlive the calls; out pointers are valid.
        unsafe {
            self.dev
                .CreateVertexShader(bytes(&vs_code), None, Some(&mut vs))
                .and_then(|()| {
                    self.dev
                        .CreatePixelShader(bytes(&ps_code), None, Some(&mut ps))
                })
                .and_then(|()| {
                    self.dev
                        .CreateInputLayout(&elems, bytes(&vs_code), Some(&mut layout))
                })
        }
        .map_err(|e| self.fail("scene shaders", &e))?;
        let (verts, idx) = sphere(STACKS, SLICES);
        let vb = self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: (verts.len() * 24) as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                StructureByteStride: 0,
            },
            Some(as_bytes(&verts)),
        )?;
        let ib = self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: (idx.len() * 2) as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_INDEX_BUFFER.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                StructureByteStride: 0,
            },
            Some(as_bytes(&idx)),
        )?;
        let frame_cb = self.constants(std::mem::size_of::<FrameConsts>() as u32, false)?;
        let draw_cb = self.constants(16, true)?;
        let (w, h) = self.size;
        let tex = |format, bind: i32| -> Result<ID3D11Texture2D, GpuError> {
            let desc = D3D11_TEXTURE2D_DESC {
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
                BindFlags: bind as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut t = None;
            // SAFETY: valid descriptor and out pointer.
            unsafe { self.dev.CreateTexture2D(&desc, None, Some(&mut t)) }
                .map_err(|e| self.fail("render target", &e))?;
            t.ok_or_else(|| GpuError::Other("render target: none".into()))
        };
        let color = tex(DXGI_FORMAT_R8G8B8A8_UNORM, D3D11_BIND_RENDER_TARGET.0)?;
        let depth = tex(DXGI_FORMAT_D32_FLOAT, D3D11_BIND_DEPTH_STENCIL.0)?;
        let (mut rtv, mut dsv, mut raster) = (None, None, None);
        let rd = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: false.into(),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: true.into(),
            ScissorEnable: false.into(),
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
        };
        // SAFETY: live resources, valid descriptors and out pointers.
        unsafe {
            self.dev
                .CreateRenderTargetView(&color, None, Some(&mut rtv))
                .and_then(|()| {
                    self.dev
                        .CreateDepthStencilView(&depth, None, Some(&mut dsv))
                })
                .and_then(|()| self.dev.CreateRasterizerState(&rd, Some(&mut raster)))
        }
        .map_err(|e| self.fail("render target", &e))?;
        match (vs, ps, layout, rtv, dsv, raster) {
            (Some(vs), Some(ps), Some(layout), Some(rtv), Some(dsv), Some(raster)) => {
                self.scene = Some(Scene {
                    vs,
                    ps,
                    layout,
                    vb,
                    ib,
                    indices: idx.len() as u32,
                    frame_cb,
                    draw_cb,
                    color,
                    rtv,
                    dsv,
                    raster,
                    frame: 0,
                });
                Ok(())
            }
            _ => Err(GpuError::Other("scene: incomplete".into())),
        }
    }

    /// Submits one frame of the scene: the next one, or frame `fixed`.
    fn draw(&mut self, fixed: Option<u64>) -> Result<(), GpuError> {
        let (w, h) = self.size;
        let draws = self.draws;
        let Some(s) = self.scene.as_mut() else {
            return Err(GpuError::Other("scene: not ready".into()));
        };
        let frame = fixed.unwrap_or_else(|| {
            s.frame += 1;
            s.frame
        });
        let consts = FrameConsts {
            view_proj: view_proj(w as f32 / h as f32),
            light: [40.0, 120.0, -30.0, 0.0],
            eye: [EYE[0], EYE[1], EYE[2], 0.0],
            clock: [frame as f32 * 0.01, 0.0, 0.0, 0.0],
        };
        let ctx = &self.ctx;
        let stride = 24u32;
        let offset = 0u32;
        // SAFETY: every resource is live; pointers passed for the call's duration only.
        unsafe {
            ctx.UpdateSubresource(
                &s.frame_cb,
                0,
                None,
                std::ptr::from_ref(&consts).cast(),
                0,
                0,
            );
            ctx.OMSetRenderTargets(Some(&[Some(s.rtv.clone())]), &s.dsv);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            ctx.RSSetState(&s.raster);
            ctx.ClearRenderTargetView(&s.rtv, &[0.02, 0.02, 0.05, 1.0]);
            ctx.ClearDepthStencilView(&s.dsv, D3D11_CLEAR_DEPTH.0, 1.0, 0);
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            ctx.IASetInputLayout(&s.layout);
            ctx.IASetVertexBuffers(
                0,
                1,
                Some(&Some(s.vb.clone())),
                Some(&stride),
                Some(&offset),
            );
            ctx.IASetIndexBuffer(&s.ib, DXGI_FORMAT_R16_UINT, 0);
            ctx.VSSetShader(&s.vs, None);
            ctx.PSSetShader(&s.ps, None);
            ctx.VSSetConstantBuffers(
                0,
                Some(&[Some(s.frame_cb.clone()), Some(s.draw_cb.clone())]),
            );
            ctx.PSSetConstantBuffers(0, Some(&[Some(s.frame_cb.clone())]));
            for d in 0..draws {
                let mut m = D3D11_MAPPED_SUBRESOURCE::default();
                ctx.Map(&s.draw_cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m))
                    .map_err(|e| GpuError::Other(format!("map: {e}")))?;
                let base: [f32; 4] = [
                    (d % 20) as f32 * 11.0 - 104.5,
                    0.0,
                    (d / 20) as f32 * 11.0 + 10.0,
                    d as f32,
                ];
                std::ptr::copy_nonoverlapping(base.as_ptr(), m.pData.cast::<f32>(), 4);
                ctx.Unmap(&s.draw_cb, 0);
                ctx.DrawIndexedInstanced(s.indices, INSTANCES, 0, 0, 0);
            }
        }
        Ok(())
    }
}

impl D3d {
    fn compute_entry(&self, src: &str, entry: PCSTR) -> Result<ID3D11ComputeShader, GpuError> {
        let code = compile(src, entry, windows::core::s!("cs_5_0"))?;
        let mut cs = None;
        // SAFETY: the bytecode outlives the call.
        unsafe {
            self.dev
                .CreateComputeShader(bytes(&code), None, Some(&mut cs))
        }
        .map_err(|e| self.fail("compute shader", &e))?;
        cs.ok_or_else(|| GpuError::Other("compute shader: none".into()))
    }

    fn raw(&self, bytes: u32, usage_staging: bool) -> Result<ID3D11Buffer, GpuError> {
        self.buffer(
            &if usage_staging {
                D3D11_BUFFER_DESC {
                    ByteWidth: bytes,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                }
            } else {
                D3D11_BUFFER_DESC {
                    ByteWidth: bytes,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_UNORDERED_ACCESS.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
                    StructureByteStride: 0,
                }
            },
            None,
        )
    }

    /// The pattern kernels and `chunks` chunks of video memory (fewer when the card has no
    /// room for more; at least one).
    fn vram_setup(&mut self, chunks: u32) -> Result<(), GpuError> {
        if self
            .vram
            .as_ref()
            .is_some_and(|v| v.chunks.len() as u32 == chunks)
        {
            return Ok(());
        }
        self.vram = None;
        let bytes = VRAM_CHUNK_MB << 20;
        let mut views = Vec::new();
        for _ in 0..chunks.max(1) {
            let made = self
                .raw(bytes, false)
                .and_then(|b| self.uav(&b, DXGI_FORMAT_R32_TYPELESS, bytes / 4, true));
            match made {
                Ok(v) => views.push(v),
                Err(e @ GpuError::Reset(_)) => return Err(e),
                Err(e) if views.is_empty() => return Err(e),
                Err(_) => break,
            }
        }
        let bad = self.raw(16, false)?;
        let bad_view = self.uav(&bad, DXGI_FORMAT_R32_TYPELESS, 4, true)?;
        self.vram = Some(Vram {
            fill: self.compute_entry(VRAM_HLSL, windows::core::s!("fill"))?,
            verify: self.compute_entry(VRAM_HLSL, windows::core::s!("verify"))?,
            params: self.constants(16, false)?,
            chunks: views,
            bad,
            bad_view,
            bad_read: self.raw(16, true)?,
        });
        Ok(())
    }
}

fn dispatch_fma(c: &ID3D11DeviceContext, f: &Fma) {
    // SAFETY: live shader, view and buffer.
    unsafe {
        c.CSSetShader(&f.cs, None);
        c.CSSetUnorderedAccessViews(0, 1, Some(&Some(f.out.clone())), None);
        c.CSSetConstantBuffers(0, Some(&[Some(f.params.clone())]));
        c.Dispatch(FMA_GROUPS, 1, 1);
    }
}

fn dispatch_copy(c: &ID3D11DeviceContext, k: &CopyKernel) {
    // SAFETY: live shader and views.
    unsafe {
        c.CSSetShader(&k.cs, None);
        c.CSSetUnorderedAccessViews(0, 2, Some(k.views.as_ptr()), None);
        c.Dispatch(k.groups, 1, 1);
    }
}

impl Gpu for D3d {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn compute(&mut self, secs: f64, stop: &AtomicBool) -> Result<f64, GpuError> {
        self.fma_setup()?;
        let f = self
            .fma
            .as_ref()
            .ok_or_else(|| GpuError::Other("FP32: not ready".into()))?;
        let iters = u64::from(f.iters);
        let (n, took) = self.batches(secs, stop, &|c| dispatch_fma(c, f))?;
        Ok((n * FMA_THREADS * iters * FLOPS_PER_ITER) as f64 / took / 1e9)
    }

    fn bandwidth(&mut self, secs: f64, stop: &AtomicBool) -> Result<f64, GpuError> {
        self.copy_setup()?;
        let k = self
            .copy
            .as_ref()
            .ok_or_else(|| GpuError::Other("copy: not ready".into()))?;
        let per = k.per_batch;
        let (n, took) = self.batches(secs, stop, &|c| {
            for _ in 0..per {
                dispatch_copy(c, k);
            }
        })?;
        Ok((n * u64::from(per) * COPY_BYTES) as f64 / took / 1e9)
    }

    fn check(&mut self) -> Result<u32, GpuError> {
        let cs = self.compute_shader(CHECK_HLSL)?;
        let out = self.structured(CHECK_N, 4)?;
        let view = self.uav(&out, DXGI_FORMAT_UNKNOWN, CHECK_N, false)?;
        let staging = self.buffer(
            &D3D11_BUFFER_DESC {
                ByteWidth: CHECK_N * 4,
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                StructureByteStride: 0,
            },
            None,
        )?;
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: live resources; the mapped memory is read only while mapped, within its size.
        unsafe {
            self.ctx.CSSetShader(&cs, None);
            self.ctx
                .CSSetUnorderedAccessViews(0, 1, Some(&Some(view)), None);
            self.ctx.Dispatch(CHECK_N / 256, 1, 1);
            self.ctx.CopyResource(&staging, &out);
            self.ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
                .map_err(|e| self.fail("reading back", &e))?;
            let got = std::slice::from_raw_parts(m.pData.cast::<u32>(), CHECK_N as usize);
            let bad = (0..CHECK_N as usize)
                .step_by(CHECK_EVERY)
                .filter(|&i| got[i] != check_hash(i as u32))
                .count();
            self.ctx.Unmap(&staging, 0);
            Ok(bad as u32)
        }
    }

    fn frames(
        &mut self,
        secs: f64,
        stop: &AtomicBool,
        times: &mut Vec<f64>,
        on_frame: &mut dyn FnMut(),
    ) -> Result<(), GpuError> {
        self.scene_setup()?;
        let q = [self.event()?, self.event()?, self.event()?];
        let until = Instant::now() + Duration::from_secs_f64(secs);
        let (mut sent, mut done) = (0u64, 0u64);
        let mut last: Option<Instant> = None;
        let mut finish = |me: &Self, done: &mut u64| -> Result<(), GpuError> {
            me.wait(&q[(*done % 3) as usize])?;
            let now = Instant::now();
            if let Some(l) = last {
                times.push(now.duration_since(l).as_secs_f64() * 1000.0);
            }
            last = Some(now);
            *done += 1;
            on_frame();
            Ok(())
        };
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            self.draw(None)?;
            self.end(&q[(sent % 3) as usize]);
            sent += 1;
            // Two frames in flight, as in games.
            while sent - done > 2 {
                finish(self, &mut done)?;
            }
        }
        while done < sent {
            finish(self, &mut done)?;
        }
        Ok(())
    }

    fn frame_hash(&mut self) -> Result<u64, GpuError> {
        self.hash_frame()
    }

    fn vram_check(&mut self, mb: u32, seed: u32) -> Result<(u64, f64), GpuError> {
        self.check_vram(mb, seed)
    }

    fn free_vram_mb(&self) -> Option<u64> {
        self.vram_free()
    }
}

impl D3d {
    /// [`Gpu::frame_hash`]: frame [`HASH_FRAME`], copied back and hashed row by row.
    fn hash_frame(&mut self) -> Result<u64, GpuError> {
        self.scene_setup()?;
        self.draw(Some(HASH_FRAME))?;
        let (w, h) = self.size;
        let s = self
            .scene
            .as_ref()
            .ok_or_else(|| GpuError::Other("scene: not ready".into()))?;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        // SAFETY: valid descriptor and out pointer.
        unsafe { self.dev.CreateTexture2D(&desc, None, Some(&mut staging)) }
            .map_err(|e| self.fail("frame read-back", &e))?;
        let staging = staging.ok_or_else(|| GpuError::Other("frame read-back: none".into()))?;
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: live resources; each row is read within the mapped pitch while mapped.
        unsafe {
            self.ctx.CopyResource(&staging, &s.color);
            self.ctx
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))
                .map_err(|e| self.fail("frame read-back", &e))?;
            let mut hash = crate::bench::gpu::FNV_START;
            for row in 0..h as usize {
                let p = m.pData.cast::<u8>().add(row * m.RowPitch as usize);
                hash =
                    crate::bench::gpu::fnv1a(hash, std::slice::from_raw_parts(p, w as usize * 4));
            }
            self.ctx.Unmap(&staging, 0);
            Ok(hash)
        }
    }

    /// [`Gpu::vram_check`].
    fn check_vram(&mut self, mb: u32, seed: u32) -> Result<(u64, f64), GpuError> {
        self.vram_setup(mb.div_ceil(VRAM_CHUNK_MB).max(1))?;
        let q = self.event()?;
        let v = self
            .vram
            .as_ref()
            .ok_or_else(|| GpuError::Other("video memory: not ready".into()))?;
        let words = (VRAM_CHUNK_MB << 20) / 4;
        let groups = words / 16 / 256;
        let start = Instant::now();
        // SAFETY: live shaders, views and buffers; the constants are 16 bytes as declared.
        unsafe {
            self.ctx.ClearUnorderedAccessViewUint(&v.bad_view, &[0; 4]);
            for cs in [&v.fill, &v.verify] {
                self.ctx.CSSetShader(cs, None);
                for (n, chunk) in v.chunks.iter().enumerate() {
                    let p: [u32; 4] = [seed, (n as u32).wrapping_mul(words), 0, 0];
                    self.ctx
                        .UpdateSubresource(&v.params, 0, None, p.as_ptr().cast(), 0, 0);
                    self.ctx
                        .CSSetConstantBuffers(0, Some(&[Some(v.params.clone())]));
                    self.ctx.CSSetUnorderedAccessViews(
                        0,
                        2,
                        Some([Some(chunk.clone()), Some(v.bad_view.clone())].as_ptr()),
                        None,
                    );
                    self.ctx.Dispatch(groups, 1, 1);
                }
            }
            self.ctx.CopyResource(&v.bad_read, &v.bad);
        }
        self.end(&q);
        self.wait(&q)?;
        let took = start.elapsed().as_secs_f64();
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: a 16-byte staging buffer, read while mapped.
        let bad = unsafe {
            self.ctx
                .Map(&v.bad_read, 0, D3D11_MAP_READ, 0, Some(&mut m))
                .map_err(|e| self.fail("reading back", &e))?;
            let n = *m.pData.cast::<u32>();
            self.ctx.Unmap(&v.bad_read, 0);
            n
        };
        let bytes = 2.0 * v.chunks.len() as f64 * f64::from(VRAM_CHUNK_MB << 20);
        Ok((u64::from(bad), bytes / took.max(1e-6) / 1e9))
    }

    /// [`Gpu::free_vram_mb`]: the adapter's budget for this process less what it uses.
    fn vram_free(&self) -> Option<u64> {
        use windows::Win32::Graphics::Dxgi::{
            IDXGIAdapter3, IDXGIDevice, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
            DXGI_QUERY_VIDEO_MEMORY_INFO,
        };
        // SAFETY: plain interface queries on live objects; the info struct is written in place.
        unsafe {
            let dxgi: IDXGIDevice = self.dev.cast().ok()?;
            let a: IDXGIAdapter3 = dxgi.GetAdapter().ok()?.cast().ok()?;
            let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                .ok()?;
            Some(info.Budget.saturating_sub(info.CurrentUsage) >> 20)
        }
    }
}

const EYE: [f32; 3] = [0.0, 45.0, -60.0];

/// The fixed camera: from [`EYE`] towards the middle of the grid, 60° vertical field of view,
/// depth 0–1 (left-handed, column vectors, stored row by row).
fn view_proj(aspect: f32) -> [[f32; 4]; 4] {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let norm = |a: [f32; 3]| {
        let l = dot(a, a).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let z = norm(sub([0.0, 0.0, 110.0], EYE));
    let x = norm(cross([0.0, 1.0, 0.0], z));
    let y = cross(z, x);
    let view = [
        [x[0], x[1], x[2], -dot(x, EYE)],
        [y[0], y[1], y[2], -dot(y, EYE)],
        [z[0], z[1], z[2], -dot(z, EYE)],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let (near, far) = (1.0f32, 400.0f32);
    let ys = 1.0 / (30f32.to_radians()).tan();
    let xs = ys / aspect;
    let proj = [
        [xs, 0.0, 0.0, 0.0],
        [0.0, ys, 0.0, 0.0],
        [0.0, 0.0, far / (far - near), -near * far / (far - near)],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let mut m = [[0.0f32; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| proj[r][k] * view[k][c]).sum();
        }
    }
    m
}

/// A unit sphere: position and normal per vertex, and triangle indices.
fn sphere(stacks: u32, slices: u32) -> (Vec<[f32; 6]>, Vec<u16>) {
    use std::f32::consts::PI;
    let mut v = Vec::new();
    for i in 0..=stacks {
        let phi = PI * i as f32 / stacks as f32;
        for j in 0..=slices {
            let th = 2.0 * PI * j as f32 / slices as f32;
            let n = [phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin()];
            v.push([n[0], n[1], n[2], n[0], n[1], n[2]]);
        }
    }
    let mut idx = Vec::new();
    for i in 0..stacks {
        for j in 0..slices {
            let a = (i * (slices + 1) + j) as u16;
            let b = a + slices as u16 + 1;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    (v, idx)
}

fn as_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: plain-old-data slices of floats or integers, read as bytes.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), std::mem::size_of_val(v)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' software rasteriser runs every workload, and the shader's hashes match the
    /// CPU's.
    #[test]
    fn every_workload_runs_on_warp() {
        let stop = AtomicBool::new(false);
        let mut g = D3d::open(true).expect("WARP device");
        assert_eq!(g.name(), "WARP");
        assert!(g.compute(0.2, &stop).unwrap() > 0.0);
        assert!(g.bandwidth(0.2, &stop).unwrap() > 0.0);
        assert_eq!(g.check().unwrap(), 0, "the GPU's hashes match the CPU's");
        let mut times = Vec::new();
        let mut n = 0;
        g.frames(0.3, &stop, &mut times, &mut || n += 1).unwrap();
        assert!(
            n >= 2 && times.len() + 1 == n,
            "{n} frames, {} times",
            times.len()
        );
        // The stability test's checks: a fixed frame hashes the same every time, and video
        // memory keeps its pattern.
        let a = g.frame_hash().unwrap();
        g.frames(0.1, &AtomicBool::new(false), &mut Vec::new(), &mut || {})
            .unwrap();
        assert_eq!(g.frame_hash().unwrap(), a);
        let (bad, gbs) = g.vram_check(256, 7).unwrap();
        assert_eq!(bad, 0);
        assert!(gbs > 0.0);
        assert_eq!(g.vram_check(256, 8).unwrap().0, 0);
        // A stop ends it after the frames in flight.
        let stop = AtomicBool::new(true);
        let mut times = Vec::new();
        g.frames(60.0, &stop, &mut times, &mut || {}).unwrap();
        assert!(times.len() <= 2);
    }

    #[test]
    fn the_camera_sees_the_grid() {
        let m = view_proj(16.0 / 9.0);
        // The grid's middle lands near the middle of the screen, in front of the camera.
        let p = [0.0, 0.0, 110.0, 1.0];
        let clip: Vec<f32> = (0..4)
            .map(|r| (0..4).map(|c| m[r][c] * p[c]).sum())
            .collect();
        let (x, y, z) = (clip[0] / clip[3], clip[1] / clip[3], clip[2] / clip[3]);
        assert!(
            x.abs() < 0.01 && y.abs() < 0.01 && (0.0..1.0).contains(&z),
            "{x} {y} {z}"
        );
        let (v, i) = sphere(STACKS, SLICES);
        assert_eq!((v.len(), i.len()), (325, 1728));
    }
}

//! GPU workloads (method v1), behind [`Gpu`] so the runner stays testable. On Windows,
//! [`open`] gives the Direct3D 11 implementation on the high-performance GPU; elsewhere there is
//! none and the GPU areas are missing.
//!
//! Direct3D devices are used on the thread that made them: the runner calls a [`GpuFactory`]
//! on the worker thread that runs the GPU area.

use std::sync::atomic::AtomicBool;

/// Offscreen scene size.
pub const SCENE_W: u32 = 2560;
pub const SCENE_H: u32 = 1440;
/// Bytes copied by the bandwidth shader.
pub const COPY_BYTES: u64 = 256 << 20;
/// A batch of GPU work aims to take about this long, s; a stop waits at most for one batch.
pub const BATCH_SECS: f64 = 0.02;

/// A GPU failure: a driver reset ("device removed") counts towards Stability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuError {
    Reset(String),
    Other(String),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reset(m) => write!(f, "the GPU driver reset ({m})"),
            Self::Other(m) => f.write_str(m),
        }
    }
}

pub trait Gpu {
    /// The adapter's name, as Windows gives it.
    fn name(&self) -> String;
    /// FP32 throughput over about `secs`, GFLOPS (2 operations per multiply-add).
    fn compute(&mut self, secs: f64, stop: &AtomicBool) -> Result<f64, GpuError>;
    /// Video memory copy bandwidth over about `secs`, GB/s (bytes read plus written).
    fn bandwidth(&mut self, secs: f64, stop: &AtomicBool) -> Result<f64, GpuError>;
    /// Runs the integer hash shader and checks a sample on the CPU: the wrong values.
    fn check(&mut self) -> Result<u32, GpuError>;
    /// Renders scene frames for about `secs` (two in flight), pushing each frame's time in ms.
    /// `on_frame` is called after each frame (Sustained counts them).
    fn frames(
        &mut self,
        secs: f64,
        stop: &AtomicBool,
        times: &mut Vec<f64>,
        on_frame: &mut dyn FnMut(),
    ) -> Result<(), GpuError>;
    /// Renders one fixed frame of the scene and hashes it (GPU tuning's stability test, 1.6):
    /// a card renders it the same every time, unless it makes mistakes.
    fn frame_hash(&mut self) -> Result<u64, GpuError> {
        Err(GpuError::Other("not available".into()))
    }
    /// Writes a pattern made from `seed` over about `mb` MB of video memory, then reads it back
    /// on the GPU: the words that came back different, and the GB/s written and read.
    fn vram_check(&mut self, mb: u32, seed: u32) -> Result<(u64, f64), GpuError> {
        let _ = (mb, seed);
        Err(GpuError::Other("not available".into()))
    }
    /// Video memory Windows lets this program use now, MB.
    fn free_vram_mb(&self) -> Option<u64> {
        None
    }
}

/// FNV-1a over `bytes`, continuing from `h` (start with [`FNV_START`]).
pub fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub const FNV_START: u64 = 0xCBF2_9CE4_8422_2325;

/// The pattern word `i` of [`Gpu::vram_check`] holds for `seed` (the shader works it out the
/// same way).
pub fn vram_pattern(i: u32, seed: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ seed;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x
}

/// Makes a [`Gpu`] on the calling thread.
pub type GpuFactory = std::sync::Arc<dyn Fn() -> Result<Box<dyn Gpu>, GpuError> + Send + Sync>;

/// The integer hash both the shader and the CPU check work out for element `i`.
pub fn check_hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x5652_4F21;
    for _ in 0..64 {
        x ^= x >> 16;
        x = x.wrapping_mul(0x7FEB_352D);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846C_A68B);
        x ^= x >> 16;
    }
    x
}

/// The frame rate and the 1 % low (1000 ÷ the 99th-percentile frame time) from frame times in
/// ms.
pub fn frame_stats(times: &[f64], secs: f64) -> Option<(f64, f64)> {
    if times.is_empty() || secs <= 0.0 {
        return None;
    }
    let mut s = times.to_vec();
    s.sort_by(f64::total_cmp);
    let idx = ((s.len() as f64 * 0.99).ceil() as usize).clamp(1, s.len()) - 1;
    let p99 = s[idx];
    Some((
        times.len() as f64 / secs,
        if p99 > 0.0 { 1000.0 / p99 } else { 0.0 },
    ))
}

/// The high-performance GPU's name, without making a device; `None` when there is no hardware
/// Direct3D 11 GPU.
pub fn adapter_name() -> Option<String> {
    #[cfg(windows)]
    return super::win::gpu::adapter_name();
    #[cfg(not(windows))]
    None
}

/// The Direct3D 11 implementation on the high-performance GPU, where there is one.
pub fn factory() -> Option<GpuFactory> {
    #[cfg(windows)]
    {
        adapter_name()?;
        Some(std::sync::Arc::new(|| {
            super::win::gpu::D3d::open(false).map(|g| Box::new(g) as Box<dyn Gpu>)
        }))
    }
    #[cfg(not(windows))]
    None
}

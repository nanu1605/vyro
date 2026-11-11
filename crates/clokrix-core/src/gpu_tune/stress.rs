//! The stability test (1.6): rounds of full load, each checked. A round runs the FP32 kernel,
//! then the integer hash the CPU checks, then the scene, then a fixed frame whose hash must
//! match the first one of the test, then a pattern over most of the free video memory. A wrong
//! result, a different frame, a memory word that changed or a driver reset fails the settings.
//! The app runs it on its own thread while the service holds the settings on trial.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::bench::gpu::{Gpu, GpuError};

/// Seconds of FP32 load and of scene frames each round.
const LOAD_SECS: f64 = 2.0;
/// Most video memory a round checks, MB.
pub const VRAM_MAX_MB: u64 = 4096;
/// Least, MB (also when Windows does not say how much is free).
pub const VRAM_MIN_MB: u64 = 256;

/// How a test went.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub passed: bool,
    /// Why it failed, in words ("3 wrong results").
    pub reason: Option<String>,
    /// The median memory bandwidth the pattern passes measured, GB/s.
    pub bandwidth_gbs: Option<f64>,
    pub rounds: u32,
    /// Whether it was stopped before its time.
    pub stopped: bool,
}

impl Outcome {
    fn failed(reason: String, rounds: u32, bandwidth_gbs: Option<f64>) -> Self {
        Self {
            passed: false,
            reason: Some(reason),
            bandwidth_gbs,
            rounds,
            stopped: false,
        }
    }
}

/// Video memory one round checks: about 70 % of what is free, within the bounds above.
pub fn vram_mb(free_mb: Option<u64>) -> u32 {
    let mb = free_mb.map_or(VRAM_MIN_MB, |f| f * 7 / 10);
    mb.clamp(VRAM_MIN_MB, VRAM_MAX_MB) as u32
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// Runs the test for `secs` seconds (whole rounds), calling `progress` with 0–1 after each step.
/// `Err` when the test could not run at all (no GPU, a check that is not available): that says
/// nothing about the settings.
pub fn run(
    gpu: &mut dyn Gpu,
    secs: f64,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<Outcome, String> {
    let start = Instant::now();
    let mb = vram_mb(gpu.free_vram_mb());
    let mut reference: Option<u64> = None;
    let mut bandwidths = Vec::new();
    let mut rounds = 0u32;
    let mut times = Vec::new();
    let elapsed = |start: Instant| start.elapsed().as_secs_f64();
    // A driver reset fails the settings; any other error means the test itself could not run.
    let fail = |e: GpuError, rounds: u32, bw: &mut Vec<f64>| -> Result<Outcome, String> {
        match e {
            GpuError::Reset(m) => Ok(Outcome::failed(
                format!("the driver reset ({m})"),
                rounds,
                median(bw),
            )),
            GpuError::Other(m) => Err(m),
        }
    };
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(Outcome {
                passed: false,
                reason: Some("stopped".into()),
                bandwidth_gbs: median(&mut bandwidths),
                rounds,
                stopped: true,
            });
        }
        if rounds > 0 && elapsed(start) >= secs {
            break;
        }
        let seed = 0x5652_4F00 ^ rounds;
        if let Err(e) = gpu.compute(LOAD_SECS, stop) {
            return fail(e, rounds, &mut bandwidths);
        }
        progress((elapsed(start) / secs).min(1.0));
        match gpu.check() {
            Ok(0) => {}
            Ok(n) => {
                return Ok(Outcome::failed(
                    format!("{n} wrong results from the GPU's calculations"),
                    rounds,
                    median(&mut bandwidths),
                ))
            }
            Err(e) => return fail(e, rounds, &mut bandwidths),
        }
        times.clear();
        if let Err(e) = gpu.frames(LOAD_SECS, stop, &mut times, &mut || {}) {
            return fail(e, rounds, &mut bandwidths);
        }
        progress((elapsed(start) / secs).min(1.0));
        match gpu.frame_hash() {
            Ok(h) => match reference {
                None => reference = Some(h),
                Some(r) if r != h => {
                    return Ok(Outcome::failed(
                        "a frame came out different (rendering mistakes)".into(),
                        rounds,
                        median(&mut bandwidths),
                    ))
                }
                Some(_) => {}
            },
            Err(e) => return fail(e, rounds, &mut bandwidths),
        }
        match gpu.vram_check(mb, seed) {
            Ok((0, gbs)) => bandwidths.push(gbs),
            Ok((n, _)) => {
                return Ok(Outcome::failed(
                    format!("{n} words of video memory changed"),
                    rounds,
                    median(&mut bandwidths),
                ))
            }
            Err(e) => return fail(e, rounds, &mut bandwidths),
        }
        rounds += 1;
        progress((elapsed(start) / secs).min(1.0));
    }
    Ok(Outcome {
        passed: true,
        reason: None,
        bandwidth_gbs: median(&mut bandwidths),
        rounds,
        stopped: false,
    })
}

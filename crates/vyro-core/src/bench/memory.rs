//! Memory workloads (method v1): read and copy bandwidth over two 256 MiB buffers on every
//! logical processor, and latency as nanoseconds per step along a random cycle over 256 MiB that
//! neither the caches nor the prefetchers can follow. The buffers are freed when the area ends.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// Bytes in each bandwidth buffer, and in the latency chain.
pub const BUFFER_BYTES: usize = 256 << 20;
const WORDS: usize = BUFFER_BYTES / 8;
const PATTERN: u64 = 0x9E37_79B9_7F4A_7C15;
/// The latency chain visits one 4-byte slot per 64-byte line.
const LINE_SLOTS: usize = 16;
/// Steps between checks for a stop or the end of a pass.
const CHASE_CHUNK: u32 = 1 << 16;

/// The two bandwidth buffers. `src[i] = i × PATTERN`, so any share's sum is known in advance.
pub struct Buffers {
    src: Vec<u64>,
    dst: Vec<u64>,
}

impl Buffers {
    /// `None` when the memory cannot be had.
    pub fn new(words: usize) -> Option<Self> {
        let mut src = Vec::new();
        src.try_reserve_exact(words).ok()?;
        src.extend((0..words as u64).map(|i| i.wrapping_mul(PATTERN)));
        let mut dst = Vec::new();
        dst.try_reserve_exact(words).ok()?;
        dst.resize(words, 0);
        Some(Self { src, dst })
    }

    pub fn full() -> Option<Self> {
        Self::new(WORDS)
    }

    pub fn bytes(&self) -> usize {
        self.src.len() * 8
    }
}

/// The sum of `i × PATTERN` for `i` in `lo..hi`, wrapping.
fn expected_sum(lo: usize, hi: usize) -> u64 {
    // Σ i = (hi − lo)(lo + hi − 1) / 2, worked out without overflow by halving the even factor.
    let (n, m) = ((hi - lo) as u64, (lo + hi) as u64 - 1);
    let s = if n % 2 == 0 {
        (n / 2).wrapping_mul(m)
    } else {
        n.wrapping_mul(m / 2)
    };
    s.wrapping_mul(PATTERN)
}

fn shares(len: usize, threads: usize) -> Vec<(usize, usize)> {
    let t = threads.max(1);
    (0..t).map(|k| (len * k / t, len * (k + 1) / t)).collect()
}

/// Read bandwidth, GB/s: every thread sums its share again and again for `secs`. A wrong sum
/// adds to `errors`.
pub fn read(
    buf: &Buffers,
    threads: usize,
    secs: f64,
    stop: &AtomicBool,
    errors: &AtomicU32,
) -> f64 {
    let src = &buf.src;
    let start = Instant::now();
    let until = start + Duration::from_secs_f64(secs);
    let bytes: u64 = std::thread::scope(|sc| {
        let workers: Vec<_> = shares(src.len(), threads)
            .into_iter()
            .map(|(lo, hi)| {
                sc.spawn(move || {
                    let want = expected_sum(lo, hi);
                    let mut done = 0u64;
                    while !stop.load(Ordering::Relaxed) && Instant::now() < until {
                        let sum = src[lo..hi].iter().fold(0u64, |a, &w| a.wrapping_add(w));
                        if sum != want {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                        done += ((hi - lo) * 8) as u64;
                    }
                    done
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or(0)).sum()
    });
    bytes as f64 / start.elapsed().as_secs_f64().max(1e-9) / 1e9
}

/// Copy bandwidth, GB/s (bytes read plus bytes written): every thread copies its share from
/// one buffer to the other for `secs`, then checks what arrived.
pub fn copy(
    buf: &mut Buffers,
    threads: usize,
    secs: f64,
    stop: &AtomicBool,
    errors: &AtomicU32,
) -> f64 {
    let src = &buf.src;
    let len = src.len();
    let start = Instant::now();
    let until = start + Duration::from_secs_f64(secs);
    let mut rest: &mut [u64] = &mut buf.dst;
    let mut parts = Vec::new();
    for (lo, hi) in shares(len, threads) {
        let (a, b) = rest.split_at_mut(hi - lo);
        parts.push((lo, hi, a));
        rest = b;
    }
    let bytes: u64 = std::thread::scope(|sc| {
        let workers: Vec<_> = parts
            .into_iter()
            .map(|(lo, hi, dst)| {
                sc.spawn(move || {
                    let mut done = 0u64;
                    while !stop.load(Ordering::Relaxed) && Instant::now() < until {
                        dst.copy_from_slice(&src[lo..hi]);
                        done += ((hi - lo) * 16) as u64;
                    }
                    if done > 0 {
                        let sum = dst.iter().fold(0u64, |a, &w| a.wrapping_add(w));
                        if sum != expected_sum(lo, hi) {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    done
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or(0)).sum()
    });
    let secs = start.elapsed().as_secs_f64().max(1e-9);
    bytes as f64 / secs / 1e9
}

/// A random cycle through one slot of every 64-byte line (Sattolo's algorithm over the lines,
/// fixed seed): `next[i]` is the slot after `i`.
pub struct Chain {
    next: Vec<u32>,
}

impl Chain {
    pub fn new(bytes: usize) -> Option<Self> {
        let slots = bytes / 4;
        let lines = slots / LINE_SLOTS;
        if lines < 2 {
            return None;
        }
        let mut order = Vec::new();
        order.try_reserve_exact(lines).ok()?;
        order.extend(0..u32::try_from(lines).ok()?);
        let mut rng = fastrand::Rng::with_seed(0x4c41_5445_4e43_5921);
        for i in (1..lines).rev() {
            order.swap(i, rng.usize(..i));
        }
        let mut next = Vec::new();
        next.try_reserve_exact(slots).ok()?;
        next.resize(slots, 0u32);
        // Sattolo leaves `order` a single cycle read as "line → order[line]".
        for line in 0..lines {
            let to = order[line] as usize;
            next[line * LINE_SLOTS] = u32::try_from(to * LINE_SLOTS).ok()?;
        }
        Some(Self { next })
    }

    pub fn full() -> Option<Self> {
        Self::new(BUFFER_BYTES)
    }

    /// Nanoseconds per step over at least `secs`; `None` when stopped first.
    pub fn latency(&self, secs: f64, stop: &AtomicBool) -> Option<f64> {
        let start = Instant::now();
        let until = start + Duration::from_secs_f64(secs);
        let mut at = 0u32;
        let mut steps = 0u64;
        while Instant::now() < until {
            if stop.load(Ordering::Relaxed) {
                return None;
            }
            for _ in 0..CHASE_CHUNK {
                at = self.next[at as usize];
            }
            steps += u64::from(CHASE_CHUNK);
        }
        // Keep the chase from being optimised away.
        std::hint::black_box(at);
        Some(start.elapsed().as_secs_f64() * 1e9 / steps.max(1) as f64)
    }

    #[cfg(test)]
    pub(crate) fn cycle_len(&self) -> usize {
        let mut at = 0usize;
        let mut n = 0usize;
        loop {
            at = self.next[at] as usize;
            n += 1;
            if at == 0 || n > self.next.len() {
                return n;
            }
        }
    }
}

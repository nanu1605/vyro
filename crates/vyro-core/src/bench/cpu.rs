//! CPU workloads (method v1). One round runs four fixed kernels, each on its own deterministic
//! input, and yields a checksum; points are rounds per second. A round's checksum must equal the
//! reference worked out before the run, so a PC that calculates wrongly (an unstable overclock,
//! heat) shows up as calculation errors, the way Prime95 finds one.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

const WORDS: usize = 2048;
const TEXT: usize = 64 * 1024;
const TABLE_BITS: u32 = 12;
const N: usize = 32;
const KEYS: usize = 8192;

/// The fixed inputs, shared read-only by every worker.
pub struct Inputs {
    words: Vec<u64>,
    text: Vec<u8>,
    a: Vec<f64>,
    b: Vec<f64>,
    keys: Vec<u32>,
}

impl Default for Inputs {
    fn default() -> Self {
        Self::new()
    }
}

impl Inputs {
    pub fn new() -> Self {
        let mut rng = fastrand::Rng::with_seed(0x5652_4f42_454e_4348);
        let words = (0..WORDS).map(|_| rng.u64(..)).collect();
        // Compressible text: words from a small vocabulary.
        let vocab: Vec<Vec<u8>> = (0..64)
            .map(|_| {
                let len = rng.usize(3..9);
                (0..len).map(|_| rng.u8(b'a'..=b'z')).collect()
            })
            .collect();
        let mut text = Vec::with_capacity(TEXT + 16);
        while text.len() < TEXT {
            text.extend_from_slice(&vocab[rng.usize(..vocab.len())]);
            text.push(if rng.u8(..) < 24 { b'\n' } else { b' ' });
        }
        text.truncate(TEXT);
        let a = (0..N * N).map(|_| rng.f64() * 2.0 - 1.0).collect();
        let b = (0..N * N).map(|_| rng.f64() * 2.0 - 1.0).collect();
        let keys = (0..KEYS).map(|_| rng.u32(..)).collect();
        Self {
            words,
            text,
            a,
            b,
            keys,
        }
    }
}

/// A worker's own working memory.
pub struct Scratch {
    table: Vec<u32>,
    c: Vec<f64>,
    sort: Vec<u32>,
}

impl Default for Scratch {
    fn default() -> Self {
        Self {
            table: vec![u32::MAX; 1 << TABLE_BITS],
            c: vec![0.0; N * N],
            sort: vec![0; KEYS],
        }
    }
}

/// One round: the four kernels, folded into one checksum.
pub fn round(inp: &Inputs, s: &mut Scratch) -> u64 {
    hash(&inp.words)
        ^ lz(&inp.text, &mut s.table).rotate_left(16)
        ^ matmul(&inp.a, &inp.b, &mut s.c).rotate_left(32)
        ^ sort(&inp.keys, &mut s.sort).rotate_left(48)
}

/// Multiply-and-shift mixing over 16 KiB (integer ALU, L1).
fn hash(words: &[u64]) -> u64 {
    let mut acc = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..4 {
        for &w in words {
            let mut x = w ^ acc;
            x ^= x >> 33;
            x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
            x ^= x >> 33;
            x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
            x ^= x >> 33;
            acc = acc.rotate_left(7) ^ x;
        }
    }
    acc
}

/// Greedy LZ-style match search with a hash table (branches, L2).
fn lz(text: &[u8], table: &mut [u32]) -> u64 {
    table.fill(u32::MAX);
    let n = text.len();
    let mut i = 0usize;
    let mut total = 0u64;
    while i + 4 <= n {
        let key = u32::from_le_bytes([text[i], text[i + 1], text[i + 2], text[i + 3]]);
        let h = (key.wrapping_mul(2_654_435_761) >> (32 - TABLE_BITS)) as usize;
        let cand = table[h];
        table[h] = u32::try_from(i).unwrap_or(u32::MAX);
        if cand != u32::MAX {
            let c = cand as usize;
            let mut len = 0usize;
            while i + len < n && len < 258 && text[c + len] == text[i + len] {
                len += 1;
            }
            if len >= 4 {
                total = total
                    .wrapping_mul(31)
                    .wrapping_add((len as u64) << 20 | c as u64);
                i += len;
                continue;
            }
        }
        i += 1;
    }
    total
}

/// A 32 × 32 double-precision matrix product (floating point).
fn matmul(a: &[f64], b: &[f64], c: &mut [f64]) -> u64 {
    for i in 0..N {
        for j in 0..N {
            let mut s = 0.0;
            for k in 0..N {
                s += a[i * N + k] * b[k * N + j];
            }
            c[i * N + j] = s;
        }
    }
    c.iter().fold(0u64, |h, v| h.rotate_left(5) ^ v.to_bits())
}

/// Sorting 8 192 integers (branches, memory).
fn sort(keys: &[u32], out: &mut [u32]) -> u64 {
    out.copy_from_slice(keys);
    out.sort_unstable();
    out.iter().enumerate().fold(0u64, |h, (i, &v)| {
        h.rotate_left(3) ^ (u64::from(v) * (i as u64 | 1))
    })
}

/// The reference checksum every round must match.
pub fn reference(inp: &Inputs) -> u64 {
    round(inp, &mut Scratch::default())
}

/// Runs rounds until `until` or a stop; returns the rounds done. Each round takes well under
/// a millisecond, so a stop is seen at once. A wrong checksum adds to `errors`; `count`, when
/// given, is bumped every round (Sustained reads it per loop).
pub fn spin(
    inp: &Inputs,
    expect: u64,
    until: Instant,
    stop: &AtomicBool,
    errors: &AtomicU32,
    count: Option<&AtomicU64>,
) -> u64 {
    let mut s = Scratch::default();
    let mut done = 0u64;
    // At least one round unless stopped: a worker the OS starts after the pass's end (a busy
    // PC) still counts, so a pass never scores zero.
    while !stop.load(Ordering::Relaxed) {
        if round(inp, &mut s) != expect {
            errors.fetch_add(1, Ordering::Relaxed);
        }
        done += 1;
        if let Some(c) = count {
            c.fetch_add(1, Ordering::Relaxed);
        }
        if Instant::now() >= until {
            break;
        }
    }
    done
}

/// Points over `secs` on `threads` threads started together: rounds per second.
pub fn pass(
    inp: &Inputs,
    expect: u64,
    threads: usize,
    secs: f64,
    stop: &AtomicBool,
    errors: &AtomicU32,
) -> f64 {
    let start = Instant::now();
    let until = start + std::time::Duration::from_secs_f64(secs);
    let total: u64 = std::thread::scope(|sc| {
        let workers: Vec<_> = (0..threads.max(1))
            .map(|_| sc.spawn(|| spin(inp, expect, until, stop, errors, None)))
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or(0)).sum()
    });
    total as f64 / start.elapsed().as_secs_f64().max(1e-9)
}

//! Storage workloads (method v1): a 1 GiB temporary file on the drive that holds the temporary
//! folder, read and written uncached (Windows: no buffering, write through) so that Windows'
//! cache does not count. Sequential passes use 1 MiB blocks from 4 threads, each on its own
//! quarter; random reads take 4 KiB at a time from one thread (queue depth 1). Every 4 KiB
//! sector starts with its own offset, so data read back is checked. The file is deleted when
//! the area ends or stops, and one left behind by a crash is deleted at the next run.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub const BLOCK: usize = 1 << 20;
pub const SECTOR: usize = 4096;
pub const THREADS: usize = 4;
/// A measured sequential pass stops after this long even if its quarter is not done.
pub const PASS_CAP_SECS: f64 = 8.0;
pub const RANDOM_SECS: f64 = 4.0;
const PREFIX: &str = "vyro-bench-";
const STAMP: u64 = 0x5659_524f_4449_534b;

/// The temporary file; deleted on drop.
pub struct TempFile {
    path: PathBuf,
    bytes: u64,
}

impl TempFile {
    /// Deletes files left by earlier runs, then creates this run's file at full length.
    pub fn create(dir: &Path, bytes: u64) -> io::Result<Self> {
        clean(dir);
        let path = dir.join(format!("{PREFIX}{}.tmp", std::process::id()));
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        let me = Self { path, bytes };
        f.set_len(bytes)?;
        Ok(me)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            log::warn!("benchmark: cannot delete {}: {e}", self.path.display());
        }
    }
}

/// Deletes benchmark files left in `dir` (by a crash or a power cut).
pub fn clean(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(PREFIX) && name.ends_with(".tmp") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// A buffer aligned for uncached I/O.
struct Aligned {
    raw: Vec<u8>,
    off: usize,
    len: usize,
}

impl Aligned {
    fn new(len: usize) -> Self {
        let raw = vec![0u8; len + SECTOR];
        let off = raw.as_ptr().align_offset(SECTOR);
        Self { raw, off, len }
    }

    fn get(&mut self) -> &mut [u8] {
        &mut self.raw[self.off..self.off + self.len]
    }
}

fn open(path: &Path, write: bool) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true).write(write);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
        const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;
        o.custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH);
    }
    o.open(path)
}

fn write_at(f: &File, mut buf: &[u8], mut at: u64) -> io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_write(f, buf, at)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::write_at(f, buf, at)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        at += n as u64;
    }
    Ok(())
}

fn read_at(f: &File, mut buf: &mut [u8], mut at: u64) -> io::Result<()> {
    while !buf.is_empty() {
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(f, buf, at)?;
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(f, buf, at)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        at += n as u64;
    }
    Ok(())
}

/// Incompressible filler; each sector's first 8 bytes are then its offset.
fn filler(seed: u64) -> Vec<u8> {
    let mut rng = fastrand::Rng::with_seed(seed);
    (0..BLOCK).map(|_| rng.u8(..)).collect()
}

fn stamp(block: &mut [u8], at: u64) {
    for (i, sector) in block.as_chunks_mut::<SECTOR>().0.iter_mut().enumerate() {
        let v = (at + (i * SECTOR) as u64) ^ STAMP;
        sector[..8].copy_from_slice(&v.to_le_bytes());
    }
}

/// Sectors in `block` (read from `at`) whose stamp is wrong.
fn bad_sectors(block: &[u8], at: u64) -> u32 {
    let mut bad = 0;
    for (i, sector) in block.as_chunks::<SECTOR>().0.iter().enumerate() {
        let want = (at + (i * SECTOR) as u64) ^ STAMP;
        let mut b = [0u8; 8];
        b.copy_from_slice(&sector[..8]);
        if u64::from_le_bytes(b) != want {
            bad += 1;
        }
    }
    bad
}

/// One sequential pass over the file from [`THREADS`] threads, MB/s. `cap` stops it early
/// (the warm-up writes the whole file). Bad sectors read back add to `errors`.
pub fn sequential(
    file: &TempFile,
    write: bool,
    cap: Option<f64>,
    stop: &AtomicBool,
    errors: &AtomicU32,
) -> io::Result<f64> {
    let quarter = file.bytes / THREADS as u64;
    let start = Instant::now();
    let until = cap.map(|c| start + Duration::from_secs_f64(c));
    let bytes = std::thread::scope(|sc| -> io::Result<u64> {
        let workers: Vec<_> = (0..THREADS)
            .map(|k| {
                let path = file.path();
                sc.spawn(move || -> io::Result<u64> {
                    let f = open(path, write)?;
                    let mut buf = Aligned::new(BLOCK);
                    if write {
                        buf.get().copy_from_slice(&filler(k as u64 + 1));
                    }
                    let lo = quarter * k as u64;
                    let mut at = lo;
                    while at + BLOCK as u64 <= lo + quarter {
                        if stop.load(Ordering::Relaxed)
                            || until.is_some_and(|u| Instant::now() >= u)
                        {
                            break;
                        }
                        if write {
                            stamp(buf.get(), at);
                            write_at(&f, buf.get(), at)?;
                        } else {
                            read_at(&f, buf.get(), at)?;
                            let bad = bad_sectors(buf.get(), at);
                            if bad > 0 {
                                errors.fetch_add(bad, Ordering::Relaxed);
                            }
                        }
                        at += BLOCK as u64;
                    }
                    Ok(at - lo)
                })
            })
            .collect();
        let mut total = 0;
        for w in workers {
            total += w
                .join()
                .map_err(|_| io::Error::other("storage worker panicked"))??;
        }
        Ok(total)
    })?;
    Ok(bytes as f64 / start.elapsed().as_secs_f64().max(1e-9) / 1e6)
}

/// Random 4 KiB reads, one at a time, for `secs`: IOPS.
pub fn random_read(
    file: &TempFile,
    secs: f64,
    seed: u64,
    stop: &AtomicBool,
    errors: &AtomicU32,
) -> io::Result<f64> {
    let f = open(file.path(), false)?;
    let mut buf = Aligned::new(SECTOR);
    let sectors = file.bytes / SECTOR as u64;
    let mut rng = fastrand::Rng::with_seed(seed);
    let start = Instant::now();
    let until = start + Duration::from_secs_f64(secs);
    let mut n = 0u64;
    while !stop.load(Ordering::Relaxed) && Instant::now() < until {
        let at = rng.u64(..sectors) * SECTOR as u64;
        read_at(&f, buf.get(), at)?;
        let bad = bad_sectors(buf.get(), at);
        if bad > 0 {
            errors.fetch_add(bad, Ordering::Relaxed);
        }
        n += 1;
    }
    Ok(n as f64 / start.elapsed().as_secs_f64().max(1e-9))
}

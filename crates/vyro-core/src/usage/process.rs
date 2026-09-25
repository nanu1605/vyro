//! Per-process CPU / RAM from `NtQuerySystemInformation(SystemProcessInformation)`.
//!
//! The buffer is a chain of `SYSTEM_PROCESS_INFORMATION` records (x64 layout below); decoding
//! it here keeps the unsafe Windows side to a single syscall.

use std::collections::HashMap;

use crate::snapshot::ProcessSnapshot;

// x64 `SYSTEM_PROCESS_INFORMATION` field offsets.
const NEXT_ENTRY: usize = 0;
const CREATE_TIME: usize = 32;
const USER_TIME: usize = 40;
const KERNEL_TIME: usize = 48;
const IMAGE_NAME_LEN: usize = 56;
const IMAGE_NAME_PTR: usize = 64;
const PID: usize = 80;
const WORKING_SET: usize = 144;
const WORKING_SET_PRIVATE: usize = 8;
/// Fixed part of a record (thread records follow it).
pub const RECORD_SIZE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProcess {
    pub pid: u32,
    pub name: String,
    /// Distinguishes a reused pid.
    pub create_time: u64,
    /// User + kernel, 100 ns units.
    pub cpu_time: u64,
    /// Private working set, bytes (Task Manager's "Memory" column).
    pub private_ws: u64,
    pub working_set: u64,
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off + 8)?.try_into().ok()?))
}

/// Decodes the process chain. `base` is the buffer's address in memory, needed because
/// `ImageName.Buffer` is an absolute pointer into the same buffer.
pub fn parse_process_list(buf: &[u8], base: usize) -> Vec<RawProcess> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(rec) = buf.get(at..).filter(|r| r.len() >= RECORD_SIZE) {
        let (Some(next), Some(pid)) = (u32_at(rec, NEXT_ENTRY), u64_at(rec, PID)) else {
            break;
        };
        let name = image_name(buf, base, rec).unwrap_or_default();
        let pid = u32::try_from(pid).unwrap_or(u32::MAX);
        let name = match (pid, name.is_empty()) {
            (0, true) => "System Idle Process".to_owned(),
            (_, true) => "System".to_owned(),
            _ => name,
        };
        out.push(RawProcess {
            pid,
            name,
            create_time: u64_at(rec, CREATE_TIME).unwrap_or(0),
            cpu_time: u64_at(rec, USER_TIME)
                .unwrap_or(0)
                .saturating_add(u64_at(rec, KERNEL_TIME).unwrap_or(0)),
            private_ws: u64_at(rec, WORKING_SET_PRIVATE).unwrap_or(0),
            working_set: u64_at(rec, WORKING_SET).unwrap_or(0),
        });
        if next == 0 {
            break;
        }
        at += next as usize;
    }
    out
}

fn image_name(buf: &[u8], base: usize, rec: &[u8]) -> Option<String> {
    let len = usize::from(u16_at(rec, IMAGE_NAME_LEN)?);
    let ptr = usize::try_from(u64_at(rec, IMAGE_NAME_PTR)?).ok()?;
    if len == 0 || ptr == 0 {
        return None;
    }
    let start = ptr.checked_sub(base)?;
    let bytes = buf.get(start..start.checked_add(len)?)?;
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    Some(String::from_utf16_lossy(&wide))
}

/// A process with rates computed against the previous sample.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    /// Share of total machine CPU, %; `None` on a process's first sample.
    pub cpu_pct: Option<f64>,
    pub ram_mb: f64,
}

/// Turns successive cumulative CPU times into percentages.
#[derive(Debug, Default)]
pub struct ProcessTracker {
    logical_cpus: u32,
    prev: HashMap<(u32, u64), u64>,
    prev_at: Option<u64>,
}

impl ProcessTracker {
    pub fn new(logical_cpus: u32) -> Self {
        Self {
            logical_cpus: logical_cpus.max(1),
            ..Default::default()
        }
    }

    /// `now` is a monotonic clock in 100 ns units.
    pub fn update(&mut self, list: Vec<RawProcess>, now: u64) -> Vec<ProcessSample> {
        let elapsed = self
            .prev_at
            .map(|t| now.saturating_sub(t))
            .filter(|&e| e > 0);
        let mut next = HashMap::with_capacity(list.len());
        let out = list
            .into_iter()
            .filter(|p| p.pid != 0)
            .map(|p| {
                let key = (p.pid, p.create_time);
                let cpu_pct = match (elapsed, self.prev.get(&key)) {
                    (Some(e), Some(&before)) => Some(
                        (p.cpu_time.saturating_sub(before) as f64
                            / (e as f64 * f64::from(self.logical_cpus))
                            * 100.0)
                            .clamp(0.0, 100.0),
                    ),
                    _ => None,
                };
                next.insert(key, p.cpu_time);
                ProcessSample {
                    pid: p.pid,
                    name: p.name,
                    cpu_pct,
                    // Private working set is what Task Manager shows; some kernels (and Wine)
                    // leave it zero, so fall back to the full working set.
                    ram_mb: if p.private_ws > 0 {
                        p.private_ws
                    } else {
                        p.working_set
                    } as f64
                        / (1024.0 * 1024.0),
                }
            })
            .collect();
        self.prev = next;
        self.prev_at = Some(now);
        out
    }
}

/// The union of the top `n` processes by CPU, by RAM and by GPU, ordered by CPU then RAM.
pub fn top_processes(
    samples: Vec<ProcessSample>,
    gpu: &HashMap<u32, f64>,
    n: usize,
) -> Vec<ProcessSnapshot> {
    let mut all: Vec<ProcessSnapshot> = samples
        .into_iter()
        .map(|s| ProcessSnapshot {
            pid: s.pid,
            gpu_pct: gpu.get(&s.pid).copied(),
            name: s.name,
            cpu_pct: s.cpu_pct,
            ram_mb: Some(s.ram_mb),
        })
        .collect();
    let mut keep = vec![false; all.len()];
    let mut mark = |key: &dyn Fn(&ProcessSnapshot) -> f64| {
        let mut idx: Vec<usize> = (0..all.len()).collect();
        idx.sort_by(|&a, &b| key(&all[b]).total_cmp(&key(&all[a])));
        for &i in idx.iter().take(n) {
            if key(&all[i]) > 0.0 {
                keep[i] = true;
            }
        }
    };
    mark(&|p| p.cpu_pct.unwrap_or(0.0));
    mark(&|p| p.ram_mb.unwrap_or(0.0));
    mark(&|p| p.gpu_pct.unwrap_or(0.0));
    let mut kept: Vec<ProcessSnapshot> = all
        .drain(..)
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(p))
        .collect();
    kept.sort_by(|a, b| {
        b.cpu_pct
            .unwrap_or(0.0)
            .total_cmp(&a.cpu_pct.unwrap_or(0.0))
            .then(b.ram_mb.unwrap_or(0.0).total_cmp(&a.ram_mb.unwrap_or(0.0)))
    });
    kept
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::*;

    pub struct Proc<'a> {
        pub pid: u64,
        pub name: &'a str,
        pub create: u64,
        pub user: u64,
        pub kernel: u64,
        pub private_ws: u64,
        /// Extra bytes after the record (thread entries in the real thing).
        pub threads: usize,
    }

    /// Builds a buffer in the kernel's layout, with names stored after all records.
    pub fn build(procs: &[Proc], base: usize) -> Vec<u8> {
        let sizes: Vec<usize> = procs.iter().map(|p| RECORD_SIZE + p.threads).collect();
        let names_at: usize = sizes.iter().sum();
        let mut buf = vec![0u8; names_at];
        let mut name_off = names_at;
        let mut at = 0;
        for (i, p) in procs.iter().enumerate() {
            let wide: Vec<u8> = p.name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let next = if i + 1 == procs.len() {
                0
            } else {
                sizes[i] as u32
            };
            let put = |buf: &mut Vec<u8>, off: usize, bytes: &[u8]| {
                buf[at + off..at + off + bytes.len()].copy_from_slice(bytes);
            };
            put(&mut buf, NEXT_ENTRY, &next.to_le_bytes());
            put(&mut buf, WORKING_SET_PRIVATE, &p.private_ws.to_le_bytes());
            put(&mut buf, CREATE_TIME, &p.create.to_le_bytes());
            put(&mut buf, USER_TIME, &p.user.to_le_bytes());
            put(&mut buf, KERNEL_TIME, &p.kernel.to_le_bytes());
            put(&mut buf, PID, &p.pid.to_le_bytes());
            put(&mut buf, WORKING_SET, &(p.private_ws * 2).to_le_bytes());
            if !wide.is_empty() {
                put(&mut buf, IMAGE_NAME_LEN, &(wide.len() as u16).to_le_bytes());
                put(
                    &mut buf,
                    IMAGE_NAME_PTR,
                    &((base + name_off) as u64).to_le_bytes(),
                );
            }
            buf.extend_from_slice(&wide);
            name_off += wide.len();
            at += sizes[i];
        }
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{build, Proc};
    use super::*;

    const BASE: usize = 0x7FF6_0000_0000;
    const MIB: u64 = 1024 * 1024;

    fn procs(chrome_cpu: u64) -> Vec<u8> {
        build(
            &[
                Proc {
                    pid: 0,
                    name: "",
                    create: 0,
                    user: 0,
                    kernel: 900,
                    private_ws: 0,
                    threads: 64,
                },
                Proc {
                    pid: 4,
                    name: "",
                    create: 0,
                    user: 0,
                    kernel: 100,
                    private_ws: MIB,
                    threads: 0,
                },
                Proc {
                    pid: 1200,
                    name: "chrome.exe",
                    create: 77,
                    user: chrome_cpu,
                    kernel: 0,
                    private_ws: 512 * MIB,
                    threads: 128,
                },
                Proc {
                    pid: 3000,
                    name: "vyro.exe",
                    create: 5,
                    user: 10,
                    kernel: 10,
                    private_ws: 40 * MIB,
                    threads: 0,
                },
            ],
            BASE,
        )
    }

    #[test]
    fn parses_the_record_chain() {
        let list = parse_process_list(&procs(1000), BASE);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].name, "System Idle Process");
        assert_eq!(list[1].name, "System");
        assert_eq!(list[2].pid, 1200);
        assert_eq!(list[2].name, "chrome.exe");
        assert_eq!(list[2].cpu_time, 1000);
        assert_eq!(list[2].private_ws, 512 * MIB);
        assert_eq!(list[2].working_set, 1024 * MIB);
        assert_eq!(list[3].name, "vyro.exe");
    }

    #[test]
    fn survives_truncated_and_bogus_buffers() {
        let full = procs(1000);
        assert!(parse_process_list(&full[..100], BASE).is_empty());
        // name pointer outside the buffer → name falls back, record still read
        let list = parse_process_list(&full, BASE + 1_000_000);
        assert_eq!(list.len(), 4);
        assert_eq!(list[2].name, "System");
    }

    #[test]
    fn cpu_percent_over_time() {
        let mut t = ProcessTracker::new(4);
        let first = t.update(parse_process_list(&procs(0), BASE), 0);
        assert!(first.iter().all(|p| p.cpu_pct.is_none()));
        assert!(first.iter().all(|p| p.pid != 0), "idle process dropped");
        // 1 s = 10_000_000 × 100 ns; chrome burned 2 s of CPU over 4 CPUs → 50 %
        let second = t.update(parse_process_list(&procs(20_000_000), BASE), 10_000_000);
        let chrome = second.iter().find(|p| p.pid == 1200).unwrap();
        assert_eq!(chrome.cpu_pct, Some(50.0));
        assert_eq!(chrome.ram_mb, 512.0);
        let vyro = second.iter().find(|p| p.pid == 3000).unwrap();
        assert_eq!(vyro.cpu_pct, Some(0.0));
    }

    #[test]
    fn top_n_is_a_union() {
        let s = |pid, cpu, ram| ProcessSample {
            pid,
            name: format!("p{pid}"),
            cpu_pct: Some(cpu),
            ram_mb: ram,
        };
        let gpu = HashMap::from([(5, 30.0)]);
        let top = top_processes(
            vec![
                s(1, 50.0, 10.0),
                s(2, 0.0, 900.0),
                s(3, 1.0, 1.0),
                s(4, 0.0, 0.5),
                s(5, 0.0, 0.1),
            ],
            &gpu,
            1,
        );
        let pids: Vec<u32> = top.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![1, 2, 5]);
        assert_eq!(top[2].gpu_pct, Some(30.0));
    }
}

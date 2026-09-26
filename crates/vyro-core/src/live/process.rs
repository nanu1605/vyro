//! Top processes by CPU, RAM and GPU from one `NtQuerySystemInformation` call per tick.

use std::time::Instant;

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SystemProcessInformation};
use windows::Win32::Foundation::STATUS_INFO_LENGTH_MISMATCH;

use super::{logical_processors, GpuShare};
use crate::lock;
use crate::provider::{BoxError, Provider};
use crate::registry::Registry;
use crate::snapshot::SensorSnapshot;
use crate::usage::process::{parse_process_list, top_processes, ProcessTracker};

/// How many processes each ranking (CPU, RAM, GPU) contributes to the snapshot.
const TOP_N: usize = 25;

pub struct ProcessProvider {
    gpu: GpuShare,
    tracker: ProcessTracker,
    /// Reused between ticks; u64 storage keeps the records 8-byte aligned.
    buf: Vec<u64>,
    started: Instant,
}

impl ProcessProvider {
    pub fn new(gpu: GpuShare) -> Self {
        Self {
            gpu,
            tracker: ProcessTracker::new(logical_processors()),
            buf: vec![0; 64 * 1024],
            started: Instant::now(),
        }
    }

    fn query(&mut self) -> Result<&[u8], BoxError> {
        for _ in 0..6 {
            let len = u32::try_from(self.buf.len() * 8).unwrap_or(u32::MAX);
            let mut needed = 0u32;
            // SAFETY: the buffer holds `len` writable bytes.
            let status = unsafe {
                NtQuerySystemInformation(
                    SystemProcessInformation,
                    self.buf.as_mut_ptr().cast(),
                    len,
                    &mut needed,
                )
            };
            if status == STATUS_INFO_LENGTH_MISMATCH {
                // Grow with headroom: processes start between calls.
                let want = (needed as usize).max(len as usize) * 5 / 4;
                self.buf.resize(want.div_ceil(8), 0);
                continue;
            }
            status.ok()?;
            let used = (needed as usize).min(self.buf.len() * 8);
            // SAFETY: reinterpreting initialised u64 storage as bytes.
            let bytes = unsafe {
                std::slice::from_raw_parts(self.buf.as_ptr().cast::<u8>(), self.buf.len() * 8)
            };
            return Ok(&bytes[..used]);
        }
        Err("process list kept growing".into())
    }
}

impl Provider for ProcessProvider {
    fn name(&self) -> &'static str {
        "processes"
    }

    fn init(&mut self, _reg: &mut Registry) -> Result<(), BoxError> {
        self.query().map(|_| ())
    }

    fn poll(&mut self, _reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let now = u64::try_from(self.started.elapsed().as_nanos() / 100).unwrap_or(u64::MAX);
        let bytes = self.query()?;
        let list = parse_process_list(bytes, bytes.as_ptr() as usize);
        // The Memory Compression process's working set is the compressed store.
        if let Some(p) = list
            .iter()
            .find(|p| matches!(p.name.as_str(), "Memory Compression" | "MemCompression"))
        {
            snap.memory.compressed_mb = Some(p.working_set as f64 / (1024.0 * 1024.0));
        }
        let samples = self.tracker.update(list, now);
        let gpu = lock(&self.gpu).clone();
        snap.processes = top_processes(samples, &gpu, TOP_N);
        Ok(())
    }
}

//! System memory: totals from `GlobalMemoryStatusEx` / `GetPerformanceInfo`, the page lists
//! and fault rates from `\Memory\*` PDH counters, page-file use from
//! `SystemPageFileInformation`. The compressed store is filled in by the process provider
//! (it already walks the process list that contains `MemCompression`).

use windows::Wdk::System::SystemInformation::{NtQuerySystemInformation, SYSTEM_INFORMATION_CLASS};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use super::pdh::{Counter, Query};
use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;
use crate::usage::pagefile_pages;

const MIB: f64 = 1024.0 * 1024.0;
const SYSTEM_PAGE_FILE_INFORMATION: SYSTEM_INFORMATION_CLASS = SYSTEM_INFORMATION_CLASS(18);

struct Keys {
    used: SensorKey,
    available: SensorKey,
    committed: SensorKey,
    page_faults: SensorKey,
}

struct Lists {
    query: Query,
    standby: [Counter; 3],
    modified: Counter,
    free: Counter,
    faults: Counter,
    hard: Counter,
}

impl Lists {
    fn open() -> Result<Self, BoxError> {
        let query = Query::open()?;
        let c = |name: &str| query.add(&format!(r"\Memory\{name}"));
        let lists = Self {
            standby: [
                c("Standby Cache Reserve Bytes")?,
                c("Standby Cache Normal Priority Bytes")?,
                c("Standby Cache Core Bytes")?,
            ],
            modified: c("Modified Page List Bytes")?,
            free: c("Free & Zero Page List Bytes")?,
            faults: c("Page Faults/sec")?,
            hard: c("Page Reads/sec")?,
            query,
        };
        lists.query.collect()?;
        Ok(lists)
    }
}

#[derive(Default)]
pub struct MemoryProvider {
    keys: Option<Keys>,
    lists: Option<Lists>,
    page_size: f64,
    pagefile_buf: Vec<u64>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// (total, in use) page-file pages.
    fn pagefile(&mut self) -> Option<(u64, u64)> {
        if self.pagefile_buf.is_empty() {
            self.pagefile_buf = vec![0; 512];
        }
        let len = u32::try_from(self.pagefile_buf.len() * 8).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: the buffer holds `len` writable, 8-byte aligned bytes.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_PAGE_FILE_INFORMATION,
                self.pagefile_buf.as_mut_ptr().cast(),
                len,
                &mut needed,
            )
        };
        if status.is_err() {
            return None;
        }
        let used = (needed as usize).min(len as usize);
        // SAFETY: reinterpreting initialised u64 storage as bytes.
        let bytes =
            unsafe { std::slice::from_raw_parts(self.pagefile_buf.as_ptr().cast::<u8>(), used) };
        Some(pagefile_pages(bytes))
    }
}

impl Provider for MemoryProvider {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let mut r = |id: &str, name: &str, unit, history: bool| {
            let d = SensorDesc::new(id, Group::Memory, name, unit, Source::Os);
            reg.register(if history { d.with_history() } else { d })
        };
        self.keys = Some(Keys {
            used: r("memory.used", "Memory Used", Unit::Megabyte, true),
            available: r(
                "memory.available",
                "Memory Available",
                Unit::Megabyte,
                false,
            ),
            committed: r(
                "memory.committed",
                "Memory Committed",
                Unit::Megabyte,
                false,
            ),
            page_faults: r("memory.pageFaults", "Page Faults", Unit::PerSecond, false),
        });
        match Lists::open() {
            Ok(l) => self.lists = Some(l),
            Err(e) => log::warn!("memory: PDH page-list counters unavailable: {e}"),
        }
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let Some(k) = &self.keys else {
            return Err("poll before init".into());
        };
        let (k_used, k_avail, k_commit, k_faults) =
            (k.used, k.available, k.committed, k.page_faults);
        let mut st = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: dwLength is set; `st` is a valid out-pointer.
        unsafe { GlobalMemoryStatusEx(&mut st) }?;
        let total = st.ullTotalPhys as f64 / MIB;
        let avail = st.ullAvailPhys as f64 / MIB;
        {
            let m = &mut snap.memory;
            m.total_mb = Some(total);
            m.available_mb = reg.set(k_avail, Some(avail));
            m.used_mb = reg.set(k_used, Some(total - avail));
        }

        let mut pi = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        // SAFETY: `cb` matches the struct size.
        if unsafe { GetPerformanceInfo(&mut pi, pi.cb) }.is_ok() {
            let page = pi.PageSize as f64 / MIB;
            self.page_size = page;
            let m = &mut snap.memory;
            m.committed_mb = reg.set(k_commit, Some(pi.CommitTotal as f64 * page));
            m.commit_limit_mb = Some(pi.CommitLimit as f64 * page);
            m.cached_mb = Some(pi.SystemCache as f64 * page);
            m.paged_pool_mb = Some(pi.KernelPaged as f64 * page);
            m.non_paged_pool_mb = Some(pi.KernelNonpaged as f64 * page);
        }

        if let Some(l) = &self.lists {
            if l.query.collect().is_ok() {
                let mb = |c: &Counter| c.value().map(|v| v / MIB);
                let m = &mut snap.memory;
                let standby: Option<f64> = l.standby.iter().map(mb).sum();
                m.standby_mb = standby;
                m.modified_mb = mb(&l.modified);
                m.free_mb = mb(&l.free);
                // Task Manager's "Cached" is standby + modified.
                if let (Some(s), Some(md)) = (m.standby_mb, m.modified_mb) {
                    m.cached_mb = Some(s + md);
                }
                m.page_faults_per_sec = reg.set(k_faults, l.faults.value());
                m.hard_faults_per_sec = l.hard.value();
            }
        }

        if self.page_size > 0.0 {
            if let Some((pf_total, pf_used)) = self.pagefile() {
                let page = self.page_size;
                snap.memory.page_file_total_mb = Some(pf_total as f64 * page);
                snap.memory.page_file_used_mb = Some(pf_used as f64 * page);
            }
        }
        Ok(())
    }
}

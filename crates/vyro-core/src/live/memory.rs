//! System memory totals (`GlobalMemoryStatusEx`, `GetPerformanceInfo`). The full runtime set
//! (compression, page faults, …) arrives in Phase 4.

use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;

const MIB: f64 = 1024.0 * 1024.0;

#[derive(Default)]
pub struct MemoryProvider {
    keys: Option<(SensorKey, SensorKey)>,
}

impl MemoryProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Provider for MemoryProvider {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        let used = reg.register(
            SensorDesc::new(
                "memory.used",
                Group::Memory,
                "Memory Used",
                Unit::Megabyte,
                Source::Os,
            )
            .with_history(),
        );
        let available = reg.register(SensorDesc::new(
            "memory.available",
            Group::Memory,
            "Memory Available",
            Unit::Megabyte,
            Source::Os,
        ));
        self.keys = Some((used, available));
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let Some((k_used, k_avail)) = self.keys else {
            return Err("poll before init".into());
        };
        let mut st = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: dwLength is set; `st` is a valid out-pointer.
        unsafe { GlobalMemoryStatusEx(&mut st) }?;
        let m = &mut snap.memory;
        let total = st.ullTotalPhys as f64 / MIB;
        let avail = st.ullAvailPhys as f64 / MIB;
        m.total_mb = Some(total);
        m.available_mb = reg.set(k_avail, Some(avail));
        m.used_mb = reg.set(k_used, Some(total - avail));

        let mut pi = PERFORMANCE_INFORMATION {
            cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32,
            ..Default::default()
        };
        // SAFETY: `cb` matches the struct size.
        if unsafe { GetPerformanceInfo(&mut pi, pi.cb) }.is_ok() {
            let page = pi.PageSize as f64 / MIB;
            m.committed_mb = Some(pi.CommitTotal as f64 * page);
            m.commit_limit_mb = Some(pi.CommitLimit as f64 * page);
            m.cached_mb = Some(pi.SystemCache as f64 * page);
        }
        Ok(())
    }
}

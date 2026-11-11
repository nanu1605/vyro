//! Static inventory cache (spec §4.2: read once, never in the poll loop).

use std::sync::OnceLock;
use std::time::Instant;

use crate::info::{HardwareId, SystemInfo};
use crate::{hwid, sysinfo, unix_now};

/// Hardware and system information, collected at most once per process. Concurrent callers
/// block until the first collection finishes.
#[derive(Default)]
pub struct Inventory {
    hardware: OnceLock<HardwareId>,
    system: OnceLock<SystemInfo>,
}

impl Inventory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hardware(&self) -> &HardwareId {
        self.hardware.get_or_init(|| {
            let t = Instant::now();
            let hw = hwid::collect();
            log::info!("inventory: hardware collected in {:?}", t.elapsed());
            hw
        })
    }

    /// System information with uptime (and other volatile fields) as of now.
    pub fn system(&self) -> SystemInfo {
        let cached = self.system.get_or_init(|| {
            let t = Instant::now();
            let info = sysinfo::collect(hwid::cpuid_hypervisor_present());
            log::info!("inventory: system info collected in {:?}", t.elapsed());
            info
        });
        sysinfo::with_uptime(cached.clone(), unix_now())
    }

    /// Collects everything now; call from a background thread at startup.
    pub fn warm(&self) {
        self.hardware();
        self.system();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_once_and_refreshes_uptime() {
        let inv = Inventory::new();
        let a: *const HardwareId = inv.hardware();
        let b: *const HardwareId = inv.hardware();
        assert_eq!(a, b, "hardware is cached");
        let _ = inv.system();
    }
}

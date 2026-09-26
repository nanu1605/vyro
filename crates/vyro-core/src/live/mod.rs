//! Live Windows providers (Phase 2): CPU, memory, GPU and per-process readings from OS
//! counters and vendor libraries. No driver needed.

mod adl;
mod cpu;
mod dll;
mod gpu;
mod igcl;
mod memory;
mod nvapi;
mod pdh;
mod process;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::provider::Provider;
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};

pub use cpu::{logical_processors, CpuProvider};
pub use gpu::GpuProvider;
pub use memory::MemoryProvider;
pub use process::ProcessProvider;

/// Per-process GPU utilization, written by the GPU provider and read by the process provider.
pub type GpuShare = Arc<Mutex<HashMap<u32, f64>>>;

/// The real providers, in poll order (GPU before processes so per-process GPU is current).
pub fn providers() -> Vec<Box<dyn Provider>> {
    let share = GpuShare::default();
    vec![
        Box::new(CpuProvider::new()),
        Box::new(MemoryProvider::new()),
        Box::new(GpuProvider::new(Arc::clone(&share))),
        Box::new(ProcessProvider::new(share)),
    ]
}

/// Per-logical-processor sensors, registered as processors first report.
struct PerCore {
    what: &'static str,
    label: &'static str,
    unit: Unit,
    keys: Vec<SensorKey>,
}

impl PerCore {
    fn new(what: &'static str, label: &'static str, unit: Unit) -> Self {
        Self {
            what,
            label,
            unit,
            keys: Vec::new(),
        }
    }

    /// Sets each processor's sensor and returns the values that were set.
    fn set(&mut self, reg: &mut Registry, values: impl IntoIterator<Item = f64>) -> Vec<f64> {
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                while self.keys.len() <= i {
                    let n = self.keys.len();
                    self.keys.push(reg.register(SensorDesc::new(
                        format!("cpu.core{n}.{}", self.what),
                        Group::Cpu,
                        format!("CPU #{n} {}", self.label),
                        self.unit,
                        Source::Pdh,
                    )));
                }
                reg.set(self.keys[i], Some(v));
                v
            })
            .collect()
    }
}

//! Power (spec §2 Power, Phase 5 partial). Sums what the other providers measured: GPU board
//! power now; CPU package power arrives with the driver (Phase 6), and until then the total
//! stays `null` rather than a guess that leaves the CPU out. Runs after the GPU provider.

use crate::provider::{BoxError, Provider};
use crate::registry::{Group, Registry, SensorDesc, SensorKey, Source, Unit};
use crate::snapshot::SensorSnapshot;

#[derive(Default)]
pub struct PowerProvider {
    gpu: Option<SensorKey>,
}

impl PowerProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Sum of the GPUs that report power; `None` when none do.
pub fn gpu_total(snap: &SensorSnapshot) -> Option<f64> {
    snap.gpu
        .iter()
        .filter_map(|g| g.power_w)
        .fold(None, |acc, w| Some(acc.unwrap_or(0.0) + w))
}

impl Provider for PowerProvider {
    fn name(&self) -> &'static str {
        "power"
    }

    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError> {
        self.gpu = Some(
            reg.register(
                SensorDesc::new(
                    "power.gpu",
                    Group::Power,
                    "GPU Power (all)",
                    Unit::Watt,
                    Source::App,
                )
                .with_history(),
            ),
        );
        Ok(())
    }

    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError> {
        let gpu = gpu_total(snap);
        snap.power.gpu_w = self.gpu.map_or(gpu, |k| reg.set(k, gpu));
        if let (Some(cpu), Some(gpu)) = (snap.power.cpu_w, snap.power.gpu_w) {
            // Board, RAM, drives and fans: a flat allowance until they are measured.
            snap.power.est_total_w = Some(cpu + gpu + 40.0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::GpuSnapshot;

    #[test]
    fn sums_reporting_gpus_and_leaves_total_without_cpu() {
        let mut snap = SensorSnapshot::default();
        assert_eq!(gpu_total(&snap), None);
        snap.gpu = vec![
            GpuSnapshot {
                power_w: Some(70.5),
                ..Default::default()
            },
            GpuSnapshot {
                power_w: None,
                ..Default::default()
            },
            GpuSnapshot {
                power_w: Some(4.5),
                ..Default::default()
            },
        ];
        let mut reg = Registry::new();
        let mut p = PowerProvider::new();
        p.init(&mut reg).expect("init");
        p.poll(&mut reg, &mut snap).expect("poll");
        assert_eq!(snap.power.gpu_w, Some(75.0));
        assert_eq!(snap.power.est_total_w, None);
    }
}

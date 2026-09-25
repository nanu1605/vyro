//! Sensor provider contract. Each hardware source (PDH, NVML, driver, …) implements this.

use crate::registry::Registry;
use crate::snapshot::SensorSnapshot;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub trait Provider: Send {
    /// Short stable name used in logs and diagnostics.
    fn name(&self) -> &'static str;

    /// Called once on the poller thread before the first poll: probe hardware and register
    /// sensors. An error disables the provider for the session.
    fn init(&mut self, reg: &mut Registry) -> Result<(), BoxError>;

    /// Called every tick: update registered sensors and fill this provider's part of the
    /// snapshot. An error (or panic) affects only this provider's readings for this tick.
    fn poll(&mut self, reg: &mut Registry, snap: &mut SensorSnapshot) -> Result<(), BoxError>;
}

//! Vyro sensor engine: unified registry, provider contract, poller, history store and the
//! data contracts shared with the frontend. Platform-neutral; Windows providers live behind
//! `#[cfg(windows)]` and the Tauri shell wires this crate to the UI.

pub mod amd;
pub mod display;
pub mod history;
pub mod hwid;
pub mod info;
pub mod inventory;
#[cfg(windows)]
pub mod live;
pub mod network;
pub mod pawnio;
pub mod poller;
pub mod power;
pub mod provider;
pub mod ram;
pub mod registry;
pub mod sim;
pub mod sio;
pub mod snapshot;
pub mod storage;
pub mod sysinfo;
pub mod usage;
#[cfg(windows)]
mod winutil;

pub use history::{History, HistoryConfig, HistoryRange, HistorySeries};
pub use inventory::Inventory;
pub use poller::{Poller, PollerConfig, PollerHandle, SnapshotSink};
pub use provider::{BoxError, Provider};
pub use registry::{Group, Registry, Sensor, SensorDesc, SensorKey, SensorTree, Source, Unit};
pub use sim::SimProvider;
pub use snapshot::SensorSnapshot;

/// Current Unix time in seconds.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Locks a mutex, recovering the data if a previous holder panicked. Readings are
/// independent per tick, so continuing with possibly half-updated values beats wedging the
/// engine.
pub fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

//! Vyro sensor engine: unified registry, provider contract, poller, history store and the
//! data contracts shared with the frontend. Platform-neutral; Windows providers live behind
//! `#[cfg(windows)]` and the Tauri shell wires this crate to the UI.

pub mod alerts;
pub mod amd;
pub mod analysis;
pub mod autostart;
pub mod display;
pub mod engine;
pub mod fan;
pub mod gaming;
pub mod health;
pub mod history;
pub mod hwid;
pub mod info;
pub mod inventory;
pub mod ipc;
#[cfg(windows)]
pub mod live;
pub mod network;
pub mod pawnio;
pub mod poller;
pub mod power;
pub mod profiles;
pub mod provider;
pub mod ram;
pub mod registry;
pub mod sim;
pub mod sio;
pub mod snapshot;
pub mod storage;
pub mod sysinfo;
pub mod units;
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

static PROCESS_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// When the process started, for "startup to first data" (spec §8). Call it first thing in
/// `main`; otherwise the first call (the poller's) stands in.
pub fn process_start() -> std::time::Instant {
    *PROCESS_START.get_or_init(std::time::Instant::now)
}

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

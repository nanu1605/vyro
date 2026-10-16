//! Remote companion (P26): a read-only page for a phone or another computer on the same local
//! network, served by the desktop app while the owner has turned it on.
//!
//! - [`view`]: what the page may see, built from each snapshot (no processes, serials,
//!   addresses or names).
//! - [`auth`]: pairing codes and sessions; only hashes of the session tokens are kept.
//! - [`http`]: one HTTP/1.1 request per connection, with tight limits.
//! - [`net`]: which peers and Host names are let in, and the addresses to show.
//! - [`firewall`]: what Windows Defender Firewall does with the app (read only).
//! - [`server`]: the listener, the routes and the live view.
//!
//! The server never runs as an administrator: the app hosts it, as the signed-in user.

pub mod auth;
pub mod firewall;
pub mod http;
pub mod net;
pub mod qr;
pub mod server;
pub mod view;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use auth::{Auth, PairError};
pub use server::{Hooks, RemoteServer, ServerConfig};
pub use view::RemoteView;

/// Unassigned by IANA; "VYRO" on a phone keypad.
pub const DEFAULT_PORT: u16 = 8976;
/// Days a paired device stays signed in.
pub const DEFAULT_DAYS: u32 = 30;
pub const DAY_CHOICES: [u32; 4] = [1, 7, 30, 90];
/// Paired devices at once.
pub const MAX_DEVICES: usize = 8;
/// A pairing code lasts this long and pairs one device.
pub const CODE_SECS: i64 = 300;
/// Wrong codes before the current one is spent.
pub const CODE_TRIES: u8 = 5;
/// Lowest port the owner may pick (below are the system's).
pub const MIN_PORT: u16 = 1024;
/// File in the app's data folder.
pub const FILE: &str = "remote.json";

/// The owner's choices, kept with the paired devices in [`FILE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct RemoteSettings {
    pub enabled: bool,
    pub port: u16,
    /// How long a pairing lasts, one of [`DAY_CHOICES`].
    pub session_days: u32,
}

impl Default for RemoteSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            session_days: DEFAULT_DAYS,
        }
    }
}

impl RemoteSettings {
    /// Out-of-range values back to their defaults.
    pub fn sanitized(self) -> Self {
        Self {
            enabled: self.enabled,
            port: if self.port >= MIN_PORT {
                self.port
            } else {
                DEFAULT_PORT
            },
            session_days: if DAY_CHOICES.contains(&self.session_days) {
                self.session_days
            } else {
                DEFAULT_DAYS
            },
        }
    }
}

/// A paired device, as Settings lists it (never its token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteDevice {
    pub id: String,
    pub name: String,
    /// Unix seconds.
    pub paired: i64,
    pub expires: i64,
    pub last_seen: Option<i64>,
    /// The address it last came from, on this network.
    pub last_address: Option<String>,
    /// Has a live view open now.
    pub watching: bool,
}

/// The current pairing code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Pairing {
    /// `K7QM-4XPA`.
    pub code: String,
    pub expires: i64,
    pub tries_left: u8,
    /// Address to open, with the code in the fragment (the QR code's content).
    pub url: Option<String>,
    /// The QR code as an SVG path in a square of [`Pairing::qr_size`] modules.
    pub qr: Option<String>,
    pub qr_size: u32,
}

/// What Windows Defender Firewall does with Vyro on the network in use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FirewallState {
    /// No rule yet: Windows asks the first time a device connects.
    Ask,
    Allowed,
    Blocked,
    /// The firewall is off for this network.
    Off,
    #[default]
    Unknown,
}

/// The header chip and the tray: on, and how many live views are open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteBrief {
    pub running: bool,
    pub watching: u32,
}

/// Bytes from the system's secure random source.
pub fn random<const N: usize>() -> Option<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).ok()?;
    Some(b)
}

/// Settings → Remote.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteStatus {
    pub settings: RemoteSettings,
    /// Listening now.
    pub running: bool,
    /// Why it is not, when it should be (the port is taken, ...).
    pub error: Option<String>,
    /// Addresses to open, best first (`http://192.168.1.20:8976/`).
    pub urls: Vec<String>,
    pub devices: Vec<RemoteDevice>,
    /// Live views open.
    pub watching: u32,
    pub pairing: Option<Pairing>,
    pub firewall: FirewallState,
}

/// `/api/info`: who is asking and how the owner reads the app.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteInfo {
    /// This PC's name, as the network already knows it.
    pub pc: String,
    pub version: String,
    /// The app's theme and accent ids.
    pub theme: String,
    pub accent: String,
    pub unit: crate::units::TempUnit,
    /// This device's name and when its pairing ends.
    pub device: String,
    pub expires: i64,
}

#[cfg(test)]
mod tests;

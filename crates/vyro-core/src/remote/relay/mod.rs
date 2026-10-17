//! Reach from anywhere (1.4): the PC keeps one outbound connection to a Vyro relay (a Cloudflare
//! Worker, `relay/` in this repository), and paired phones reach it there. What the PC and a
//! phone say to each other is sealed end to end; the relay sees only who is connected, when, and
//! how much. No port is opened on the PC.
//!
//! - [`seal`]: a phone's keys from its secret, and sealed frames.
//! - [`proto`]: the messages, the pairing link and the relay's addresses.
//! - [`phones`]: the paired phones, pairing, switching to the phone's own secret, expiry.
//! - [`vault`]: secrets at rest, sealed to the Windows account (DPAPI).
//! - [`session`]: one connection's conversation, without I/O, so it is tested as is.
//! - [`client`]: the connection itself, on its own thread, reconnecting as needed.
//!
//! Like the local companion it runs in the app, as the signed-in user, never in the service.

pub mod client;
pub mod phones;
pub mod proto;
pub mod seal;
pub mod session;
pub mod vault;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use client::{RelayClient, RelayHooks};
pub use phones::{Phones, StoredPhone, StoredRelay};

/// An open pairing lasts this long, and its secret works once.
pub const PAIR_SECS: i64 = 600;

/// The relay connection, for Settings, the tray and the header chip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RelayState {
    #[default]
    Off,
    Connecting,
    Online,
    /// Not reached; Vyro tries again by itself (see the status's error).
    Unreachable,
}

/// A phone pairing in progress: its link (the QR code's content) until a phone claims it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RelayPairing {
    pub url: String,
    pub expires: i64,
    /// The QR code as an SVG path in a square of [`RelayPairing::qr_size`] modules.
    pub qr: Option<String>,
    pub qr_size: u32,
}

/// Settings → Remote → From anywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RelayStatus {
    pub enabled: bool,
    /// The relay in use; empty when this build has none and the owner has not given one.
    pub url: String,
    /// The owner gave this address (otherwise it is the one built into Vyro).
    pub custom: bool,
    pub state: RelayState,
    pub error: Option<String>,
    pub pairing: Option<RelayPairing>,
    /// Phones with the app open now.
    pub watching: u32,
}

#[cfg(test)]
mod tests;

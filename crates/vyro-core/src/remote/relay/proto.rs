//! What the PC, the relay and a phone say to each other (1.4).
//!
//! Between the PC and the relay, JSON text the relay reads:
//! - PC → relay: `{"t":"phones","list":[{"id","hash"}]}`: the phones let in; replaces the list,
//!   and the relay closes the sockets of phones that left it or whose key changed.
//! - relay → PC: `{"t":"here","phones":[…]}`: the phones connected now.
//!
//! Data, binary, never opened by the relay: `[phone id: 4 bytes][sealed frame]` (see
//! [`super::seal`]). Inside a sealed frame, JSON:
//! - phone → PC: `hello {name, next?}`, `watch`, `unwatch`, `bye`;
//! - PC → phone: `info`, `view`, `rekeyed`, `signedOut`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::seal::{PhoneId, Room, Secret};
use crate::remote::auth::hex;
use crate::remote::RemoteInfo;

/// The sealed messages' version. A phone app that needs a newer PC says so.
pub const PROTOCOL: u32 = 1;
/// The socket protocol the relay answers with.
pub const SOCKET_PROTOCOL: &str = "vyro.1";

/// A phone the relay lets in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Allowed {
    pub id: String,
    /// SHA-256 of its auth key, hex.
    pub hash: String,
}

pub fn phones_message(list: &[Allowed]) -> String {
    serde_json::json!({ "t": "phones", "list": list }).to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum FromRelay {
    Here {
        phones: Vec<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum FromPhone {
    /// Sent on every connection. The first one, on the pairing link's secret, carries `next`:
    /// the phone's own secret, which replaces the link's.
    Hello {
        #[serde(default)]
        name: String,
        #[serde(default)]
        next: Option<String>,
    },
    /// The app is showing: send the readings.
    Watch,
    /// The app went to the background.
    Unwatch,
    /// The phone signs itself out.
    Bye,
}

#[derive(Serialize)]
#[serde(tag = "t", rename_all = "camelCase")]
enum ToPhone<'a> {
    Info {
        #[serde(flatten)]
        info: &'a RemoteInfo,
        protocol: u32,
    },
    Rekeyed,
}

pub fn info_message(info: &RemoteInfo) -> String {
    serde_json::to_string(&ToPhone::Info {
        info,
        protocol: PROTOCOL,
    })
    .unwrap_or_default()
}

pub fn rekeyed_message() -> String {
    serde_json::to_string(&ToPhone::Rekeyed).unwrap_or_default()
}

/// `view` around a [`crate::remote::RemoteView`] already in JSON.
pub fn view_message(view_json: &str) -> String {
    format!(r#"{{"t":"view","view":{view_json}}}"#)
}

pub fn b64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn secret_from(text: &str) -> Option<Secret> {
    URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()
}

const ONLY_ADDRESS: &str =
    "Enter only the relay's address, like https://vyro-relay.example.workers.dev";

/// The relay's address as the owner may give it: `https://host[:port]`, no path. Plain `http`
/// only for this computer (a relay run locally to test).
pub fn relay_base(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    let (scheme, rest) = url
        .split_once("://")
        .ok_or("Enter the relay's address, starting with https://")?;
    let rest = rest.to_ascii_lowercase();
    let host = if rest.starts_with('[') {
        rest.split_inclusive(']').next().unwrap_or("")
    } else {
        rest.split(':').next().unwrap_or("")
    };
    let port_ok = rest[host.len()..]
        .strip_prefix(':')
        .map_or(rest.len() == host.len(), |p| {
            !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())
        });
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'[' | b']' | b':'));
    if !port_ok || !host_ok {
        return Err(ONLY_ADDRESS.into());
    }
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    match scheme.to_ascii_lowercase().as_str() {
        "https" => Ok(format!("https://{rest}")),
        "http" if local => Ok(format!("http://{rest}")),
        _ => Err("The relay's address must start with https://".into()),
    }
}

/// The PC's socket: `wss://host/pc/<room>`.
pub fn pc_socket(base: &str, room: &Room) -> String {
    let ws = base
        .strip_prefix("https://")
        .map(|h| format!("wss://{h}"))
        .or_else(|| base.strip_prefix("http://").map(|h| format!("ws://{h}")))
        .unwrap_or_else(|| base.to_owned());
    format!("{ws}/pc/{}", hex(room))
}

/// The pairing link (the QR code's content). Everything after `#` stays on the phone: browsers
/// never send it to a server.
pub fn pairing_link(base: &str, room: &Room, phone: &PhoneId, secret: &Secret) -> String {
    format!(
        "{base}/#p=1.{}.{}.{}",
        hex(room),
        hex(phone),
        b64url(secret)
    )
}

/// A pairing link's room, phone and secret (the phone app's side, here for the tests).
pub fn parse_link(link: &str) -> Option<(Room, PhoneId, Secret)> {
    let (_, frag) = link.split_once("#p=")?;
    let mut parts = frag.split('.');
    if parts.next()? != "1" {
        return None;
    }
    let room = unhex(parts.next()?)?.try_into().ok()?;
    let phone = unhex(parts.next()?)?.try_into().ok()?;
    let secret = secret_from(parts.next()?)?;
    parts.next().is_none().then_some((room, phone, secret))
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

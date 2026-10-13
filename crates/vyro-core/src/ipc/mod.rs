//! Engine IPC (P8): the sensor service runs the engine elevated and serves it to the unelevated
//! desktop app over a local, ACL-restricted named pipe (a Unix socket off Windows, for
//! development and tests).
//!
//! Wire format: each message is a little-endian `u32` byte length followed by that many bytes
//! of JSON. A client opens two connections per session, each starting with a [`Hello`] that
//! the server answers with `Result<Welcome, String>`:
//! - **commands**: strictly alternating [`Request`] → [`Reply`];
//! - **snapshots**: the server writes every new [`crate::SensorSnapshot`] and never reads.
//!
//! Two one-way connections keep both ends on plain blocking I/O (a synchronous pipe handle
//! cannot read and write at the same time). [`PROTOCOL_VERSION`] changes whenever a message
//! changes shape; the server refuses a client speaking another version.

pub mod client;
pub mod server;
pub mod transport;

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::alerts::AlertRule;
use crate::fan::FanConfig;
use crate::gaming::{CaptureTarget, GameContext};
use crate::profiles::Profile;
use crate::HistoryRange;

pub use client::{RemoteEngine, RemoteStatus};
pub use server::{Hub, Server};
pub use transport::{connect, default_endpoint, Conn, Listener};

/// 2: `AppSnapshot::parts`. 3: `SensorSnapshot::game`, capture target and game sessions.
/// 4: `GameSnapshot::timeline` replaces the raw frame times.
pub const PROTOCOL_VERSION: u32 = 6;
/// Windows service name (installed by the MSI).
pub const SERVICE_NAME: &str = "VyroService";
/// Largest request the server accepts (alert rules are the biggest, a few KB).
pub const MAX_REQUEST: usize = 1 << 20;
/// Largest reply or snapshot the client accepts.
pub const MAX_MESSAGE: usize = 64 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Commands,
    Snapshots,
}

/// First message on every connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub protocol: u32,
    pub role: Role,
    /// Random per client; ties its two connections together.
    pub session: u64,
    /// The client's process id (the service prefers the one the pipe reports).
    pub pid: u32,
    /// Whether the client's window is showing (the service polls at 1 Hz while any is).
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Welcome {
    pub protocol: u32,
    /// Service version, e.g. "0.1.0".
    pub version: String,
}

/// One call on the commands connection; mirrors [`crate::engine::EngineApi`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "camelCase")]
pub enum Request {
    Latest,
    #[serde(rename_all = "camelCase")]
    SetVisible {
        visible: bool,
    },
    /// Poll interval while visible (profiles); the service clamps it.
    #[serde(rename_all = "camelCase")]
    SetPollInterval {
        millis: u32,
    },
    #[serde(rename_all = "camelCase")]
    QueryHistory {
        metric: String,
        range: HistoryRange,
    },
    SensorTree,
    ResetStats,
    HealthEvents,
    AlertRules,
    #[serde(rename_all = "camelCase")]
    SetAlertRules {
        rules: Vec<AlertRule>,
    },
    AlertLog,
    Drives,
    PawnIoStatus,
    #[serde(rename_all = "camelCase")]
    SetPingHost {
        host: String,
    },
    BitlockerSystemDrive,
    Insights,
    /// The process whose frames to capture (per client session; ends with the session).
    #[serde(rename_all = "camelCase")]
    SetCaptureTarget {
        target: Option<CaptureTarget>,
    },
    #[serde(rename_all = "camelCase")]
    SetGameContext {
        context: Option<GameContext>,
    },
    GameSessions,
    FanStatus,
    #[serde(rename_all = "camelCase")]
    SetFanConfig {
        config: FanConfig,
    },
    RestoreFans,
    FanEvents,
    #[serde(rename_all = "camelCase")]
    SetFanProfile {
        profile: Profile,
    },
}

/// The answer to a [`Request`]: the method's result as JSON, or an error message.
pub type Reply = Result<serde_json::Value, String>;

/// Writes one length-prefixed message.
pub fn write_frame(w: &mut (impl Write + ?Sized), body: &[u8]) -> io::Result<()> {
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
    let mut buf = Vec::with_capacity(4 + body.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(body);
    w.write_all(&buf)?;
    w.flush()
}

/// Reads one length-prefixed message of at most `max` bytes.
pub fn read_frame(r: &mut (impl Read + ?Sized), max: usize) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message of {len} bytes exceeds the {max}-byte limit"),
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(body)
}

pub fn send<T: Serialize + ?Sized>(w: &mut (impl Write + ?Sized), msg: &T) -> io::Result<()> {
    let body = serde_json::to_vec(msg).map_err(io::Error::other)?;
    write_frame(w, &body)
}

/// Reads and decodes one message. Malformed JSON is `InvalidData`.
pub fn recv<T: DeserializeOwned>(r: &mut (impl Read + ?Sized), max: usize) -> io::Result<T> {
    let body = read_frame(r, max)?;
    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_oversized_ones_are_refused() {
        let mut buf = Vec::new();
        send(
            &mut buf,
            &Request::SetPingHost {
                host: "1.1.1.1".into(),
            },
        )
        .unwrap();
        send(&mut buf, &Request::Latest).unwrap();
        let mut r = buf.as_slice();
        assert_eq!(
            recv::<Request>(&mut r, MAX_REQUEST).unwrap(),
            Request::SetPingHost {
                host: "1.1.1.1".into()
            }
        );
        assert_eq!(
            recv::<Request>(&mut r, MAX_REQUEST).unwrap(),
            Request::Latest
        );
        assert_eq!(
            recv::<Request>(&mut r, MAX_REQUEST).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );

        let mut big = Vec::new();
        write_frame(&mut big, &[b' '; 64]).unwrap();
        let err = read_frame(&mut big.as_slice(), 63).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn wire_shapes_are_stable() {
        // A change here breaks older clients: bump PROTOCOL_VERSION with it.
        let json = |v: &Request| serde_json::to_string(v).unwrap();
        assert_eq!(json(&Request::SensorTree), r#"{"method":"sensorTree"}"#);
        assert_eq!(
            json(&Request::SetPollInterval { millis: 500 }),
            r#"{"method":"setPollInterval","params":{"millis":500}}"#
        );
        assert_eq!(
            json(&Request::SetVisible { visible: true }),
            r#"{"method":"setVisible","params":{"visible":true}}"#
        );
        assert_eq!(
            json(&Request::SetCaptureTarget {
                target: Some(CaptureTarget {
                    pid: 9,
                    name: "g.exe".into()
                })
            }),
            r#"{"method":"setCaptureTarget","params":{"target":{"pid":9,"name":"g.exe"}}}"#
        );
        assert_eq!(
            json(&Request::SetGameContext {
                context: Some(GameContext {
                    in_front: true,
                    profile: Profile::Gaming,
                    auto: true,
                    overlay: false
                })
            }),
            r#"{"method":"setGameContext","params":{"context":{"inFront":true,"profile":"gaming","auto":true,"overlay":false}}}"#
        );
        assert_eq!(json(&Request::RestoreFans), r#"{"method":"restoreFans"}"#);
        assert_eq!(
            json(&Request::SetFanProfile {
                profile: Profile::Gaming
            }),
            r#"{"method":"setFanProfile","params":{"profile":"gaming"}}"#
        );
        let set = json(&Request::SetFanConfig {
            config: FanConfig::default(),
        });
        assert!(set.starts_with(r#"{"method":"setFanConfig","params":{"config":{"enabled":false,"#));
        let hello = Hello {
            protocol: PROTOCOL_VERSION,
            role: Role::Snapshots,
            session: 7,
            pid: 42,
            visible: false,
        };
        assert_eq!(
            serde_json::to_string(&hello).unwrap(),
            r#"{"protocol":6,"role":"snapshots","session":7,"pid":42,"visible":false}"#
        );
        let reply: Reply = Err("nope".into());
        assert_eq!(serde_json::to_string(&reply).unwrap(), r#"{"Err":"nope"}"#);
        assert!(
            serde_json::from_str::<Request>(r#"{"method":"fanControl"}"#).is_err(),
            "unknown methods are refused"
        );
    }
}

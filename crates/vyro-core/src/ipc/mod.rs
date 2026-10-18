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
use crate::bench::BenchRun;
use crate::changes::PcInventory;
use crate::diag::DiagAppInfo;
use crate::fan::FanConfig;
use crate::gaming::{CaptureTarget, GameContext};
use crate::gpu_tune::{Settings as TuneSettings, Trial, TuneConfig};
use crate::profiles::Profile;
use crate::HistoryRange;

pub use client::{RemoteEngine, RemoteStatus};
pub use server::{Hub, Server};
pub use transport::{connect, default_endpoint, Conn, Listener};

/// 2: `AppSnapshot::parts`. 3: `SensorSnapshot::game`, capture target and game sessions.
/// 4: `GameSnapshot::timeline` replaces the raw frame times. 5: fan control (0.2).
/// 6: `SetGameContext` (P13). 7: game session replay (P15): `GameTimeline`, `HistoryWindow`,
/// session ids. 8: PC change history (P19): `RecordInventory`, `HardwareChanges`. 9: the
/// intelligence engine (P16): `Insights` answers with findings and vital signs, and
/// `SensorSnapshot::insights`. 10: compound alerts (P18): rule conditions and actions,
/// `AlertUsual`, `SensorSnapshot::alert_state`, `FanStatus::boost`. 11: Performance Score
/// (P17): `PerformanceScore`. 12: benchmark runs (P20): `AddBenchmark`, `BenchmarkRuns`,
/// `BenchmarkRun`, `DeleteBenchmark`. 13: diagnostics (P21): `RunDiagnostics`, `DiagnosticRuns`,
/// `DiagnosticRun`, `DeleteDiagnostic`. 14: Bluetooth Disconnect (1.3): `BluetoothDisconnect`.
/// 15: GPU tuning (1.6): `GpuTuneStatus`, `SetGpuTune`, `RestoreGpu`, `GpuTuneEvents`,
/// `SetGpuTuneProfile`, `GpuTrial`, `GpuTested`, and `SensorSnapshot::gpu_tune`.
pub const PROTOCOL_VERSION: u32 = 15;
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
    #[serde(rename_all = "camelCase")]
    AlertUsual {
        sensor: String,
    },
    Drives,
    PawnIoStatus,
    #[serde(rename_all = "camelCase")]
    SetPingHost {
        host: String,
    },
    BitlockerSystemDrive,
    Insights,
    PerformanceScore,
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
    #[serde(rename_all = "camelCase")]
    GameTimeline {
        id: i64,
    },
    /// Unix seconds, both included.
    #[serde(rename_all = "camelCase")]
    HistoryWindow {
        metrics: Vec<String>,
        from: i64,
        to: i64,
    },
    /// The PC as the app sees it (P19); serials cross the pipe but are never stored.
    #[serde(rename_all = "camelCase")]
    RecordInventory {
        inventory: PcInventory,
    },
    #[serde(rename_all = "camelCase")]
    HardwareChanges {
        limit: u32,
    },
    /// A finished benchmark run to keep (P20).
    #[serde(rename_all = "camelCase")]
    AddBenchmark {
        run: Box<BenchRun>,
    },
    BenchmarkRuns,
    #[serde(rename_all = "camelCase")]
    BenchmarkRun {
        id: i64,
    },
    #[serde(rename_all = "camelCase")]
    DeleteBenchmark {
        id: i64,
    },
    /// Runs the diagnostics with what the app knows (P21).
    #[serde(rename_all = "camelCase")]
    RunDiagnostics {
        app: DiagAppInfo,
    },
    DiagnosticRuns,
    #[serde(rename_all = "camelCase")]
    DiagnosticRun {
        id: i64,
    },
    #[serde(rename_all = "camelCase")]
    DeleteDiagnostic {
        id: i64,
    },
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
    /// A paired Bluetooth device, by the hashed id the device list gives it.
    #[serde(rename_all = "camelCase")]
    BluetoothDisconnect {
        id: String,
    },
    GpuTuneStatus,
    #[serde(rename_all = "camelCase")]
    SetGpuTune {
        config: TuneConfig,
        apply: bool,
    },
    RestoreGpu,
    GpuTuneEvents,
    #[serde(rename_all = "camelCase")]
    SetGpuTuneProfile {
        profile: Profile,
    },
    /// Settings on trial for the stability test (renewed while it runs); `None` ends it.
    #[serde(rename_all = "camelCase")]
    GpuTrial {
        trial: Option<Trial>,
    },
    #[serde(rename_all = "camelCase")]
    GpuTested {
        settings: TuneSettings,
        passed: bool,
        note: String,
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
            json(&Request::BluetoothDisconnect {
                id: "0a1b2c3d4e5f".into()
            }),
            r#"{"method":"bluetoothDisconnect","params":{"id":"0a1b2c3d4e5f"}}"#
        );
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
            json(&Request::PerformanceScore),
            r#"{"method":"performanceScore"}"#
        );
        assert_eq!(
            json(&Request::AlertUsual {
                sensor: "cpu.temp.package".into()
            }),
            r#"{"method":"alertUsual","params":{"sensor":"cpu.temp.package"}}"#
        );
        assert_eq!(
            json(&Request::GameTimeline { id: 12 }),
            r#"{"method":"gameTimeline","params":{"id":12}}"#
        );
        assert_eq!(
            json(&Request::HistoryWindow {
                metrics: vec!["cpu.usage.total".into()],
                from: 100,
                to: 220
            }),
            r#"{"method":"historyWindow","params":{"metrics":["cpu.usage.total"],"from":100,"to":220}}"#
        );
        assert_eq!(
            json(&Request::RecordInventory {
                inventory: PcInventory::default()
            }),
            r#"{"method":"recordInventory","params":{"inventory":{"items":[]}}}"#
        );
        assert_eq!(
            json(&Request::HardwareChanges { limit: 50 }),
            r#"{"method":"hardwareChanges","params":{"limit":50}}"#
        );
        assert_eq!(
            json(&Request::BenchmarkRuns),
            r#"{"method":"benchmarkRuns"}"#
        );
        assert_eq!(
            json(&Request::DeleteBenchmark { id: 3 }),
            r#"{"method":"deleteBenchmark","params":{"id":3}}"#
        );
        assert_eq!(
            json(&Request::DiagnosticRun { id: 4 }),
            r#"{"method":"diagnosticRun","params":{"id":4}}"#
        );
        assert!(json(&Request::RunDiagnostics {
            app: DiagAppInfo::default()
        })
        .starts_with(r#"{"method":"runDiagnostics","params":{"app":{"appVersion":"#));
        assert_eq!(
            json(&Request::SetFanProfile {
                profile: Profile::Gaming
            }),
            r#"{"method":"setFanProfile","params":{"profile":"gaming"}}"#
        );
        assert_eq!(
            json(&Request::GpuTrial { trial: None }),
            r#"{"method":"gpuTrial","params":{"trial":null}}"#
        );
        assert_eq!(
            json(&Request::GpuTested {
                settings: TuneSettings::STOCK,
                passed: true,
                note: String::new()
            }),
            r#"{"method":"gpuTested","params":{"settings":{"coreMhz":0,"memMhz":0,"powerPct":100},"passed":true,"note":""}}"#
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
            r#"{"protocol":15,"role":"snapshots","session":7,"pid":42,"visible":false}"#
        );
        let reply: Reply = Err("nope".into());
        assert_eq!(serde_json::to_string(&reply).unwrap(), r#"{"Err":"nope"}"#);
        assert!(
            serde_json::from_str::<Request>(r#"{"method":"fanControl"}"#).is_err(),
            "unknown methods are refused"
        );
    }
}

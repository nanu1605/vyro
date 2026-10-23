//! Vyro sensor service (P8). Runs the sensor engine with the rights low-level sensors need
//! (LocalSystem: PawnIO, SMART) and serves it to the unelevated desktop app over a local,
//! ACL-restricted named pipe (`vyro_core::ipc`). History and alert rules live in
//! `%ProgramData%\Vyro`, readable but not writable by standard users.
//!
//! ```text
//! vyro-service                      started by the Service Control Manager (installed by the MSI)
//! vyro-service --console            run in a terminal until Enter (as administrator on Windows)
//!              [--data DIR] [--endpoint NAME]
//! ```
//! A console run on Windows is not the installed service, so the app only trusts it with
//! `VYRO_DEV_PIPE=1`. Off Windows it serves simulated sensors on a Unix socket
//! (`VYRO_SOCKET`), for developing the app against a real service.

mod logger;
#[cfg(windows)]
mod win;

use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use vyro_core::engine::{ClientPids, EngineApi, EngineConfig, LocalEngine};
use vyro_core::ipc::{Hub, Listener, Server};
use vyro_core::PollerConfig;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Log file inside the data folder (rotated at 1 MB, one old file kept).
pub const LOG_FILE: &str = "service.log";

fn main() {
    vyro_core::process_start();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--console") {
        std::process::exit(console(&args));
    }
    #[cfg(windows)]
    win::dispatch();
    #[cfg(not(windows))]
    {
        eprintln!("usage: vyro-service --console [--data DIR] [--endpoint PATH]");
        std::process::exit(2);
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Foreground run for debugging: logs to the terminal and the data folder, stops on Enter.
fn console(args: &[String]) -> i32 {
    let data = arg(args, "--data")
        .map(PathBuf::from)
        .map_or_else(default_data_dir, Ok);
    let data = match data.and_then(|d| {
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        Ok(d)
    }) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("vyro-service: {e}; running without history");
            None
        }
    };
    logger::init(data.as_deref().map(|d| d.join(LOG_FILE)), true);
    let endpoint = arg(args, "--endpoint").unwrap_or_else(vyro_core::ipc::default_endpoint);
    let (stop_tx, stop_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        let _ = stop_tx.send(());
    });
    let ready = || log::info!("serving on {endpoint}; press Enter to stop");
    match run(data, &endpoint, &stop_rx, ready) {
        Ok(()) => 0,
        Err(e) => {
            log::error!("{e}");
            1
        }
    }
}

/// The service's data folder: `%ProgramData%\Vyro` with a locked-down ACL on Windows, else a
/// user data folder.
fn default_data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    return win::data_dir();
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .ok_or("no data folder (set XDG_DATA_HOME)")?;
        Ok(base.join("vyro-service"))
    }
}

/// Claims the endpoint before touching the history, so a second instance fails early. Retries
/// for a while: right after a restart, an app may still hold a connection to the old pipe
/// until it notices the service went away.
fn bind(endpoint: &str) -> Result<Listener, String> {
    let mut last = None;
    for attempt in 0..20 {
        match Listener::bind(endpoint) {
            Ok(listener) => return Ok(listener),
            Err(e) => {
                if attempt == 0 {
                    log::warn!("cannot serve {endpoint} yet: {e}; retrying");
                }
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    Err(format!(
        "cannot serve {endpoint}: {}",
        last.map_or_else(String::new, |e| e.to_string())
    ))
}

/// Engine plus IPC server until `stop` fires (or its sender goes away). `ready` runs once
/// clients can connect.
pub fn run(
    data_dir: Option<PathBuf>,
    endpoint: &str,
    stop: &mpsc::Receiver<()>,
    ready: impl FnOnce(),
) -> Result<(), String> {
    log::info!("vyro-service {VERSION} starting");
    let listener = bind(endpoint)?;
    let hub = Hub::new();
    let clients = ClientPids::default();
    let providers = LocalEngine::default_providers(&clients);
    let publish = Arc::clone(&hub);
    let engine = Arc::new(
        LocalEngine::start(
            EngineConfig {
                data_dir,
                poller: PollerConfig::default(),
                fan_control: true,
            },
            providers,
            clients,
            move |snap: &vyro_core::SensorSnapshot| publish.publish(snap),
        )
        .map_err(|e| format!("cannot start the engine: {e}"))?,
    );
    // Poll at 1 Hz while any app window shows, at the tray rate otherwise; count the apps in
    // the footprint.
    let control = Arc::clone(&engine);
    let server = Server::new(
        Arc::clone(&engine) as Arc<dyn EngineApi>,
        Arc::clone(&hub),
        VERSION,
        move |visible, pids| {
            control.set_visible(visible);
            control.set_clients(pids);
        },
    );
    std::thread::Builder::new()
        .name("vyro-ipc-accept".into())
        .spawn(move || {
            if let Err(e) = server.serve(&listener) {
                log::error!("ipc: stopped accepting clients: {e}");
            }
        })
        .map_err(|e| format!("cannot start the IPC server: {e}"))?;
    // Fans back to the BIOS before the PC sleeps; control resumes after it wakes.
    #[cfg(windows)]
    {
        let power = Arc::clone(&engine);
        win::on_power(Some(Box::new(move |suspend| power.set_suspended(suspend))));
    }
    ready();
    let _ = stop.recv();
    log::info!("stopping");
    #[cfg(windows)]
    win::on_power(None);
    hub.close();
    engine.shutdown();
    log::info!("stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;
    use vyro_core::ipc::{RemoteEngine, RemoteStatus};
    use vyro_core::SensorSnapshot;

    #[test]
    fn serves_the_app_until_stopped_and_keeps_history() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let endpoint = format!(r"\\.\pipe\vyro-test-service-{}", std::process::id());
        #[cfg(unix)]
        let endpoint = dir
            .path()
            .join("engine.sock")
            .to_string_lossy()
            .into_owned();
        let data = dir.path().join("data");
        let (stop_tx, stop_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let run_data = data.clone();
        let run_endpoint = endpoint.clone();
        let service = std::thread::spawn(move || {
            run(Some(run_data), &run_endpoint, &stop_rx, move || {
                let _ = ready_tx.send(());
            })
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let (tx, snaps) = mpsc::channel();
        let tx = Mutex::new(tx);
        let app = RemoteEngine::connect(
            &endpoint,
            true,
            move |s: &SensorSnapshot| {
                let _ = tx.lock().unwrap().send(s.clone());
            },
            |_: &RemoteStatus| {},
        )
        .unwrap();
        // Becoming visible ticks at once; the engine's first tick may still be running, and CPU
        // use needs two samples, so on a busy machine the first readings can come without it.
        let until = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = 0;
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            let snap = snaps
                .recv_timeout(left)
                .unwrap_or_else(|e| panic!("no CPU use in {seen} readings in 10 s: {e}"));
            seen += 1;
            if snap.cpu.usage_total.is_some() {
                break;
            }
        }
        let rules = app.alert_rules().unwrap();
        app.set_alert_rules(rules).unwrap();
        app.shutdown();

        stop_tx.send(()).unwrap();
        service.join().unwrap().unwrap();
        assert!(data.join(vyro_core::engine::HISTORY_FILE).exists());
        assert!(data.join(vyro_core::engine::ALERTS_FILE).exists());
    }
}

//! Checks the Vyro sensor service from the outside, the way the app talks to it: prints the
//! service state, connects over its pipe, waits for a snapshot and lists what it reports.
//! Double-click friendly: waits for Enter at the end.
//!
//! ```sh
//! cargo run -p vyro-core --example remote                  # the installed service
//! cargo run -p vyro-core --example remote -- --endpoint X  # a console service elsewhere
//! ```

use std::io::{BufRead, IsTerminal};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use vyro_core::engine::EngineApi;
use vyro_core::ipc::transport::service_state;
use vyro_core::ipc::{default_endpoint, RemoteEngine, RemoteStatus};
use vyro_core::SensorSnapshot;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let endpoint = args
        .iter()
        .position(|a| a == "--endpoint")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(default_endpoint);
    println!("service state: {:?}", service_state());
    println!("endpoint:      {endpoint}");

    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Arc::new(Mutex::new(tx));
    // A service that is still starting gets a few seconds.
    let mut remote = Err(String::new());
    for _ in 0..20 {
        let tx = std::sync::Arc::clone(&tx);
        remote = RemoteEngine::connect(
            &endpoint,
            true,
            move |s: &SensorSnapshot| {
                if let Ok(t) = tx.lock() {
                    let _ = t.send(s.clone());
                }
            },
            |s: &RemoteStatus| println!("status change: {s:?}"),
        );
        if remote.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    match remote {
        Err(e) => println!("connect failed: {e}"),
        Ok(remote) => {
            println!("connected:     {:?}", remote.status());
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(snap) => println!(
                    "snapshot:      CPU {:?}% at {:?} °C, {} process(es), app footprint {:?} MB",
                    snap.cpu.usage_total,
                    snap.cpu.temp_package,
                    snap.processes.len(),
                    snap.app.ram_mb
                ),
                Err(_) => println!("snapshot:      none within 10 s"),
            }
            match remote.sensor_tree() {
                Ok(tree) => {
                    for g in tree.groups {
                        let live = g.sensors.iter().filter(|s| s.available).count();
                        println!(
                            "  {:?}: {live} of {} sensors live",
                            g.group,
                            g.sensors.len()
                        );
                    }
                }
                Err(e) => println!("sensor tree failed: {e}"),
            }
            println!("pawnio:        {:?}", remote.pawnio_status());
            println!("alert rules:   {:?}", remote.alert_rules().map(|r| r.len()));
            remote.shutdown();
        }
    }
    if std::io::stdin().is_terminal() && !args.iter().any(|a| a == "--no-wait") {
        println!("\nPress Enter to close.");
        let _ = std::io::stdin().lock().lines().next();
    }
}

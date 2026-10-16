//! Asks the running Vyro service whether it works (P27's clean-install check in CI; also handy
//! on a PC). Connects to its pipe like the app does, waits for readings, checks that the history
//! store answers, and watches the service's own CPU and memory for a while.
//!
//! ```text
//! vyro-probe [--endpoint PATH] [--wait SECS] [--watch SECS]
//! ```
//!
//! `--wait` (default 60) is how long the service may take to answer and to write its first
//! history; `--watch` (default 30) how long its footprint is sampled. Prints a Markdown summary;
//! exits 1 if the service does not answer, sends no readings or has no history store.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vyro_core::engine::EngineApi;
use vyro_core::ipc::client::RemoteEngine;
use vyro_core::{lock, HistoryRange, SensorSnapshot};

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn secs(name: &str, default: u64) -> Duration {
    Duration::from_secs(arg(name).and_then(|v| v.parse().ok()).unwrap_or(default))
}

fn fail(why: &str) -> ! {
    eprintln!("vyro-probe: {why}");
    std::process::exit(1);
}

fn main() {
    let endpoint = arg("--endpoint").unwrap_or_else(vyro_core::ipc::default_endpoint);
    let deadline = Instant::now() + secs("--wait", 60);
    let watch = secs("--watch", 30);

    let latest: Arc<Mutex<Option<SensorSnapshot>>> = Arc::default();
    let sink = Arc::clone(&latest);
    let engine = loop {
        let sink = Arc::clone(&sink);
        let on_snap = move |s: &SensorSnapshot| {
            *lock(&sink) = Some(s.clone());
        };
        match RemoteEngine::connect(&endpoint, true, on_snap, |_| {}) {
            Ok(e) => break e,
            Err(e) if Instant::now() < deadline => {
                eprintln!("waiting for the service: {e}");
                std::thread::sleep(Duration::from_secs(2));
            }
            Err(e) => fail(&format!("no answer on {endpoint}: {e}")),
        }
    };
    let version = engine.status().version.unwrap_or_default();

    let first = loop {
        if let Some(s) = lock(&latest).clone() {
            break s;
        }
        if Instant::now() > deadline {
            fail("connected, but no readings came");
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    // The service writes history every 10 s: wait for a point.
    let points = loop {
        match engine.query_history("cpu.usage.total", HistoryRange::M5) {
            Ok(s) if !s.points.is_empty() => break s.points.len(),
            Ok(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_secs(2)),
            Ok(_) => fail("the history store answers but holds no readings"),
            Err(e) => fail(&format!("no history store: {e}")),
        }
    };

    // The service's own footprint, sampled once a second.
    let (mut cpu, mut ram, mut peak, mut n) = (0.0, 0.0, 0.0_f64, 0u32);
    let until = Instant::now() + watch;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_secs(1));
        let Some(s) = lock(&latest).clone() else {
            continue;
        };
        if let Some(p) = s
            .app
            .parts
            .iter()
            .find(|p| p.name.to_ascii_lowercase().starts_with("vyro-service"))
        {
            cpu += p.cpu_pct;
            ram += p.ram_mb;
            peak = peak.max(p.ram_mb);
            n += 1;
        }
    }

    println!("### Clean install: the service\n");
    println!("| | |\n|---|---|");
    println!("| Service | {version} on `{endpoint}` |");
    println!("| Sensors read | {} |", first.app.active_sensors);
    println!("| History | {points} point(s) of the last 5 minutes |");
    if n > 0 {
        let avg = f64::from(n);
        println!(
            "| Service footprint ({n} s) | {:.2} % CPU, {:.1} MB average, {:.1} MB peak |",
            cpu / avg,
            ram / avg,
            peak
        );
    } else {
        println!("| Service footprint | not reported |");
    }
}

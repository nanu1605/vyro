//! Samples the live sensor providers for a few seconds and saves the last snapshot plus the
//! sensor list as JSON, for checking against Task Manager / `nvidia-smi`. Double-click
//! friendly: writes `vyro-live.json` next to the executable and waits for Enter.
//!
//! ```sh
//! cargo run -p vyro-core --example live                 # 5 samples, 1 s apart
//! cargo run -p vyro-core --example live -- --no-wait --samples 3
//! ```
//! Off Windows (or with `VYRO_SIM=1`) it runs the simulator instead.

use std::io::{BufRead, IsTerminal};
use std::time::{Duration, Instant};

use vyro_core::{unix_now, Provider, Registry, SensorSnapshot};

fn providers() -> Vec<Box<dyn Provider>> {
    #[cfg(windows)]
    if std::env::var_os("VYRO_SIM").is_none_or(|v| v != "1") {
        return vyro_core::live::providers();
    }
    vec![Box::new(vyro_core::SimProvider::new())]
}

fn fmt(v: Option<f64>, unit: &str) -> String {
    v.map_or_else(|| "—".to_owned(), |v| format!("{v:.1}{unit}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let no_wait = args.iter().any(|a| a == "--no-wait");
    let samples: usize = args
        .iter()
        .position(|a| a == "--samples")
        .and_then(|i| args.get(i + 1)?.parse().ok())
        .unwrap_or(5)
        .max(1);

    let mut reg = Registry::new();
    let mut active = Vec::new();
    let mut status = serde_json::Map::new();
    for mut p in providers() {
        let t = Instant::now();
        let result = p.init(&mut reg);
        let ms = t.elapsed().as_millis();
        match &result {
            Ok(()) => println!("provider {:<10} ready ({ms} ms)", p.name()),
            Err(e) => println!("provider {:<10} FAILED: {e}", p.name()),
        }
        status.insert(
            p.name().into(),
            serde_json::json!({ "ok": result.is_ok(), "error": result.err().map(|e| e.to_string()), "initMs": ms }),
        );
        active.push(p);
    }

    let mut last = SensorSnapshot::default();
    let mut poll_ms = Vec::new();
    let mut per_provider: std::collections::BTreeMap<&'static str, Vec<f64>> = Default::default();
    for n in 0..samples {
        std::thread::sleep(Duration::from_secs(1));
        let mut snap = SensorSnapshot {
            ts: unix_now(),
            ..Default::default()
        };
        let t = Instant::now();
        for p in &mut active {
            if status[p.name()]["ok"] == true {
                let t = Instant::now();
                if let Err(e) = p.poll(&mut reg, &mut snap) {
                    println!("  {} poll error: {e}", p.name());
                }
                per_provider
                    .entry(p.name())
                    .or_default()
                    .push(t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        poll_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        let c = &snap.cpu;
        println!(
            "#{n} CPU {} @ {} | {} GPU(s)",
            fmt(c.usage_total, "%"),
            fmt(c.mhz_current, " MHz"),
            snap.gpu.len()
        );
        for g in &snap.gpu {
            println!(
                "     {}: {} @ {} | {} | VRAM {} | {} | fan {}",
                g.name,
                fmt(g.usage, "%"),
                fmt(g.mhz_core, " MHz"),
                fmt(g.temp_edge, " °C"),
                fmt(g.vram_used_mb, " MB"),
                fmt(g.power_w, " W"),
                fmt(g.fan_pct, "%")
            );
        }
        if let Some(p) = snap.processes.first() {
            println!(
                "     top process: {} (pid {}) {} CPU, {} RAM",
                p.name,
                p.pid,
                fmt(p.cpu_pct, "%"),
                fmt(p.ram_mb, " MB")
            );
        }
        last = snap;
    }

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "providers": status,
        "pollMs": poll_ms,
        "pollMsByProvider": per_provider,
        "snapshot": last,
        "sensors": reg.tree(),
    }))?;
    let out = std::env::current_exe()?.with_file_name("vyro-live.json");
    match std::fs::write(&out, &json) {
        Ok(()) => println!("\nSaved to {}", out.display()),
        Err(e) => println!("\nCould not save {}: {e}", out.display()),
    }
    if !no_wait && std::io::stdin().is_terminal() {
        println!("Press Enter to close.");
        let _ = std::io::stdin().lock().lines().next();
    }
    Ok(())
}

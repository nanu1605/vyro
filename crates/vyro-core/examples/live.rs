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
use std::sync::Mutex;
use std::time::{Duration, Instant};

use vyro_core::{unix_now, Provider, Registry, SensorSnapshot};

/// Prints engine log lines and keeps them for the JSON report (vendor-library diagnostics).
struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let line = format!("[{}] {}", r.level(), r.args());
            println!("  {line}");
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
    }
    fn flush(&self) {}
}

static LOG: Capture = Capture(Mutex::new(Vec::new()));

fn providers() -> Vec<Box<dyn Provider>> {
    #[cfg(windows)]
    if std::env::var_os("VYRO_SIM").is_none_or(|v| v != "1") {
        return vyro_core::live::providers();
    }
    vec![Box::new(vyro_core::SimProvider::new())]
}

/// Raw AMD registers, to check decoding (VID formula, CCD slots) against HWiNFO.
#[cfg(windows)]
fn amd_raw() -> serde_json::Value {
    use vyro_core::amd::{self, MSR_HW_PSTATE_STATUS, MSR_PWR_UNIT, SMN_THM_TCON_CUR_TMP};
    use vyro_core::pawnio::{on_cpu, Handle, Module};
    let id = vyro_core::hwid::cpuid::read_live();
    if !amd::supported(&id.vendor, id.family) {
        return serde_json::Value::Null;
    }
    let io = match Handle::open(Module::AmdFamily17) {
        Ok(io) => io,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let msr = |i: u32| io.exec1("ioctl_read_msr", &[u64::from(i)]).ok();
    let smn = |a: u32| io.exec1("ioctl_read_smn", &[u64::from(a)]).ok();
    let ccd = amd::ccd_base(id.family, id.model);
    let pstates: Vec<_> = (0..vyro_core::live::logical_processors())
        .map(|cpu| {
            let raw = on_cpu(cpu, || msr(MSR_HW_PSTATE_STATUS)).flatten();
            let vid = raw.map(amd::vid);
            serde_json::json!({
                "cpu": cpu,
                "raw": raw.map(|r| format!("{r:#018x}")),
                "vid": vid,
                "svi2V": vid.map(|v| amd::vid_volts(v, false)),
                "svi3V": vid.map(|v| amd::vid_volts(v, true)),
            })
        })
        .collect();
    serde_json::json!({
        "family": format!("{:#x}", id.family),
        "model": format!("{:#x}", id.model),
        "svi3Assumed": amd::svi3(id.family, id.model),
        "pwrUnit": msr(MSR_PWR_UNIT).map(|r| format!("{r:#x}")),
        "tctlRaw": smn(SMN_THM_TCON_CUR_TMP).map(|r| format!("{r:#010x}")),
        "ccdBase": ccd.map(|b| format!("{b:#x}")),
        "ccdRaw": ccd.map(|b| (0..amd::MAX_CCDS).map(|i| smn(b + i * 4).map(|r| format!("{r:#010x}"))).collect::<Vec<_>>()),
        "pstate": pstates,
    })
}

#[cfg(not(windows))]
fn amd_raw() -> serde_json::Value {
    serde_json::Value::Null
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

    let _ = log::set_logger(&LOG).map(|()| log::set_max_level(log::LevelFilter::Info));
    let pawnio = vyro_core::pawnio::status();
    println!(
        "PawnIO: {:?}{}{}",
        pawnio.state,
        pawnio
            .version
            .as_deref()
            .map_or(String::new(), |v| format!(" {v}")),
        pawnio
            .detail
            .as_deref()
            .map_or(String::new(), |d| format!(" ({d})")),
    );
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
        println!(
            "     CPU Tdie {} CCD {:?} | package {} cores {} | VID {} | effective {}",
            fmt(c.temp_package, " °C"),
            c.temp_ccd,
            fmt(c.power_pkg_w, " W"),
            fmt(c.power_core_w, " W"),
            c.vid_core
                .map_or_else(|| "—".to_owned(), |v| format!("{v:.3} V")),
            fmt(c.mhz_effective, " MHz"),
        );
        for g in &snap.gpu {
            println!(
                "     {}: {} @ {} | {} (hotspot {}, mem {}) | {} | VRAM {} | {} | fan {} / {}",
                g.name,
                fmt(g.usage, "%"),
                fmt(g.mhz_core, " MHz"),
                fmt(g.temp_edge, " °C"),
                fmt(g.temp_hotspot, " °C"),
                fmt(g.temp_mem, " °C"),
                fmt(g.voltage_v, " V"),
                fmt(g.vram_used_mb, " MB"),
                fmt(g.power_w, " W"),
                fmt(g.fan_pct, "%"),
                fmt(g.fan_rpm, " RPM")
            );
        }
        for n in snap.network.iter().filter(|n| n.connected) {
            println!(
                "     {}: down {} up {} (link {}){}",
                n.adapter,
                fmt(n.down_mbps, " Mbps"),
                fmt(n.up_mbps, " Mbps"),
                fmt(n.link_mbps, " Mbps"),
                n.wifi_signal_pct
                    .map_or(String::new(), |s| format!(", signal {s:.0}%")),
            );
        }
        println!(
            "     ping {}: {} (avg {}, loss {}){}",
            snap.ping.host,
            fmt(snap.ping.rtt_ms, " ms"),
            fmt(snap.ping.avg_ms, " ms"),
            fmt(snap.ping.loss_pct, "%"),
            snap.ping
                .error
                .as_deref()
                .map_or(String::new(), |e| format!(" [{e}]")),
        );
        println!(
            "     self: CPU {} RAM {} | GPU power {}",
            fmt(snap.app.cpu_pct, "%"),
            fmt(snap.app.ram_mb, " MB"),
            fmt(snap.power.gpu_w, " W"),
        );
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
        "pawnio": pawnio,
        "amdRaw": amd_raw(),
        "providers": status,
        "pollMs": poll_ms,
        "pollMsByProvider": per_provider,
        "log": *LOG.0.lock().unwrap_or_else(|e| e.into_inner()),
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

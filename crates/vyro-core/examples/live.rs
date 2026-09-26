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

/// Raw Super-I/O monitor registers, to name each board's voltage and temperature channels
/// against HWiNFO or MSI Center.
#[cfg(windows)]
fn sio_raw() -> serde_json::Value {
    use vyro_core::pawnio::{BusLock, Handle, Module, MUTEX_ISABUS};
    use vyro_core::sio::{self, BLOCK_LEN, BLOCK_START};
    let io = match Handle::open(Module::LpcIo) {
        Ok(io) => io,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let lock = BusLock::new(MUTEX_ISABUS);
    let Some(result) = lock.hold(500, || {
        let found = sio::detect(&io)?;
        let block = match found {
            Some(f) => sio::read_block(&io, f.base, BLOCK_START, BLOCK_LEN)?,
            None => None,
        };
        Ok::<_, vyro_core::pawnio::PawnIoError>((found, block))
    }) else {
        return serde_json::json!({ "error": "ISA bus busy" });
    };
    match result {
        Ok((None, _)) => serde_json::json!({ "chip": null }),
        Ok((Some(f), block)) => serde_json::json!({
            "chip": f.chip.name(),
            "id": format!("{:#04x}", f.id),
            "rev": format!("{:#04x}", f.rev),
            "configPort": format!("{:#x}", f.config_port),
            "base": format!("{:#06x}", f.base),
            "start": format!("{BLOCK_START:#05x}"),
            "block": block.map(|b| b.chunks(16).map(|row| row.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>()),
        }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
}

#[cfg(not(windows))]
fn sio_raw() -> serde_json::Value {
    serde_json::Value::Null
}

/// Raw SPD hub registers (no NVM, so no serial numbers) and UMC timing registers, to check the
/// DIMM temperatures and timings against HWiNFO or ZenTimings.
#[cfg(windows)]
fn ram_raw() -> serde_json::Value {
    use vyro_core::pawnio::{BusLock, Handle, Module, MUTEX_PCI, MUTEX_SMBUS};
    use vyro_core::ram;
    let hex8 = |v: Option<u8>| v.map(|v| format!("{v:#04x}"));
    let spd = match Handle::open(Module::SmbusPiix4) {
        Ok(io) => match BusLock::new(MUTEX_SMBUS).hold(500, || ram::probe_spd(&io)) {
            Some(Some(found)) => serde_json::json!(found
                .iter()
                .map(|s| serde_json::json!({
                    "addr": format!("{:#04x}", s.addr),
                    "mr0": format!("{:#04x}", s.mr0),
                    "mr1": format!("{:#04x}", s.mr1),
                    "mr5": hex8(s.mr5),
                    "mr26": hex8(s.mr26),
                    "temp": s.temp.map(|t| format!("{t:#06x}")),
                    "tempC": s.temp.and_then(ram::spd5_temp),
                }))
                .collect::<Vec<_>>()),
            Some(None) => serde_json::json!({ "error": "SMBus mux not on the SPD port" }),
            None => serde_json::json!({ "error": "SMBus busy" }),
        },
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    };
    let id = vyro_core::hwid::cpuid::read_live();
    let umc = if vyro_core::amd::supported(&id.vendor, id.family) {
        match Handle::open(Module::AmdFamily17) {
            Ok(io) => match BusLock::new(MUTEX_PCI).hold(500, || ram::read_umc(&io)) {
                Some(channels) => serde_json::json!(channels
                    .iter()
                    .map(|c| serde_json::json!({
                        "channel": c.channel,
                        "regs": c.regs.iter().map(|(a, v)| (format!("{a:#07x}"), serde_json::Value::from(format!("{v:#010x}")))).collect::<serde_json::Map<_, _>>(),
                    }))
                    .collect::<Vec<_>>()),
                None => serde_json::json!({ "error": "PCI bus busy" }),
            },
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    } else {
        serde_json::Value::Null
    };
    serde_json::json!({ "spd": spd, "umc": umc })
}

#[cfg(not(windows))]
fn ram_raw() -> serde_json::Value {
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
        if !snap.fans.is_empty() || !snap.voltages.is_empty() {
            let b = &snap.board;
            println!(
                "     board: system {} VRM {} chipset {} socket {} | fans {} | volts {}",
                fmt(b.temp_system, " °C"),
                fmt(b.temp_vrm_mos, " °C"),
                fmt(b.temp_chipset, " °C"),
                fmt(b.temp_socket, " °C"),
                snap.fans
                    .iter()
                    .map(|f| format!("{} {}", f.name, fmt(f.rpm, "")))
                    .collect::<Vec<_>>()
                    .join(", "),
                snap.voltages
                    .iter()
                    .map(|v| format!(
                        "{} {}",
                        v.name,
                        v.value.map_or("—".into(), |v| format!("{v:.3}"))
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        let m = &snap.memory;
        if !m.dimms.is_empty() || m.timings.is_some() {
            println!(
                "     RAM: {} | {}",
                m.dimms
                    .iter()
                    .map(|d| format!("{} {}", d.slot, fmt(d.temp_c, " °C")))
                    .collect::<Vec<_>>()
                    .join(", "),
                m.timings.as_ref().map_or_else(
                    || "timings —".to_owned(),
                    |t| format!(
                        "{:.0} MT/s {}-{}-{}-{} tRC {} tRFC {} {}T",
                        t.mts, t.cl, t.rcd_rd, t.rp, t.ras, t.rc, t.rfc, t.command_rate
                    )
                ),
            );
        }
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
        "sioRaw": sio_raw(),
        "ramRaw": ram_raw(),
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

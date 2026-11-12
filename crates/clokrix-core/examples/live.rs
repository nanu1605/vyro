//! Samples the live sensor providers for a few seconds and saves the last snapshot plus the
//! sensor list as JSON, for checking against Task Manager / `nvidia-smi`. Double-click
//! friendly: writes `clokrix-live.json` next to the executable and waits for Enter.
//!
//! ```sh
//! cargo run -p clokrix-core --example live                 # 5 samples, 1 s apart
//! cargo run -p clokrix-core --example live -- --no-wait --samples 3
//! ```
//! Off Windows (or with `CLOKRIX_SIM=1`) it runs the simulator instead.

use std::io::{BufRead, IsTerminal};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use clokrix_core::{unix_now, Provider, Registry, SensorSnapshot};

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
    if std::env::var_os("CLOKRIX_SIM").is_none_or(|v| v != "1") {
        return clokrix_core::live::providers(Default::default());
    }
    vec![Box::new(clokrix_core::SimProvider::new())]
}

/// Raw AMD registers, to check decoding (VID formula, CCD slots) against HWiNFO.
#[cfg(windows)]
fn amd_raw() -> serde_json::Value {
    use clokrix_core::amd::{self, MSR_HW_PSTATE_STATUS, MSR_PWR_UNIT, SMN_THM_TCON_CUR_TMP};
    use clokrix_core::pawnio::{on_cpu, Handle, Module};
    let id = clokrix_core::hwid::cpuid::read_live();
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
    let pstates: Vec<_> = (0..clokrix_core::live::logical_processors())
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

/// Raw Intel registers, to check TjMax, the thermal readouts, RAPL and the core voltage against
/// HWiNFO (2.1).
#[cfg(windows)]
fn intel_raw() -> serde_json::Value {
    use clokrix_core::intel_cpu::{
        self, MSR_IA32_PACKAGE_THERM_STATUS, MSR_IA32_PERF_STATUS, MSR_IA32_THERM_STATUS,
        MSR_PKG_ENERGY_STATUS, MSR_PP0_ENERGY_STATUS, MSR_PP1_ENERGY_STATUS, MSR_RAPL_POWER_UNIT,
        MSR_TEMPERATURE_TARGET,
    };
    use clokrix_core::pawnio::{on_cpu, Handle, Module};
    let id = clokrix_core::hwid::cpuid::read_live();
    if !intel_cpu::supported(&id.vendor, id.family) {
        return serde_json::Value::Null;
    }
    let io = match Handle::open(Module::IntelMsr) {
        Ok(io) => io,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let msr = |i: u32| io.exec1("ioctl_read_msr", &[u64::from(i)]).ok();
    let hex = |v: Option<u64>| v.map(|v| format!("{v:#018x}"));
    let cpus: Vec<_> = (0..clokrix_core::live::logical_processors())
        .map(|cpu| {
            let (therm, perf, target) = on_cpu(cpu, || {
                (
                    msr(MSR_IA32_THERM_STATUS),
                    msr(MSR_IA32_PERF_STATUS),
                    msr(MSR_TEMPERATURE_TARGET),
                )
            })
            .unwrap_or_default();
            serde_json::json!({
                "cpu": cpu,
                "thermStatus": hex(therm),
                "perfStatus": hex(perf),
                "temperatureTarget": hex(target),
                "volts": perf.and_then(intel_cpu::perf_status_volts),
            })
        })
        .collect();
    serde_json::json!({
        "family": format!("{:#x}", id.family),
        "model": format!("{:#x}", id.model),
        "temperatureTarget": hex(msr(MSR_TEMPERATURE_TARGET)),
        "packageThermStatus": hex(msr(MSR_IA32_PACKAGE_THERM_STATUS)),
        "raplUnit": hex(msr(MSR_RAPL_POWER_UNIT)),
        "pkgEnergy": hex(msr(MSR_PKG_ENERGY_STATUS)),
        "pp0Energy": hex(msr(MSR_PP0_ENERGY_STATUS)),
        "pp1Energy": hex(msr(MSR_PP1_ENERGY_STATUS)),
        "cpus": cpus,
    })
}

#[cfg(not(windows))]
fn intel_raw() -> serde_json::Value {
    serde_json::Value::Null
}

/// Where the AMD provider's poll time goes under load: PCI-mutex wait, SMN temperature reads,
/// and the per-core MSR sweep (slowest single-core hop too), each at normal and time-critical
/// priority. Run it with a benchmark going.
#[cfg(windows)]
fn amd_timing() -> serde_json::Value {
    use clokrix_core::amd::{self, MSR_APERF_RO, MSR_CORE_ENERGY, SMN_THM_TCON_CUR_TMP};
    use clokrix_core::pawnio::{on_cpu, urgent, BusLock, Handle, Module, MUTEX_PCI};
    let id = clokrix_core::hwid::cpuid::read_live();
    if !amd::supported(&id.vendor, id.family) {
        return serde_json::Value::Null;
    }
    let Ok(io) = Handle::open(Module::AmdFamily17) else {
        return serde_json::Value::Null;
    };
    let lock = BusLock::new(MUTEX_PCI);
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
    let cpus = clokrix_core::live::logical_processors();
    let round = || {
        let t = Instant::now();
        let waited = lock.hold(500, || {
            let wait = ms(t);
            let r = Instant::now();
            let _ = io.exec1("ioctl_read_smn", &[u64::from(SMN_THM_TCON_CUR_TMP)]);
            (wait, ms(r))
        });
        let s = Instant::now();
        let mut hop: f64 = 0.0;
        for cpu in 0..cpus {
            let h = Instant::now();
            let _ = on_cpu(cpu, || {
                let _ = io.exec1("ioctl_read_msr", &[u64::from(MSR_APERF_RO)]);
                let _ = io.exec1("ioctl_read_msr", &[u64::from(MSR_CORE_ENERGY)]);
            });
            hop = hop.max(ms(h));
        }
        let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
        serde_json::json!({
            "lockWaitMs": waited.map(|w| r3(w.0)),
            "smnMs": waited.map(|w| r3(w.1)),
            "sweepMs": r3(ms(s)),
            "slowestHopMs": r3(hop),
        })
    };
    let mut normal = Vec::new();
    let mut critical = Vec::new();
    for _ in 0..6 {
        normal.push(round());
        std::thread::sleep(Duration::from_millis(250));
        critical.push(urgent(round));
        std::thread::sleep(Duration::from_millis(250));
    }
    serde_json::json!({ "normal": normal, "timeCritical": critical })
}

#[cfg(not(windows))]
fn amd_timing() -> serde_json::Value {
    serde_json::Value::Null
}

#[cfg(not(windows))]
fn amd_raw() -> serde_json::Value {
    serde_json::Value::Null
}

/// Raw Super-I/O monitor registers, to name each board's voltage and temperature channels
/// against HWiNFO or MSI Center.
#[cfg(windows)]
fn sio_raw() -> serde_json::Value {
    use clokrix_core::pawnio::{BusLock, Handle, Module, MUTEX_ISABUS};
    use clokrix_core::sio::{self, BLOCK_LEN, BLOCK_START};
    let io = match Handle::open(Module::LpcIo) {
        Ok(io) => io,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let lock = BusLock::new(MUTEX_ISABUS);
    let Some(result) = lock.hold(500, || {
        let found = sio::detect(&io)?;
        let (block, banked, vendor) = match found {
            // A NCT679x: its vendor ID and every register Clokrix decodes, by address (only read:
            // a locked monitor shows as a wrong vendor ID here, it is not unlocked).
            Some(f) if f.chip.family() == sio::Family::Nct679x => {
                let t = sio::nct679x::tables(f.chip).expect("a NCT679x has tables");
                let regs = sio::nct679x::registers(t);
                let vendor = sio::nct679x::vendor(&mut sio::Ports(&io), f.base)?;
                (
                    None,
                    Some(sio::nct679x::read(&mut sio::Ports(&io), f.base, &regs)?),
                    Some(vendor),
                )
            }
            // An ITE chip: the registers its setup comes from and every one Clokrix decodes with
            // all fans and temperatures counted in, by address (bank 0).
            Some(f) if f.chip.family() == sio::Family::Ite => {
                let all = sio::ite::all(f.chip);
                let mut regs = vec![
                    sio::ite::CONFIG,
                    sio::ite::BANK,
                    sio::ite::VENDOR,
                    sio::ite::FAN_16BIT,
                    sio::ite::FAN_MAIN_CTRL,
                    sio::ite::TEMP456,
                ];
                regs.extend(sio::ite::registers(all));
                let banks = sio::ite::has_banks(f.chip);
                let dump = sio::ite::read(&mut sio::Ports(&io), f.base, banks, &regs)?;
                let wide = dump.into_iter().map(|(a, v)| (u16::from(a), v)).collect();
                (None, Some(wide), None)
            }
            Some(f) => (
                sio::read_block(&io, f.base, BLOCK_START, BLOCK_LEN)?,
                None,
                None,
            ),
            None => (None, None, None),
        };
        Ok::<_, clokrix_core::pawnio::PawnIoError>((found, block, banked, vendor))
    }) else {
        return serde_json::json!({ "error": "ISA bus busy" });
    };
    let second = match &result {
        Ok((Some(f), ..)) if f.chip.family() == sio::Family::Ite => second_raw(&lock),
        _ => serde_json::Value::Null,
    };
    match result {
        Ok((None, ..)) => serde_json::json!({ "chip": null }),
        Ok((Some(f), block, banked, vendor)) => serde_json::json!({
            "second": second,
            "chip": f.chip.name(),
            "id": format!("{:#04x}", f.id),
            "rev": format!("{:#04x}", f.rev),
            "configPort": format!("{:#x}", f.config_port),
            "base": format!("{:#06x}", f.base),
            "monitorOn": f.active,
            "ioLocked": f.io_locked,
            "vendor": vendor.map(|v| format!("{v:#06x}")),
            "start": format!("{BLOCK_START:#05x}"),
            "block": block.map(|b| b.chunks(16).map(|row| row.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>()),
            "registers": banked.map(|r| r.iter().map(|(a, v)| (format!("{a:#05x}"), serde_json::Value::from(format!("{v:#04x}")))).collect::<serde_json::Map<_, _>>()),
        }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
}

/// The second ITE chip of a Gigabyte board (2.4), on its own handle as the app reads it: where it
/// answered and every register Clokrix decodes, with the ones its setup comes from.
#[cfg(windows)]
fn second_raw(lock: &clokrix_core::pawnio::BusLock) -> serde_json::Value {
    use clokrix_core::pawnio::{Handle, Module};
    use clokrix_core::sio::{self, ite};
    let io = match Handle::open(Module::LpcIo) {
        Ok(io) => io,
        Err(e) => return serde_json::json!({ "error": e.to_string() }),
    };
    let Some(result) = lock.hold(500, || {
        let Some(f) = sio::detect_second(&io)? else {
            return Ok(None);
        };
        let mut regs = vec![ite::CONFIG, ite::VENDOR, ite::FAN_16BIT, ite::FAN_MAIN_CTRL];
        regs.extend(ite::registers(ite::all(f.chip)));
        let dump = ite::read(&mut sio::Ports(&io), f.base, false, &regs)?;
        Ok::<_, clokrix_core::pawnio::PawnIoError>(Some((f, dump)))
    }) else {
        return serde_json::json!({ "error": "ISA bus busy" });
    };
    match result {
        Ok(None) => serde_json::json!({ "chip": null }),
        Ok(Some((f, dump))) => serde_json::json!({
            "chip": f.chip.name(),
            "id": format!("{:#04x}{:02x}", f.id, f.rev),
            "configPort": format!("{:#x}", f.config_port),
            "base": format!("{:#06x}", f.base),
            "monitorOn": f.active,
            "registers": dump.iter().map(|(a, v)| (format!("{a:#04x}"), serde_json::Value::from(format!("{v:#04x}")))).collect::<serde_json::Map<_, _>>(),
        }),
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    }
}

#[cfg(not(windows))]
fn sio_raw() -> serde_json::Value {
    serde_json::Value::Null
}

/// Raw SPD hub registers (no NVM, so no serial numbers), DDR4 module sensor registers, and the
/// memory controller's timing registers (AMD UMC or Intel MCHBAR), to check the DIMM
/// temperatures and timings against HWiNFO, ZenTimings or memtest86+.
#[cfg(windows)]
fn ram_raw() -> serde_json::Value {
    use clokrix_core::pawnio::{BusLock, Handle, Module, MUTEX_PCI, MUTEX_SMBUS};
    use clokrix_core::ram;
    let hex8 = |v: Option<u8>| v.map(|v| format!("{v:#04x}"));
    let hex16 = |v: u16| format!("{v:#06x}");
    let id = clokrix_core::hwid::cpuid::read_live();
    let spd = match ram::Bus::of(&id.vendor) {
        None => serde_json::Value::Null,
        Some(bus) => match Handle::open(bus.module()) {
            Ok(io) => match BusLock::new(MUTEX_SMBUS).hold(500, || ram::probe_spd(&io, bus)) {
                Some(Some(found)) => serde_json::json!({
                    "bus": format!("{bus:?}"),
                    "hubs": found.spd.iter().map(|s| serde_json::json!({
                        "addr": format!("{:#04x}", s.addr),
                        "mr0": format!("{:#04x}", s.mr0),
                        "mr1": format!("{:#04x}", s.mr1),
                        "mr5": hex8(s.mr5),
                        "mr26": hex8(s.mr26),
                        "temp": s.temp.map(|t| format!("{t:#06x}")),
                        "tempC": s.temp.and_then(ram::spd5_temp),
                    })).collect::<Vec<_>>(),
                    "ddr4Sensors": found.tsod.iter().map(|t| serde_json::json!({
                        "addr": format!("{:#04x}", t.addr),
                        "capability": hex16(t.capability),
                        "config": hex16(t.config),
                        "maker": hex16(t.maker),
                        "device": hex16(t.device),
                        "recognised": t.is_sensor(),
                        "temp": t.temp.map(hex16),
                        "tempC": t.temp.and_then(ram::tsod_temp),
                    })).collect::<Vec<_>>(),
                }),
                Some(None) => serde_json::json!({ "error": "SMBus mux not on the SPD port" }),
                None => serde_json::json!({ "error": "SMBus busy" }),
            },
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        },
    };
    let regs = |r: &[(u32, u32)]| {
        r.iter()
            .map(|(a, v)| {
                (
                    format!("{a:#07x}"),
                    serde_json::Value::from(format!("{v:#010x}")),
                )
            })
            .collect::<serde_json::Map<_, _>>()
    };
    let imc = if clokrix_core::amd::supported(&id.vendor, id.family) {
        let layout = ram::umc_layout(id.family, id.model);
        match Handle::open(Module::AmdFamily17) {
            Ok(io) => match BusLock::new(MUTEX_PCI).hold(500, || ram::read_umc(&io, layout)) {
                Some(channels) => serde_json::json!({
                    "model": format!("{:#x}", id.model),
                    "layout": format!("{layout:?}"),
                    "channels": channels
                        .iter()
                        .map(|c| serde_json::json!({ "channel": c.channel, "regs": regs(&c.regs) }))
                        .collect::<Vec<_>>(),
                }),
                None => serde_json::json!({ "error": "PCI bus busy" }),
            },
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        }
    } else if id.vendor == "GenuineIntel" {
        match (
            Handle::open(Module::IntelMchbar),
            ram::intel_layout(id.family, id.model),
        ) {
            (Ok(io), Some(layout)) => {
                let r = ram::read_imc(&io, layout);
                serde_json::json!({
                    "model": format!("{:#x}", id.model),
                    "layout": format!("{layout:?}"),
                    "regs": regs(&r),
                })
            }
            // Tiger Lake (2.4): not decoded yet, so every register a decoder may need.
            (Ok(io), None) => match ram::intel_dump_regs(id.family, id.model) {
                Some(dump) => serde_json::json!({
                    "model": format!("{:#x}", id.model),
                    "layout": "none: raw dump for a decoder",
                    "regs": regs(&ram::read_imc_regs(&io, &dump)),
                }),
                None => serde_json::json!({
                    "model": format!("{:#x}", id.model),
                    "error": "no memory controller layout known for this CPU",
                }),
            },
            (Err(e), _) => serde_json::json!({ "error": e.to_string() }),
        }
    } else {
        serde_json::Value::Null
    };
    serde_json::json!({ "spd": spd, "imc": imc })
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
    let pawnio = clokrix_core::pawnio::status();
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
            "     CPU temp {} CCD {:?} | package {} cores {} | VID {} | effective {}",
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
                "     board: system {} VRM MOS {} chipset {} VR {} | fans {} | volts {}",
                fmt(b.temp_system, " °C"),
                fmt(b.temp_vrm_mos, " °C"),
                fmt(b.temp_chipset, " °C"),
                fmt(b.temp_vrm, " °C"),
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
        "intelRaw": intel_raw(),
        "amdTiming": amd_timing(),
        "sioRaw": sio_raw(),
        "ramRaw": ram_raw(),
        "providers": status,
        "pollMs": poll_ms,
        "pollMsByProvider": per_provider,
        "log": *LOG.0.lock().unwrap_or_else(|e| e.into_inner()),
        "snapshot": last,
        "sensors": reg.tree(),
    }))?;
    let out = std::env::current_exe()?.with_file_name("clokrix-live.json");
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

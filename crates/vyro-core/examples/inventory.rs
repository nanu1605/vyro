//! Collects the static inventory (hardware, system, displays, drives, network, devices) and saves it as JSON, for checking against msinfo32 / Task
//! Manager. Double-click friendly: writes `vyro-inventory.json` next to the executable, prints
//! it, and waits for Enter before closing.
//!
//! ```sh
//! cargo run -p vyro-core --example inventory            # board / monitor serials redacted
//! cargo run -p vyro-core --example inventory -- --show-serial
//! cargo run -p vyro-core --example inventory -- --no-wait   # for CI / scripts
//! cargo run -p vyro-core --example inventory -- --delay 10  # time to switch to a game first
//! ```

use std::io::{BufRead, IsTerminal};
use std::sync::Mutex;

use vyro_core::info::LocalOnly;
use vyro_core::{display, hwid, sysinfo, unix_now};

/// Keeps log lines so the JSON carries the collectors' diagnostics.
struct Capture(Mutex<Vec<String>>);

impl log::Log for Capture {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let line = format!("[{}] {}", r.level(), r.args());
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
    }
    fn flush(&self) {}
}

static LOG: Capture = Capture(Mutex::new(Vec::new()));

#[cfg(windows)]
fn displays() -> Vec<vyro_core::info::DisplayInfo> {
    display::collect()
}

#[cfg(not(windows))]
fn displays() -> Vec<vyro_core::info::DisplayInfo> {
    display::sample()
}

#[cfg(windows)]
fn network() -> Vec<vyro_core::info::NetAdapterInfo> {
    vyro_core::network::collect()
}

#[cfg(not(windows))]
fn network() -> Vec<vyro_core::info::NetAdapterInfo> {
    vyro_core::network::sample()
}

#[cfg(windows)]
fn drives() -> Vec<vyro_core::info::DriveInfo> {
    vyro_core::storage::collect()
}

#[cfg(not(windows))]
fn drives() -> Vec<vyro_core::info::DriveInfo> {
    vyro_core::storage::sample()
}

#[cfg(windows)]
fn devices() -> vyro_core::devices::Devices {
    vyro_core::devices::collect()
}

#[cfg(not(windows))]
fn devices() -> vyro_core::devices::Devices {
    vyro_core::devices::sample()
}

/// Every HID collection by vendor and product: what to send when a wireless device's battery is
/// missing (its maker's protocol can then be added).
#[cfg(windows)]
fn hid() -> Vec<String> {
    vyro_core::devices::hid_collections()
        .iter()
        .map(|c| {
            format!(
                "{:04x}:{:04x} page {:04x} usage {:04x}",
                c.vendor, c.product, c.page, c.usage
            )
        })
        .collect()
}

#[cfg(not(windows))]
fn hid() -> Vec<String> {
    Vec::new()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let show_serial = args.iter().any(|a| a == "--show-serial");
    let no_wait = args.iter().any(|a| a == "--no-wait");
    let delay: u64 = args
        .iter()
        .position(|a| a == "--delay")
        .and_then(|i| args.get(i + 1)?.parse().ok())
        .unwrap_or(0);

    let _ = log::set_logger(&LOG).map(|()| log::set_max_level(log::LevelFilter::Info));
    let mut hw = hwid::collect();
    if !show_serial && hw.board.serial.is_some() {
        hw.board.serial = Some("<redacted; pass --show-serial>".to_owned());
    }
    let sys = sysinfo::with_uptime(
        sysinfo::collect(hwid::cpuid_hypervisor_present()),
        unix_now(),
    );
    // VRR only varies while a fullscreen game has focus, so allow switching back to it before
    // the displays are sampled.
    if delay > 0 {
        println!("Sampling displays in {delay} s: switch to your game now.");
        std::thread::sleep(std::time::Duration::from_secs(delay));
    }
    let mut shown = displays();
    if !show_serial {
        for d in &mut shown {
            if d.serial.is_some() {
                d.serial = Some(LocalOnly("<redacted; pass --show-serial>".to_owned()));
            }
        }
    }
    let mut disks = drives();
    if !show_serial {
        for d in &mut disks {
            if d.serial.is_some() {
                d.serial = Some(LocalOnly("<redacted; pass --show-serial>".to_owned()));
            }
        }
    }
    let mut nets = network();
    if !show_serial {
        let hide = || LocalOnly("<redacted; pass --show-serial>".to_owned());
        for n in &mut nets {
            n.mac = n.mac.as_ref().map(|_| hide());
            for ip in &mut n.ipv6 {
                // Keep link-local (fe80::) addresses; global ones identify the connection.
                if !ip.0.starts_with("fe80:") {
                    *ip = hide();
                }
            }
            // DNS servers and gateways on IPv6 carry the provider-assigned prefix too.
            for ip in n.dns.iter_mut().chain(n.gateways.iter_mut()) {
                if ip.contains(':') && !ip.starts_with("fe80:") {
                    *ip = hide().0;
                }
            }
            if let Some(w) = &mut n.wifi {
                w.ssid = w.ssid.as_ref().map(|_| hide());
                w.bssid = w.bssid.as_ref().map(|_| hide());
            }
        }
    }
    let log = LOG.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "hardware": hw,
        "system": sys,
        "displays": shown,
        "drives": disks,
        "network": nets,
        "devices": devices(),
        "hid": hid(),
        "pawnio": vyro_core::pawnio::status(),
        "log": log,
    }))?;
    println!("{json}");

    let out = std::env::current_exe()?.with_file_name("vyro-inventory.json");
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

//! Collects the Phase 1 inventory and saves it as JSON, for checking against msinfo32 / Task
//! Manager. Double-click friendly: writes `vyro-inventory.json` next to the executable, prints
//! it, and waits for Enter before closing.
//!
//! ```sh
//! cargo run -p vyro-core --example inventory            # board serial redacted
//! cargo run -p vyro-core --example inventory -- --show-serial
//! cargo run -p vyro-core --example inventory -- --no-wait   # for CI / scripts
//! ```

use std::io::{BufRead, IsTerminal};

use vyro_core::{hwid, sysinfo, unix_now};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let show_serial = args.iter().any(|a| a == "--show-serial");
    let no_wait = args.iter().any(|a| a == "--no-wait");

    let mut hw = hwid::collect();
    if !show_serial && hw.board.serial.is_some() {
        hw.board.serial = Some("<redacted; pass --show-serial>".to_owned());
    }
    let sys = sysinfo::with_uptime(
        sysinfo::collect(hwid::cpuid_hypervisor_present()),
        unix_now(),
    );
    let json = serde_json::to_string_pretty(&serde_json::json!({ "hardware": hw, "system": sys }))?;
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

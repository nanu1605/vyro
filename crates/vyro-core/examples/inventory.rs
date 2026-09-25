//! Prints the Phase 1 inventory as JSON, for checking against msinfo32 / Task Manager.
//!
//! ```sh
//! cargo run -p vyro-core --example inventory            # board serial redacted
//! cargo run -p vyro-core --example inventory -- --show-serial
//! ```

use vyro_core::{hwid, sysinfo, unix_now};

fn main() -> Result<(), serde_json::Error> {
    let show_serial = std::env::args().any(|a| a == "--show-serial");
    let mut hw = hwid::collect();
    if !show_serial && hw.board.serial.is_some() {
        hw.board.serial = Some("<redacted; pass --show-serial>".to_owned());
    }
    let hypervisor = hwid::cpuid_hypervisor_present();
    let sys = sysinfo::with_uptime(sysinfo::collect(hypervisor), unix_now());
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "hardware": hw, "system": sys }))?
    );
    Ok(())
}

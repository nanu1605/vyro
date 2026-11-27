//! OpenRGB set up as the app sets it up (2.6.1), for CI's Windows runner: downloads the pinned
//! installer, checks it, installs it with the app's msiexec options, waits for OpenRGB's SDK
//! server on 127.0.0.1 and asks it for its devices with Clokrix's own client.
//!
//! ```text
//! openrgb_setup [--log PATH] [--wait SECS]
//! ```
//!
//! The runner is already an administrator, so msiexec runs directly, without the app's prompt.
//! `--log` is msiexec's verbose log; `--wait` (default 180) how long the server may take to
//! answer. Prints a Markdown summary; exits 1 if any step fails.

#[cfg(windows)]
fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    use std::net::{Ipv4Addr, SocketAddr};
    use std::os::windows::process::CommandExt;
    use std::time::{Duration, Instant};

    use clokrix_core::lighting::openrgb::{Client, PORT};
    use clokrix_core::lighting::setup;

    let wait = Duration::from_secs(arg("--wait").and_then(|v| v.parse().ok()).unwrap_or(180));
    let took = Instant::now();
    let bytes = setup::download()?;
    println!(
        "| Downloaded | OpenRGB {} ({} bytes, SHA-256 as pinned) in {:.1} s |",
        setup::VERSION,
        bytes.len(),
        took.elapsed().as_secs_f32()
    );

    let dir = std::env::temp_dir().join("clokrix-openrgb");
    let package = setup::Package::write(&bytes, &dir)?;
    let mut params = package.params();
    if let Some(log) = arg("--log") {
        params.push_str(&format!(" /l*v \"{log}\""));
    }
    let status = std::process::Command::new(setup::msiexec())
        .raw_arg(&params)
        .status()
        .map_err(|e| format!("msiexec did not start: {e}"))?;
    let code = status.code().unwrap_or(-1) as u32;
    let installed = setup::outcome(code)?;
    drop(package);
    println!("| Installed | msiexec {params} → {code} ({installed:?}) |");

    let found = setup::find();
    println!(
        "| Found | {}; service {:?}; running {} |",
        found
            .exe
            .as_ref()
            .map_or("no OpenRGB.exe".to_owned(), |p| p.display().to_string()),
        found.service,
        found.open
    );
    if found.exe.is_none() {
        return Err("OpenRGB.exe is not in Program Files\\OpenRGB".into());
    }

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, PORT));
    let until = Instant::now() + wait;
    let started = Instant::now();
    loop {
        let tried = Client::connect(addr).and_then(|mut c| c.controllers().map(|d| (c, d)));
        match tried {
            Ok((c, devices)) => {
                println!(
                    "| Answered | 127.0.0.1:{PORT} after {:.1} s: protocol {}, {} devices |",
                    started.elapsed().as_secs_f32(),
                    c.version,
                    devices.len()
                );
                return Ok(());
            }
            Err(e) if Instant::now() >= until => {
                return Err(format!("OpenRGB did not answer on 127.0.0.1:{PORT}: {e}"));
            }
            Err(_) => std::thread::sleep(Duration::from_secs(2)),
        }
    }
}

fn main() {
    #[cfg(windows)]
    {
        println!("| OpenRGB set-up | |\n|---|---|");
        if let Err(e) = run() {
            println!("| Failed | {e} |");
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    println!("OpenRGB is set up on Windows only.");
}

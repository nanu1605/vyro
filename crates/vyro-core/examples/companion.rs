//! Serves the remote companion page from a frontend build with simulated readings, for working
//! on the page without the app (P26). Prints a pairing code; Enter shows a new one.
//!
//! ```sh
//! pnpm build
//! cargo run -p vyro-core --example companion                       # http://127.0.0.1:8976/
//! cargo run -p vyro-core --example companion -- --bind 0.0.0.0     # reachable from a phone
//! cargo run -p vyro-core --example companion -- --dist dist --port 8976 --theme daylight
//! ```

use std::io::BufRead;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vyro_core::engine::{ClientPids, EngineApi, EngineConfig, LocalEngine};
use vyro_core::remote::{net, Auth, Hooks, RemoteInfo, RemoteServer, ServerConfig};
use vyro_core::units::TempUnit;
use vyro_core::{unix_now, PollerConfig, SimProvider};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dist = PathBuf::from(arg(&args, "--dist").unwrap_or_else(|| "dist".into()));
    let port: u16 = arg(&args, "--port")
        .map_or(Ok(vyro_core::remote::DEFAULT_PORT), |p| p.parse())
        .map_err(|e| format!("--port: {e}"))?;
    let bind: Ipv4Addr = arg(&args, "--bind")
        .map_or(Ok(Ipv4Addr::LOCALHOST), |b| b.parse())
        .map_err(|e| format!("--bind: {e}"))?;
    let theme = arg(&args, "--theme").unwrap_or_else(|| "midnight".into());
    if !dist.join("remote.html").exists() {
        return Err(format!(
            "{} has no remote.html: run `pnpm build` first",
            dist.display()
        ));
    }

    let auth = Arc::new(Mutex::new(Auth::default()));
    let pc = net::pc_name();
    let hooks = Hooks {
        asset: Box::new(move |path| std::fs::read(dist.join(path)).ok()),
        info: Box::new(move || RemoteInfo {
            pc: pc.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            theme: theme.clone(),
            accent: "theme".into(),
            unit: TempUnit::C,
            ..Default::default()
        }),
        saved: Box::new(|| {}),
        watching: Box::new(|n| println!("live views: {n}")),
    };
    let server = Arc::new(RemoteServer::start(
        ServerConfig {
            port,
            bind,
            names: vec![net::pc_name()],
            session_days: vyro_core::remote::DEFAULT_DAYS,
        },
        Arc::clone(&auth),
        hooks,
    )?);
    let feed = Arc::clone(&server);
    let engine = LocalEngine::start(
        EngineConfig {
            data_dir: None,
            poller: PollerConfig::default(),
            fan_control: false,
        },
        vec![Box::new(SimProvider::new())],
        ClientPids::default(),
        move |snap: &vyro_core::SensorSnapshot| feed.publish(snap, TempUnit::C),
    )
    .map_err(|e| e.to_string())?;
    engine.set_visible(true);

    let host = if bind.is_unspecified() {
        net::lan_addresses(&net::local_addresses())
            .first()
            .copied()
            .unwrap_or(Ipv4Addr::LOCALHOST)
    } else {
        bind
    };
    let base = net::url(host, server.addr().port());
    let show = || {
        let code = vyro_core::remote::random().unwrap_or_default();
        let p = vyro_core::lock(&auth).start_pairing(unix_now(), code);
        println!("open {base}#pair={}", p.code.replace('-', ""));
        println!("code: {}", p.code);
    };
    show();
    println!("Enter: a new code · q then Enter: quit");
    for line in std::io::stdin().lock().lines() {
        match line.as_deref() {
            Ok("q") | Err(_) => break,
            Ok(_) => show(),
        }
    }
    engine.shutdown();
    Ok(())
}

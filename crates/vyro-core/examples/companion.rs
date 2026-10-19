//! Serves the remote companion page from a frontend build with simulated readings, for working
//! on the page without the app (P26). Prints a pairing code; Enter shows a new one.
//!
//! ```sh
//! pnpm build
//! cargo run -p vyro-core --example companion                       # http://127.0.0.1:8976/
//! cargo run -p vyro-core --example companion -- --bind 0.0.0.0     # reachable from a phone
//! cargo run -p vyro-core --example companion -- --dist dist --port 8976 --theme daylight
//! ```
//!
//! With `--relay`, it plays a PC reached through a relay instead (1.4), for the phone app
//! (`pnpm build:phone`, then `pnpm --dir relay dev`): it prints a pairing link; Enter shows a new
//! one, `out` signs every phone out, `control` lets every phone control it and `view` only
//! look, `screen` lets every phone see its screen, `q` quits. It answers a phone's requests (1.5)
//! with simulated data; actions are only printed (`asked: power {…}`), never done. Its screen
//! (1.7) is a test picture in the slow view; a direct session connects and takes input (printed
//! as `screen input: {…}`) but sends no video, since only Windows encodes H.264 here.
//!
//! ```sh
//! cargo run -p vyro-core --example companion -- --relay http://127.0.0.1:8787
//! ```

use std::io::BufRead;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use vyro_core::engine::{ClientPids, EngineApi, EngineConfig, LocalEngine};
use vyro_core::remote::relay::proto::{pairing_link, relay_base};
use vyro_core::remote::relay::{Phones, RelayClient, RelayHooks, Request};
use vyro_core::remote::{net, random, Auth, Hooks, RemoteInfo, RemoteServer, ServerConfig};
use vyro_core::screen::rtc::{self, SessionEvent};
use vyro_core::screen::share::Note;
use vyro_core::units::TempUnit;
use vyro_core::{unix_now, PollerConfig, SimProvider};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn info(theme: &str) -> RemoteInfo {
    RemoteInfo {
        pc: net::pc_name(),
        version: env!("CARGO_PKG_VERSION").into(),
        theme: theme.into(),
        accent: "theme".into(),
        unit: TempUnit::C,
        ..Default::default()
    }
}

fn engine(
    publish: impl Fn(&vyro_core::SensorSnapshot) + Send + 'static,
) -> Result<LocalEngine, String> {
    let engine = LocalEngine::start(
        EngineConfig {
            data_dir: None,
            poller: PollerConfig::default(),
            fan_control: false,
            gpu_tune: false,
        },
        vec![Box::new(SimProvider::new())],
        ClientPids::default(),
        move |snap: &vyro_core::SensorSnapshot| publish(snap),
    )
    .map_err(|e| e.to_string())?;
    engine.set_visible(true);
    Ok(engine)
}

/// The slow view's picture.
const SCREEN_JPEG: &[u8] = include_bytes!("screen-test.jpg");
/// The direct screen session, while there is one.
static SCREEN: Mutex<Option<rtc::Session>> = Mutex::new(None);

/// This machine's address on its network (no packet is sent); loopback without one.
fn local_ipv4() -> std::net::IpAddr {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").map(|()| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip())
        .unwrap_or(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST))
}

/// A direct session: the phone's offer answered; input printed.
fn screen_start(r: &Request) -> Result<serde_json::Value, String> {
    let offer = r
        .params
        .get("offer")
        .and_then(serde_json::Value::as_str)
        .ok_or("no offer")?;
    let opts = rtc::Options {
        hosts: vec![local_ipv4()],
        stun: Vec::new(),
        ..rtc::Options::default()
    };
    let (session, answer) = rtc::start(
        offer,
        &opts,
        Box::new(|e| match e {
            SessionEvent::Connected => println!("screen: connected"),
            SessionEvent::Input(i) => println!("screen input: {i:?}"),
            SessionEvent::Closed(why) => println!("screen: closed ({why})"),
        }),
    )?;
    session.tell(Note::Sending { w: 320, h: 180 }.json());
    *vyro_core::lock(&SCREEN) = Some(session);
    Ok(serde_json::json!({ "answer": answer }))
}

/// A phone's request, answered with the simulated engine's data. Actions are printed only.
fn answer(engine: &OnceLock<Arc<LocalEngine>>, r: &Request) -> Result<serde_json::Value, String> {
    let latest = || engine.get().and_then(|e| e.latest());
    match r.method.as_str() {
        "screen.monitors" => Ok(serde_json::json!([
            { "index": 0, "name": "Test", "width": 1280, "height": 720, "primary": true },
            { "index": 1, "name": "Test 2", "width": 1920, "height": 1080, "primary": false }
        ])),
        "screen.start" => screen_start(r),
        "screen.slow" => Ok(serde_json::json!({
            "jpeg": vyro_core::remote::relay::proto::b64url(SCREEN_JPEG),
            "w": 320,
            "h": 180,
            "left": 1200
        })),
        "screen.stop" | "screen.input" => {
            if r.method == "screen.stop" {
                vyro_core::lock(&SCREEN).take();
            }
            println!("asked: {} {}", r.method, r.params);
            Ok(serde_json::json!({ "done": true }))
        }
        "processes" => Ok(
            serde_json::to_value(latest().map(|s| s.processes).unwrap_or_default())
                .unwrap_or_default(),
        ),
        "power.state" => Ok(serde_json::json!({ "pending": null })),
        "power" | "power.cancel" | "process.end" | "profile.pick" | "fans.bios" | "wake" => {
            println!("asked: {} {}", r.method, r.params);
            Ok(serde_json::json!({ "done": true }))
        }
        _ => Err("This example PC does not answer that.".into()),
    }
}

/// A PC reached through a relay: prints `link: …` for each pairing and `state: …`,
/// `watching: …` and `asked: …` as they happen (the end-to-end test reads them).
fn relay(url: &str, theme: String) -> Result<(), String> {
    let base = relay_base(url)?;
    let (room, key) = random::<16>().zip(random::<32>()).ok_or("no randomness")?;
    let phones = Arc::new(Mutex::new(Phones::fresh(room, key)));
    let slot: Arc<OnceLock<Arc<LocalEngine>>> = Arc::new(OnceLock::new());
    let asked = Arc::clone(&slot);
    let client = Arc::new(RelayClient::start(
        base.clone(),
        Arc::clone(&phones),
        vyro_core::remote::DEFAULT_DAYS,
        RelayHooks {
            info: Box::new(move || info(&theme)),
            saved: Box::new(|| println!("saved")),
            watching: Box::new(|n| println!("watching: {n}")),
            state: Box::new(|s| {
                println!("state: {}", serde_json::to_string(&s).unwrap_or_default())
            }),
            request: Box::new(move |r, reply| reply.send(answer(&asked, &r))),
        },
    )?);
    let feed = Arc::clone(&client);
    let engine = Arc::new(engine(move |snap| feed.publish(snap, TempUnit::C))?);
    let _ = slot.set(Arc::clone(&engine));
    let show = || {
        let (Some(secret), Some(id)) = (random::<32>(), random::<4>()) else {
            return;
        };
        let mut p = vyro_core::lock(&phones);
        if let Err(e) = p.start_pairing(unix_now(), secret, id) {
            println!("{e}");
            return;
        }
        let (generation, link) = (p.generation(), pairing_link(&base, p.room(), &id, &secret));
        drop(p);
        client.nudge();
        // As the app does: the link once the relay has the phone's key.
        client.wait_listed(generation, std::time::Duration::from_secs(10));
        println!("link: {link}");
    };
    show();
    println!("Enter: a new pairing link · out: sign every phone out · q: quit");
    for line in std::io::stdin().lock().lines() {
        match line.as_deref() {
            Ok("q") | Err(_) => break,
            Ok("out") => {
                vyro_core::lock(&phones).sign_out_all();
                client.nudge();
                println!("signed out");
            }
            Ok(which @ ("control" | "view")) => {
                let mut p = vyro_core::lock(&phones);
                for d in p.devices(&[]) {
                    p.set_control(&d.id, which == "control");
                }
                println!("phones may {which}");
            }
            Ok("screen") => {
                let mut p = vyro_core::lock(&phones);
                for d in p.devices(&[]) {
                    p.set_screen(&d.id, true);
                }
                println!("phones may see the screen");
            }
            Ok(_) => show(),
        }
    }
    engine.shutdown();
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(url) = arg(&args, "--relay") {
        return relay(
            &url,
            arg(&args, "--theme").unwrap_or_else(|| "midnight".into()),
        );
    }
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
    let hooks = Hooks {
        asset: Box::new(move |path| std::fs::read(dist.join(path)).ok()),
        info: Box::new(move || info(&theme)),
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
    let engine = engine(move |snap| feed.publish(snap, TempUnit::C))?;

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

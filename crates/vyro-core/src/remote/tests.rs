use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::auth::{self, Auth, StoredDevice};
use super::http::{self, Method, ReadError, Response};
use super::net::{self, LocalAddr};
use super::view::{self, FiredView};
use super::*;
use crate::alerts::{ActiveAlert, AlertFired};
use crate::gaming::{CaptureState, GameSnapshot};
use crate::info::SubsystemHealth;
use crate::snapshot::{GpuSnapshot, NetworkSnapshot, ProcessSnapshot, Status, StorageSnapshot};
use crate::units::TempUnit;
use crate::SensorSnapshot;

const NOW: i64 = 1_760_000_000;

fn bytes(n: u8) -> [u8; 32] {
    [n; 32]
}

// --- pairing and sessions -------------------------------------------------------------------

#[test]
fn codes_read_back_however_they_are_typed() {
    let mut a = Auth::default();
    let p = a.start_pairing(NOW, [0x12, 0x34, 0x56, 0x78, 0x9a]);
    assert_eq!(p.code.len(), 9);
    assert_eq!(&p.code[4..5], "-");
    assert!(p
        .code
        .chars()
        .all(|c| c == '-' || "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".contains(c)));
    let canon = p.code.replace('-', "");
    for typed in [
        p.code.clone(),
        canon.to_lowercase(),
        format!(" {} ", canon),
        canon.clone(),
    ] {
        assert_eq!(
            auth::parse(&typed).map(|c| String::from_utf8_lossy(&c).into_owned()),
            Some(canon.clone())
        );
    }
    // Look-alike letters are not in the alphabet; too short or too long is no code.
    assert_eq!(auth::parse("O0I1ABCD"), None);
    assert_eq!(auth::parse("ABCD"), None);
    assert_eq!(auth::parse("ABCDEFGHJ"), None);
    assert_eq!(p.expires, NOW + CODE_SECS);
    assert_eq!(p.tries_left, CODE_TRIES);
}

fn paired(a: &mut Auth, name: &str, token: u8) -> (String, StoredDevice) {
    let code = a.start_pairing(NOW, [token, 1, 2, 3, 4]).code;
    a.pair(
        NOW,
        &code,
        name,
        "192.168.1.30",
        30,
        bytes(token),
        [token, 0, 0, 1],
    )
    .expect("pairs")
}

#[test]
fn a_device_pairs_once_and_only_its_hash_is_kept() {
    let mut a = Auth::default();
    let code = a.start_pairing(NOW, [9, 9, 9, 9, 9]).code;
    let (token, d) = a
        .pair(
            NOW + 10,
            &code,
            "  Pixel\u{7}  8 ",
            "192.168.1.30",
            30,
            bytes(7),
            [1, 2, 3, 4],
        )
        .unwrap();
    assert_eq!(token.len(), 64);
    assert_eq!(d.name, "Pixel 8");
    assert_eq!(d.id, "01020304");
    assert_eq!(d.expires, NOW + 10 + 30 * 86_400);
    let saved = serde_json::to_string(a.stored()).unwrap();
    assert!(!saved.contains(&token), "the token itself is never kept");
    assert!(saved.contains(&auth::hash(&token)));
    assert!(a.take_dirty());
    // The code pairs one device only.
    assert_eq!(
        a.pair(
            NOW + 11,
            &code,
            "Other",
            "192.168.1.31",
            30,
            bytes(8),
            [5; 4]
        ),
        Err(PairError::NoCode)
    );
    // The token signs in; an upper-case copy too; anything else does not.
    assert_eq!(
        a.check(NOW + 20, &token, "192.168.1.30").map(|d| d.id),
        Some(d.id.clone())
    );
    assert!(a
        .check(NOW + 20, &token.to_uppercase(), "192.168.1.30")
        .is_some());
    assert!(a.check(NOW + 20, &"0".repeat(64), "192.168.1.30").is_none());
    assert!(a.check(NOW + 20, "short", "192.168.1.30").is_none());
    assert!(a.pairing(NOW + 20).is_none());
}

#[test]
fn wrong_codes_spend_the_code_and_it_expires() {
    let mut a = Auth::default();
    let code = a.start_pairing(NOW, [1, 2, 3, 4, 5]).code;
    for left in (1..CODE_TRIES).rev() {
        assert_eq!(
            a.pair(NOW, "ZZZZ-ZZZZ", "x", "10.0.0.2", 30, bytes(1), [1; 4]),
            Err(PairError::Wrong(left))
        );
        assert_eq!(a.pairing(NOW).map(|p| p.tries_left), Some(left));
    }
    assert_eq!(
        a.pair(NOW, "not a code", "x", "10.0.0.2", 30, bytes(1), [1; 4]),
        Err(PairError::Spent)
    );
    // Spent: even the right code no longer works.
    assert_eq!(
        a.pair(NOW, &code, "x", "10.0.0.2", 30, bytes(1), [1; 4]),
        Err(PairError::NoCode)
    );
    let code = a.start_pairing(NOW, [5, 4, 3, 2, 1]).code;
    assert_eq!(
        a.pair(
            NOW + CODE_SECS,
            &code,
            "x",
            "10.0.0.2",
            30,
            bytes(1),
            [1; 4]
        ),
        Err(PairError::Expired)
    );
    assert!(a.stored().is_empty());
    assert!(PairError::Wrong(1).to_string().contains("One try left"));
}

#[test]
fn sessions_expire_and_sign_out_at_once() {
    let mut a = Auth::default();
    let (t1, d1) = paired(&mut a, "Phone", 1);
    let (t2, d2) = paired(&mut a, "Tablet", 2);
    assert_eq!(a.stored().len(), 2);
    assert!(a.sign_out(&d1.id));
    assert!(!a.sign_out(&d1.id));
    assert!(a.check(NOW, &t1, "192.168.1.30").is_none());
    assert!(a.check(NOW, &t2, "192.168.1.30").is_some());
    // Past its expiry the session is gone, and so is the device.
    assert!(a.check(d2.expires, &t2, "192.168.1.30").is_none());
    assert!(a.stored().is_empty());
    let (t3, _) = paired(&mut a, "Laptop", 3);
    a.sign_out_all();
    assert!(a.check(NOW, &t3, "192.168.1.30").is_none());
    // Unknown session lengths fall back to the default.
    let code = a.start_pairing(NOW, [7; 5]).code;
    let (_, d) = a
        .pair(NOW, &code, "", "10.0.0.9", 5, bytes(4), [4; 4])
        .unwrap();
    assert_eq!(d.expires, NOW + i64::from(DEFAULT_DAYS) * 86_400);
    assert_eq!(d.name, "Device");
}

#[test]
fn at_most_eight_devices_and_seen_is_saved_sparingly() {
    let mut a = Auth::default();
    for i in 0..MAX_DEVICES {
        paired(&mut a, "d", u8::try_from(i).unwrap() + 10);
    }
    let code = a.start_pairing(NOW, [1; 5]).code;
    assert_eq!(
        a.pair(NOW, &code, "one more", "10.0.0.2", 30, bytes(99), [9; 4]),
        Err(PairError::Full)
    );
    let mut a = Auth::default();
    let (t, _) = paired(&mut a, "Phone", 1);
    a.take_dirty();
    a.check(NOW + 5, &t, "192.168.1.30");
    assert!(
        !a.take_dirty(),
        "seen again within a minute from the same place"
    );
    a.check(NOW + 70, &t, "192.168.1.30");
    assert!(a.take_dirty());
    a.check(NOW + 71, &t, "192.168.1.44");
    assert!(a.take_dirty(), "a new address is noted at once");
    let list = a.devices(&[a.stored()[0].id.clone()]);
    assert_eq!(list[0].last_address.as_deref(), Some("192.168.1.44"));
    assert!(list[0].watching);
}

#[test]
fn saved_devices_that_look_wrong_or_expired_are_dropped() {
    let good = StoredDevice {
        id: "a1".into(),
        name: "Phone".into(),
        token_hash: auth::hash("x"),
        paired: NOW,
        expires: NOW + 100,
        last_seen: None,
        last_address: None,
    };
    let bad = StoredDevice {
        token_hash: "not-a-hash".into(),
        ..good.clone()
    };
    let old = StoredDevice {
        expires: NOW - 1,
        ..good.clone()
    };
    let a = Auth::new(vec![good.clone(), bad, old], NOW);
    assert_eq!(a.stored(), &[good]);
    let s: auth::Stored =
        serde_json::from_str(r#"{"settings":{"enabled":true,"port":80}}"#).unwrap();
    assert_eq!(s.settings.sanitized().port, DEFAULT_PORT);
    assert!(s.settings.enabled);
    assert_eq!(
        RemoteSettings {
            session_days: 3,
            ..RemoteSettings::default()
        }
        .sanitized()
        .session_days,
        DEFAULT_DAYS
    );
    assert!(!RemoteSettings::default().enabled, "off by default");
}

// --- HTTP ------------------------------------------------------------------------------------

fn read(raw: &[u8]) -> Result<http::Request, ReadError> {
    http::read_request(&mut &raw[..], Duration::from_secs(1))
}

#[test]
fn requests_parse_within_limits() {
    let r = read(
        b"GET /api/view?x=1 HTTP/1.1\r\nHost: 192.168.1.5:8976\r\nCookie: a=b; vyro=abc\r\n\r\n",
    )
    .unwrap();
    assert_eq!(r.method, Method::Get);
    assert_eq!(r.path, "/api/view");
    assert_eq!(r.header("host"), Some("192.168.1.5:8976"));
    assert_eq!(r.cookie("vyro"), Some("abc"));
    assert_eq!(r.cookie("nope"), None);
    let r = read(b"POST /api/pair HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello-extra").unwrap();
    assert_eq!(r.body, b"hello");

    let status = |raw: &[u8]| match read(raw) {
        Err(ReadError::Status(s)) => s,
        other => panic!("{other:?}"),
    };
    let mut big = b"GET / HTTP/1.1\r\nX: ".to_vec();
    big.extend(std::iter::repeat_n(b'a', http::MAX_HEAD));
    assert_eq!(status(&big), 431);
    let mut many = b"GET / HTTP/1.1\r\n".to_vec();
    for i in 0..40 {
        many.extend(format!("H{i}: v\r\n").bytes());
    }
    many.extend(b"\r\n");
    assert_eq!(status(&many), 431);
    assert_eq!(
        status(b"POST / HTTP/1.1\r\nContent-Length: 999999\r\n\r\n"),
        413
    );
    assert_eq!(
        status(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"),
        501
    );
    assert_eq!(status(b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n"), 400);
    assert_eq!(status(b"\x16\x03\x01 not http\r\n\r\n"), 400);
    assert!(matches!(read(b"GET / HTTP/1.1\r\n"), Err(ReadError::Gone)));
    assert!(matches!(
        read(b"POST / HTTP/1.1\r\nContent-Length: 9\r\n\r\nabc"),
        Err(ReadError::Gone)
    ));
}

#[test]
fn responses_carry_the_safe_headers_and_cannot_be_split() {
    let mut out = Vec::new();
    let r = Response::new(200, "text/plain", "hi").with("X-Test", "a\r\nSet-Cookie: evil=1");
    http::write_response(&mut out, &r, false).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(text.contains("X-Content-Type-Options: nosniff\r\n"));
    assert!(text.contains("X-Frame-Options: DENY\r\n"));
    assert!(text.contains("Connection: close\r\n"));
    assert!(text.contains("X-Test: aSet-Cookie: evil=1\r\n"));
    assert!(!text.contains("\r\nSet-Cookie"));
    assert!(text.ends_with("\r\n\r\nhi"));
}

// --- network rules ---------------------------------------------------------------------------

#[test]
fn only_the_local_network_gets_in() {
    let ok = |s: &str| net::peer_allowed(s.parse::<IpAddr>().unwrap());
    for ip in [
        "192.168.1.2",
        "10.1.2.3",
        "172.16.5.4",
        "172.31.255.1",
        "169.254.7.7",
        "127.0.0.1",
        "::1",
        "::ffff:192.168.0.9",
    ] {
        assert!(ok(ip), "{ip}");
    }
    for ip in [
        "8.8.8.8",
        "172.32.0.1",
        "100.64.0.1",
        "203.0.113.4",
        "2001:db8::1",
        "fe80::1",
        "::ffff:8.8.8.8",
    ] {
        assert!(!ok(ip), "{ip}");
    }
}

#[test]
fn host_and_origin_must_name_this_pc() {
    let names = vec!["GAMING-PC".to_owned()];
    for h in [
        "192.168.1.5:8976",
        "192.168.1.5",
        "localhost:8976",
        "gaming-pc:8976",
        "Gaming-PC.local:8976",
        "[::1]:8976",
        "gaming-pc.",
    ] {
        assert!(net::host_allowed(h, &names), "{h}");
    }
    for h in [
        "evil.example:8976",
        "192.168.1.5.nip.io",
        "[fe80::1]:8976",
        "",
        "gaming-pc.evil.example",
        "[::1",
    ] {
        assert!(!net::host_allowed(h, &names), "{h}");
    }
    assert!(net::origin_allowed(None, "192.168.1.5:8976"));
    assert!(net::origin_allowed(
        Some("http://192.168.1.5:8976"),
        "192.168.1.5:8976"
    ));
    assert!(!net::origin_allowed(
        Some("http://evil.example"),
        "192.168.1.5:8976"
    ));
    assert!(!net::origin_allowed(Some("null"), "192.168.1.5:8976"));
}

#[test]
fn the_best_address_comes_first() {
    let a = |ip: [u8; 4], primary: bool, virtual_adapter: bool, gateway: bool| LocalAddr {
        ip: Ipv4Addr::from(ip),
        primary,
        virtual_adapter,
        gateway,
    };
    let list = [
        a([172, 20, 0, 1], false, true, false),
        a([169, 254, 3, 3], false, false, false),
        a([100, 70, 1, 1], false, true, true),
        a([10, 0, 0, 7], false, false, true),
        a([192, 168, 1, 20], true, false, true),
        a([8, 8, 4, 4], false, false, true),
        a([192, 168, 1, 20], true, false, true),
    ];
    let ips: Vec<String> = net::lan_addresses(&list)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        ips,
        ["192.168.1.20", "10.0.0.7", "172.20.0.1", "169.254.3.3"]
    );
    assert_eq!(
        net::url(Ipv4Addr::new(192, 168, 1, 20), 8976),
        "http://192.168.1.20:8976/"
    );
}

// --- the view --------------------------------------------------------------------------------

fn snapshot() -> SensorSnapshot {
    let mut s = SensorSnapshot {
        ts: NOW,
        ..Default::default()
    };
    s.cpu.usage_total = Some(37.0);
    s.cpu.temp_package = Some(64.5);
    s.cpu.mhz_current = Some(4800.0);
    s.gpu.push(GpuSnapshot {
        name: "NVIDIA GeForce RTX 4070".into(),
        usage: Some(91.0),
        temp_edge: Some(70.0),
        ..Default::default()
    });
    s.storage.push(StorageSnapshot {
        id: "disk-serial-XYZ".into(),
        model: "Samsung SSD 990 PRO 2TB".into(),
        volumes: "C:".into(),
        capacity_mb: Some(2000.0),
        used_mb: Some(500.0),
        temp_c: Some(41.0),
        ..Default::default()
    });
    s.processes.push(ProcessSnapshot {
        pid: 42,
        name: "secret-project.exe".into(),
        ..Default::default()
    });
    s.network.push(NetworkSnapshot {
        id: "if7".into(),
        adapter: "Home Wi-Fi of Somebody".into(),
        connected: true,
        down_mbps: Some(12.0),
        up_mbps: Some(1.5),
        ..Default::default()
    });
    s.network.push(NetworkSnapshot {
        connected: true,
        down_mbps: Some(3.0),
        ..Default::default()
    });
    s.board.temp_system = Some(33.0);
    s.health.overall = Status::Warn;
    s.health.subsystems = vec![
        SubsystemHealth {
            name: "Storage".into(),
            status: Status::Warn,
            messages: vec!["Drive at 61 °C".into()],
            notes: vec!["Needs PawnIO for the board".into()],
        },
        SubsystemHealth {
            name: "CPU".into(),
            status: Status::Fail,
            messages: vec![],
            notes: vec![],
        },
        SubsystemHealth {
            name: "GPU".into(),
            status: Status::Ok,
            messages: vec![],
            notes: vec![],
        },
    ];
    s.alert_state.active.push(ActiveAlert {
        rule_id: "r1".into(),
        name: "CPU hot".into(),
        since: NOW - 30,
        tray: true,
        profile: None,
        fan_boost: None,
    });
    s
}

#[test]
fn the_view_carries_readings_but_nothing_private() {
    let snap = snapshot();
    let v = view::build(&snap, &VecDeque::new(), TempUnit::F);
    assert_eq!(v.cpu.temp, Some(64.5), "numbers stay °C");
    assert_eq!(v.cpu.mhz, Some(4800.0));
    assert_eq!(v.gpus[0].usage, Some(91.0));
    assert_eq!(v.drives[0].used_pct, Some(25.0));
    assert_eq!(v.network.down_mbps, Some(15.0));
    assert_eq!(v.network.up_mbps, Some(1.5));
    assert_eq!(v.temps[0].name, "System");
    assert_eq!(v.alerts[0].name, "CPU hot");
    let parts: Vec<&str> = v.health.issues.iter().map(|i| i.part.as_str()).collect();
    assert_eq!(parts, ["CPU", "Storage"], "worst first, OK parts left out");
    assert_eq!(
        v.health.issues[1].message.as_deref(),
        Some("Drive at 142 °F")
    );
    let json = serde_json::to_string(&v).unwrap();
    for private in [
        "secret-project",
        "disk-serial",
        "Home Wi-Fi",
        "if7",
        "PawnIO",
        "C:",
    ] {
        assert!(!json.contains(private), "{private} leaked");
    }
}

#[test]
fn games_and_recent_alerts() {
    let mut snap = snapshot();
    snap.game = Some(GameSnapshot {
        state: CaptureState::Capturing,
        detail: None,
        app: "Cyberpunk2077.exe".into(),
        pid: 9,
        runtime: None,
        present_mode: None,
        fps: Some(98.0),
        frame_ms: Some(10.2),
        low1_fps: Some(71.0),
        low01_fps: None,
        cpu_busy_ms: None,
        gpu_busy_ms: None,
        latency_ms: None,
        displayed_fps: None,
        bound: None,
        started: NOW,
        frames: 0,
        session_fps: None,
        session_low1_fps: None,
        timeline: Vec::new(),
    });
    let mut recent = VecDeque::new();
    let mut quiet = AlertFired::new(NOW, "r2", "Logged".into(), "only".into(), false);
    quiet.notify = false;
    let mut again = AlertFired::new(NOW, "r3", "Repeat".into(), "dup".into(), false);
    again.also = Some("r1".into());
    snap.alerts = vec![
        AlertFired::new(NOW, "r1", "CPU at 95 °C".into(), "Hot".into(), true),
        quiet,
        again,
    ];
    view::note_fired(&mut recent, &snap, TempUnit::C);
    let v = view::build(&snap, &recent, TempUnit::C);
    assert_eq!(
        v.game.as_ref().map(|g| g.app.as_str()),
        Some("Cyberpunk2077")
    );
    assert_eq!(v.recent.len(), 1);
    assert_eq!(v.recent[0].title, "CPU at 95 °C");
    for i in 0..20 {
        recent.push_front(FiredView {
            ts: i,
            ..Default::default()
        });
    }
    view::note_fired(&mut recent, &snap, TempUnit::C);
    assert_eq!(recent.len(), view::RECENT);
    assert_eq!(recent[0].title, "CPU at 95 °C", "newest first");
    // A game with no frames yet is not shown.
    snap.game.as_mut().unwrap().fps = None;
    assert!(view::build(&snap, &recent, TempUnit::C).game.is_none());
}

#[test]
fn the_firewall_state_follows_its_rules() {
    use firewall::{decide, Rule};
    let private = 2;
    let allow = Rule {
        enabled: true,
        allow: true,
        profiles: private,
        tcp: true,
    };
    let block = Rule {
        allow: false,
        ..allow
    };
    assert_eq!(decide(false, private, &[block]), FirewallState::Off);
    assert_eq!(decide(true, private, &[]), FirewallState::Ask);
    assert_eq!(decide(true, private, &[allow]), FirewallState::Allowed);
    assert_eq!(
        decide(true, private, &[allow, block]),
        FirewallState::Blocked,
        "block wins"
    );
    // Rules for another network, switched off or for UDP only do not count.
    let public_only = Rule {
        profiles: 4,
        ..block
    };
    let off = Rule {
        enabled: false,
        ..block
    };
    let udp = Rule {
        tcp: false,
        ..block
    };
    assert_eq!(
        decide(true, private, &[allow, public_only, off, udp]),
        FirewallState::Allowed
    );
}

#[test]
fn the_qr_code_is_a_square_path() {
    let (path, size) = qr::path("http://192.168.1.20:8976/#pair=K7QM4XPA").unwrap();
    assert!(path.starts_with('M'));
    assert!(size >= 21 + 2 * 4);
    assert!(path.matches('z').count() > 50);
}

// --- the server ------------------------------------------------------------------------------

struct Test {
    server: RemoteServer,
    auth: Arc<Mutex<Auth>>,
    saved: Arc<AtomicU32>,
    watching: Arc<AtomicU32>,
}

fn start() -> Test {
    let auth = Arc::new(Mutex::new(Auth::default()));
    let saved = Arc::new(AtomicU32::new(0));
    let watching = Arc::new(AtomicU32::new(0));
    let files: HashMap<&str, &[u8]> = HashMap::from([
        ("remote.html", &b"<!doctype html><title>Vyro</title>"[..]),
        ("assets/remote-abc.js", &b"console.log(1)"[..]),
    ]);
    let (s, w) = (Arc::clone(&saved), Arc::clone(&watching));
    let hooks = Hooks {
        asset: Box::new(move |p| files.get(p).map(|b| b.to_vec())),
        info: Box::new(|| RemoteInfo {
            pc: "GAMING-PC".into(),
            version: "0.5.0".into(),
            theme: "midnight".into(),
            accent: "theme".into(),
            ..Default::default()
        }),
        saved: Box::new(move || {
            s.fetch_add(1, Ordering::SeqCst);
        }),
        watching: Box::new(move |n| w.store(n, Ordering::SeqCst)),
    };
    let server = RemoteServer::start(
        ServerConfig {
            port: 0,
            bind: Ipv4Addr::LOCALHOST,
            names: vec!["GAMING-PC".into()],
            session_days: 7,
        },
        Arc::clone(&auth),
        hooks,
    )
    .unwrap();
    Test {
        server,
        auth,
        saved,
        watching,
    }
}

impl Test {
    fn send(&self, raw: &str) -> String {
        let mut c = TcpStream::connect(self.server.addr()).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(
            raw.replace("{port}", &self.server.addr().port().to_string())
                .as_bytes(),
        )
        .unwrap();
        let mut out = String::new();
        let _ = c.read_to_string(&mut out);
        out
    }

    fn get(&self, path: &str, cookie: &str) -> String {
        self.send(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nCookie: {cookie}\r\n\r\n"
        ))
    }

    fn post(&self, path: &str, body: &str, extra: &str) -> String {
        self.send(&format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nOrigin: http://127.0.0.1:{{port}}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}\r\n{body}",
            body.len()
        ))
    }
}

fn status(resp: &str) -> u16 {
    resp.split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn cookie_of(resp: &str) -> String {
    resp.lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: "))
        .and_then(|c| c.split(';').next())
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_page_is_served_and_the_api_needs_a_pairing() {
    let t = start();
    let page = t.get("/", "");
    assert_eq!(status(&page), 200);
    assert!(page.contains("Content-Security-Policy: default-src 'self'"));
    assert!(page.ends_with("<title>Vyro</title>"));
    let js = t.get("/assets/remote-abc.js", "");
    assert_eq!(status(&js), 200);
    assert!(js.contains("Content-Type: text/javascript"));
    for bad in [
        "/assets/../remote.html",
        "/assets/x.exe",
        "/assets/",
        "/assets/missing.js",
        "/secret",
    ] {
        assert_eq!(status(&t.get(bad, "")), 404, "{bad}");
    }
    for api in ["/api/info", "/api/view", "/api/stream"] {
        assert_eq!(status(&t.get(api, "vyro=nope")), 401, "{api}");
    }
    // DNS rebinding: another site's name in Host is refused.
    let rebound = t.send("GET / HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n");
    assert_eq!(status(&rebound), 421);
    assert_eq!(
        status(&t.send("DELETE / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")),
        405
    );
}

#[test]
fn pairing_gives_a_cookie_that_reads_the_view() {
    let t = start();
    let body = r#"{"code":"ABCD-EFGH","name":"Phone"}"#;
    let none = t.post("/api/pair", body, "");
    assert_eq!(status(&none), 403);
    assert!(none.contains("No pairing code is open"));
    let code = t
        .auth
        .lock()
        .unwrap()
        .start_pairing(crate::unix_now(), [3, 1, 4, 1, 5])
        .code;
    let good = format!(r#"{{"code":"{code}","name":"Phone"}}"#);
    // From another page, or not JSON: refused before the code is even looked at.
    let foreign = t.send(&format!(
        "POST /api/pair HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nOrigin: http://evil.example\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{good}",
        good.len()
    ));
    assert_eq!(status(&foreign), 403);
    let form = t.send(&format!(
        "POST /api/pair HTTP/1.1\r\nHost: 127.0.0.1:{{port}}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{good}",
        good.len()
    ));
    assert_eq!(status(&form), 415);
    assert_eq!(
        t.auth
            .lock()
            .unwrap()
            .pairing(crate::unix_now())
            .map(|p| p.tries_left),
        Some(CODE_TRIES)
    );
    let wrong = t.post("/api/pair", r#"{"code":"2222-2222","name":"x"}"#, "");
    assert_eq!(status(&wrong), 403);
    assert!(wrong.contains("4 tries left"));

    let ok = t.post("/api/pair", &good, "");
    assert_eq!(status(&ok), 200, "{ok}");
    assert!(ok.contains("HttpOnly; SameSite=Strict"));
    assert!(ok.contains(&format!("Max-Age={}", 7 * 86_400)));
    assert!(t.saved.load(Ordering::SeqCst) >= 1);
    let cookie = cookie_of(&ok);
    assert!(cookie.starts_with("vyro="));

    let info = t.get("/api/info", &cookie);
    assert_eq!(status(&info), 200);
    assert!(info.contains(r#""device":"Phone""#));
    assert!(info.contains(r#""pc":"GAMING-PC""#));
    assert_eq!(status(&t.get("/api/view", &cookie)), 503, "no reading yet");
    t.server.publish(&snapshot(), TempUnit::C);
    let view = t.get("/api/view", &cookie);
    assert_eq!(status(&view), 200);
    assert!(view.contains(r#""usage":37.0"#));
    assert!(!view.contains("secret-project"));

    // Signing out from the page ends the session.
    let out = t.post("/api/logout", "{}", &format!("Cookie: {cookie}\r\n"));
    assert_eq!(status(&out), 204);
    assert!(out.contains("vyro=; Path=/; Max-Age=0"));
    assert_eq!(status(&t.get("/api/view", &cookie)), 401);
    assert!(t.auth.lock().unwrap().stored().is_empty());
}

fn read_until(c: &mut TcpStream, needle: &str, within: Duration) -> (String, bool) {
    let end = Instant::now() + within;
    let mut seen = String::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < end {
        match c.read(&mut buf) {
            Ok(0) => return (seen, false),
            Ok(n) => {
                seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                if seen.contains(needle) {
                    return (seen, true);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return (seen, false),
        }
    }
    (seen, false)
}

#[test]
fn the_live_view_streams_and_signing_out_closes_it() {
    let t = start();
    let code = t
        .auth
        .lock()
        .unwrap()
        .start_pairing(crate::unix_now(), [2; 5])
        .code;
    let ok = t.post(
        "/api/pair",
        &format!(r#"{{"code":"{code}","name":"Phone"}}"#),
        "",
    );
    let cookie = cookie_of(&ok);
    let id = t.auth.lock().unwrap().stored()[0].id.clone();

    let mut c = TcpStream::connect(t.server.addr()).unwrap();
    c.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    write!(
        c,
        "GET /api/stream HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n"
    )
    .unwrap();
    let (head, ok) = read_until(&mut c, "retry: 3000", Duration::from_secs(3));
    assert!(ok, "{head}");
    assert!(head.contains("Content-Type: text/event-stream"));
    t.server.publish(&snapshot(), TempUnit::C);
    let (data, ok) = read_until(&mut c, "\n\n", Duration::from_secs(3));
    assert!(ok && data.starts_with("data: {"), "{data}");
    assert_eq!(t.watching.load(Ordering::SeqCst), 1);
    assert_eq!(t.server.watching(), vec![id.clone()]);

    // The owner signs the device out in the app: the stream ends within a second.
    let at = Instant::now();
    assert!(t.auth.lock().unwrap().sign_out(&id));
    t.server.close_device(Some(&id));
    let (_, more) = read_until(&mut c, "data: {", Duration::from_secs(2));
    assert!(!more);
    assert!(at.elapsed() < Duration::from_secs(1), "{:?}", at.elapsed());
    let end = Instant::now() + Duration::from_secs(2);
    while t.watching.load(Ordering::SeqCst) != 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(t.watching.load(Ordering::SeqCst), 0);
    assert_eq!(status(&t.get("/api/view", &cookie)), 401);
}

#[test]
fn connections_are_capped_and_stop_closes_everything() {
    let mut t = start();
    // Eight idle connections from one address are allowed; a ninth is closed unread.
    let idle: Vec<TcpStream> = (0..server::MAX_PER_PEER)
        .map(|_| TcpStream::connect(t.server.addr()).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(100));
    let mut extra = TcpStream::connect(t.server.addr()).unwrap();
    extra
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = extra.write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    let mut out = String::new();
    let _ = extra.read_to_string(&mut out);
    assert!(out.is_empty(), "{out}");
    drop(idle);

    let addr = t.server.addr();
    let mut open = TcpStream::connect(addr).unwrap();
    open.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    t.server.stop();
    let mut buf = [0u8; 16];
    assert!(
        matches!(open.read(&mut buf), Ok(0) | Err(_)),
        "open connections are closed"
    );
    assert!(
        TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err(),
        "the port is closed"
    );
}

#[cfg(windows)]
#[test]
fn reads_this_pcs_firewall_and_addresses() {
    let exe = std::env::current_exe().unwrap();
    // Any answer will do; it must not fail or hang.
    eprintln!("firewall: {:?}", firewall::state(&exe));
    let addrs = net::local_addresses();
    eprintln!("addresses: {addrs:?}");
    assert!(addrs.iter().any(|a| !a.ip.is_loopback()));
}

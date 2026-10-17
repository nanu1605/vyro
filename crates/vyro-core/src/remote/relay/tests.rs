use std::net::TcpListener;
use std::sync::atomic::AtomicU32;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tungstenite::handshake::server::{Request, Response};
use tungstenite::Message;

use super::phones::{Hello, Phones, StoredRelay};
use super::proto::{self, b64url, Allowed};
use super::seal::{aad, Dir, Keys, PhoneId, Room, Secret};
use super::session::{Now, Out, Session};
use super::*;
use crate::remote::auth::hex;
use crate::remote::{RemoteInfo, MAX_DEVICES};
use crate::units::TempUnit;
use crate::SensorSnapshot;

const NOW: i64 = 1_760_000_000;
const ROOM: Room = [0x11; 16];
const PC_KEY: Secret = [0x22; 32];
const ID: PhoneId = [0x0a, 0x1b, 0x2c, 0x3d];
const LINK_SECRET: Secret = [0x33; 32];
const OWN_SECRET: Secret = [0x44; 32];

fn info() -> RemoteInfo {
    RemoteInfo {
        pc: "DESKTOP-TEST".into(),
        version: "1.4.0".into(),
        theme: "midnight".into(),
        accent: "theme".into(),
        ..RemoteInfo::default()
    }
}

/// A phone as the app on it would be: seals to the PC, opens what the PC sends.
struct TestPhone {
    id: PhoneId,
    keys: Keys,
}

impl TestPhone {
    fn new(id: PhoneId, secret: &Secret) -> Self {
        Self {
            id,
            keys: Keys::new(secret),
        }
    }

    /// `[id][sealed]`, as the relay hands it to the PC.
    fn frame(&self, json: &str) -> Vec<u8> {
        let sealed = self
            .keys
            .seal(&aad(&ROOM, &self.id, Dir::ToPc), [7; 12], json.as_bytes())
            .unwrap();
        [self.id.as_slice(), &sealed].concat()
    }

    /// What the PC sealed for this phone, or `None` when it was not for it.
    fn read(&self, out: &Out) -> Option<serde_json::Value> {
        let Out::Binary(b) = out else { return None };
        let (id, sealed) = b.split_at(4);
        if id != self.id {
            return None;
        }
        let plain = self
            .keys
            .open(&aad(&ROOM, &self.id, Dir::ToPhone), sealed)?;
        serde_json::from_slice(&plain).ok()
    }
}

fn paired() -> Phones {
    let mut p = Phones::fresh(ROOM, PC_KEY);
    p.start_pairing(NOW, LINK_SECRET, ID).unwrap();
    assert!(matches!(
        p.hello(&ID, "Pixel 9 · Chrome", Some(&b64url(&OWN_SECRET)), NOW, 30),
        Hello::Rekeyed(_)
    ));
    p
}

// --- addresses and messages -----------------------------------------------------------------

#[test]
fn relay_addresses_are_https_hosts_only() {
    assert_eq!(
        proto::relay_base(" https://Vyro-Relay.example.workers.dev/ ").unwrap(),
        "https://vyro-relay.example.workers.dev"
    );
    assert_eq!(
        proto::relay_base("https://relay.example.com:8443").unwrap(),
        "https://relay.example.com:8443"
    );
    assert_eq!(
        proto::relay_base("http://127.0.0.1:8787").unwrap(),
        "http://127.0.0.1:8787"
    );
    assert_eq!(
        proto::relay_base("http://[::1]:8787").unwrap(),
        "http://[::1]:8787"
    );
    for bad in [
        "relay.example.com",
        "http://relay.example.com",
        "ftp://relay.example.com",
        "https://relay.example.com/path",
        "https://relay.example.com?x=1",
        "https://user@relay.example.com",
        "https://relay.example.com:",
        "https://relay.example.com:99999x",
        "https://",
    ] {
        assert!(proto::relay_base(bad).is_err(), "{bad}");
    }
}

#[test]
fn sockets_and_pairing_links() {
    assert_eq!(
        proto::pc_socket("https://r.example.dev", &ROOM),
        format!("wss://r.example.dev/pc/{}", hex(&ROOM))
    );
    assert_eq!(
        proto::pc_socket("http://127.0.0.1:8787", &ROOM),
        format!("ws://127.0.0.1:8787/pc/{}", hex(&ROOM))
    );
    let link = proto::pairing_link("https://r.example.dev", &ROOM, &ID, &LINK_SECRET);
    assert!(link.starts_with("https://r.example.dev/#p=1."));
    assert_eq!(proto::parse_link(&link), Some((ROOM, ID, LINK_SECRET)));
    assert_eq!(proto::parse_link(&link.replace("#p=1.", "#p=2.")), None);
    assert_eq!(proto::parse_link(&format!("{link}.extra")), None);
    assert_eq!(proto::parse_link("https://r.example.dev/"), None);
}

#[test]
fn messages_have_the_shapes_the_relay_and_the_phone_read() {
    let list = proto::phones_message(&[Allowed {
        id: "0a1b2c3d".into(),
        hash: "ab".repeat(32),
    }]);
    let v: serde_json::Value = serde_json::from_str(&list).unwrap();
    assert_eq!(v["t"], "phones");
    assert_eq!(v["list"][0]["id"], "0a1b2c3d");

    let v: serde_json::Value = serde_json::from_str(&proto::info_message(&info())).unwrap();
    assert_eq!(v["t"], "info");
    assert_eq!(v["pc"], "DESKTOP-TEST");
    assert_eq!(v["protocol"], proto::PROTOCOL);

    let v: serde_json::Value = serde_json::from_str(&proto::view_message(r#"{"ts":5}"#)).unwrap();
    assert_eq!(v["t"], "view");
    assert_eq!(v["view"]["ts"], 5);
    let v: serde_json::Value = serde_json::from_str(&proto::rekeyed_message()).unwrap();
    assert_eq!(v["t"], "rekeyed");
}

// --- phones ---------------------------------------------------------------------------------

#[test]
fn a_pairing_link_works_once_and_is_replaced_by_the_phones_own_secret() {
    let mut p = Phones::fresh(ROOM, PC_KEY);
    p.start_pairing(NOW, LINK_SECRET, ID).unwrap();
    assert_eq!(p.pairing(NOW).map(|x| x.1), Some(LINK_SECRET));
    let link_hash = Keys::new(&LINK_SECRET).auth_hash();
    assert_eq!(p.allowed()[0].hash, link_hash);
    assert!(!p.is_paired(&ID), "not paired until the phone answers");
    assert!(p.devices(&[]).is_empty());

    assert_eq!(
        p.hello(&ID, "x", None, NOW, 30),
        Hello::Refused,
        "the link's secret must be replaced"
    );
    let before = p.generation();
    let Hello::Rekeyed(old) = p.hello(&ID, "  Pixel\n9 ", Some(&b64url(&OWN_SECRET)), NOW + 5, 30)
    else {
        panic!("not rekeyed");
    };
    assert_eq!(old, Keys::new(&LINK_SECRET));
    assert!(p.generation() > before, "the relay must hear the new key");
    assert_eq!(p.allowed()[0].hash, Keys::new(&OWN_SECRET).auth_hash());
    assert!(p.is_paired(&ID));
    assert_eq!(p.pairing(NOW), None);
    let d = &p.devices(&["0a1b2c3d".into()])[0];
    assert_eq!(d.name, "Pixel 9");
    assert!(d.anywhere && d.watching);
    assert_eq!(d.expires, NOW + 5 + 30 * 86_400);

    // The link again, or another `next`, changes nothing.
    assert_eq!(
        p.hello(&ID, "y", Some(&b64url(&[0x55; 32])), NOW + 6, 30),
        Hello::Known
    );
    assert_eq!(p.allowed()[0].hash, Keys::new(&OWN_SECRET).auth_hash());
}

#[test]
fn a_pairing_nobody_claims_ends_and_bad_secrets_are_refused() {
    let mut p = Phones::fresh(ROOM, PC_KEY);
    p.start_pairing(NOW, LINK_SECRET, ID).unwrap();
    assert_eq!(
        p.hello(&ID, "x", Some(&b64url(&LINK_SECRET)), NOW, 30),
        Hello::Refused,
        "the same secret again"
    );
    assert_eq!(
        p.hello(&ID, "x", Some("not a secret"), NOW, 30),
        Hello::Refused
    );
    assert_eq!(
        p.hello(&ID, "x", Some(&b64url(&OWN_SECRET)), NOW + PAIR_SECS, 30),
        Hello::Refused,
        "expired"
    );
    assert!(p.prune(NOW + PAIR_SECS));
    assert!(p.allowed().is_empty());
    assert_eq!(p.hello(&[9; 4], "x", None, NOW, 30), Hello::Refused);
}

#[test]
fn a_phone_stays_paired_while_it_is_used() {
    let mut p = paired();
    let first = p.devices(&[])[0].expires;
    p.seen(&ID, NOW + 86_400, 30);
    assert_eq!(p.devices(&[])[0].expires, first + 86_400);
    assert!(!p.prune(NOW + 86_400 + 30 * 86_400 - 1));
    assert!(p.prune(NOW + 86_400 + 30 * 86_400));
    assert!(p.devices(&[]).is_empty());
}

#[test]
fn at_most_eight_phones_and_signing_out() {
    let mut p = Phones::fresh(ROOM, PC_KEY);
    for i in 0..MAX_DEVICES {
        let id = [0, 0, 0, u8::try_from(i).unwrap()];
        p.start_pairing(NOW, [1; 32], id).unwrap();
        let next = b64url(&[u8::try_from(i).unwrap() + 100; 32]);
        assert!(matches!(
            p.hello(&id, "p", Some(&next), NOW, 30),
            Hello::Rekeyed(_)
        ));
    }
    assert!(p.start_pairing(NOW, [1; 32], [9; 4]).is_err());
    assert!(!p.id_free(&[0, 0, 0, 1]));
    assert!(p.sign_out("00000001"));
    assert!(!p.sign_out("00000001"));
    assert!(p.start_pairing(NOW, [1; 32], [9; 4]).is_ok());
    p.sign_out_all();
    assert!(p.allowed().is_empty());
}

#[test]
fn saved_and_loaded_without_the_open_pairing() {
    let mut p = paired();
    p.start_pairing(NOW, [0x66; 32], [5; 4]).unwrap();
    let stored = p.stored(true, Some("https://r.example.dev".into()));
    assert!(stored.enabled);
    assert_eq!(stored.phones.len(), 1, "the open pairing is not saved");
    let text = serde_json::to_string(&stored).unwrap();
    assert!(
        !text.contains(&hex(&OWN_SECRET)),
        "no secret in the clear as hex"
    );
    let back = Phones::load(&stored, NOW).unwrap();
    assert_eq!(back.room(), &ROOM);
    assert_eq!(back.pc_key(), &PC_KEY);
    assert!(back.is_paired(&ID));
    assert_eq!(back.keys(&ID), Some(&Keys::new(&OWN_SECRET)));

    let mut damaged = stored.clone();
    damaged.phones[0].secret = "dpapi:AAAA".into();
    assert!(!Phones::load(&damaged, NOW).unwrap().is_paired(&ID));
    damaged.pc_key = "dpapi:AAAA".into();
    assert!(
        Phones::load(&damaged, NOW).is_none(),
        "a key that does not open: start afresh"
    );
    assert!(Phones::load(&StoredRelay::default(), NOW).is_none());
}

#[test]
fn a_remote_json_from_1_3_reads_with_the_relay_off() {
    let v1 =
        r#"{"version":1,"settings":{"enabled":true,"port":8976,"sessionDays":30},"devices":[]}"#;
    let s: crate::remote::auth::Stored = serde_json::from_str(v1).unwrap();
    assert!(s.relay.is_none());
    assert!(s.settings.enabled);
}

// --- one connection's conversation ----------------------------------------------------------

fn now<'a>(info: &'a dyn Fn() -> RemoteInfo, view: Option<&'a str>) -> Now<'a> {
    Now {
        at: NOW,
        days: 30,
        info,
        view,
    }
}

#[test]
fn pairing_over_the_relay_then_reading() {
    let mut p = Phones::fresh(ROOM, PC_KEY);
    let mut s = Session::new();
    assert!(matches!(s.sync(&p), Some(Out::Text(_))));
    assert_eq!(s.sync(&p), None, "sent once");
    p.start_pairing(NOW, LINK_SECRET, ID).unwrap();
    assert!(
        matches!(s.sync(&p), Some(Out::Text(_))),
        "the pairing is let in"
    );

    let info_fn = info;
    let link_phone = TestPhone::new(ID, &LINK_SECRET);
    let hello = format!(
        r#"{{"t":"hello","name":"iPhone","next":"{}"}}"#,
        b64url(&OWN_SECRET)
    );
    let (outs, fx) = s.on_frame(&mut p, &link_phone.frame(&hello), &now(&info_fn, None));
    assert!(fx.saved);
    assert_eq!(outs.len(), 2);
    assert_eq!(link_phone.read(&outs[0]).unwrap()["t"], "rekeyed");
    let Out::Text(list) = &outs[1] else { panic!() };
    assert!(list.contains(&Keys::new(&OWN_SECRET).auth_hash()));

    // The phone comes back on its own secret.
    let phone = TestPhone::new(ID, &OWN_SECRET);
    let (outs, _) = s.on_frame(
        &mut p,
        &phone.frame(r#"{"t":"hello","name":"iPhone"}"#),
        &now(&info_fn, None),
    );
    let info = phone.read(&outs[0]).unwrap();
    assert_eq!(info["t"], "info");
    assert_eq!(info["device"], "iPhone");
    assert_eq!(info["pc"], "DESKTOP-TEST");
    assert_eq!(
        link_phone.read(&outs[0]),
        None,
        "the link's keys no longer open it"
    );

    // Watching gets the latest view at once, then each new one.
    let (outs, fx) = s.on_frame(
        &mut p,
        &phone.frame(r#"{"t":"watch"}"#),
        &now(&info_fn, Some(r#"{"ts":1}"#)),
    );
    assert!(fx.watching);
    assert_eq!(phone.read(&outs[0]).unwrap()["view"]["ts"], 1);
    let outs = s.views(&p, 2, r#"{"ts":2}"#);
    assert_eq!(phone.read(&outs[0]).unwrap()["view"]["ts"], 2);
    assert!(s.views(&p, 2, r#"{"ts":2}"#).is_empty(), "each view once");

    // The relay says the phone left; this is not an answer to a list.
    assert!(s.on_text(r#"{"t":"here","phones":[]}"#).watching);
    assert_eq!(s.listed(), None);
    let _ = s.on_text(r#"{"t":"here","phones":[],"listed":true}"#);
    assert_eq!(s.listed(), Some(p.generation()));
    assert!(s.views(&p, 3, r#"{"ts":3}"#).is_empty());
}

#[test]
fn frames_that_do_not_open_are_ignored_and_a_phone_can_sign_itself_out() {
    let mut p = paired();
    let mut s = Session::new();
    let _ = s.sync(&p);
    let info_fn = info;
    let stranger = TestPhone::new(ID, &[0x77; 32]);
    let (outs, fx) = s.on_frame(
        &mut p,
        &stranger.frame(r#"{"t":"bye"}"#),
        &now(&info_fn, None),
    );
    assert!(outs.is_empty() && !fx.saved);
    assert!(p.is_paired(&ID));
    let mut tampered = TestPhone::new(ID, &OWN_SECRET).frame(r#"{"t":"bye"}"#);
    tampered[10] ^= 1;
    assert!(s
        .on_frame(&mut p, &tampered, &now(&info_fn, None))
        .0
        .is_empty());
    assert!(s
        .on_frame(&mut p, &[1, 2], &now(&info_fn, None))
        .0
        .is_empty());

    let phone = TestPhone::new(ID, &OWN_SECRET);
    let _ = s.on_frame(
        &mut p,
        &phone.frame(r#"{"t":"watch"}"#),
        &now(&info_fn, None),
    );
    let (outs, fx) = s.on_frame(&mut p, &phone.frame(r#"{"t":"bye"}"#), &now(&info_fn, None));
    assert!(fx.saved && fx.watching);
    let Out::Text(list) = &outs[0] else { panic!() };
    assert!(!list.contains("0a1b2c3d"));
    assert!(!p.is_paired(&ID));
}

#[test]
fn a_phone_signed_out_in_the_app_stops_watching_at_once() {
    let mut p = paired();
    let mut s = Session::new();
    let info_fn = info;
    let phone = TestPhone::new(ID, &OWN_SECRET);
    let _ = s.on_frame(
        &mut p,
        &phone.frame(r#"{"t":"watch"}"#),
        &now(&info_fn, None),
    );
    assert_eq!(s.watching(), vec!["0a1b2c3d".to_owned()]);
    p.sign_out("0a1b2c3d");
    assert!(s.forget_gone(&p));
    assert!(s.watching().is_empty());
}

// --- the connection, against a stand-in relay -----------------------------------------------

struct FakeRelay {
    ws: tungstenite::WebSocket<std::net::TcpStream>,
    offered: String,
}

#[allow(clippy::result_large_err)] // tungstenite's callback type
fn accept(listener: &TcpListener) -> FakeRelay {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let offered = Arc::new(Mutex::new(String::new()));
    let seen = Arc::clone(&offered);
    let ws = tungstenite::accept_hdr(stream, move |req: &Request, mut res: Response| {
        *seen.lock().unwrap() = req
            .headers()
            .get("Sec-WebSocket-Protocol")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        res.headers_mut()
            .insert("Sec-WebSocket-Protocol", "vyro.1".parse().unwrap());
        Ok(res)
    })
    .unwrap();
    let offered = offered.lock().unwrap().clone();
    FakeRelay { ws, offered }
}

impl FakeRelay {
    fn next(&mut self) -> Message {
        loop {
            match self.ws.read().unwrap() {
                Message::Ping(_) | Message::Pong(_) => {}
                m => return m,
            }
        }
    }
}

#[test]
fn the_client_shows_its_key_sends_the_list_answers_phones_and_reconnects() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let phones = Arc::new(Mutex::new(paired()));
    let (tx, rx) = mpsc::channel::<RelayState>();
    let watching = Arc::new(AtomicU32::new(0));
    let w = Arc::clone(&watching);
    let tx = Mutex::new(tx);
    let mut client = RelayClient::start(
        base,
        Arc::clone(&phones),
        30,
        RelayHooks {
            info: Box::new(info),
            saved: Box::new(|| {}),
            watching: Box::new(move |n| w.store(n, std::sync::atomic::Ordering::SeqCst)),
            state: Box::new(move |s| {
                let _ = tx.lock().unwrap().send(s);
            }),
        },
    )
    .unwrap();

    let mut relay = accept(&listener);
    assert!(relay.offered.contains("vyro.1"));
    assert!(relay.offered.contains(&format!("k.{}", b64url(&PC_KEY))));
    let Message::Text(list) = relay.next() else {
        panic!("expected the phone list")
    };
    assert!(list.as_str().contains(&Keys::new(&OWN_SECRET).auth_hash()));

    let phone = TestPhone::new(ID, &OWN_SECRET);
    relay
        .ws
        .send(Message::Text(
            r#"{"t":"here","phones":["0a1b2c3d"]}"#.into(),
        ))
        .unwrap();
    relay
        .ws
        .send(Message::Binary(
            phone.frame(r#"{"t":"hello","name":"iPhone"}"#).into(),
        ))
        .unwrap();
    let Message::Binary(b) = relay.next() else {
        panic!("expected info")
    };
    assert_eq!(phone.read(&Out::Binary(b.to_vec())).unwrap()["t"], "info");

    relay
        .ws
        .send(Message::Binary(phone.frame(r#"{"t":"watch"}"#).into()))
        .unwrap();
    client.publish(&SensorSnapshot::default(), TempUnit::C);
    let deadline = Instant::now() + Duration::from_secs(5);
    let view = loop {
        let Message::Binary(b) = relay.next() else {
            continue;
        };
        let v = phone.read(&Out::Binary(b.to_vec())).unwrap();
        if v["t"] == "view" {
            break v;
        }
        assert!(Instant::now() < deadline);
    };
    assert!(view["view"].is_object());
    assert_eq!(watching.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(client.watching(), vec!["0a1b2c3d".to_owned()]);

    // Signing the phone out in the app reaches the relay at once.
    phones.lock().unwrap().sign_out("0a1b2c3d");
    client.nudge();
    let Message::Text(list) = relay.next() else {
        panic!("expected the new list")
    };
    assert!(!list.as_str().contains("0a1b2c3d"));

    // The relay goes away: the client comes back by itself.
    drop(relay);
    let relay = accept(&listener);
    assert!(relay.offered.contains(&format!("k.{}", b64url(&PC_KEY))));
    let states: Vec<RelayState> = rx.try_iter().collect();
    assert!(states.contains(&RelayState::Online));

    client.stop();
    assert_eq!(client.status().0, RelayState::Off);
}

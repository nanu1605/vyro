use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use str0m::change::SdpAnswer;
use str0m::media::{Direction, MediaKind};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, RtcConfig};

use super::input::{self, Button, Key};
use super::producer::{self, Encode, Encoded, TestScreen};
use super::rtc::{self, Frame, Options, SessionEvent};
use super::stun;

#[test]
fn stun_request_and_xor_mapped_address() {
    let txid = [7u8; 12];
    let req = stun::request(txid);
    assert_eq!(&req[..4], &[0x00, 0x01, 0x00, 0x00]);
    assert_eq!(&req[4..8], &[0x21, 0x12, 0xA4, 0x42]);
    // A success reporting 203.0.113.9:40000, XOR-ed with the cookie.
    let port = 40000u16 ^ 0x2112;
    let ip = [203 ^ 0x21, 0x12, 113 ^ 0xA4, 9 ^ 0x42]; // 0 ^ 0x12 = 0x12
    let mut msg = vec![0x01, 0x01, 0x00, 0x0C, 0x21, 0x12, 0xA4, 0x42];
    msg.extend_from_slice(&txid);
    msg.extend_from_slice(&[0x00, 0x20, 0x00, 0x08, 0x00, 0x01]);
    msg.extend_from_slice(&port.to_be_bytes());
    msg.extend_from_slice(&ip);
    let want: SocketAddr = "203.0.113.9:40000".parse().unwrap();
    assert_eq!(stun::mapped_address(&msg, txid), Some(want));
    // Another transaction, or not a success: nothing.
    assert_eq!(stun::mapped_address(&msg, [8; 12]), None);
    let mut err = msg.clone();
    err[1] = 0x11;
    assert_eq!(stun::mapped_address(&err, txid), None);
    assert_eq!(stun::mapped_address(&msg[..19], txid), None);
    // The plain MAPPED-ADDRESS of old servers.
    let mut old = vec![0x01, 0x01, 0x00, 0x0C, 0x21, 0x12, 0xA4, 0x42];
    old.extend_from_slice(&txid);
    old.extend_from_slice(&[
        0x00, 0x01, 0x00, 0x08, 0x00, 0x01, 0x9C, 0x40, 203, 0, 113, 9,
    ]);
    assert_eq!(stun::mapped_address(&old, txid), Some(want));
}

#[test]
fn input_messages_are_checked() {
    assert_eq!(input::parse(r#"{"t":"here"}"#), Ok(input::Input::Here));
    assert_eq!(
        input::parse(r#"{"t":"press","c":"c"}"#),
        Ok(input::Input::Press { c: 'c' })
    );
    assert!(input::parse(r#"{"t":"press","c":"é"}"#).is_err());
    assert!(input::parse(r#"{"t":"press","c":"+"}"#).is_err());
    assert_eq!(
        input::parse(r#"{"t":"move","x":0.5,"y":0.25}"#),
        Ok(input::Input::Move { x: 0.5, y: 0.25 })
    );
    assert_eq!(
        input::parse(r#"{"t":"button","b":"right","down":true}"#),
        Ok(input::Input::Button {
            b: Button::Right,
            down: true
        })
    );
    assert_eq!(
        input::parse(r#"{"t":"key","k":"Escape","down":false}"#),
        Ok(input::Input::Key {
            k: Key::Escape,
            down: false
        })
    );
    assert!(input::parse(r#"{"t":"move","x":1.5,"y":0}"#).is_err());
    assert!(input::parse(r#"{"t":"scroll","dx":0,"dy":40}"#).is_err());
    assert!(input::parse(r#"{"t":"scroll","dx":0,"dy":-3}"#).is_ok());
    assert!(input::parse(r#"{"t":"key","k":"F13","down":true}"#).is_err());
    assert!(input::parse(r#"{"t":"text","s":"héllo wörld"}"#).is_ok());
    assert!(input::parse(&format!(r#"{{"t":"text","s":"{}"}}"#, "a".repeat(201))).is_err());
    assert!(
        input::parse("{\"t\":\"text\",\"s\":\"\\u0003\"}").is_err(),
        "Ctrl+C is a key"
    );
    assert!(input::parse(r#"{"t":"run","cmd":"x"}"#).is_err());
}

/// Ctrl and Win tapped on the phone's key bar, and a drag begun, when the session ends: its end
/// lets go of them, and input after it is not sent.
#[test]
fn a_session_lets_go_of_what_it_held_when_it_ends() {
    use input::Input::{Button as B, Key as K, Press};
    let mut held = input::Held::default();
    for i in [
        K {
            k: Key::Control,
            down: true,
        },
        Press { c: 'c' },
        K {
            k: Key::Escape,
            down: true,
        },
        K {
            k: Key::Escape,
            down: false,
        },
        K {
            k: Key::Meta,
            down: true,
        },
        K {
            k: Key::Control,
            down: true,
        },
        B {
            b: Button::Right,
            down: true,
        },
        B {
            b: Button::Right,
            down: false,
        },
        B {
            b: Button::Left,
            down: true,
        },
    ] {
        assert!(held.note(&i));
    }
    assert_eq!(
        held.end(),
        [
            B {
                b: Button::Left,
                down: false
            },
            K {
                k: Key::Control,
                down: false
            },
            K {
                k: Key::Meta,
                down: false
            },
        ]
    );
    assert!(!held.note(&K {
        k: Key::Shift,
        down: true
    }));
    assert!(held.end().is_empty(), "let go once");
    // What the phone let go of itself is not let go again.
    let mut tidy = input::Held::default();
    for down in [true, false] {
        assert!(tidy.note(&K { k: Key::Alt, down }));
    }
    assert!(tidy.end().is_empty());
}

/// An H.264 keyframe in Annex B form: SPS, PPS and an IDR slice (the contents do not matter to
/// the transport).
fn keyframe(n: u8) -> Vec<u8> {
    let mut v = vec![0, 0, 0, 1, 0x67, 0x42, 0xe0, 0x1f, 0x8d, 0x68, 0x05];
    v.extend_from_slice(&[0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    v.extend_from_slice(&[0, 0, 0, 1, 0x65, 0x88, 0x84, n]);
    v.extend(std::iter::repeat_n(n, 3000));
    v
}

/// A phone (str0m standing in for the browser) offers to receive video and opens an input
/// channel; the PC's session answers on localhost, sends frames and reports the input.
#[test]
fn a_phone_receives_video_and_sends_input() {
    loopback(false);
}

/// Browsers hide their own addresses behind mDNS names (`….local`) the PC cannot look up: such
/// candidates are skipped, and the PC learns the phone's address from its first check.
#[test]
fn a_phone_hiding_its_address_still_connects() {
    loopback(true);
}

fn loopback(mdns: bool) {
    let mut phone = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .build(Instant::now());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let local = sock.local_addr().unwrap();
    phone.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = phone.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    let cid = api.add_channel("input".into());
    let (offer, pending) = api.apply().unwrap();

    let (tx, events) = mpsc::channel();
    let opts = Options {
        hosts: vec!["127.0.0.1".parse().unwrap()],
        stun: Vec::new(),
        stun_timeout: Duration::ZERO,
    };
    let mut offer = offer.to_sdp_string();
    if mdns {
        let ip = local.ip().to_string();
        assert!(offer.contains(&ip), "{offer}");
        offer = offer.replace(&ip, "4a1b2c3d-1234-4678-9abc-def012345678.local");
    }
    let (mut session, answer) = rtc::start(
        &offer,
        &opts,
        Box::new(move |e| {
            let _ = tx.send(e);
        }),
    )
    .unwrap();
    assert!(answer.contains("H264"), "{answer}");
    session.tell(r#"{"t":"sending","w":2,"h":2}"#.into());
    phone
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();

    let until = Instant::now() + Duration::from_secs(15);
    let mut got_video = None;
    let mut sent_input = false;
    let mut note = None;
    let mut n = 0u8;
    let mut buf = vec![0u8; 2000];
    while got_video.is_none() || !sent_input || note.is_none() {
        if Instant::now() >= until {
            let seen: Vec<SessionEvent> = events.try_iter().collect();
            panic!("timed out (video {got_video:?}, input {sent_input}, note {note:?}, connected {}, closed {}, events {seen:?})", session.connected(), session.closed());
        }
        let deadline = loop {
            match phone.poll_output().unwrap() {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    sock.send_to(&t.contents, t.destination).unwrap();
                }
                Output::Event(Event::ChannelOpen(id, _)) if id == cid && !sent_input => {
                    let mut ch = phone.channel(cid).unwrap();
                    ch.write(false, br#"{"t":"move","x":0.5,"y":0.25}"#)
                        .unwrap();
                    sent_input = true;
                }
                Output::Event(Event::MediaData(d)) => got_video = Some(d.data.to_vec()),
                Output::Event(Event::ChannelData(d)) => {
                    note = Some(String::from_utf8_lossy(&d.data).into_owned());
                }
                Output::Event(_) => {}
            }
        };
        if session.connected() && got_video.is_none() {
            n = n.wrapping_add(1);
            session.send(Frame {
                data: keyframe(n),
                rtp_90khz: u64::from(n) * 3000,
                captured: Instant::now(),
            });
        }
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        sock.set_read_timeout(Some(wait)).unwrap();
        let input = match sock.recv_from(&mut buf) {
            Ok((len, source)) => Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: local,
                    contents: buf[..len].try_into().unwrap(),
                },
            ),
            Err(_) => Input::Timeout(Instant::now()),
        };
        phone.handle_input(input).unwrap();
    }
    assert_eq!(
        note.as_deref(),
        Some(r#"{"t":"sending","w":2,"h":2}"#),
        "the PC's note"
    );
    let video = got_video.unwrap();
    assert!(
        video.windows(5).any(|w| w == [0, 0, 1, 0x65, 0x88])
            || video.windows(2).any(|w| w == [0x65, 0x88])
    );
    // The input arrives at the PC.
    let until = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    while Instant::now() < until {
        if let Ok(e) = events.recv_timeout(Duration::from_millis(100)) {
            seen.push(e.clone());
            if e == SessionEvent::Input(input::Input::Move { x: 0.5, y: 0.25 }) {
                break;
            }
        }
    }
    assert!(seen.contains(&SessionEvent::Connected), "{seen:?}");
    assert!(
        seen.contains(&SessionEvent::Input(input::Input::Move { x: 0.5, y: 0.25 })),
        "{seen:?}"
    );
    session.stop();
    assert!(session.closed());
}

/// An encoder that, like Windows' own, holds back `latency` pictures before giving each out:
/// SPS, PPS and an IDR slice for a keyframe, a P slice otherwise, each carrying its number.
struct HoldingEncoder {
    latency: usize,
    held: std::collections::VecDeque<(i64, bool, u8)>,
    n: u8,
}

impl Encode for HoldingEncoder {
    fn encode(&mut self, _: &[u8], time_100ns: i64, key: bool) -> Result<Vec<Encoded>, String> {
        self.n = self.n.wrapping_add(1);
        self.held.push_back((time_100ns, key, self.n));
        let mut out = Vec::new();
        while self.held.len() > self.latency {
            let (time_100ns, key, n) = self.held.pop_front().unwrap();
            let data = if key {
                keyframe(n)
            } else {
                vec![0, 0, 0, 1, 0x41, 0x9a, n, n, n]
            };
            out.push(Encoded { data, time_100ns });
        }
        Ok(out)
    }
}

/// A phone (str0m) connected to a PC session on localhost, with the PC's screen sent by the
/// producer from a test screen through `encoder`s; collects what the phone receives for `secs`.
/// Returns each picture received and when, and the producer's reports.
fn watch_screen(
    still: bool,
    secs: u64,
    encoder: impl Fn() -> HoldingEncoder + Send + 'static,
) -> (Vec<(Instant, Vec<u8>)>, Vec<producer::Report>) {
    let mut phone = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .build(Instant::now());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let local = sock.local_addr().unwrap();
    phone.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = phone.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    api.add_channel("input".into());
    let (offer, pending) = api.apply().unwrap();
    let opts = Options {
        hosts: vec!["127.0.0.1".parse().unwrap()],
        stun: Vec::new(),
        stun_timeout: Duration::ZERO,
    };
    let (session, answer) = rtc::start(&offer.to_sdp_string(), &opts, Box::new(|_| {})).unwrap();
    let session = std::sync::Arc::new(session);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let screen = TestScreen::new(64, 36);
    screen
        .moving
        .store(!still, std::sync::atomic::Ordering::SeqCst);
    let reports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let producing = {
        let (session, stop, reports) = (
            std::sync::Arc::clone(&session),
            std::sync::Arc::clone(&stop),
            std::sync::Arc::clone(&reports),
        );
        std::thread::spawn(move || {
            let settings = producer::Settings::default();
            let status = |s: producer::Status| {
                if let producer::Status::Report(r) = s {
                    reports.lock().unwrap().push(r);
                }
            };
            producer::drive(&session, &settings, &stop, &status, screen, |_, _, _, _| {
                Ok(encoder())
            })
        })
    };
    // The PC encodes from the start; the phone takes a moment to connect, as over a network.
    std::thread::sleep(Duration::from_millis(500));
    phone
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();
    let until = Instant::now() + Duration::from_secs(secs);
    let mut got = Vec::new();
    let mut buf = vec![0u8; 2000];
    while Instant::now() < until {
        let deadline = loop {
            match phone.poll_output().unwrap() {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    sock.send_to(&t.contents, t.destination).unwrap();
                }
                Output::Event(Event::MediaData(d)) => got.push((Instant::now(), d.data.to_vec())),
                Output::Event(_) => {}
            }
        };
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        sock.set_read_timeout(Some(wait)).unwrap();
        let input = match sock.recv_from(&mut buf) {
            Ok((len, source)) => Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: local,
                    contents: buf[..len].try_into().unwrap(),
                },
            ),
            Err(_) => Input::Timeout(Instant::now()),
        };
        phone.handle_input(input).unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    producing.join().unwrap().unwrap();
    let reports = reports.lock().unwrap().clone();
    (got, reports)
}

/// The phone's first picture is a keyframe, though the encoder made pictures before the phone
/// connected and gives each out four pictures late: one the phone cannot decode is never sent.
#[test]
fn the_phone_gets_a_keyframe_first() {
    let (got, reports) = watch_screen(false, 4, || HoldingEncoder {
        latency: 4,
        held: Default::default(),
        n: 0,
    });
    assert!(got.len() > 10, "{} pictures", got.len());
    assert!(
        frames::is_keyframe(&got[0].1),
        "first: {:?}",
        frames::nal_types(&got[0].1)
    );
    // Every two seconds, what went out: the test screen's size, new pictures, a keyframe and
    // a picture that is not one flat colour.
    let first = reports.first().expect("no report");
    assert_eq!((first.from, first.sent), ((64, 36), (64, 36)), "{first:?}");
    assert!(first.taken > 5 && first.encoded >= first.taken, "{first:?}");
    assert!(first.bitrate > 0 && first.spread > 20, "{first:?}");
    assert!(
        reports.iter().map(|r| r.keyframes).sum::<u32>() >= 1,
        "{reports:?}"
    );
}

/// A still screen: the keyframe made when the phone connected reaches it within about a second,
/// though the encoder holds four pictures back (fed once a second, it took four), and the
/// picture goes on arriving now and then.
#[test]
fn a_still_screen_reaches_the_phone_at_once() {
    let started = Instant::now() + Duration::from_millis(500);
    let (got, reports) = watch_screen(true, 6, || HoldingEncoder {
        latency: 4,
        held: Default::default(),
        n: 0,
    });
    let first = got.first().expect("no picture at all");
    assert!(frames::is_keyframe(&first.1));
    let after = first.0 - started;
    assert!(
        after < Duration::from_millis(2500),
        "the first picture came after {after:?}"
    );
    let last = got.last().unwrap().0;
    assert!(
        last > first.0 + Duration::from_secs(2),
        "a still screen is sent again now and then ({} pictures)",
        got.len()
    );
    // Taken once, then repeated: no new pictures in the later reports, but some encoded.
    let later = reports.last().expect("no report");
    assert_eq!(later.taken, 0, "{reports:?}");
    assert!(later.encoded >= 1, "{reports:?}");
}

#[test]
fn a_bad_offer_is_refused() {
    let opts = Options {
        hosts: vec!["127.0.0.1".parse().unwrap()],
        stun: Vec::new(),
        stun_timeout: Duration::ZERO,
    };
    assert!(rtc::start("not sdp", &opts, Box::new(|_| {})).is_err());
    let none = Options {
        hosts: Vec::new(),
        ..opts
    };
    assert!(rtc::start("v=0\r\n", &none, Box::new(|_| {})).is_err());
}

// ---- Pictures ----

use super::frames;

#[test]
fn the_sent_size_keeps_the_shape() {
    assert_eq!(frames::fit(1920, 1080, 1280, 720), (1280, 720));
    assert_eq!(frames::fit(2560, 1440, 1280, 720), (1280, 720));
    assert_eq!(frames::fit(3440, 1440, 1280, 720), (1280, 534));
    assert_eq!(frames::fit(1366, 768, 1280, 720), (1280, 718));
    assert_eq!(frames::fit(1024, 768, 1280, 720), (960, 720));
    assert_eq!(
        frames::fit(800, 600, 1280, 720),
        (800, 600),
        "never larger than the monitor"
    );
    assert_eq!(frames::fit(0, 0, 1280, 720), (2, 2));
    let (w, h) = frames::fit(1081, 1081, 1279, 1279);
    assert!(w % 2 == 0 && h % 2 == 0);
}

#[test]
fn bgra_becomes_nv12_at_the_sent_size() {
    // 4×2 source: white | black halves, scaled to 2×2.
    let mut src = Vec::new();
    for _ in 0..2 {
        for x in 0..4 {
            let c = if x < 2 { 255 } else { 0 };
            src.extend_from_slice(&[c, c, c, 255]);
        }
    }
    let nv12 = frames::bgra_to_nv12(&src, 4, 2, 16, 2, 2);
    assert_eq!(nv12.len(), 2 * 2 * 3 / 2);
    assert_eq!(&nv12[..4], &[235, 16, 235, 16], "white and black luma");
    assert_eq!(&nv12[4..], &[128, 128], "grey has neutral chroma");
    // Pure red: low U, high V.
    let red = [0u8, 0, 255, 255].repeat(4);
    let nv12 = frames::bgra_to_nv12(&red, 2, 2, 8, 2, 2);
    assert_eq!(nv12[0], 82);
    assert!(nv12[4] < 128 && nv12[5] > 200, "{:?}", &nv12[4..]);
    // A row pitch wider than the picture is honoured; a short buffer gives black, not a panic.
    let padded: Vec<u8> = [[255u8, 255, 255, 255].repeat(2), vec![9; 8]]
        .concat()
        .repeat(2);
    assert_eq!(
        &frames::bgra_to_nv12(&padded, 2, 2, 16, 2, 2)[..4],
        &[235; 4]
    );
    assert_eq!(frames::bgra_to_nv12(&[0; 3], 2, 2, 8, 2, 2), vec![0; 6]);
}

/// A monitor sent smaller keeps thin lines: one-pixel black lines every third column of a white
/// picture, scaled to half, still darken every column they fall in (picking the nearest pixel
/// lost half of them).
#[test]
fn thin_lines_survive_scaling() {
    let (sw, sh) = (12u32, 2u32);
    let src: Vec<u8> = (0..sh)
        .flat_map(|_| (0..sw).flat_map(|x| if x % 3 == 1 { [0, 0, 0, 255] } else { [255; 4] }))
        .collect();
    let bgr = frames::bgra_to_bgr(&src, sw, sh, sw as usize * 4, 6, 2);
    let row: Vec<u8> = bgr[..6 * 3].iter().step_by(3).copied().collect();
    let lines = sw as usize / 3;
    assert!(row.iter().filter(|&&v| v < 200).count() >= lines, "{row:?}");
    let nv12 = frames::bgra_to_nv12(&src, sw, sh, sw as usize * 4, 6, 2);
    assert!(
        nv12[..6].iter().filter(|&&v| v < 200).count() >= lines,
        "{:?}",
        &nv12[..6]
    );
}

#[test]
fn the_test_pattern_moves() {
    let a = frames::test_pattern(64, 32, 0);
    let b = frames::test_pattern(64, 32, 3);
    assert_eq!(a.len(), 64 * 32 * 4);
    assert_ne!(a, b);
    assert_eq!(&a[..4], &[255, 255, 255, 255], "white bar first");
}

#[test]
fn nal_units_and_parameter_sets() {
    let k = keyframe(1);
    assert_eq!(frames::nal_types(&k), [7, 8, 5]);
    assert!(frames::is_keyframe(&k));
    let p = vec![0, 0, 0, 1, 0x41, 0x9a, 0, 0, 1, 0x41, 1];
    assert_eq!(frames::nal_types(&p), [1, 1]);
    assert!(!frames::is_keyframe(&p));
    let headers = k[..19].to_vec();
    let idr = k[19..].to_vec();
    assert_eq!(frames::nal_types(&idr), [5]);
    assert_eq!(
        frames::nal_types(&frames::with_headers(idr, &headers)),
        [7, 8, 5]
    );
    assert_eq!(
        frames::with_headers(p.clone(), &headers),
        p,
        "only keyframes"
    );
    assert_eq!(frames::with_headers(k.clone(), &headers), k, "not twice");
}

#[test]
fn positions_land_on_the_shared_monitor() {
    let primary = input::Rect {
        left: 0,
        top: 0,
        width: 1920,
        height: 1080,
    };
    // One monitor: the corners are the desktop's corners.
    assert_eq!(input::absolute(0.0, 0.0, primary, primary), (0, 0));
    assert_eq!(input::absolute(1.0, 1.0, primary, primary), (65535, 65535));
    assert_eq!(input::absolute(0.5, 0.5, primary, primary), (32768, 32768));
    // A second monitor to the left: the desktop starts at -2560.
    let left = input::Rect {
        left: -2560,
        top: 0,
        width: 2560,
        height: 1440,
    };
    let desktop = input::Rect {
        left: -2560,
        top: 0,
        width: 4480,
        height: 1440,
    };
    assert_eq!(input::absolute(0.0, 0.0, primary, desktop), (37457, 0));
    assert_eq!(input::absolute(0.0, 0.0, left, desktop), (0, 0));
    let (x, y) = input::absolute(1.0, 1.0, primary, desktop);
    assert_eq!(x, 65535);
    assert_eq!(y, 49140, "the primary monitor is shorter");
}

#[test]
fn an_unused_session_ends_after_ten_minutes() {
    use super::share::idle;
    assert!(!idle(Duration::from_secs(9 * 60 + 59)));
    assert!(idle(Duration::from_secs(10 * 60)));
}

#[test]
fn the_slow_view_has_twenty_minutes_a_day() {
    use super::share::{SlowAllowance, SLOW_DAILY_SECS};
    let day = 20_000 * 86_400;
    let mut a = SlowAllowance::default();
    assert_eq!(a.left(day + 10), SLOW_DAILY_SECS);
    assert!(a.spend(day + 10, 19 * 60));
    assert_eq!(a.left(day + 20), 60);
    assert!(a.spend(day + 30, 120), "the last bit is taken, capped");
    assert_eq!(a.left(day + 40), 0);
    assert!(!a.spend(day + 50, 1));
    // The next day (UTC) starts over.
    assert_eq!(a.left(day + 86_400), SLOW_DAILY_SECS);
    assert!(a.spend(day + 86_400, 1));
    // As kept in remote.json.
    let back: SlowAllowance = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(back, a);
    assert_eq!(
        serde_json::from_str::<SlowAllowance>("{}").unwrap(),
        SlowAllowance::default()
    );
}

#[test]
fn notes_for_the_phone() {
    use super::share::Note;
    assert_eq!(
        Note::Sending { w: 1280, h: 720 }.json(),
        r#"{"t":"sending","w":1280,"h":720}"#
    );
    assert_eq!(Note::Secure.json(), r#"{"t":"secure"}"#);
    let r = super::producer::Report {
        capture: "GDI",
        from: (2560, 1440),
        sent: (1920, 1080),
        taken: 30,
        encoded: 30,
        keyframes: 1,
        bitrate: 5_000_000,
        allowed: 6_200_000,
        luma: 120,
        spread: 60,
        asked: 2,
    };
    assert_eq!(
        Note::report(&r).json(),
        r#"{"t":"report","capture":"GDI","from":[2560,1440],"sent":[1920,1080],"taken":30,"encoded":30,"keyframes":1,"bitrate":5000000,"allowed":6200000,"luma":120,"spread":60,"asked":2}"#
    );
    assert_eq!(
        r.to_string(),
        "2560x1440 through GDI, sent as 1920x1080: 30 new and 30 encoded a second, 1 keyframes, \
         5000 kbit/s (the connection carries 6200 kbit/s), brightness 120 varying 60, 2 \
         keyframes asked for"
    );
    let unknown = super::producer::Report { allowed: 0, ..r };
    assert!(
        unknown
            .to_string()
            .contains("(the connection carries not known yet)"),
        "{unknown}"
    );
    assert_eq!(
        Note::End {
            why: "stopped on the PC".into()
        }
        .json(),
        r#"{"t":"end","why":"stopped on the PC"}"#
    );
    assert_eq!(
        Note::Mic {
            on: true,
            into: Some("CABLE Output (VB-Audio Virtual Cable)".into()),
            default: true,
            why: None,
        }
        .json(),
        r#"{"t":"mic","on":true,"into":"CABLE Output (VB-Audio Virtual Cable)","default":true,"why":null}"#
    );
}

/// A picture of zeros is blank (alpha aside); one lit pixel on the grid, or a dark grey desktop,
/// is not.
#[test]
fn a_picture_of_zeros_is_blank() {
    let (w, h) = (2560usize, 1440usize);
    let mut px = vec![0u8; w * h * 4];
    assert!(frames::blank(&px));
    for a in px.iter_mut().skip(3).step_by(4) {
        *a = 255;
    }
    assert!(frames::blank(&px), "alpha alone is still blank");
    px[0] = 1;
    assert!(!frames::blank(&px));
    let grey = [28u8, 28, 28, 255].repeat(w * h);
    assert!(!frames::blank(&grey));
    assert!(frames::blank(&[]));
}

/// A black picture reads dark and flat; the test pattern bright and varied; nothing to read, zero.
#[test]
fn brightness_tells_a_black_picture() {
    let (w, h) = (64u32, 36u32);
    let black = [
        vec![16u8; (w * h) as usize],
        vec![128; (w * h / 2) as usize],
    ]
    .concat();
    assert_eq!(frames::brightness(&black, w, h), (16, 0));
    let bars = frames::test_pattern(w, h, 0);
    let nv12 = frames::bgra_to_nv12(&bars, w, h, w as usize * 4, w, h);
    let (luma, spread) = frames::brightness(&nv12, w, h);
    assert!(luma > 60 && spread > 20, "{luma} {spread}");
    assert_eq!(frames::brightness(&[], w, h), (0, 0));
    // Larger than the grid: sampled, not every pixel.
    let big = [vec![200u8; 1920 * 1080], vec![128; 1920 * 540]].concat();
    assert_eq!(frames::brightness(&big, 1920, 1080), (200, 0));
}

#[test]
fn the_slow_view_scales_to_bgr() {
    // 4×2 BGRA: each pixel's blue is its index; halved to 2×1, the average of each 2×2 block.
    let src: Vec<u8> = (0..8u8).flat_map(|i| [i, 100 + i, 200 + i, 255]).collect();
    assert_eq!(
        frames::bgra_to_bgr(&src, 4, 2, 16, 2, 1),
        vec![3, 103, 203, 5, 105, 205]
    );
    assert_eq!(frames::bgra_to_bgr(&src, 4, 2, 16, 4, 2).len(), 4 * 2 * 3);
    // Too short a picture: black, not a panic.
    assert_eq!(frames::bgra_to_bgr(&src[..8], 4, 2, 16, 2, 1), vec![0; 6]);
    assert_eq!(frames::fit(1920, 1080, 960, 600), (960, 540));
}

/// The size and pictures a second for each bit rate, never above what the settings ask.
#[test]
fn the_step_follows_the_bit_rate() {
    use producer::{step, Settings, Step};
    let s = Settings::default();
    let at = |w, h, fps| Step {
        max_width: w,
        max_height: h,
        fps,
    };
    assert_eq!(step(5_000_000, &s), at(1920, 1080, 30));
    assert_eq!(step(2_000_000, &s), at(1920, 1080, 30));
    assert_eq!(step(1_999_999, &s), at(1280, 720, 30));
    assert_eq!(step(1_000_000, &s), at(1280, 720, 30));
    assert_eq!(step(800_000, &s), at(1280, 720, 15));
    assert_eq!(step(300_000, &s), at(960, 540, 10));
    let small = Settings {
        max_width: 640,
        max_height: 360,
        fps: 12,
        ..s
    };
    assert_eq!(step(5_000_000, &small), at(640, 360, 12));
    assert_eq!(step(300_000, &small), at(640, 360, 10));
}

/// SD (1.7) caps the size, the pictures a second and the bit rate, whatever the connection
/// carries; HD is the settings as they are. The phone says which in a message.
#[test]
fn sd_spares_mobile_data() {
    use producer::{step, Adapt, Quality, Settings};
    let s = Settings::default();
    assert_eq!(Quality::Hd.apply(&s), s);
    let sd = Quality::Sd.apply(&s);
    assert_eq!(
        (sd.max_width, sd.max_height, sd.fps, sd.bitrate),
        (960, 540, 15, 1_000_000)
    );
    assert_eq!(sd.monitor, s.monitor);
    // A fast connection still gets SD's size and rate.
    let t0 = Instant::now();
    let mut a = Adapt::new(&sd, t0);
    a.update(Some(20_000_000), &sd, t0 + Duration::from_secs(10));
    assert_eq!(a.bitrate, 1_000_000);
    assert_eq!(
        (a.step.max_width, a.step.max_height, a.step.fps),
        (960, 540, 15)
    );
    // Back to HD: everything at once, without waiting for the connection to prove it.
    let hd = Adapt::new(&Quality::Hd.apply(&s), t0);
    assert_eq!((hd.bitrate, hd.step), (5_000_000, step(5_000_000, &s)));
    // SD never asks for more than smaller settings already do.
    let small = Settings {
        max_width: 640,
        max_height: 360,
        fps: 10,
        bitrate: 500_000,
        ..s
    };
    assert_eq!(Quality::Sd.apply(&small), small);
    // The slow view in SD: smaller pictures, more compressed.
    let ((w, h), q) = Quality::Sd.slow();
    let ((hw, hh), hq) = Quality::Hd.slow();
    assert!(w < hw && h < hh && q < hq);
    // The phone's message, and the ids the session keeps.
    assert_eq!(
        input::parse(r#"{"t":"quality","q":"sd"}"#),
        Ok(input::Input::Quality { q: Quality::Sd })
    );
    assert!(input::parse(r#"{"t":"quality","q":"4k"}"#).is_err());
    for q in [Quality::Hd, Quality::Sd] {
        assert_eq!(Quality::from_u8(q.to_u8()), q);
    }
}

/// A switch to SD, and back, takes effect at once on a still screen too: the picture is taken
/// again at the new size, rather than the last one going on at the old size until something
/// moves (the phone's own button does not move anything on the PC).
#[test]
fn a_quality_switch_takes_effect_on_a_still_screen() {
    use producer::Quality;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let mut phone = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .build(Instant::now());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    phone.add_local_candidate(Candidate::host(sock.local_addr().unwrap(), "udp").unwrap());
    let mut api = phone.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    api.add_channel("input".into());
    let (offer, _pending) = api.apply().unwrap();
    let opts = Options {
        hosts: vec!["127.0.0.1".parse().unwrap()],
        stun: Vec::new(),
        stun_timeout: Duration::ZERO,
    };
    let (session, _answer) = rtc::start(&offer.to_sdp_string(), &opts, Box::new(|_| {})).unwrap();
    let session = Arc::new(session);
    let stop = Arc::new(AtomicBool::new(false));
    let screen = TestScreen::new(1920, 1080);
    screen.moving.store(false, Ordering::SeqCst);
    // Each encoder made: its size and pictures a second.
    let made = Arc::new(Mutex::new(Vec::<(u32, u32, u32)>::new()));
    let producing = {
        let (session, stop, made) = (Arc::clone(&session), Arc::clone(&stop), Arc::clone(&made));
        std::thread::spawn(move || {
            producer::drive(
                &session,
                &producer::Settings::default(),
                &stop,
                &|_| {},
                screen,
                |w, h, fps, bitrate| {
                    made.lock().unwrap().push((w, h, fps));
                    Ok(SizedEncoder { fps, bitrate, n: 0 })
                },
            )
        })
    };
    let wait_for = |want: (u32, u32, u32), from: usize| {
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if made.lock().unwrap()[from..].contains(&want) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no encoder for {want:?}: {:?}", made.lock().unwrap());
    };
    wait_for((1920, 1080, 30), 0);
    session.set_quality(Quality::Sd);
    wait_for((960, 540, 15), 1);
    let n = made.lock().unwrap().len();
    session.set_quality(Quality::Hd);
    wait_for((1920, 1080, 30), n);
    stop.store(true, Ordering::SeqCst);
    producing.join().unwrap().unwrap();
}

/// The bit rate and step follow the connection: down at once, up only once the connection has
/// carried more for a while, and the bit rate not for every small change.
#[test]
fn quality_follows_the_connection() {
    use producer::{Adapt, Change, Settings};
    let s = Settings::default();
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut a = Adapt::new(&s, t0);
    // No estimate yet: everything the settings ask.
    assert_eq!(a.update(None, &s, at(100)), Change::default());
    assert_eq!((a.bitrate, a.step.fps), (5_000_000, 30));
    // A slow connection: 85 % of it, 720p at 15 a second, at once.
    let c = a.update(Some(1_000_000), &s, at(200));
    assert_eq!(
        c,
        Change {
            bitrate: true,
            step: true
        }
    );
    assert_eq!(a.bitrate, 850_000);
    assert_eq!((a.step.max_height, a.step.fps), (720, 15));
    // A few per cent more changes nothing; a fifth more does, but not sooner than half a
    // second after the last change.
    assert_eq!(a.update(Some(1_050_000), &s, at(500)), Change::default());
    assert_eq!(a.update(Some(1_200_000), &s, at(600)), Change::default());
    let c = a.update(Some(1_200_000), &s, at(1000));
    assert!(c.bitrate && !c.step, "{c:?}");
    assert_eq!(a.bitrate, 1_020_000);
    // A fast connection: the bit rate follows within the step; a larger step waits until a
    // larger one has been allowed for 5 s (here since 600 ms).
    let c = a.update(Some(8_000_000), &s, at(1600));
    assert!(c.bitrate && !c.step, "{c:?}");
    assert_eq!(a.bitrate, 5_000_000, "never above the settings");
    assert_eq!(a.step.max_height, 720);
    assert!(!a.update(Some(8_000_000), &s, at(5599)).step);
    let c = a.update(Some(8_000_000), &s, at(5600));
    assert!(c.step && !c.bitrate, "{c:?}");
    assert_eq!((a.step.max_height, a.step.fps), (1080, 30));
    // A dip: the step goes down at once, and back up only after 5 s.
    assert!(a.update(Some(200_000), &s, at(7000)).step);
    assert_eq!(a.bitrate, producer::LEAST, "never below the least");
    assert_eq!((a.step.max_height, a.step.fps), (540, 10));
    assert!(!a.update(Some(8_000_000), &s, at(8000)).step);
    assert!(!a.update(Some(8_000_000), &s, at(12_999)).step);
    assert!(a.update(Some(8_000_000), &s, at(13_000)).step);
}

/// An encoder whose pictures are as large as its bit rate makes them (the contents do not
/// matter to the transport), and which takes a new rate while encoding.
struct SizedEncoder {
    fps: u32,
    bitrate: u32,
    n: u8,
}

impl Encode for SizedEncoder {
    fn encode(&mut self, _: &[u8], time_100ns: i64, key: bool) -> Result<Vec<Encoded>, String> {
        self.n = self.n.wrapping_add(1);
        let mut data = if key {
            keyframe(self.n)
        } else {
            vec![0, 0, 0, 1, 0x41, 0x9a]
        };
        // No zeros: nothing in it reads as a start code.
        let size = (self.bitrate / 8 / self.fps.max(1)) as usize;
        data.resize(data.len().max(size), 0x80 | self.n);
        Ok(vec![Encoded { data, time_100ns }])
    }
    fn set_bitrate(&mut self, bitrate: u32) -> bool {
        self.bitrate = bitrate;
        true
    }
}

/// How long a packet waited in [`slow_link`]'s queue, and when it came.
type Waits = std::sync::Arc<std::sync::Mutex<Vec<(Instant, Duration)>>>;

/// A link from the PC to the phone that carries `bps` and holds up to half a second of packets,
/// as a mobile network does; beyond that they are lost. The phone's packets pass as they come.
/// Returns the address the phone sends to, and each packet's wait in the queue.
fn slow_link(
    pc: SocketAddr,
    bps: u64,
    stop: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (SocketAddr, Waits) {
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    const HOLDS: Duration = Duration::from_millis(500);
    let near = UdpSocket::bind("127.0.0.1:0").unwrap();
    let far = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = near.local_addr().unwrap();
    let phone: Arc<Mutex<Option<SocketAddr>>> = Arc::default();
    let waits: Waits = Arc::default();
    {
        let (near, far, phone, stop) = (
            near.try_clone().unwrap(),
            far.try_clone().unwrap(),
            Arc::clone(&phone),
            Arc::clone(stop),
        );
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 2000];
            near.set_read_timeout(Some(Duration::from_millis(20)))
                .unwrap();
            while !stop.load(Ordering::SeqCst) {
                if let Ok((n, from)) = near.recv_from(&mut buf) {
                    *phone.lock().unwrap() = Some(from);
                    let _ = far.send_to(&buf[..n], pc);
                }
            }
        });
    }
    {
        let (waits, stop) = (Arc::clone(&waits), Arc::clone(stop));
        std::thread::spawn(move || {
            let mut queue = std::collections::VecDeque::<(Instant, Vec<u8>)>::new();
            let mut free = Instant::now();
            let mut buf = vec![0u8; 2000];
            while !stop.load(Ordering::SeqCst) {
                let now = Instant::now();
                while queue.front().is_some_and(|(at, _)| *at <= now) {
                    let (_, p) = queue.pop_front().unwrap();
                    if let Some(to) = *phone.lock().unwrap() {
                        let _ = near.send_to(&p, to);
                    }
                }
                let wait = queue.front().map_or(Duration::from_millis(5), |(at, _)| {
                    at.saturating_duration_since(now)
                });
                far.set_read_timeout(Some(wait.max(Duration::from_micros(200))))
                    .unwrap();
                if let Ok((n, _)) = far.recv_from(&mut buf) {
                    let now = Instant::now();
                    let gone = free.max(now) + Duration::from_secs_f64((n * 8) as f64 / bps as f64);
                    let wait = gone - now;
                    if wait <= HOLDS {
                        free = gone;
                        queue.push_back((gone, buf[..n].to_vec()));
                        waits.lock().unwrap().push((now, wait));
                    }
                }
            }
        });
    }
    (addr, waits)
}

/// On a connection that carries 800 kbit/s (a slow mobile network), the PC soon sends less
/// than that, in fewer pictures, and the delay stays low: without it, 2.5 Mbit/s and more would
/// fill the network's queue, and the picture would fall seconds behind and lose pieces.
#[test]
fn a_slow_connection_gets_less_and_stays_current() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let mut phone = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .build(Instant::now());
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let local = sock.local_addr().unwrap();
    phone.add_local_candidate(Candidate::host(local, "udp").unwrap());
    let mut api = phone.sdp_api();
    api.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
    api.add_channel("input".into());
    let (offer, pending) = api.apply().unwrap();
    // The PC learns the phone's address from its first check, which comes through the link.
    let offer = offer.to_sdp_string().replace(
        &local.ip().to_string(),
        "4a1b2c3d-1234-4678-9abc-def012345678.local",
    );
    let opts = Options {
        hosts: vec!["127.0.0.1".parse().unwrap()],
        stun: Vec::new(),
        stun_timeout: Duration::ZERO,
    };
    let (session, answer) = rtc::start(&offer, &opts, Box::new(|_| {})).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let candidate = answer
        .lines()
        .find(|l| l.starts_with("a=candidate:"))
        .expect("the PC's address");
    let f: Vec<&str> = candidate.split_whitespace().collect();
    let pc: SocketAddr = format!("{}:{}", f[4], f[5]).parse().unwrap();
    let (link, waits) = slow_link(pc, 800_000, &stop);
    let answer = answer.replace(
        &format!("{} {} typ", f[4], f[5]),
        &format!("{} {} typ", link.ip(), link.port()),
    );
    let session = Arc::new(session);
    let reports = Arc::new(Mutex::new(Vec::new()));
    let producing = {
        let (session, stop, reports) = (
            Arc::clone(&session),
            Arc::clone(&stop),
            Arc::clone(&reports),
        );
        std::thread::spawn(move || {
            let status = |s: producer::Status| {
                if let producer::Status::Report(r) = s {
                    reports.lock().unwrap().push(r);
                }
            };
            producer::drive(
                &session,
                &producer::Settings::default(),
                &stop,
                &status,
                TestScreen::new(64, 36),
                |_, _, fps, bitrate| Ok(SizedEncoder { fps, bitrate, n: 0 }),
            )
        })
    };
    phone
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();
    let started = Instant::now();
    let until = started + Duration::from_secs(16);
    let mut got = Vec::new();
    let mut buf = vec![0u8; 2000];
    while Instant::now() < until {
        let deadline = loop {
            match phone.poll_output().unwrap() {
                Output::Timeout(t) => break t,
                Output::Transmit(t) => {
                    sock.send_to(&t.contents, t.destination).unwrap();
                }
                Output::Event(Event::MediaData(_)) => got.push(Instant::now()),
                Output::Event(_) => {}
            }
        };
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(1), Duration::from_millis(10));
        sock.set_read_timeout(Some(wait)).unwrap();
        let input = match sock.recv_from(&mut buf) {
            Ok((len, source)) => Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: local,
                    contents: buf[..len].try_into().unwrap(),
                },
            ),
            Err(_) => Input::Timeout(Instant::now()),
        };
        phone.handle_input(input).unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    producing.join().unwrap().unwrap();
    let reports = reports.lock().unwrap().clone();
    for r in &reports {
        eprintln!(
            "screen: {} a second, {} kbit/s sent, the connection carries {} kbit/s",
            r.encoded,
            r.bitrate / 1000,
            r.allowed / 1000
        );
    }
    let waits = waits.lock().unwrap().clone();
    let mean = |from: Instant, to: Instant| {
        let w: Vec<Duration> = waits
            .iter()
            .filter(|(at, _)| (from..to).contains(at))
            .map(|(_, w)| *w)
            .collect();
        w.iter().sum::<Duration>() / u32::try_from(w.len().max(1)).unwrap()
    };
    let end = started + Duration::from_secs(12);
    eprintln!(
        "screen: packets waited {:?} in the link on average in the first 4 s, {:?} in the last \
         4 s; the phone got {} pictures",
        mean(started, started + Duration::from_secs(4)),
        mean(end, until),
        got.len()
    );
    let last = reports.last().expect("no report");
    assert!(
        last.allowed > 0 && last.allowed < 1_300_000,
        "the estimate follows the link: {last:?}"
    );
    assert!(last.bitrate < 900_000, "less than the link: {last:?}");
    assert!(
        mean(end, until) < Duration::from_millis(250),
        "the queue drains: {:?}",
        mean(end, until)
    );
    assert!(
        got.iter().filter(|t| **t >= end).count() >= 10,
        "pictures still arrive"
    );
}

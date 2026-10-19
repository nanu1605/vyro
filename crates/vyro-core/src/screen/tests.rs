use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use str0m::change::SdpAnswer;
use str0m::media::{Direction, MediaKind};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, Input, Output, RtcConfig};

use super::input::{self, Button, Key};
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
    let (mut session, answer) = rtc::start(
        &offer.to_sdp_string(),
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
    assert_eq!(
        Note::End {
            why: "stopped on the PC".into()
        }
        .json(),
        r#"{"t":"end","why":"stopped on the PC"}"#
    );
}

#[test]
fn the_slow_view_scales_to_bgr() {
    // 4×2 BGRA: each pixel's blue is its index; halved to 2×1, the left pixel of each pair.
    let src: Vec<u8> = (0..8u8).flat_map(|i| [i, 100 + i, 200 + i, 255]).collect();
    assert_eq!(
        frames::bgra_to_bgr(&src, 4, 2, 16, 2, 1),
        vec![0, 100, 200, 2, 102, 202]
    );
    assert_eq!(frames::bgra_to_bgr(&src, 4, 2, 16, 4, 2).len(), 4 * 2 * 3);
    // Too short a picture: black, not a panic.
    assert_eq!(frames::bgra_to_bgr(&src[..8], 4, 2, 16, 2, 1), vec![0; 6]);
    assert_eq!(frames::fit(1920, 1080, 960, 600), (960, 540));
}

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
    phone
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();

    let until = Instant::now() + Duration::from_secs(15);
    let mut got_video = None;
    let mut sent_input = false;
    let mut n = 0u8;
    let mut buf = vec![0u8; 2000];
    while got_video.is_none() || !sent_input {
        if Instant::now() >= until {
            let seen: Vec<SessionEvent> = events.try_iter().collect();
            panic!("timed out (video {got_video:?}, input {sent_input}, connected {}, closed {}, events {seen:?})", session.connected(), session.closed());
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

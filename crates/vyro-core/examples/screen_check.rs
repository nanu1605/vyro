//! The app's screen video, played in a real browser (1.7): the producer the app runs (on
//! Windows the monitor through Windows' own encoder; elsewhere a moving test pattern through
//! OpenH264) sends to a WebRTC session on this machine. `scripts/screen-check.mjs` plays the
//! phone: it writes `{"offer": …}` as a line and reads `{"answer": …}` back; `still` and
//! `moving` lines set the test pattern (off Windows). Prints `sending …`, `connected` and
//! `closed …` as they happen.
//!
//! ```sh
//! node scripts/screen-check.mjs   # builds and runs this
//! ```

use std::io::BufRead;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use vyro_core::screen::producer::{self, Status};
use vyro_core::screen::rtc::{self, SessionEvent};
use vyro_core::screen::share::Note;

#[cfg(not(windows))]
#[path = "common/openh264.rs"]
mod openh264_encoder;

fn main() -> Result<(), String> {
    let mut lines = std::io::stdin().lock().lines();
    let first = lines.next().ok_or("no offer")?.map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&first).map_err(|e| e.to_string())?;
    let offer = v["offer"].as_str().ok_or("no offer")?;
    let opts = rtc::Options {
        hosts: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        stun: Vec::new(),
        ..rtc::Options::default()
    };
    let (session, answer) = rtc::start(
        offer,
        &opts,
        Box::new(|e| match e {
            SessionEvent::Connected => println!("connected"),
            SessionEvent::Input(i) => println!("input {i:?}"),
            SessionEvent::Closed(why) => println!("closed {why}"),
        }),
    )?;
    println!("{}", serde_json::json!({ "answer": answer }));
    let session = Arc::new(session);
    let stop = Arc::new(AtomicBool::new(false));
    let moving = Arc::new(AtomicBool::new(true));
    let producing = std::thread::spawn({
        let (session, stop, moving) =
            (Arc::clone(&session), Arc::clone(&stop), Arc::clone(&moving));
        move || {
            let status = |s: Status| match s {
                Status::Sending { width, height, .. } => {
                    session.tell(
                        Note::Sending {
                            w: width,
                            h: height,
                        }
                        .json(),
                    );
                    println!("sending {width}x{height}");
                }
                Status::Secure => println!("secure"),
            };
            #[cfg(windows)]
            {
                let _ = moving;
                producer::run(&session, &producer::Settings::default(), &stop, &status)
            }
            #[cfg(not(windows))]
            {
                let mut screen = producer::TestScreen::new(640, 360);
                screen.moving = moving;
                let settings = producer::Settings {
                    bitrate: 1_000_000,
                    ..producer::Settings::default()
                };
                producer::drive(
                    &session,
                    &settings,
                    &stop,
                    &status,
                    screen,
                    openh264_encoder::openh264,
                )
            }
        }
    });
    for line in lines {
        match line.as_deref() {
            Ok("still") => moving.store(false, Ordering::SeqCst),
            Ok("moving") => moving.store(true, Ordering::SeqCst),
            Ok("q") | Err(_) => break,
            Ok(_) => {}
        }
    }
    stop.store(true, Ordering::SeqCst);
    match producing.join() {
        Ok(Err(e)) => println!("closed the video stopped: {e}"),
        Err(_) => println!("closed the video thread panicked"),
        Ok(Ok(())) => {}
    }
    Ok(())
}

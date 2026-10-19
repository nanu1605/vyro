//! The app's screen video, played in a real browser (1.7): the producer the app runs (on
//! Windows the monitor through Windows' own encoder; elsewhere a moving test pattern through
//! OpenH264) sends to a WebRTC session on this machine. `scripts/screen-check.mjs` plays the
//! phone: it writes `{"offer": …}` as a line and reads `{"answer": …}` back; `still` and
//! `moving` lines set the test pattern (off Windows, or with `--test`). Prints `sending …`,
//! `connected` and `closed …` as they happen.
//!
//! `--test WxH` sends a moving test pattern of that size instead of the monitor (on Windows still
//! through Windows' encoder: a monitor larger than 1080p is scaled as the app scales it).
//! `--dump FILE` writes every picture the encoder gives out to FILE (Annex B) and a line per
//! picture to FILE.txt (time, bytes, NAL unit types), for reading the H.264 elsewhere.
//!
//! ```sh
//! node scripts/screen-check.mjs   # builds and runs this
//! ```

use std::fs::File;
use std::io::{BufRead, Write};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use vyro_core::screen::frames;
use vyro_core::screen::producer::{self, Encode, Encoded, Status};
use vyro_core::screen::rtc::{self, SessionEvent};
use vyro_core::screen::share::Note;

#[cfg(not(windows))]
#[path = "common/openh264.rs"]
mod openh264_encoder;

/// An encoder whose pictures are also written to a file.
struct Dump<E> {
    inner: E,
    files: Option<(File, File)>,
}

impl<E: Encode> Encode for Dump<E> {
    fn encode(
        &mut self,
        nv12: &[u8],
        time_100ns: i64,
        keyframe: bool,
    ) -> Result<Vec<Encoded>, String> {
        let out = self.inner.encode(nv12, time_100ns, keyframe)?;
        if let Some((h264, list)) = &mut self.files {
            for e in &out {
                let _ = h264.write_all(&e.data);
                let _ = writeln!(
                    list,
                    "{} {} {:?}",
                    e.time_100ns,
                    e.data.len(),
                    frames::nal_types(&e.data)
                );
            }
        }
        Ok(out)
    }
}

/// What the command line asks for: `--test WxH`, `--dump FILE`.
#[derive(Default)]
struct Args {
    test: Option<(u32, u32)>,
    dump: Option<String>,
}

fn args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--test" => {
                let v = it.next().ok_or("--test needs WxH")?;
                let (w, h) = v.split_once('x').ok_or("--test needs WxH")?;
                a.test = Some((
                    w.parse().map_err(|_| "--test needs WxH")?,
                    h.parse().map_err(|_| "--test needs WxH")?,
                ));
            }
            "--dump" => a.dump = Some(it.next().ok_or("--dump needs a file")?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn main() -> Result<(), String> {
    let Args { test, dump } = args()?;
    let files = match &dump {
        Some(path) => Some((
            File::create(path).map_err(|e| e.to_string())?,
            File::create(format!("{path}.txt")).map_err(|e| e.to_string())?,
        )),
        None => None,
    };
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
            let settings = producer::Settings::default();
            // Off Windows there is no monitor: always a test screen.
            let test = test.or((!cfg!(windows)).then_some((640, 360)));
            if let Some((w, h)) = test {
                let mut screen = producer::TestScreen::new(w, h);
                screen.moving = moving;
                #[cfg(windows)]
                let make = vyro_core::screen::encode::Encoder::new;
                #[cfg(not(windows))]
                let (settings, make) = (
                    producer::Settings {
                        bitrate: 1_000_000,
                        ..settings
                    },
                    openh264_encoder::openh264,
                );
                let mut files = files;
                return producer::drive(
                    &session,
                    &settings,
                    &stop,
                    &status,
                    screen,
                    |w, h, fps, bitrate| {
                        make(w, h, fps, bitrate).map(|inner| Dump {
                            inner,
                            files: files.take(),
                        })
                    },
                );
            }
            #[cfg(windows)]
            {
                let cap = vyro_core::screen::capture::Capture::open(settings.monitor)
                    .map_err(|e| e.to_string())?;
                let mut files = files;
                producer::drive(
                    &session,
                    &settings,
                    &stop,
                    &status,
                    cap,
                    |w, h, fps, bitrate| {
                        vyro_core::screen::encode::Encoder::new(w, h, fps, bitrate).map(|inner| {
                            Dump {
                                inner,
                                files: files.take(),
                            }
                        })
                    },
                )
            }
            #[cfg(not(windows))]
            unreachable!("off Windows there is always a test screen")
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

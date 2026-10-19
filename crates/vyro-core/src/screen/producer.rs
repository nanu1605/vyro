//! A session's video (1.7): pictures of the monitor, scaled and encoded, at a steady rate. A
//! still screen is sent again once a second so the phone stays current; a keyframe goes out
//! whenever the phone asks. The secure desktop and screen changes are waited out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::capture::{Capture, CaptureError};
use super::encode::Encoder;
use super::frames;
use super::input::Rect;
use super::rtc::{Frame, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// 0: the primary monitor.
    pub monitor: usize,
    pub max_width: u32,
    pub max_height: u32,
    pub fps: u32,
    /// Bits a second.
    pub bitrate: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            monitor: 0,
            max_width: 1280,
            max_height: 720,
            fps: 30,
            bitrate: 2_500_000,
        }
    }
}

/// What the producer reports as it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Sending `width`×`height` of the monitor at `rect` on the desktop (input goes there).
    Sending { width: u32, height: u32, rect: Rect },
    /// Windows shows its secure desktop: nothing can be sent until it goes.
    Secure,
}

/// A still screen is sent again after this long.
const REPEAT: Duration = Duration::from_secs(1);

/// Sends the monitor to `session` until `stop` or the session ends.
pub fn run(
    session: &Session,
    s: &Settings,
    stop: &AtomicBool,
    status: &dyn Fn(Status),
) -> Result<(), String> {
    let mut cap = open(s.monitor, stop, status)?;
    let mut enc: Option<(Encoder, u32, u32)> = None;
    let interval = Duration::from_secs(1) / s.fps.max(1);
    let started = Instant::now();
    let mut last: Option<(Vec<u8>, Instant)> = None;
    let mut secure = false;
    while !stop.load(Ordering::SeqCst) && !session.closed() {
        let tick = Instant::now();
        let picture = match cap.next(interval.as_millis() as u32) {
            Ok(p) => {
                if secure && p.is_some() {
                    // The secure desktop went: a fresh encoder says so, with a keyframe.
                    secure = false;
                    enc = None;
                }
                p
            }
            Err(CaptureError::Lost) => {
                std::thread::sleep(Duration::from_millis(100));
                if let Err(e) = cap.reopen() {
                    if e == CaptureError::Secure && !secure {
                        secure = true;
                        status(Status::Secure);
                    }
                }
                continue;
            }
            Err(CaptureError::Secure) => {
                if !secure {
                    secure = true;
                    status(Status::Secure);
                }
                std::thread::sleep(Duration::from_millis(500));
                let _ = cap.reopen();
                continue;
            }
            Err(CaptureError::Other(e)) => return Err(e),
        };
        let keyframe = session.take_keyframe_request();
        // A new encoder starts with a keyframe.
        let mut made = false;
        let nv12 = match picture {
            Some(p) => {
                let (w, h) = frames::fit(p.width, p.height, s.max_width, s.max_height);
                if enc.as_ref().is_none_or(|(_, ew, eh)| (*ew, *eh) != (w, h)) {
                    enc = Some((Encoder::new(w, h, s.fps, s.bitrate)?, w, h));
                    made = true;
                    status(Status::Sending {
                        width: w,
                        height: h,
                        rect: cap.monitor.rect,
                    });
                }
                frames::bgra_to_nv12(&p.bgra, p.width, p.height, p.width as usize * 4, w, h)
            }
            // A still screen: the last picture again, now and then or when a keyframe is due.
            None => match &last {
                Some((nv12, at)) if keyframe || at.elapsed() >= REPEAT => nv12.clone(),
                _ => continue,
            },
        };
        let Some((encoder, _, _)) = enc.as_mut() else {
            continue;
        };
        let at = started.elapsed();
        let time_100ns = i64::try_from(at.as_nanos() / 100).unwrap_or(i64::MAX);
        let fresh = last.is_none() || keyframe || made;
        for data in encoder.encode(&nv12, time_100ns, fresh)? {
            session.send(Frame {
                data,
                rtp_90khz: (at.as_micros() * 9 / 100) as u64,
                captured: tick,
            });
        }
        last = Some((nv12, Instant::now()));
        if let Some(rest) = interval.checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Ok(())
}

/// Opens the monitor, waiting out the secure desktop.
fn open(monitor: usize, stop: &AtomicBool, status: &dyn Fn(Status)) -> Result<Capture, String> {
    let mut told = false;
    loop {
        match Capture::open(monitor) {
            Ok(c) => return Ok(c),
            Err(CaptureError::Secure | CaptureError::Lost) if !stop.load(Ordering::SeqCst) => {
                if !told {
                    told = true;
                    status(Status::Secure);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

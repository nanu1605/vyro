//! A session's video (1.7): pictures of the monitor, scaled and encoded, at a steady rate. After
//! a change (and a keyframe) the picture goes on being sent at that rate for a moment, so the
//! encoder gives out what it holds and sharpens it; a still screen is then sent again once a
//! second so the phone stays current. A keyframe goes out whenever the phone asks. The secure
//! desktop and screen changes are waited out.
//!
//! What goes out follows what the connection carries ([`Session::estimate`], [`Adapt`]): the
//! bit rate within a step, and on a slow connection fewer, smaller pictures that stay sharp
//! rather than large ones that blur and fall behind.
//!
//! The rules are the same whatever makes the pictures and the H.264 ([`Source`], [`Encode`]):
//! the monitor and Windows' encoder in the app ([`run`]), a test screen and another encoder in
//! the tests and the example PC.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
        // A 1080p monitor goes as it is (text stays sharp); larger ones are scaled to fit.
        Self {
            monitor: 0,
            max_width: 1920,
            max_height: 1080,
            fps: 30,
            bitrate: 5_000_000,
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
    /// What went out lately, every [`REPORT`].
    Report(Report),
}

/// What a session sent over the last few seconds: for the phone's details and the app's log,
/// so a picture that does not show can be traced to where it went wrong.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    /// How the monitor's pictures are taken ([`Source::method`]).
    pub capture: &'static str,
    /// The monitor's picture as taken.
    pub from: (u32, u32),
    /// The size sent.
    pub sent: (u32, u32),
    /// New pictures of the monitor a second.
    pub taken: u32,
    /// Pictures encoded a second (a still screen's repeats too).
    pub encoded: u32,
    /// Keyframes encoded in the period.
    pub keyframes: u32,
    /// Bits a second out of the encoder.
    pub bitrate: u32,
    /// Bits a second the connection carries, as estimated (0: not known yet).
    pub allowed: u32,
    /// The latest picture's brightness and how much it varies ([`frames::brightness`]).
    pub luma: u8,
    pub spread: u8,
    /// Keyframes the phone asked for since the session began.
    pub asked: u32,
}

/// How often the producer reports.
pub const REPORT: Duration = Duration::from_secs(2);

/// A BGRA picture, rows packed.
pub struct Picture {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Why a picture could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// Windows shows its secure desktop (sign-in, lock screen, an administrator prompt).
    Secure,
    /// The duplication ended (a mode change, a full-screen game, the secure desktop came and
    /// went): open it again.
    Lost,
    Other(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Secure => f.write_str("the PC is showing a secure screen"),
            Self::Lost => f.write_str("the screen changed"),
            Self::Other(m) => f.write_str(m),
        }
    }
}

/// Where the pictures come from.
pub trait Source {
    /// The next picture, waiting at most `timeout_ms`; `None` when the screen did not change.
    fn next(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError>;
    /// Starts again after [`CaptureError::Lost`] or [`CaptureError::Secure`].
    fn reopen(&mut self) -> Result<(), CaptureError>;
    /// Where the pictures are on the desktop (input goes there).
    fn rect(&self) -> Rect;
    /// How the pictures are taken, for the report.
    fn method(&self) -> &'static str {
        "a test screen"
    }
}

/// One encoded picture and the time of the picture it came from (100 ns units), which can be
/// an earlier one than the last given: an encoder may hold a few.
pub struct Encoded {
    pub data: Vec<u8>,
    pub time_100ns: i64,
}

/// What makes H.264 (Annex B; a keyframe with its SPS and PPS) of NV12 pictures.
pub trait Encode {
    /// Encodes one picture taken at `time_100ns`; returns the pictures that came out. With
    /// `keyframe`, the next one is a keyframe.
    fn encode(
        &mut self,
        nv12: &[u8],
        time_100ns: i64,
        keyframe: bool,
    ) -> Result<Vec<Encoded>, String>;

    /// Changes the bit rate while encoding; false when this encoder cannot (the new rate then
    /// waits for the next encoder).
    fn set_bitrate(&mut self, _bitrate: u32) -> bool {
        false
    }
}

/// The video's share of what the connection carries: the rest is the input channel, packet
/// headers, resent packets and the estimate's own probing.
const SHARE: f64 = 0.85;
/// The least bit rate asked of the encoder.
pub const LEAST: u32 = 300_000;
/// A larger step waits until the connection has carried enough for it this long; a smaller
/// one is taken at once.
const UP_AFTER: Duration = Duration::from_secs(5);
/// Within a step the bit rate follows at most this often, and only a change of a tenth or more.
const RETUNE: Duration = Duration::from_millis(500);
const RETUNE_BY: f64 = 0.1;

/// The largest size sent and the pictures a second, for a bit rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub max_width: u32,
    pub max_height: u32,
    pub fps: u32,
}

impl Step {
    fn rank(&self) -> u64 {
        u64::from(self.max_width) * u64::from(self.max_height) * u64::from(self.fps)
    }
}

/// The step for `bitrate`: everything from 2 Mbit/s, then 720p, then 720p at 15 a second, then
/// 540p at 10, never more than `s` asks for.
pub fn step(bitrate: u32, s: &Settings) -> Step {
    let (w, h, fps) = match bitrate {
        2_000_000.. => (s.max_width, s.max_height, s.fps),
        1_000_000.. => (1280, 720, s.fps),
        500_000.. => (1280, 720, 15),
        _ => (960, 540, 10),
    };
    Step {
        max_width: w.min(s.max_width),
        max_height: h.min(s.max_height),
        fps: fps.min(s.fps).max(1),
    }
}

/// Follows what the connection carries: the bit rate to encode at and the step.
#[derive(Debug, Clone)]
pub struct Adapt {
    pub bitrate: u32,
    pub step: Step,
    /// Since when a larger step has been allowed.
    up_since: Option<Instant>,
    retuned: Instant,
}

/// What [`Adapt::update`] changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change {
    pub bitrate: bool,
    pub step: bool,
}

impl Adapt {
    /// Everything `s` asks for, until the connection says otherwise.
    pub fn new(s: &Settings, now: Instant) -> Self {
        Self {
            bitrate: s.bitrate,
            step: step(s.bitrate, s),
            up_since: None,
            retuned: now,
        }
    }

    /// Takes the connection's latest estimate (`None`: none yet, so as `s` asks).
    pub fn update(&mut self, estimate: Option<u32>, s: &Settings, now: Instant) -> Change {
        let target = estimate.map_or(s.bitrate, |e| {
            ((f64::from(e) * SHARE) as u32).clamp(LEAST.min(s.bitrate), s.bitrate)
        });
        let mut change = Change::default();
        let want = step(target, s);
        if want.rank() < self.step.rank() {
            self.step = want;
            self.up_since = None;
            change.step = true;
        } else if want.rank() > self.step.rank() {
            let since = *self.up_since.get_or_insert(now);
            if now.duration_since(since) >= UP_AFTER {
                self.step = want;
                self.up_since = None;
                change.step = true;
            }
        } else {
            self.up_since = None;
        }
        let off =
            (f64::from(target) - f64::from(self.bitrate)).abs() / f64::from(self.bitrate.max(1));
        let due = off >= RETUNE_BY && now.duration_since(self.retuned) >= RETUNE;
        if target != self.bitrate && (change.step || due) {
            self.bitrate = target;
            self.retuned = now;
            change.bitrate = true;
        }
        change
    }
}

/// A still screen is sent again after this long.
const REPEAT: Duration = Duration::from_secs(1);
/// After a change or a keyframe, the picture goes on at full rate this long: the encoder holds a
/// few pictures before giving them out, and refines a still one with each it is given.
const SETTLE: Duration = Duration::from_secs(1);

/// Sends the monitor to `session` until `stop` or the session ends (Windows: Desktop
/// Duplication and Windows' own H.264 encoder).
#[cfg(windows)]
pub fn run(
    session: &Session,
    s: &Settings,
    stop: &AtomicBool,
    status: &dyn Fn(Status),
) -> Result<(), String> {
    use super::encode::Encoder;
    let cap = open(s.monitor, stop, status)?;
    drive(session, s, stop, status, cap, |w, h, fps, bitrate| {
        Encoder::new(w, h, fps, bitrate)
    })
}

#[cfg(windows)]
impl Source for super::capture::Capture {
    fn next(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError> {
        super::capture::Capture::next(self, timeout_ms)
    }
    fn reopen(&mut self) -> Result<(), CaptureError> {
        super::capture::Capture::reopen(self)
    }
    fn rect(&self) -> Rect {
        self.monitor.rect
    }
    fn method(&self) -> &'static str {
        super::capture::Capture::method(self)
    }
}

#[cfg(windows)]
impl Encode for super::encode::Encoder {
    fn encode(
        &mut self,
        nv12: &[u8],
        time_100ns: i64,
        keyframe: bool,
    ) -> Result<Vec<Encoded>, String> {
        super::encode::Encoder::encode(self, nv12, time_100ns, keyframe)
    }
    fn set_bitrate(&mut self, bitrate: u32) -> bool {
        super::encode::Encoder::set_bitrate(self, bitrate)
    }
}

/// Sends `cap`'s pictures to `session`, encoded by encoders `make` makes for a size
/// (`w`, `h`, `fps`, `bitrate`), until `stop` or the session ends.
pub fn drive<S: Source, E: Encode>(
    session: &Session,
    s: &Settings,
    stop: &AtomicBool,
    status: &dyn Fn(Status),
    mut cap: S,
    mut make: impl FnMut(u32, u32, u32, u32) -> Result<E, String>,
) -> Result<(), String> {
    // The encoder, with the size and pictures a second it was made for.
    let mut enc: Option<(E, u32, u32, u32)> = None;
    let mut adapt = Adapt::new(s, Instant::now());
    let started = Instant::now();
    let mut last: Option<(Vec<u8>, Instant)> = None;
    let mut settle_until = Instant::now();
    let mut secure = false;
    let mut report = Report::default();
    let mut counted = Instant::now();
    while !stop.load(Ordering::SeqCst) && !session.closed() {
        let tick = Instant::now();
        let change = adapt.update(session.estimate(), s, tick);
        if change.step {
            log::info!(
                "screen: the connection carries {} kbit/s: up to {}x{} at {} a second, {} kbit/s",
                session.estimate().unwrap_or(0) / 1000,
                adapt.step.max_width,
                adapt.step.max_height,
                adapt.step.fps,
                adapt.bitrate / 1000
            );
        }
        if change.bitrate {
            if let Some((e, ..)) = enc.as_mut() {
                // An encoder that cannot change its rate gets the new one when the next is made
                // (a new step, or the next picture of another size).
                e.set_bitrate(adapt.bitrate);
            }
        }
        let interval = Duration::from_secs(1) / adapt.step.fps;
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
                let step = adapt.step;
                let (w, h) = frames::fit(p.width, p.height, step.max_width, step.max_height);
                if enc
                    .as_ref()
                    .is_none_or(|(_, ew, eh, ef)| (*ew, *eh, *ef) != (w, h, step.fps))
                {
                    enc = Some((make(w, h, step.fps, adapt.bitrate)?, w, h, step.fps));
                    made = true;
                    status(Status::Sending {
                        width: w,
                        height: h,
                        rect: cap.rect(),
                    });
                }
                settle_until = Instant::now() + SETTLE;
                let nv12 =
                    frames::bgra_to_nv12(&p.bgra, p.width, p.height, p.width as usize * 4, w, h);
                report.from = (p.width, p.height);
                report.sent = (w, h);
                report.taken += 1;
                (report.luma, report.spread) = frames::brightness(&nv12, w, h);
                nv12
            }
            // A still screen: the last picture again, while it settles, now and then, or when a
            // keyframe is due.
            None => match &last {
                Some((nv12, at))
                    if keyframe || Instant::now() < settle_until || at.elapsed() >= REPEAT =>
                {
                    nv12.clone()
                }
                _ => continue,
            },
        };
        if keyframe {
            settle_until = Instant::now() + SETTLE;
        }
        let Some((encoder, ..)) = enc.as_mut() else {
            continue;
        };
        let at = started.elapsed();
        let time_100ns = i64::try_from(at.as_nanos() / 100).unwrap_or(i64::MAX);
        let fresh = last.is_none() || keyframe || made;
        report.encoded += 1;
        for e in encoder.encode(&nv12, time_100ns, fresh)? {
            report.bitrate = report
                .bitrate
                .saturating_add(u32::try_from(e.data.len() * 8).unwrap_or(u32::MAX));
            report.keyframes += u32::from(frames::is_keyframe(&e.data));
            // Each picture's own time: one the encoder held back keeps the time it was taken.
            let rtp_90khz = u64::try_from(e.time_100ns).unwrap_or(0) * 9 / 1000;
            let captured = tick
                .checked_sub(Duration::from_nanos(
                    u64::try_from(time_100ns - e.time_100ns).unwrap_or(0) * 100,
                ))
                .unwrap_or(tick);
            session.send(Frame {
                data: e.data,
                rtp_90khz,
                captured,
            });
        }
        last = Some((nv12, Instant::now()));
        let period = counted.elapsed();
        if period >= REPORT {
            let per_sec = |n: u32| (f64::from(n) / period.as_secs_f64()).round() as u32;
            status(Status::Report(Report {
                capture: cap.method(),
                taken: per_sec(report.taken),
                encoded: per_sec(report.encoded),
                bitrate: per_sec(report.bitrate),
                allowed: session.estimate().unwrap_or(0),
                asked: session.keyframes_asked(),
                ..report
            }));
            report = Report {
                capture: report.capture,
                from: report.from,
                sent: report.sent,
                luma: report.luma,
                spread: report.spread,
                ..Report::default()
            };
            counted = Instant::now();
        }
        if let Some(rest) = interval.checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Ok(())
}

/// Opens the monitor, waiting out the secure desktop.
#[cfg(windows)]
fn open(
    monitor: usize,
    stop: &AtomicBool,
    status: &dyn Fn(Status),
) -> Result<super::capture::Capture, String> {
    use super::capture::Capture;
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

/// A test screen: the moving test pattern ([`frames::test_pattern`]) while `moving` is set,
/// otherwise still. For the tests and the example PC, which have no monitor to send.
pub struct TestScreen {
    width: u32,
    height: u32,
    /// Change the picture on every look (true), or keep it still.
    pub moving: Arc<AtomicBool>,
    /// Pictures made so far (each one a step of the pattern).
    pub made: Arc<AtomicU32>,
    shown: bool,
}

impl TestScreen {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            moving: Arc::new(AtomicBool::new(true)),
            made: Arc::new(AtomicU32::new(0)),
            shown: false,
        }
    }
}

impl Source for TestScreen {
    fn next(&mut self, timeout_ms: u32) -> Result<Option<Picture>, CaptureError> {
        if self.shown && !self.moving.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(u64::from(timeout_ms)));
            return Ok(None);
        }
        self.shown = true;
        let n = self.made.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Picture {
            bgra: frames::test_pattern(self.width, self.height, n),
            width: self.width,
            height: self.height,
        }))
    }
    fn reopen(&mut self) -> Result<(), CaptureError> {
        Ok(())
    }
    fn rect(&self) -> Rect {
        Rect {
            left: 0,
            top: 0,
            width: self.width as i32,
            height: self.height as i32,
        }
    }
}

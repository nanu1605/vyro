//! A screen session's connection (1.7): the phone's offer comes through the relay, sealed; the PC
//! answers with its own addresses and the public one STUN reports, and from then on video goes
//! to the phone and input comes back directly, over WebRTC (str0m, no OpenSSL). The fingerprints
//! that secure it travel in the sealed offer and answer, so the relay cannot step in.

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use str0m::change::{SdpAnswer, SdpOffer};
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use super::frames;
use super::input;
use super::stun;

/// Frames waiting to be sent; more than this and the newest is dropped (the phone is behind).
const QUEUE: usize = 4;
/// Longest the loop sleeps between looks at the frame queue.
const TICK: Duration = Duration::from_millis(4);
/// A session that does not connect within this time ends.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// A connection that drops (the phone's Wi-Fi stalls, it changes networks) may come back; the
/// session ends when it has not within this time.
const RECONNECT: Duration = Duration::from_secs(10);

/// Where the session finds its addresses.
#[derive(Debug, Clone)]
pub struct Options {
    /// This PC's addresses on its networks (host candidates).
    pub hosts: Vec<IpAddr>,
    /// STUN servers ("host:port") that report the public address.
    pub stun: Vec<String>,
    pub stun_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            hosts: Vec::new(),
            stun: vec![
                "stun.cloudflare.com:3478".into(),
                "stun.l.google.com:19302".into(),
            ],
            stun_timeout: Duration::from_millis(800),
        }
    }
}

/// One encoded picture: H.264 in Annex B form (start codes), a keyframe with its SPS and PPS.
#[derive(Debug, Clone)]
pub struct Frame {
    pub data: Vec<u8>,
    /// Capture time, 90 kHz.
    pub rtp_90khz: u64,
    pub captured: Instant,
}

/// What a session tells the app.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    Connected,
    Input(input::Input),
    /// The session ended, and why.
    Closed(String),
}

pub type EventSink = Box<dyn Fn(SessionEvent) + Send>;

#[derive(Default)]
struct State {
    connected: bool,
    closed: bool,
    /// The newest note for the phone, and whether it still has to go.
    note: Option<(String, bool)>,
}

/// A running session. Dropping it ends it.
pub struct Session {
    frames: SyncSender<Frame>,
    keyframe: Arc<AtomicBool>,
    /// A frame was dropped: the phone cannot decode the ones after it until a keyframe.
    gap: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    thread: Option<JoinHandle<()>>,
}

impl Session {
    /// Queues a frame; false when the session has ended. A full queue drops the frame, and as
    /// each picture builds on the one before, nothing more goes until the next keyframe, which
    /// is asked for.
    pub fn send(&self, frame: Frame) -> bool {
        match self.frames.try_send(frame) {
            Ok(()) => !self.closed(),
            Err(TrySendError::Full(_)) => {
                self.gap.store(true, Ordering::SeqCst);
                self.keyframe.store(true, Ordering::SeqCst);
                !self.closed()
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Whether the phone asked for a keyframe (it lost one, or just connected); asking clears it.
    pub fn take_keyframe_request(&self) -> bool {
        self.keyframe.swap(false, Ordering::SeqCst)
    }

    pub fn connected(&self) -> bool {
        crate::lock(&self.state).connected
    }

    /// Tells the phone something over the data channel (the screen's state, as JSON). Only the
    /// newest note matters: one not yet sent is replaced, and a phone whose channel opens later
    /// gets it then.
    pub fn tell(&self, note: String) {
        crate::lock(&self.state).note = Some((note, true));
    }

    pub fn closed(&self) -> bool {
        crate::lock(&self.state).closed
    }

    /// Ends the session from any thread; it closes soon after.
    pub fn end(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The public address of `socket` from the first STUN server that answers.
fn public_address(socket: &UdpSocket, opts: &Options) -> Option<SocketAddr> {
    use std::net::ToSocketAddrs;
    let txid: [u8; 12] = std::array::from_fn(|_| fastrand::u8(..));
    let servers: Vec<SocketAddr> = opts
        .stun
        .iter()
        .filter_map(|s| s.to_socket_addrs().ok())
        .flatten()
        .filter(SocketAddr::is_ipv4)
        .collect();
    for s in &servers {
        let _ = socket.send_to(&stun::request(txid), s);
    }
    let until = Instant::now() + opts.stun_timeout;
    let mut buf = [0u8; 512];
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
    {
        let _ = socket.set_read_timeout(Some(left));
        match socket.recv_from(&mut buf) {
            Ok((n, from)) if servers.contains(&from) => {
                if let Some(a) = stun::mapped_address(&buf[..n], txid) {
                    return Some(a);
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    None
}

/// Answers the phone's offer and starts the session: the answer goes back to the phone.
/// One UDP socket on the PC's main address (the first of [`Options::hosts`]) carries everything:
/// the phone reaches it on the same network, or through the router at the public address STUN
/// reports.
pub fn start(
    offer_sdp: &str,
    opts: &Options,
    events: EventSink,
) -> Result<(Session, String), String> {
    let offer =
        SdpOffer::from_sdp_string(offer_sdp).map_err(|e| format!("the phone's offer: {e}"))?;
    let host = opts
        .hosts
        .iter()
        .find(|ip| ip.is_ipv4())
        .copied()
        .ok_or("this PC has no network address to offer")?;
    let socket =
        UdpSocket::bind(SocketAddr::new(host, 0)).map_err(|e| format!("a UDP port: {e}"))?;
    let local = socket.local_addr().map_err(|e| e.to_string())?;
    let mut rtc = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .build(Instant::now());
    let c = Candidate::host(local, "udp").map_err(|e| format!("this PC's address: {e}"))?;
    rtc.add_local_candidate(c);
    if !host.is_loopback() {
        if let Some(public) = public_address(&socket, opts).filter(|p| *p != local) {
            if let Ok(c) = Candidate::server_reflexive(public, local, "udp") {
                rtc.add_local_candidate(c);
            }
        }
    }
    let answer: SdpAnswer = rtc
        .sdp_api()
        .accept_offer(offer)
        .map_err(|e| format!("the phone's offer: {e}"))?;
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let keyframe = Arc::new(AtomicBool::new(true));
    let gap = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(State::default()));
    let thread = {
        let flags = Flags {
            keyframe: Arc::clone(&keyframe),
            gap: Arc::clone(&gap),
            stop: Arc::clone(&stop),
        };
        let state = Arc::clone(&state);
        std::thread::Builder::new()
            .name("vyro-screen-rtc".into())
            .spawn(move || {
                let why = run(rtc, &socket, &rx, &flags, &state, &events);
                crate::lock(&state).closed = true;
                events(SessionEvent::Closed(why));
            })
            .map_err(|e| e.to_string())?
    };
    Ok((
        Session {
            frames: tx,
            keyframe,
            gap,
            stop,
            state,
            thread: Some(thread),
        },
        answer.to_sdp_string(),
    ))
}

/// The H.264 payload type to send: packetization mode 1, constrained baseline preferred.
fn h264_pt(rtc: &mut Rtc, mid: Mid) -> Option<str0m::format::PayloadParams> {
    let w = rtc.writer(mid)?;
    let mut best: Option<(u8, str0m::format::PayloadParams)> = None;
    for p in w.payload_params() {
        let spec = p.spec();
        if spec.codec != Codec::H264 || spec.format.packetization_mode != Some(1) {
            continue;
        }
        let rank = match spec.format.profile_level_id.map(|v| v >> 8) {
            Some(0x42e0) => 3,
            Some(0x4200) => 2,
            _ => 1,
        };
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, *p));
        }
    }
    best.map(|(_, p)| p)
}

/// Writes the note still due to the phone's channel; whether there was one.
fn send_note(rtc: &mut Rtc, id: ChannelId, state: &Mutex<State>) -> bool {
    let note = match &mut crate::lock(state).note {
        Some((text, due)) if *due => {
            *due = false;
            text.clone()
        }
        _ => return false,
    };
    if let Some(mut ch) = rtc.channel(id) {
        if let Err(e) = ch.write(false, note.as_bytes()) {
            log::debug!("screen: note not sent: {e}");
        }
    }
    true
}

/// What the producer and the session's loop share.
struct Flags {
    /// The next picture should be a keyframe.
    keyframe: Arc<AtomicBool>,
    /// A frame was dropped before it was sent.
    gap: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

/// The session's loop; returns why it ended.
fn run(
    mut rtc: Rtc,
    socket: &UdpSocket,
    frames: &Receiver<Frame>,
    flags: &Flags,
    state: &Mutex<State>,
    events: &EventSink,
) -> String {
    let Flags {
        keyframe,
        gap,
        stop,
    } = flags;
    let started = Instant::now();
    let local = socket.local_addr().ok();
    let mut video: Option<(Mid, str0m::format::PayloadParams)> = None;
    let mut channel: Option<ChannelId> = None;
    // Until a keyframe goes, the phone can decode nothing (pictures build on the one before):
    // none is sent before it, after connecting and after a frame was lost.
    let mut need_key = true;
    let mut dropped_since: Option<Instant> = None;
    let mut buf = vec![0u8; 2000];
    loop {
        // Everything str0m has to say, until it waits for time or input.
        let deadline = loop {
            match rtc.poll_output() {
                Ok(Output::Timeout(t)) => break t,
                Ok(Output::Transmit(t)) => {
                    let _ = socket.send_to(&t.contents, t.destination);
                }
                Ok(Output::Event(e)) => match e {
                    // ICE and DTLS both done: media can flow.
                    Event::Connected => {
                        crate::lock(state).connected = true;
                        keyframe.store(true, Ordering::SeqCst);
                        events(SessionEvent::Connected);
                    }
                    Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                        dropped_since.get_or_insert_with(Instant::now);
                    }
                    Event::IceConnectionStateChange(_) => dropped_since = None,
                    Event::MediaAdded(m) if m.kind == MediaKind::Video => {
                        video = h264_pt(&mut rtc, m.mid).map(|p| (m.mid, p));
                        if video.is_none() {
                            return "the phone cannot play H.264 video".into();
                        }
                    }
                    Event::KeyframeRequest(_) => keyframe.store(true, Ordering::SeqCst),
                    Event::ChannelOpen(id, _) => {
                        channel = Some(id);
                        if let Some((_, due)) = &mut crate::lock(state).note {
                            *due = true;
                        }
                    }
                    Event::ChannelClose(id) if channel == Some(id) => channel = None,
                    Event::ChannelData(d) => {
                        let text = String::from_utf8_lossy(&d.data);
                        match input::parse(&text) {
                            Ok(input::Input::Picture) => {
                                log::info!("screen: the phone asked for a whole picture");
                                keyframe.store(true, Ordering::SeqCst);
                            }
                            Ok(i) => events(SessionEvent::Input(i)),
                            Err(e) => log::debug!("screen: input refused: {e}"),
                        }
                    }
                    _ => {}
                },
                Err(e) => return format!("the connection failed: {e}"),
            }
        };
        if stop.load(Ordering::SeqCst) {
            // A last note (why it ended) still goes, once.
            if let Some(id) = channel {
                if send_note(&mut rtc, id, state) {
                    while let Ok(Output::Transmit(t)) = rtc.poll_output() {
                        let _ = socket.send_to(&t.contents, t.destination);
                    }
                }
            }
            return "stopped".into();
        }
        if !crate::lock(state).connected && started.elapsed() > CONNECT_TIMEOUT {
            return "the phone could not reach the PC directly".into();
        }
        if dropped_since.is_some_and(|t| t.elapsed() > RECONNECT) {
            return "the phone went away".into();
        }
        if gap.swap(false, Ordering::SeqCst) {
            need_key = true;
        }
        // A note for the phone, once its channel is open.
        if channel.is_some_and(|id| send_note(&mut rtc, id, state)) {
            continue;
        }
        // A frame to send: one mutation, then back to draining the output.
        if let Ok(f) = frames.try_recv() {
            let connected = crate::lock(state).connected;
            if !connected {
                // Made before the phone was there: a keyframe follows once it is.
                need_key = true;
            } else if need_key && !frames::is_keyframe(&f.data) {
                keyframe.store(true, Ordering::SeqCst);
            } else if let Some((mid, p)) = &video {
                if let Some(w) = rtc.writer(*mid) {
                    let wallclock = f.captured;
                    match w.write(
                        p.pt(),
                        wallclock,
                        MediaTime::from_90khz(f.rtp_90khz),
                        f.data,
                    ) {
                        Ok(()) => need_key = false,
                        Err(e) => {
                            log::debug!("screen: frame not sent: {e}");
                            need_key = true;
                            keyframe.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
            continue;
        }
        let now = Instant::now();
        let wait = deadline.saturating_duration_since(now).min(TICK);
        if wait.is_zero() {
            if let Err(e) = rtc.handle_input(Input::Timeout(now)) {
                return format!("the connection failed: {e}");
            }
            continue;
        }
        let _ = socket.set_read_timeout(Some(wait));
        let input = match socket.recv_from(&mut buf) {
            Ok((n, source)) => match (local, buf[..n].try_into()) {
                (Some(destination), Ok(contents)) => Input::Receive(
                    Instant::now(),
                    Receive {
                        proto: Protocol::Udp,
                        source,
                        destination,
                        contents,
                    },
                ),
                _ => continue,
            },
            Err(_) => Input::Timeout(Instant::now()),
        };
        if let Err(e) = rtc.handle_input(input) {
            log::debug!("screen: {e}");
        }
    }
}

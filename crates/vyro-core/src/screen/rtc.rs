//! A screen session's connection (1.7): the phone's offer comes through the relay, sealed; the PC
//! answers with its own addresses and the public one STUN reports, and from then on video goes
//! to the phone and input comes back directly, over WebRTC (str0m, no OpenSSL). The fingerprints
//! that secure it travel in the sealed offer and answer, so the relay cannot step in.
//!
//! The phone reports how the video arrives (transport-wide congestion control); from that
//! str0m estimates what the connection carries, paces what goes out, and the producer encodes
//! to fit ([`Session::estimate`]).

use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use str0m::bwe::{Bitrate, BweKind};
use str0m::change::{SdpAnswer, SdpOffer};
use str0m::channel::ChannelId;
use str0m::format::Codec;
use str0m::media::{MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig};

use super::frames;
use super::input;
use super::producer::Quality;
use super::share::Note;
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
/// What the connection is thought to carry before the phone has said, bits a second.
const START_ESTIMATE: u64 = 3_000_000;
/// The most the estimate looks for: the producer's top rate with room for the rest.
const WANTED: u64 = 6_000_000;
/// The kinds of note for the phone ([`slot`]).
const NOTE_KINDS: usize = 4;

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
    /// A voice clip came on its own channel (1.7): a WAV ([`super::voice::read_wav`]), and
    /// where to say what became of it.
    Clip(Vec<u8>, ClipReply),
    /// A frame of the phone's live microphone (1.7; [`super::voice::read_frame`]).
    Mic(Vec<u8>),
    /// The phone turned its microphone off (its channel closed).
    MicEnd,
    /// The session ended, and why.
    Closed(String),
}

/// Tells the phone what became of a clip, on the clip's channel, from any thread.
#[derive(Clone)]
pub struct ClipReply {
    state: Arc<Mutex<State>>,
    channel: ChannelId,
}

impl ClipReply {
    /// `Ok(file name)` once it is on the clipboard, or why not.
    pub fn send(&self, kept: Result<&str, &str>) {
        let msg = match kept {
            Ok(name) => serde_json::json!({ "t": "saved", "name": name }),
            Err(why) => serde_json::json!({ "t": "error", "why": why }),
        };
        crate::lock(&self.state)
            .replies
            .push((self.channel, msg.to_string()));
    }
}

impl std::fmt::Debug for ClipReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClipReply")
    }
}

impl PartialEq for ClipReply {
    fn eq(&self, other: &Self) -> bool {
        self.channel == other.channel
    }
}

/// What each of the phone's data channels carries.
enum Channel {
    /// Input from the phone, notes to it ("input").
    Input,
    /// A voice clip ("clip").
    Clip(super::voice::ClipStream),
    /// The live microphone ("mic").
    Mic,
}

pub type EventSink = Box<dyn Fn(SessionEvent) + Send>;

/// Where a note waits for the phone: one of each kind, so a note never takes another kind's
/// place, and the end goes last.
fn slot(note: &Note) -> usize {
    match note {
        Note::Sending { .. } | Note::Secure => 0,
        Note::Mic { .. } => 1,
        Note::Report { .. } => 2,
        Note::End { .. } => 3,
    }
}

#[derive(Default)]
struct State {
    connected: bool,
    closed: bool,
    /// The newest note of each kind for the phone ([`slot`]), and whether it still has to go.
    notes: [Option<(String, bool)>; NOTE_KINDS],
    /// Answers to clips, for their channels.
    replies: Vec<(ChannelId, String)>,
}

/// A running session. Dropping it ends it.
pub struct Session {
    frames: SyncSender<Frame>,
    keyframe: Arc<AtomicBool>,
    /// Keyframes the phone asked for (it lost a picture, or could not decode one).
    asked: Arc<AtomicU32>,
    /// A frame was dropped: the phone cannot decode the ones after it until a keyframe.
    gap: Arc<AtomicBool>,
    /// What the connection carries, bits a second (0: not known yet).
    estimate: Arc<AtomicU32>,
    /// HD or SD, as the phone asked ([`Quality::to_u8`]).
    quality: Arc<AtomicU8>,
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

    /// HD or SD: what the phone asked for, at the start or since (1.7).
    pub fn quality(&self) -> Quality {
        Quality::from_u8(self.quality.load(Ordering::SeqCst))
    }

    pub fn set_quality(&self, q: Quality) {
        self.quality.store(q.to_u8(), Ordering::SeqCst);
    }

    /// How many keyframes the phone has asked for since the session began.
    pub fn keyframes_asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }

    pub fn connected(&self) -> bool {
        crate::lock(&self.state).connected
    }

    /// What the connection to the phone carries, bits a second, as estimated from the phone's
    /// reports; `None` until there is an estimate.
    pub fn estimate(&self) -> Option<u32> {
        Some(self.estimate.load(Ordering::SeqCst)).filter(|&b| b > 0)
    }

    /// Tells the phone something over the data channel (the screen's state). Only the newest
    /// note of each kind matters: one not yet sent is replaced by the next of its kind (the
    /// picture's shape, the microphone, what went out lately, the end), never by another kind,
    /// and a phone whose channel opens later gets them then.
    pub fn tell(&self, note: &Note) {
        crate::lock(&self.state).notes[slot(note)] = Some((note.json(), true));
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
        .enable_bwe(Some(Bitrate::bps(START_ESTIMATE)))
        .build(Instant::now());
    rtc.bwe().set_desired_bitrate(Bitrate::bps(WANTED));
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
    let asked = Arc::new(AtomicU32::new(0));
    let gap = Arc::new(AtomicBool::new(false));
    let estimate = Arc::new(AtomicU32::new(0));
    let quality = Arc::new(AtomicU8::new(Quality::default().to_u8()));
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(State::default()));
    let thread = {
        let flags = Flags {
            keyframe: Arc::clone(&keyframe),
            asked: Arc::clone(&asked),
            gap: Arc::clone(&gap),
            estimate: Arc::clone(&estimate),
            quality: Arc::clone(&quality),
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
            asked,
            gap,
            estimate,
            quality,
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

/// The notes still due to the phone, in their kinds' order; they are due no more.
fn take_due(state: &mut State) -> Vec<String> {
    state
        .notes
        .iter_mut()
        .flatten()
        .filter(|(_, due)| *due)
        .map(|(text, due)| {
            *due = false;
            text.clone()
        })
        .collect()
}

/// Writes the notes still due to the phone's channel; whether there were any.
fn send_notes(rtc: &mut Rtc, id: ChannelId, state: &Mutex<State>) -> bool {
    let notes = take_due(&mut crate::lock(state));
    if notes.is_empty() {
        return false;
    }
    if let Some(mut ch) = rtc.channel(id) {
        for note in &notes {
            if let Err(e) = ch.write(false, note.as_bytes()) {
                log::debug!("screen: note not sent: {e}");
            }
        }
    }
    true
}

/// What the producer and the session's loop share.
struct Flags {
    /// The next picture should be a keyframe.
    keyframe: Arc<AtomicBool>,
    /// Keyframes the phone asked for.
    asked: Arc<AtomicU32>,
    /// A frame was dropped before it was sent.
    gap: Arc<AtomicBool>,
    /// What the connection carries, bits a second.
    estimate: Arc<AtomicU32>,
    /// HD or SD.
    quality: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
}

/// The session's loop; returns why it ended.
fn run(
    mut rtc: Rtc,
    socket: &UdpSocket,
    frames: &Receiver<Frame>,
    flags: &Flags,
    state: &Arc<Mutex<State>>,
    events: &EventSink,
) -> String {
    let Flags {
        keyframe,
        asked,
        gap,
        estimate,
        quality,
        stop,
    } = flags;
    let started = Instant::now();
    let local = socket.local_addr().ok();
    let mut video: Option<(Mid, str0m::format::PayloadParams)> = None;
    let mut channel: Option<ChannelId> = None;
    let mut channels: std::collections::HashMap<ChannelId, Channel> = Default::default();
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
                    Event::KeyframeRequest(_) => {
                        asked.fetch_add(1, Ordering::SeqCst);
                        keyframe.store(true, Ordering::SeqCst);
                    }
                    Event::EgressBitrateEstimate(
                        BweKind::Twcc { estimate: b, .. } | BweKind::Remb { estimate: b, .. },
                    ) => {
                        let b = u32::try_from(b.as_u64()).unwrap_or(u32::MAX);
                        estimate.store(b.max(1), Ordering::SeqCst);
                    }
                    Event::ChannelOpen(id, label) if label == "clip" => {
                        channels.insert(id, Channel::Clip(Default::default()));
                    }
                    Event::ChannelOpen(id, label) if label == "mic" => {
                        channels.insert(id, Channel::Mic);
                    }
                    Event::ChannelOpen(id, _) => {
                        channel = Some(id);
                        channels.insert(id, Channel::Input);
                        for (_, due) in crate::lock(state).notes.iter_mut().flatten() {
                            *due = true;
                        }
                    }
                    Event::ChannelClose(id) => {
                        if let Some(Channel::Mic) = channels.remove(&id) {
                            events(SessionEvent::MicEnd);
                        }
                        if channel == Some(id) {
                            channel = None;
                        }
                    }
                    Event::ChannelData(d) if matches!(channels.get(&d.id), Some(Channel::Mic)) => {
                        events(SessionEvent::Mic(d.data));
                    }
                    Event::ChannelData(d)
                        if matches!(channels.get(&d.id), Some(Channel::Clip(_))) =>
                    {
                        let Some(Channel::Clip(clip)) = channels.get_mut(&d.id) else {
                            continue;
                        };
                        let reply = ClipReply {
                            state: Arc::clone(state),
                            channel: d.id,
                        };
                        let got = if d.binary {
                            clip.binary(&d.data).map(|()| None)
                        } else {
                            clip.text(&String::from_utf8_lossy(&d.data))
                        };
                        match got {
                            Ok(Some(bytes)) => events(SessionEvent::Clip(bytes, reply)),
                            Ok(None) => {}
                            Err(why) => {
                                log::info!("screen: a clip refused: {why}");
                                reply.send(Err(&why));
                            }
                        }
                    }
                    Event::ChannelData(d) => {
                        let text = String::from_utf8_lossy(&d.data);
                        match input::parse(&text) {
                            Ok(input::Input::Picture) => {
                                log::info!("screen: the phone asked for a whole picture");
                                asked.fetch_add(1, Ordering::SeqCst);
                                keyframe.store(true, Ordering::SeqCst);
                            }
                            Ok(input::Input::Quality { q }) => {
                                quality.store(q.to_u8(), Ordering::SeqCst);
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
            // The last notes (why it ended) still go, once.
            if let Some(id) = channel {
                if send_notes(&mut rtc, id, state) {
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
        // Answers to clips, on their channels.
        let replies = std::mem::take(&mut crate::lock(state).replies);
        for (id, text) in replies {
            if let Some(mut ch) = rtc.channel(id) {
                if let Err(e) = ch.write(false, text.as_bytes()) {
                    log::debug!("screen: a clip's answer not sent: {e}");
                }
            }
        }
        // Notes for the phone, once its channel is open.
        if channel.is_some_and(|id| send_notes(&mut rtc, id, state)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The microphone's note and the picture's shape told at once both go; a newer note of a
    /// kind replaces the one not yet sent, and the end goes last.
    #[test]
    fn a_note_replaces_only_its_own_kind() {
        let state = Mutex::new(State::default());
        let tell = |n: &Note| crate::lock(&state).notes[slot(n)] = Some((n.json(), true));
        let mic = Note::Mic {
            on: true,
            into: Some("CABLE Output".into()),
            default: true,
            why: None,
        };
        tell(&Note::End {
            why: "stopped".into(),
        });
        tell(&mic);
        tell(&Note::Sending { w: 1280, h: 720 });
        tell(&Note::Secure);
        assert_eq!(
            take_due(&mut crate::lock(&state)),
            [
                Note::Secure.json(),
                mic.json(),
                Note::End {
                    why: "stopped".into()
                }
                .json()
            ]
        );
        assert!(take_due(&mut crate::lock(&state)).is_empty(), "sent once");
    }
}

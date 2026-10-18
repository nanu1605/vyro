//! The relay connection (1.4): one thread that keeps a socket to the relay open, reconnecting
//! with growing pauses, and runs a [`Session`] over it. Certificates are checked by Windows'
//! own store (rustls with the platform verifier). Nothing listens on the PC.

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Connector, HandshakeError, Message, WebSocket};

use super::phones::Phones;
use super::proto::{self, b64url, SOCKET_PROTOCOL};
use super::session::{Effects, Now, Out, Session};
use super::RelayState;
use crate::remote::view::{self, FiredView};
use crate::remote::RemoteInfo;
use crate::units::TempUnit;
use crate::{lock, unix_now, SensorSnapshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// How often the thread looks for new readings to send while it waits for messages.
const TICK: Duration = Duration::from_millis(250);
/// A ping this often (the relay answers it without waking); silence this long means the
/// connection is gone (sleep, a network change).
const KEEPALIVE: Duration = Duration::from_secs(30);
const DEAD_AFTER: Duration = Duration::from_secs(75);
const PAUSE_MIN: Duration = Duration::from_secs(1);
const PAUSE_MAX: Duration = Duration::from_secs(60);
/// After the relay refused this PC's key, try again only this often.
const PAUSE_REFUSED: Duration = Duration::from_secs(300);
/// A connection that lasted this long resets the pause.
const STABLE: Duration = Duration::from_secs(60);
/// Largest message taken from the relay.
const MAX_MESSAGE: usize = 128 * 1024;
const STACK: usize = 256 * 1024;

/// What the client asks of the app.
pub struct RelayHooks {
    /// PC name, version, theme, accent and unit (the phone's fields are filled in here).
    pub info: Box<dyn Fn() -> RemoteInfo + Send + Sync>,
    /// The phones changed (paired, signed themselves out, seen): save them.
    pub saved: Box<dyn Fn() + Send + Sync>,
    /// The number of phones watching changed.
    pub watching: Box<dyn Fn(u32) + Send + Sync>,
    /// The connection's state changed.
    pub state: Box<dyn Fn(RelayState) + Send + Sync>,
}

#[derive(Default)]
struct Feed {
    seq: u64,
    json: Option<Arc<str>>,
    recent: VecDeque<FiredView>,
}

struct Shared {
    stop: AtomicBool,
    base: String,
    phones: Arc<Mutex<Phones>>,
    days: AtomicU32,
    feed: Mutex<Feed>,
    /// Wakes a pause between attempts: the client stops, or the phones changed.
    nudge: (Mutex<bool>, Condvar),
    status: Mutex<(RelayState, Option<String>)>,
    watching: Mutex<Vec<String>>,
    /// The phone list the relay last confirmed it has ([`Phones::generation`]).
    listed: (Mutex<u64>, Condvar),
    hooks: RelayHooks,
}

/// Connected (or trying to be) until [`RelayClient::stop`] or drop.
pub struct RelayClient {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

/// Why a connection ended.
enum Ended {
    Stopped,
    /// The relay refused this PC's key (another PC holds the room), or forgot the room.
    Refused,
    Lost(String),
}

impl RelayClient {
    /// Starts the thread. `base` is a checked relay address ([`proto::relay_base`]).
    pub fn start(
        base: String,
        phones: Arc<Mutex<Phones>>,
        days: u32,
        hooks: RelayHooks,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            base,
            phones,
            days: AtomicU32::new(days),
            feed: Mutex::new(Feed::default()),
            nudge: (Mutex::new(false), Condvar::new()),
            status: Mutex::new((RelayState::Connecting, None)),
            watching: Mutex::new(Vec::new()),
            listed: (Mutex::new(0), Condvar::new()),
            hooks,
        });
        let s = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("vyro-relay".into())
            .stack_size(STACK)
            .spawn(move || run(&s))
            .map_err(|e| format!("cannot start the relay connection: {e}"))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Feeds a reading to the phones watching.
    pub fn publish(&self, snap: &SensorSnapshot, unit: TempUnit) {
        let mut feed = lock(&self.shared.feed);
        view::note_fired(&mut feed.recent, snap, unit);
        match serde_json::to_string(&view::build(snap, &feed.recent, unit)) {
            Ok(json) => {
                feed.json = Some(json.into());
                feed.seq += 1;
            }
            Err(e) => log::warn!("relay: view: {e}"),
        }
    }

    /// The phones changed (a pairing opened, a phone was signed out): tell the relay now.
    pub fn nudge(&self) {
        nudge(&self.shared);
    }

    pub fn status(&self) -> (RelayState, Option<String>) {
        lock(&self.shared.status).clone()
    }

    /// Ids of the phones with the app open now.
    pub fn watching(&self) -> Vec<String> {
        lock(&self.shared.watching).clone()
    }

    /// Waits until the relay has the phone list of `generation` ([`Phones::generation`]), at
    /// most `timeout`: a pairing link shown before would be refused. `false` when it did not.
    pub fn wait_listed(&self, generation: u64, timeout: Duration) -> bool {
        let (m, c) = &self.shared.listed;
        let listed = lock(m);
        let (listed, _) = c
            .wait_timeout_while(listed, timeout, |l| *l < generation)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *listed >= generation
    }

    pub fn set_session_days(&self, days: u32) {
        self.shared.days.store(days, Ordering::SeqCst);
    }

    /// Closes the connection and ends the thread.
    pub fn stop(&mut self) {
        if self.shared.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        nudge(&self.shared);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        log::info!("relay: stopped");
    }
}

impl Drop for RelayClient {
    fn drop(&mut self) {
        self.stop();
    }
}

fn nudge(s: &Shared) {
    *lock(&s.nudge.0) = true;
    s.nudge.1.notify_all();
}

fn stopped(s: &Shared) -> bool {
    s.stop.load(Ordering::SeqCst)
}

fn set_state(s: &Shared, state: RelayState, error: Option<String>) {
    let changed = {
        let mut st = lock(&s.status);
        let changed = st.0 != state || st.1 != error;
        *st = (state, error);
        changed
    };
    if changed {
        (s.hooks.state)(state);
    }
}

fn set_watching(s: &Shared, ids: Vec<String>) {
    let n = u32::try_from(ids.len()).unwrap_or(u32::MAX);
    let changed = {
        let mut w = lock(&s.watching);
        let changed = *w != ids;
        *w = ids;
        changed
    };
    if changed {
        (s.hooks.watching)(n);
    }
}

/// Waits `d` (a little more or less, so many PCs do not retry in step), or until nudged.
fn pause(s: &Shared, d: Duration) {
    let jitter = 0.8 + fastrand::f64() * 0.4;
    let until = Instant::now() + d.mul_f64(jitter);
    let mut nudged = lock(&s.nudge.0);
    *nudged = false;
    while !*nudged && !stopped(s) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        nudged = s
            .nudge
            .1
            .wait_timeout(nudged, left)
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .0;
    }
}

fn run(s: &Shared) {
    let mut wait = PAUSE_MIN;
    while !stopped(s) {
        set_state(s, RelayState::Connecting, None);
        match connect(s) {
            Ok(mut ws) => {
                log::info!("relay: connected");
                set_state(s, RelayState::Online, None);
                let began = Instant::now();
                let ended = talk(s, &mut ws);
                set_watching(s, Vec::new());
                wait = if began.elapsed() >= STABLE {
                    PAUSE_MIN
                } else {
                    (wait * 2).min(PAUSE_MAX)
                };
                match ended {
                    Ended::Stopped => break,
                    Ended::Refused => {
                        log::warn!("relay: this PC's key was refused");
                        set_state(
                            s,
                            RelayState::Unreachable,
                            Some(
                                "The relay refused this PC. Start over in Settings → Remote."
                                    .into(),
                            ),
                        );
                        wait = PAUSE_REFUSED;
                    }
                    Ended::Lost(why) => {
                        log::info!("relay: connection lost: {why}");
                        set_state(s, RelayState::Connecting, None);
                    }
                }
            }
            Err(e) => {
                log::info!("relay: {e}");
                set_state(s, RelayState::Unreachable, Some(e));
                wait = (wait * 2).min(PAUSE_MAX);
            }
        }
        pause(s, wait);
    }
    set_watching(s, Vec::new());
    set_state(s, RelayState::Off, None);
}

fn tls() -> Result<Arc<rustls::ClientConfig>, String> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(BuilderVerifierExt::with_platform_verifier)
        .map_err(|e| format!("cannot set up a secure connection: {e}"))?
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn connect(s: &Shared) -> Result<Ws, String> {
    let (room, key) = {
        let p = lock(&s.phones);
        (*p.room(), *p.pc_key())
    };
    let url = proto::pc_socket(&s.base, &room);
    let mut req = url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("bad relay address: {e}"))?;
    let offer = HeaderValue::from_str(&format!("{SOCKET_PROTOCOL}, k.{}", b64url(&key)))
        .map_err(|e| e.to_string())?;
    req.headers_mut().insert("Sec-WebSocket-Protocol", offer);
    let uri = req.uri().clone();
    let host = uri.host().unwrap_or_default();
    let secure = uri.scheme_str() == Some("wss");
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let addrs = (bare, port)
        .to_socket_addrs()
        .map_err(|e| format!("Can't find the relay ({host}): {e}"))?;
    let mut last = None;
    let mut tcp = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
            Ok(t) => {
                tcp = Some(t);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let tcp = tcp.ok_or_else(|| {
        format!(
            "Can't reach the relay ({host}): {}",
            last.map_or_else(|| "no address".to_owned(), |e| e.to_string())
        )
    })?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(CONNECT_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(WRITE_TIMEOUT));
    let connector = if secure {
        Connector::Rustls(tls()?)
    } else {
        Connector::Plain
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let (ws, _) = tungstenite::client_tls_with_config(req, tcp, Some(config), Some(connector))
        .map_err(|e| match e {
            HandshakeError::Failure(tungstenite::Error::Http(r)) => {
                format!("The relay answered {}", r.status())
            }
            HandshakeError::Failure(tungstenite::Error::Tls(e)) => {
                format!("The relay's secure connection failed: {e}")
            }
            HandshakeError::Failure(e) => format!("Can't reach the relay: {e}"),
            HandshakeError::Interrupted(_) => "The relay did not answer in time".into(),
        })?;
    let inner = match ws.get_ref() {
        MaybeTlsStream::Plain(t) => Some(t),
        MaybeTlsStream::Rustls(t) => Some(t.get_ref()),
        _ => None,
    };
    if let Some(t) = inner {
        let _ = t.set_read_timeout(Some(TICK));
    }
    Ok(ws)
}

fn send(ws: &mut Ws, outs: Vec<Out>) -> Result<(), String> {
    for o in outs {
        let msg = match o {
            Out::Text(t) => Message::Text(t.into()),
            Out::Binary(b) => Message::Binary(b.into()),
        };
        ws.send(msg).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn apply(s: &Shared, session: &Session, fx: Effects) {
    if fx.saved {
        (s.hooks.saved)();
    }
    if fx.watching {
        set_watching(s, session.watching());
    }
}

/// One connection, until it ends.
fn talk(s: &Shared, ws: &mut Ws) -> Ended {
    let mut session = Session::new();
    let mut heard = Instant::now();
    let mut pinged = Instant::now();
    loop {
        if stopped(s) {
            let _ = ws.close(None);
            let _ = ws.flush();
            return Ended::Stopped;
        }
        let (seq, json) = {
            let f = lock(&s.feed);
            (f.seq, f.json.clone())
        };
        let (outs, gone) = {
            let p = lock(&s.phones);
            let gone = session.forget_gone(&p);
            let mut outs: Vec<Out> = session.sync(&p).into_iter().collect();
            if let Some(json) = &json {
                outs.extend(session.views(&p, seq, json));
            }
            (outs, gone)
        };
        if gone {
            set_watching(s, session.watching());
        }
        if let Err(e) = send(ws, outs) {
            return Ended::Lost(e);
        }
        if pinged.elapsed() >= KEEPALIVE {
            if let Err(e) = ws.send(Message::Ping(Default::default())) {
                return Ended::Lost(e.to_string());
            }
            pinged = Instant::now();
        }
        if heard.elapsed() >= DEAD_AFTER {
            return Ended::Lost("no answer from the relay".into());
        }
        match ws.read() {
            Ok(msg) => {
                heard = Instant::now();
                match msg {
                    Message::Text(t) => {
                        let fx = session.on_text(t.as_str());
                        if let Some(g) = session.listed() {
                            let (m, c) = &s.listed;
                            let mut listed = lock(m);
                            if g > *listed {
                                *listed = g;
                                c.notify_all();
                            }
                        }
                        apply(s, &session, fx);
                    }
                    Message::Binary(b) => {
                        let info = || (s.hooks.info)();
                        let now = Now {
                            at: unix_now(),
                            days: s.days.load(Ordering::SeqCst),
                            info: &info,
                            view: json.as_deref(),
                        };
                        let (outs, fx) = {
                            let mut p = lock(&s.phones);
                            session.on_frame(&mut p, &b, &now)
                        };
                        // Saved and counted before the phone hears back: a phone that has its
                        // first view is already counted as watching.
                        apply(s, &session, fx);
                        if let Err(e) = send(ws, outs) {
                            return Ended::Lost(e);
                        }
                    }
                    Message::Close(frame) => {
                        let code = frame.map(|f| u16::from(f.code));
                        return if matches!(code, Some(4403 | 4410)) {
                            Ended::Refused
                        } else {
                            Ended::Lost(format!("closed by the relay ({code:?})"))
                        };
                    }
                    _ => {}
                }
            }
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(e) => return Ended::Lost(e.to_string()),
        }
    }
}

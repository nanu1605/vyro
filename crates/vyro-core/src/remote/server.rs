//! The companion's HTTP server: one accept thread and a short-lived thread per connection.
//!
//! | Route              | Who            | What                                          |
//! |--------------------|----------------|-----------------------------------------------|
//! | `GET /`            | anyone allowed | the page (`remote.html`)                      |
//! | `GET /assets/…`    | anyone allowed | the page's scripts and styles                 |
//! | `POST /api/pair`   | anyone allowed | `{code, name}` → a session cookie             |
//! | `GET /api/info`    | paired         | PC name, theme, unit, this device             |
//! | `GET /api/view`    | paired         | the latest [`super::RemoteView`]              |
//! | `GET /api/stream`  | paired         | the view as server-sent events, every reading |
//! | `POST /api/logout` | paired         | signs this device out                         |
//!
//! "Anyone allowed" is a peer on the local network ([`net::peer_allowed`]) naming this server
//! in its Host header ([`net::host_allowed`]). Nothing here changes the PC.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::auth::{Auth, StoredDevice};
use super::http::{self, Method, ReadError, Request, Response};
use super::net;
use super::view::{self, FiredView};
use super::{random, RemoteInfo};
use crate::units::TempUnit;
use crate::{lock, unix_now, SensorSnapshot};

/// Connections at once, from one address, and live views. A browser opens up to six at once
/// for a page's files (each answer closes its connection), plus the live view.
pub const MAX_CONNS: usize = 32;
pub const MAX_PER_PEER: usize = 8;
pub const MAX_LIVE: usize = 6;
/// Time to send a whole request.
pub const REQUEST_BUDGET: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// A comment line keeps a quiet live view open through proxies and sleepy phones.
const HEARTBEAT: Duration = Duration::from_secs(15);
const COOKIE: &str = "vyro";
const STACK: usize = 256 * 1024;

/// The page's content policy: its own files only, no frames, no other origins.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
img-src 'self' data:; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; \
base-uri 'none'; form-action 'none'";

/// A file of the page.
pub type Asset = Vec<u8>;
/// Looks a file of the page up by path.
pub type AssetFn = Box<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

/// What the server asks of the app.
pub struct Hooks {
    /// The page's files by path: `remote.html`, `assets/remote-1a2b3c.js`.
    pub asset: AssetFn,
    /// PC name, version, theme, accent and unit (the device fields are filled in here).
    pub info: Box<dyn Fn() -> RemoteInfo + Send + Sync>,
    /// The paired devices changed (paired, signed out, seen): save them.
    pub saved: Box<dyn Fn() + Send + Sync>,
    /// The number of live views changed.
    pub watching: Box<dyn Fn(u32) + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// `0`: any free port (tests).
    pub port: u16,
    pub bind: Ipv4Addr,
    /// This PC's names, accepted in the Host header besides addresses and `localhost`.
    pub names: Vec<String>,
    pub session_days: u32,
}

struct Conn {
    stream: TcpStream,
    peer: IpAddr,
    device: Option<String>,
    live: bool,
}

#[derive(Default)]
struct Feed {
    seq: u64,
    json: Option<Arc<str>>,
    recent: VecDeque<FiredView>,
    /// Bumped when devices are signed out, so live views check at once.
    kicks: u64,
}

struct Shared {
    stop: AtomicBool,
    auth: Arc<Mutex<Auth>>,
    names: Vec<String>,
    session_days: AtomicU32,
    hooks: Hooks,
    feed: Mutex<Feed>,
    fed: Condvar,
    conns: Mutex<(u64, HashMap<u64, Conn>)>,
}

/// Listening until [`RemoteServer::stop`] or drop.
pub struct RemoteServer {
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
    addr: SocketAddr,
    /// Where `stop` connects to wake the accept thread.
    wake: SocketAddr,
}

impl RemoteServer {
    /// Binds the port and starts accepting. Fails when the port is taken.
    pub fn start(
        config: ServerConfig,
        auth: Arc<Mutex<Auth>>,
        hooks: Hooks,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind((config.bind, config.port)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                format!("port {} is in use by another program", config.port)
            } else {
                format!("cannot listen on port {}: {e}", config.port)
            }
        })?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("cannot listen: {e}"))?;
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            auth,
            names: config.names,
            session_days: AtomicU32::new(config.session_days),
            hooks,
            feed: Mutex::new(Feed::default()),
            fed: Condvar::new(),
            conns: Mutex::new((0, HashMap::new())),
        });
        let wake_ip = if config.bind.is_unspecified() {
            Ipv4Addr::LOCALHOST
        } else {
            config.bind
        };
        let s = Arc::clone(&shared);
        let accept = std::thread::Builder::new()
            .name("vyro-remote".into())
            .stack_size(STACK)
            .spawn(move || accept_loop(&s, &listener))
            .map_err(|e| format!("cannot start the server: {e}"))?;
        log::info!("remote: listening on {addr}");
        Ok(Self {
            shared,
            accept: Some(accept),
            addr,
            wake: SocketAddr::new(wake_ip.into(), addr.port()),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Feeds a reading to the live views.
    pub fn publish(&self, snap: &SensorSnapshot, unit: TempUnit) {
        let mut feed = lock(&self.shared.feed);
        view::note_fired(&mut feed.recent, snap, unit);
        let v = view::build(snap, &feed.recent, unit);
        match serde_json::to_string(&v) {
            Ok(json) => {
                feed.json = Some(json.into());
                feed.seq += 1;
            }
            Err(e) => log::warn!("remote: view: {e}"),
        }
        drop(feed);
        self.shared.fed.notify_all();
    }

    pub fn set_session_days(&self, days: u32) {
        self.shared.session_days.store(days, Ordering::SeqCst);
    }

    /// Ids of the devices with a live view open.
    pub fn watching(&self) -> Vec<String> {
        let conns = lock(&self.shared.conns);
        let mut ids: Vec<String> = conns
            .1
            .values()
            .filter(|c| c.live)
            .filter_map(|c| c.device.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Closes the connections of a device that was signed out (`None`: every device's), at
    /// once: its live view ends within a second.
    pub fn close_device(&self, id: Option<&str>) {
        close_matching(&self.shared, |c| {
            c.device
                .as_deref()
                .is_some_and(|d| id.is_none_or(|id| id == d))
        });
        lock(&self.shared.feed).kicks += 1;
        self.shared.fed.notify_all();
    }

    /// Stops listening and closes every connection.
    pub fn stop(&mut self) {
        if self.shared.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        self.shared.fed.notify_all();
        // The accept thread wakes on the next connection; make one.
        let _ = TcpStream::connect_timeout(&self.wake, Duration::from_secs(1));
        close_matching(&self.shared, |_| true);
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
        log::info!("remote: stopped");
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn close_matching(shared: &Shared, pick: impl Fn(&Conn) -> bool) {
    for c in lock(&shared.conns).1.values().filter(|c| pick(c)) {
        let _ = c.stream.shutdown(Shutdown::Both);
    }
}

fn accept_loop(shared: &Arc<Shared>, listener: &TcpListener) {
    for incoming in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let stream = match incoming {
            Ok(s) => s,
            Err(e) => {
                log::debug!("remote: accept: {e}");
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        let Ok(peer) = stream.peer_addr() else {
            continue;
        };
        if !net::peer_allowed(peer.ip()) {
            log::warn!("remote: refused a connection from outside the local network");
            continue;
        }
        let Some(id) = register(shared, &stream, peer.ip()) else {
            continue;
        };
        let s = Arc::clone(shared);
        let spawned = std::thread::Builder::new()
            .name("vyro-remote-conn".into())
            .stack_size(STACK)
            .spawn(move || {
                handle(&s, stream, id, peer.ip());
                unregister(&s, id);
            });
        if spawned.is_err() {
            unregister(shared, id);
        }
    }
}

fn register(shared: &Shared, stream: &TcpStream, peer: IpAddr) -> Option<u64> {
    let mut conns = lock(&shared.conns);
    let from_peer = conns.1.values().filter(|c| c.peer == peer).count();
    if conns.1.len() >= MAX_CONNS || from_peer >= MAX_PER_PEER {
        return None;
    }
    let clone = stream.try_clone().ok()?;
    conns.0 += 1;
    let id = conns.0;
    conns.1.insert(
        id,
        Conn {
            stream: clone,
            peer,
            device: None,
            live: false,
        },
    );
    Some(id)
}

fn unregister(shared: &Shared, id: u64) {
    let live = {
        let mut conns = lock(&shared.conns);
        let was = conns.1.remove(&id).is_some_and(|c| c.live);
        was.then(|| live_count(&conns.1))
    };
    if let Some(n) = live {
        (shared.hooks.watching)(n);
    }
}

fn live_count(conns: &HashMap<u64, Conn>) -> u32 {
    u32::try_from(conns.values().filter(|c| c.live).count()).unwrap_or(u32::MAX)
}

fn handle(shared: &Shared, mut stream: TcpStream, id: u64, peer: IpAddr) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let req = match http::read_request(&mut stream, REQUEST_BUDGET) {
        Ok(r) => r,
        Err(ReadError::Gone) => return,
        Err(ReadError::Status(code)) => {
            let _ = http::write_response(&mut stream, &Response::error(code, "Bad request"), false);
            return;
        }
    };
    let head_only = req.method == Method::Head;
    let response = match route(shared, &req, id, peer) {
        Route::Answer(r) => r,
        Route::Stream(device) => {
            stream_view(shared, &mut stream, id, &device);
            return;
        }
    };
    let _ = http::write_response(&mut stream, &response, head_only);
}

enum Route {
    Answer(Response),
    Stream(StoredDevice),
}

fn route(shared: &Shared, req: &Request, conn: u64, peer: IpAddr) -> Route {
    let host = req.header("host").unwrap_or("");
    if !net::host_allowed(host, &shared.names) {
        return Route::Answer(Response::error(421, "Open this page by the PC's address."));
    }
    let read = matches!(req.method, Method::Get | Method::Head);
    let path = req.path.as_str();
    Route::Answer(match path {
        "/" | "/remote.html" if read => page(shared),
        p if p.starts_with("/assets/") && read => asset(shared, &p[1..]),
        "/api/pair" if req.method == Method::Post => pair(shared, req, host, peer),
        "/api/info" | "/api/view" | "/api/stream" | "/api/logout" => {
            let Some(device) = session(shared, req, peer) else {
                return Route::Answer(
                    Response::error(401, "This device is not paired.")
                        .with("Cache-Control", "no-store"),
                );
            };
            lock(&shared.conns)
                .1
                .entry(conn)
                .and_modify(|c| c.device = Some(device.id.clone()));
            match (path, req.method) {
                ("/api/info", Method::Get) => {
                    let mut info = (shared.hooks.info)();
                    info.device = device.name;
                    info.expires = device.expires;
                    Response::json(200, &info).with("Cache-Control", "no-store")
                }
                ("/api/view", Method::Get) => match lock(&shared.feed).json.clone() {
                    Some(json) => {
                        Response::new(200, "application/json; charset=utf-8", json.as_bytes())
                            .with("Cache-Control", "no-store")
                    }
                    None => Response::error(503, "No readings yet."),
                },
                ("/api/stream", Method::Get) => return Route::Stream(device),
                ("/api/logout", Method::Post) => logout(shared, req, host, &device),
                _ => Response::error(405, "Not allowed"),
            }
        }
        "/" | "/remote.html" | "/api/pair" => Response::error(405, "Not allowed"),
        _ if path.starts_with("/assets/") => Response::error(405, "Not allowed"),
        _ => Response::error(404, "Not found"),
    })
}

fn page(shared: &Shared) -> Response {
    match (shared.hooks.asset)("remote.html") {
        Some(bytes) => Response::new(200, "text/html; charset=utf-8", bytes)
            .with("Content-Security-Policy", CSP)
            .with("Cache-Control", "no-cache"),
        None => Response::new(
            503,
            "text/plain; charset=utf-8",
            "The companion page is not part of this build of Vyro.",
        ),
    }
}

/// `assets/<name>.<ext>`: a plain file name with a known type, nothing else.
fn asset(shared: &Shared, path: &str) -> Response {
    let name = &path["assets/".len()..];
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let mime = match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "",
    };
    if !plain || mime.is_empty() {
        return Response::error(404, "Not found");
    }
    match (shared.hooks.asset)(path) {
        Some(bytes) => Response::new(200, mime, bytes)
            .with("Cache-Control", "public, max-age=31536000, immutable"),
        None => Response::error(404, "Not found"),
    }
}

fn session(shared: &Shared, req: &Request, peer: IpAddr) -> Option<StoredDevice> {
    let token = req.cookie(COOKIE)?;
    let (device, dirty) = {
        let mut auth = lock(&shared.auth);
        let d = auth.check(unix_now(), token, &peer.to_string());
        (d, auth.take_dirty())
    };
    if dirty {
        (shared.hooks.saved)();
    }
    device
}

/// A sign-in or sign-out must come from the page: JSON, from this origin.
fn from_page(req: &Request, host: &str) -> Option<Response> {
    if !net::origin_allowed(req.header("origin"), host) {
        return Some(Response::error(403, "Not from this page."));
    }
    let json = req.header("content-type").is_some_and(|t| {
        t.trim_start()
            .to_ascii_lowercase()
            .starts_with("application/json")
    });
    (!json).then(|| Response::error(415, "Expected JSON."))
}

#[derive(serde::Deserialize)]
struct PairBody {
    code: String,
    #[serde(default)]
    name: String,
}

fn pair(shared: &Shared, req: &Request, host: &str, peer: IpAddr) -> Response {
    if let Some(refused) = from_page(req, host) {
        return refused;
    }
    let Ok(body) = serde_json::from_slice::<PairBody>(&req.body) else {
        return Response::error(400, "Bad request");
    };
    let (Some(token), Some(id)) = (random::<32>(), random::<4>()) else {
        return Response::error(503, "No randomness available.");
    };
    let days = shared.session_days.load(Ordering::SeqCst);
    let result = lock(&shared.auth).pair(
        unix_now(),
        &body.code,
        &body.name,
        &peer.to_string(),
        days,
        token,
        id,
    );
    match result {
        Ok((token, device)) => {
            (shared.hooks.saved)();
            log::info!("remote: paired \"{}\" from {peer}", device.name);
            let age = (device.expires - device.paired).max(0);
            Response::json(
                200,
                &serde_json::json!({ "device": device.name, "expires": device.expires }),
            )
            .with(
                "Set-Cookie",
                format!("{COOKIE}={token}; Path=/; Max-Age={age}; HttpOnly; SameSite=Strict"),
            )
            .with("Cache-Control", "no-store")
        }
        Err(e) => {
            log::info!("remote: pairing from {peer} refused: {e:?}");
            let status = if matches!(e, super::PairError::Full) {
                409
            } else {
                403
            };
            Response::error(status, &e.to_string())
        }
    }
}

fn logout(shared: &Shared, req: &Request, host: &str, device: &StoredDevice) -> Response {
    if let Some(refused) = from_page(req, host) {
        return refused;
    }
    let gone = lock(&shared.auth).sign_out(&device.id);
    if gone {
        (shared.hooks.saved)();
        log::info!("remote: \"{}\" signed itself out", device.name);
    }
    Response::new(204, "text/plain", Vec::new()).with(
        "Set-Cookie",
        format!("{COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"),
    )
}

fn stream_view(shared: &Shared, stream: &mut TcpStream, conn: u64, device: &StoredDevice) {
    let live = {
        let mut conns = lock(&shared.conns);
        let open = conns.1.values().filter(|c| c.live).count();
        if open >= MAX_LIVE {
            None
        } else {
            if let Some(c) = conns.1.get_mut(&conn) {
                c.live = true;
            }
            Some(live_count(&conns.1))
        }
    };
    let Some(n) = live else {
        let busy = Response::error(429, "Too many live views are open.").with("Retry-After", "10");
        let _ = http::write_response(stream, &busy, false);
        return;
    };
    (shared.hooks.watching)(n);
    let mut head = String::from("HTTP/1.1 200 OK\r\n");
    for (k, v) in http::SAFE_HEADERS {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(
        "Content-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nretry: 3000\n\n",
    );
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut seen = 0u64;
    let mut kicks = lock(&shared.feed).kicks;
    loop {
        let next = {
            let feed = lock(&shared.feed);
            let (feed, _) = shared
                .fed
                .wait_timeout_while(feed, HEARTBEAT, |f| {
                    f.seq == seen && f.kicks == kicks && !shared.stop.load(Ordering::SeqCst)
                })
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            kicks = feed.kicks;
            (feed.seq != seen).then(|| (feed.seq, feed.json.clone()))
        };
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let still = {
            let auth = lock(&shared.auth);
            auth.is_paired(&device.id)
        };
        if !still || device.expires <= unix_now() {
            break;
        }
        let msg = match next {
            Some((seq, Some(json))) => {
                seen = seq;
                format!("data: {json}\n\n")
            }
            Some((seq, None)) => {
                seen = seq;
                continue;
            }
            None => ": still here\n\n".to_owned(),
        };
        if stream
            .write_all(msg.as_bytes())
            .and_then(|()| stream.flush())
            .is_err()
        {
            break;
        }
    }
}

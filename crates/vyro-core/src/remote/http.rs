//! One HTTP/1.1 request per connection, parsed by `httparse` within tight limits, and the
//! response written back with `Connection: close`. Only what the companion needs: GET and POST,
//! a small JSON body, no chunked uploads, no keep-alive.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

/// Request line and headers together.
pub const MAX_HEAD: usize = 8 * 1024;
pub const MAX_HEADERS: usize = 32;
/// The only bodies are a pairing request and a sign-out.
pub const MAX_BODY: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    /// Without the query.
    pub path: String,
    /// Lower-case names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// A cookie's value.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n == "cookie")
            .flat_map(|(_, v)| v.split(';'))
            .find_map(|kv| {
                let (k, v) = kv.trim().split_once('=')?;
                (k == name).then_some(v)
            })
    }
}

#[derive(Debug)]
pub enum ReadError {
    /// Closed or timed out before a whole request came.
    Gone,
    /// 400, 413, 431 or 501: answered, then the connection closes.
    Status(u16),
}

impl From<io::Error> for ReadError {
    fn from(_: io::Error) -> Self {
        Self::Gone
    }
}

/// Reads one request, giving up after `budget` in total (a slow sender cannot hold the
/// connection by trickling bytes).
pub fn read_request(stream: &mut impl ReadTimed, budget: Duration) -> Result<Request, ReadError> {
    let deadline = Instant::now() + budget;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let (head_len, method, path, headers) = loop {
        let n = stream.read_until(&mut chunk, deadline)?;
        if n == 0 {
            return Err(ReadError::Gone);
        }
        buf.extend_from_slice(&chunk[..n]);
        let mut slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Request::new(&mut slots);
        match req.parse(&buf) {
            Ok(httparse::Status::Complete(len)) => {
                let method = match req.method {
                    Some("GET") => Method::Get,
                    Some("HEAD") => Method::Head,
                    Some("POST") => Method::Post,
                    _ => Method::Other,
                };
                let path = req.path.unwrap_or("/");
                let path = path.split_once('?').map_or(path, |(p, _)| p).to_owned();
                let headers = req
                    .headers
                    .iter()
                    .map(|h| {
                        (
                            h.name.to_ascii_lowercase(),
                            String::from_utf8_lossy(h.value).into_owned(),
                        )
                    })
                    .collect::<Vec<_>>();
                break (len, method, path, headers);
            }
            Ok(httparse::Status::Partial) if buf.len() < MAX_HEAD => {}
            Ok(httparse::Status::Partial) => return Err(ReadError::Status(431)),
            Err(httparse::Error::TooManyHeaders) => return Err(ReadError::Status(431)),
            Err(_) => return Err(ReadError::Status(400)),
        }
    };
    let find = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v);
    if find("transfer-encoding").is_some() {
        return Err(ReadError::Status(501));
    }
    let length = match find("content-length") {
        None => 0,
        Some(v) => v
            .trim()
            .parse::<usize>()
            .map_err(|_| ReadError::Status(400))?,
    };
    if length > MAX_BODY {
        return Err(ReadError::Status(413));
    }
    let mut body = buf.split_off(head_len);
    body.truncate(length);
    while body.len() < length {
        let n = stream.read_until(&mut chunk, deadline)?;
        if n == 0 {
            return Err(ReadError::Gone);
        }
        let take = n.min(length - body.len());
        body.extend_from_slice(&chunk[..take]);
    }
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

/// A reader that can stop at a deadline.
pub trait ReadTimed {
    fn read_until(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize>;
}

impl ReadTimed for std::net::TcpStream {
    fn read_until(&mut self, buf: &mut [u8], deadline: Instant) -> io::Result<usize> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.set_read_timeout(Some(left))?;
        self.read(buf)
    }
}

/// Bytes already in memory (tests).
impl ReadTimed for &[u8] {
    fn read_until(&mut self, buf: &mut [u8], _deadline: Instant) -> io::Result<usize> {
        self.read(buf)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type", content_type.into())],
            body: body.into(),
        }
    }

    pub fn json(status: u16, value: &impl serde::Serialize) -> Self {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
        Self::new(status, "application/json; charset=utf-8", body)
    }

    /// `{"error": "..."}`.
    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": message }))
    }

    pub fn with(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Headers every answer carries.
pub const SAFE_HEADERS: [(&str, &str); 5] = [
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
    ("Cross-Origin-Opener-Policy", "same-origin"),
];

/// The status line and headers (with a length and `Connection: close`), then the body unless
/// `head_only`.
pub fn write_response(w: &mut impl Write, r: &Response, head_only: bool) -> io::Result<()> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", r.status, reason(r.status));
    for (k, v) in SAFE_HEADERS {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    for (k, v) in &r.headers {
        // Values are ours; still, never let one end the header block.
        let v: String = v.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        r.body.len()
    ));
    let mut bytes = out.into_bytes();
    if !head_only {
        bytes.extend_from_slice(&r.body);
    }
    w.write_all(&bytes)?;
    w.flush()
}

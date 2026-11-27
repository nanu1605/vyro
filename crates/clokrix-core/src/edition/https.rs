//! A small HTTP/1.1 client for the checkout's few JSON calls (2.5) and OpenRGB's installer
//! (2.6.1, [`fetch`]): one request a connection, HTTPS through the same rustls set-up as the
//! relay's socket (Windows' own certificate store), or plain HTTP for the tests' local stand-in.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
/// The checkout's answers are a few hundred bytes; anything far larger is not one of them.
const MAX_RESPONSE: usize = 256 * 1024;

/// An answer: its status and body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub body: String,
}

struct Target<'a> {
    secure: bool,
    host: &'a str,
    port: u16,
    path: &'a str,
}

fn parse(url: &str) -> Result<Target<'_>, String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("not a web address: {url}"));
    };
    let (authority, path) = rest.find('/').map_or((rest, "/"), |i| rest.split_at(i));
    // An IPv6 address keeps its brackets; a port follows the last colon outside them.
    let split = match authority.rfind(']') {
        Some(end) => authority[end..].find(':').map(|i| end + i),
        None => authority.rfind(':'),
    };
    let (host, port) = match split {
        Some(i) => (
            &authority[..i],
            authority[i + 1..]
                .parse()
                .map_err(|_| format!("bad port in {url}"))?,
        ),
        None => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err(format!("no host in {url}"));
    }
    Ok(Target {
        secure,
        host,
        port,
        path,
    })
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

/// `method` on `url`, with a JSON `body` if any.
pub fn request(method: &str, url: &str, body: Option<&str>) -> Result<Answer, String> {
    let raw = send(method, url, body, "application/json", MAX_RESPONSE)?;
    read_answer(&raw)
}

/// Redirects followed at most.
const REDIRECTS: usize = 5;
/// Room for an answer's head besides its body.
const HEAD_ROOM: usize = 64 * 1024;

/// The body of `url` (a download), at most `max` bytes, following up to 5 redirects; one from
/// HTTPS to plain HTTP is refused.
pub fn fetch(url: &str, max: usize) -> Result<Vec<u8>, String> {
    let mut url = url.to_owned();
    for _ in 0..=REDIRECTS {
        let raw = send("GET", &url, None, "*/*", max.saturating_add(HEAD_ROOM))?;
        let r = read_raw(&raw)?;
        match r.status {
            200 => {
                if r.body.len() > max {
                    return Err("the download is too long".into());
                }
                return Ok(r.body);
            }
            301 | 302 | 303 | 307 | 308 => {
                let to = r
                    .header("location")
                    .ok_or("a redirect without a Location")?;
                url = redirect(&url, to)?;
            }
            s => return Err(format!("the download failed: HTTP {s}")),
        }
    }
    Err("too many redirects".into())
}

/// Where a redirect from `from` to `to` (absolute, or a path on the same server) goes.
fn redirect(from: &str, to: &str) -> Result<String, String> {
    let next = if to.starts_with("https://") || to.starts_with("http://") {
        to.to_owned()
    } else if to.starts_with('/') && !to.starts_with("//") {
        let t = parse(from)?;
        let scheme = if t.secure { "https" } else { "http" };
        let default = if t.secure { 443 } else { 80 };
        if t.port == default {
            format!("{scheme}://{}{to}", t.host)
        } else {
            format!("{scheme}://{}:{}{to}", t.host, t.port)
        }
    } else {
        return Err(format!("a redirect Clokrix does not follow: {to}"));
    };
    if from.starts_with("https://") && !next.starts_with("https://") {
        return Err("a redirect away from HTTPS was refused".into());
    }
    parse(&next)?;
    Ok(next)
}

/// `method` on `url`: the whole answer as it came, at most `max` bytes.
fn send(
    method: &str,
    url: &str,
    body: Option<&str>,
    accept: &str,
    max: usize,
) -> Result<Vec<u8>, String> {
    let t = parse(url)?;
    let bare = t.host.trim_start_matches('[').trim_end_matches(']');
    let addrs = (bare, t.port)
        .to_socket_addrs()
        .map_err(|e| format!("can't find {}: {e}", t.host))?;
    let mut last = None;
    let mut tcp = None;
    for a in addrs {
        match TcpStream::connect_timeout(&a, TIMEOUT) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let tcp = tcp.ok_or_else(|| {
        format!(
            "can't reach {}: {}",
            t.host,
            last.map_or_else(|| "no address".to_owned(), |e| e.to_string())
        )
    })?;
    let _ = tcp.set_read_timeout(Some(TIMEOUT));
    let _ = tcp.set_write_timeout(Some(TIMEOUT));
    let host_header = if (t.secure && t.port == 443) || (!t.secure && t.port == 80) {
        t.host.to_owned()
    } else {
        format!("{}:{}", t.host, t.port)
    };
    let mut head = format!(
        "{method} {} HTTP/1.1\r\nHost: {host_header}\r\nUser-Agent: Clokrix\r\nAccept: {accept}\r\nConnection: close\r\n",
        t.path
    );
    if let Some(b) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    let mut raw = Vec::new();
    if t.secure {
        let name = rustls::pki_types::ServerName::try_from(bare.to_owned())
            .map_err(|e| format!("bad host {}: {e}", t.host))?;
        let conn = rustls::ClientConnection::new(tls()?, name)
            .map_err(|e| format!("secure connection to {} failed: {e}", t.host))?;
        let mut s = rustls::StreamOwned::new(conn, tcp);
        exchange(&mut s, &head, body, &mut raw, max)?;
    } else {
        let mut s = tcp;
        exchange(&mut s, &head, body, &mut raw, max)?;
    }
    Ok(raw)
}

fn exchange(
    s: &mut (impl Read + Write),
    head: &str,
    body: Option<&str>,
    raw: &mut Vec<u8>,
    max: usize,
) -> Result<(), String> {
    s.write_all(head.as_bytes())
        .and_then(|()| s.write_all(body.unwrap_or_default().as_bytes()))
        .and_then(|()| s.flush())
        .map_err(|e| format!("sending failed: {e}"))?;
    let mut buf = [0u8; 16 * 1024];
    loop {
        match s.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > max {
                    return Err("the answer is too long".into());
                }
            }
            // A server that closes without TLS's goodbye has still sent everything it meant to.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !raw.is_empty() => {
                return Ok(())
            }
            Err(e) => return Err(format!("reading the answer failed: {e}")),
        }
    }
}

/// The status and body of a whole response, chunked or not.
pub fn read_answer(raw: &[u8]) -> Result<Answer, String> {
    let r = read_raw(raw)?;
    Ok(Answer {
        status: r.status,
        body: String::from_utf8_lossy(&r.body).into_owned(),
    })
}

/// A whole response: its status, headers (as sent) and body (as bytes).
struct Raw {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Raw {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_raw(raw: &[u8]) -> Result<Raw, String> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut res = httparse::Response::new(&mut headers);
    let len = match res.parse(raw) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => return Err("the answer ended early".into()),
        Err(e) => return Err(format!("not an HTTP answer: {e}")),
    };
    let status = res.code.unwrap_or(0);
    let headers: Vec<(String, String)> = res
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_owned(),
                String::from_utf8_lossy(h.value).trim().to_owned(),
            )
        })
        .collect();
    let lower = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.to_ascii_lowercase())
    };
    let rest = &raw[len..];
    let body = if lower("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        dechunk(rest)?
    } else if let Some(n) = lower("content-length").and_then(|v| v.parse::<usize>().ok()) {
        rest.get(..n).ok_or("the answer ended early")?.to_vec()
    } else {
        rest.to_vec()
    };
    Ok(Raw {
        status,
        headers,
        body,
    })
}

fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or("a chunk ended early")?;
        let size_text = String::from_utf8_lossy(&rest[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| format!("bad chunk size {size_text:?}"))?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(rest.get(..size).ok_or("a chunk ended early")?);
        rest = rest.get(size + 2..).ok_or("a chunk ended early")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn reads_plain_and_chunked_answers() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}";
        assert_eq!(
            read_answer(plain).unwrap(),
            Answer {
                status: 200,
                body: "{\"ok\":true}".into()
            }
        );
        let chunked = b"HTTP/1.1 409 Conflict\r\nTransfer-Encoding: chunked\r\n\r\n9\r\n{\"error\":\r\n9;x=y\r\n\"in-use\"}\r\n0\r\n\r\n";
        assert_eq!(
            read_answer(chunked).unwrap(),
            Answer {
                status: 409,
                body: "{\"error\":\"in-use\"}".into()
            }
        );
        assert!(read_answer(b"HTTP/1.1 200 OK\r\nContent-Le").is_err());
        assert!(read_answer(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort").is_err());
    }

    #[test]
    fn parses_addresses() {
        let t = parse("https://pro.example.com/activate").unwrap();
        assert!(t.secure && t.host == "pro.example.com" && t.port == 443 && t.path == "/activate");
        let t = parse("http://127.0.0.1:8787").unwrap();
        assert!(!t.secure && t.host == "127.0.0.1" && t.port == 8787 && t.path == "/");
        let t = parse("http://[::1]:9000/x").unwrap();
        assert!(t.host == "[::1]" && t.port == 9000 && t.path == "/x");
        assert!(parse("ftp://x").is_err());
        assert!(parse("https:///x").is_err());
    }

    #[test]
    fn asks_a_local_server_and_reads_its_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 1024];
            // The head and its body (the request says how long).
            while !String::from_utf8_lossy(&got).contains("\"pc\"") {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nB\r\n{\"ok\":true}\r\n0\r\n\r\n")
                .unwrap();
            String::from_utf8(got).unwrap()
        });
        let a = request(
            "POST",
            &format!("http://127.0.0.1:{port}/check"),
            Some("{\"pc\":\"x\"}"),
        )
        .unwrap();
        assert_eq!(a.status, 200);
        assert_eq!(a.body, "{\"ok\":true}");
        let sent = server.join().unwrap();
        assert!(sent.starts_with("POST /check HTTP/1.1\r\n"));
        assert!(sent.contains(&format!("Host: 127.0.0.1:{port}\r\n")));
        assert!(sent.contains("Content-Length: 10\r\n"));
        assert!(sent.ends_with("\r\n\r\n{\"pc\":\"x\"}"));
    }

    /// A stand-in that answers each connection with the next of `answers`, and returns the
    /// request lines it was sent.
    fn serve(answers: Vec<Vec<u8>>) -> (u16, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let mut lines = Vec::new();
            for a in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut got = Vec::new();
                let mut buf = [0u8; 1024];
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut buf).unwrap();
                    got.extend_from_slice(&buf[..n]);
                }
                let text = String::from_utf8_lossy(&got).into_owned();
                lines.push(text.lines().next().unwrap_or_default().to_owned());
                assert!(text.contains("Accept: */*\r\n"), "{text}");
                s.write_all(&a).unwrap();
            }
            lines
        });
        (port, server)
    }

    fn bytes(status: &str, extra: &str, body: &[u8]) -> Vec<u8> {
        let mut a = format!(
            "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        a.extend_from_slice(body);
        a
    }

    #[test]
    fn downloads_bytes_through_redirects() {
        // Bytes no text survives (an installer's), plain and then chunked.
        let body: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
        let mut chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        for part in body.chunks(30_000) {
            chunked.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
            chunked.extend_from_slice(part);
            chunked.extend_from_slice(b"\r\n");
        }
        chunked.extend_from_slice(b"0\r\n\r\n");
        let (port, server) = serve(vec![
            bytes("302 Found", "Location: /files/a.msi\r\n", b""),
            bytes("200 OK", "", &body),
        ]);
        let got = fetch(&format!("http://127.0.0.1:{port}/download/a.msi"), 100_000).unwrap();
        assert_eq!(got, body);
        assert_eq!(
            server.join().unwrap(),
            ["GET /download/a.msi HTTP/1.1", "GET /files/a.msi HTTP/1.1"]
        );

        let (port, server) = serve(vec![chunked]);
        assert_eq!(
            fetch(&format!("http://127.0.0.1:{port}/a.msi"), 100_000).unwrap(),
            body
        );
        server.join().unwrap();
    }

    #[test]
    fn refuses_a_download_too_long_or_failed_or_going_round() {
        let (port, server) = serve(vec![bytes("200 OK", "", &[7u8; 5000])]);
        let e = fetch(&format!("http://127.0.0.1:{port}/a"), 4000).unwrap_err();
        assert!(e.contains("too long"), "{e}");
        server.join().unwrap();

        let (port, server) = serve(vec![bytes("404 Not Found", "", b"gone")]);
        let e = fetch(&format!("http://127.0.0.1:{port}/a"), 4000).unwrap_err();
        assert_eq!(e, "the download failed: HTTP 404");
        server.join().unwrap();

        let round = || bytes("301 Moved Permanently", "Location: /a\r\n", b"");
        let (port, server) = serve((0..=REDIRECTS).map(|_| round()).collect());
        let e = fetch(&format!("http://127.0.0.1:{port}/a"), 4000).unwrap_err();
        assert_eq!(e, "too many redirects");
        assert_eq!(server.join().unwrap().len(), REDIRECTS + 1);
    }

    #[test]
    fn follows_redirects_only_where_they_are_safe() {
        assert_eq!(
            redirect("https://a.example/x/y", "/z?q=1").unwrap(),
            "https://a.example/z?q=1"
        );
        assert_eq!(
            redirect("http://127.0.0.1:8080/x", "/y").unwrap(),
            "http://127.0.0.1:8080/y"
        );
        assert_eq!(
            redirect("https://a.example/x", "https://cdn.example/f").unwrap(),
            "https://cdn.example/f"
        );
        // Never from HTTPS to plain HTTP, and only absolute addresses or a server's own paths.
        assert!(redirect("https://a.example/x", "http://a.example/x").is_err());
        assert!(redirect("https://a.example/x", "//b.example/x").is_err());
        assert!(redirect("https://a.example/x", "y").is_err());
        assert!(redirect("https://a.example/x", "ftp://b.example/").is_err());
    }
}

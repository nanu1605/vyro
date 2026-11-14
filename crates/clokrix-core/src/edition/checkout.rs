//! The checkout's calls (2.5, `licence/src/index.ts`): the key of a purchase the app opened,
//! and a key bound to, freed from or looked at on this PC.

use serde::Deserialize;
use serde_json::json;

use super::https;

/// Why a call did not give what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The checkout could not be reached, or answered nothing it should.
    Unreachable(String),
    /// No such key.
    NoKey,
    /// The key's payment was refunded.
    Revoked,
    /// The key is Pro on another PC.
    InUse,
    /// The key moved less than 30 days ago; it may move again from `until` (Unix seconds).
    MovedRecently { until: Option<i64> },
    /// The purchase's key was already collected (by this app, or another with its code).
    Collected,
    /// The key is not on this PC (to free it).
    NotHere,
    /// Asked too often from this address: wait a minute.
    Busy,
    /// Anything else the checkout said.
    Other(u16, String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(e) => write!(f, "Can't reach Clokrix's checkout: {e}"),
            Self::NoKey => f.write_str("That is not a Clokrix Pro key. Check it against the one you were given."),
            Self::Revoked => f.write_str("This key's payment was refunded, so it no longer turns Pro on."),
            Self::InUse => f.write_str(
                "This key is Pro on another PC. Free it there (Settings → Clokrix Pro → Free this PC), or move Pro here.",
            ),
            Self::MovedRecently { .. } => {
                f.write_str("Pro moved to another PC less than 30 days ago; it can move again 30 days after that.")
            }
            Self::Collected => f.write_str("This purchase's key was already collected. Enter it in Settings → Clokrix Pro."),
            Self::NotHere => f.write_str("This key is not Pro on this PC."),
            Self::Busy => f.write_str("The checkout was asked too often from here. Try again in a minute."),
            Self::Other(status, e) => write!(f, "The checkout answered {status}: {e}"),
        }
    }
}

/// Still Pro here, and if not, why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Here,
    Moved,
    Released,
    Revoked,
    NoKey,
}

/// The checkout at `base` (`CLOKRIX_LICENCE_URL`).
#[derive(Debug, Clone)]
pub struct Checkout {
    base: String,
}

#[derive(Deserialize)]
struct Wrong {
    error: Option<String>,
    until: Option<i64>,
}

fn refusal(status: u16, body: &str) -> Refusal {
    let w: Wrong = serde_json::from_str(body).unwrap_or(Wrong {
        error: None,
        until: None,
    });
    match w.error.as_deref() {
        // A key typed wrong is refused before it is looked up.
        Some("no-key" | "key") => Refusal::NoKey,
        Some("too-many") => Refusal::Busy,
        Some("revoked") => Refusal::Revoked,
        Some("in-use") => Refusal::InUse,
        Some("moved-recently") => Refusal::MovedRecently { until: w.until },
        Some("collected") => Refusal::Collected,
        Some("not-here") => Refusal::NotHere,
        Some(e) => Refusal::Other(status, e.to_owned()),
        None => Refusal::Other(status, body.chars().take(120).collect()),
    }
}

impl Checkout {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim().trim_end_matches('/').to_owned(),
        }
    }

    /// The page that pays, for the purchase `claim` (opened in the browser).
    pub fn buy_url(&self, claim: &str) -> String {
        format!("{}/buy?c={claim}", self.base)
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<https::Answer, Refusal> {
        let text = body.map(serde_json::Value::to_string);
        https::request(method, &format!("{}{path}", self.base), text.as_deref())
            .map_err(Refusal::Unreachable)
    }

    /// The key of the purchase `claim`, once paid: none yet while unpaid.
    pub fn claim(&self, claim: &str) -> Result<Option<String>, Refusal> {
        let a = self.call("GET", &format!("/claim/{claim}"), None)?;
        match a.status {
            200 => {
                #[derive(Deserialize)]
                struct Got {
                    key: String,
                }
                serde_json::from_str::<Got>(&a.body)
                    .map(|g| Some(g.key))
                    .map_err(|e| Refusal::Unreachable(format!("unexpected answer: {e}")))
            }
            404 => Ok(None),
            s => Err(refusal(s, &a.body)),
        }
    }

    /// `key` Pro on `pc`; with `move_here`, also from another PC. Answers the signed activation.
    pub fn activate(&self, key: &str, pc: &str, move_here: bool) -> Result<String, Refusal> {
        let a = self.call(
            "POST",
            "/activate",
            Some(&json!({ "key": key, "pc": pc, "move": move_here })),
        )?;
        if a.status != 200 {
            return Err(refusal(a.status, &a.body));
        }
        #[derive(Deserialize)]
        struct Got {
            activation: String,
        }
        serde_json::from_str::<Got>(&a.body)
            .map(|g| g.activation)
            .map_err(|e| Refusal::Unreachable(format!("unexpected answer: {e}")))
    }

    /// `key` off `pc`, for another PC to take.
    pub fn release(&self, key: &str, pc: &str) -> Result<(), Refusal> {
        let a = self.call("POST", "/release", Some(&json!({ "key": key, "pc": pc })))?;
        if a.status == 200 {
            Ok(())
        } else {
            Err(refusal(a.status, &a.body))
        }
    }

    /// Whether `key` is still Pro on `pc`.
    pub fn check(&self, key: &str, pc: &str) -> Result<Standing, Refusal> {
        let a = self.call("POST", "/check", Some(&json!({ "key": key, "pc": pc })))?;
        if a.status != 200 {
            return Err(refusal(a.status, &a.body));
        }
        #[derive(Deserialize)]
        struct Got {
            ok: bool,
            reason: Option<String>,
        }
        let g: Got = serde_json::from_str(&a.body)
            .map_err(|e| Refusal::Unreachable(format!("unexpected answer: {e}")))?;
        Ok(match (g.ok, g.reason.as_deref()) {
            (true, _) => Standing::Here,
            (false, Some("moved")) => Standing::Moved,
            (false, Some("released")) => Standing::Released,
            (false, Some("revoked")) => Standing::Revoked,
            (false, _) => Standing::NoKey,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// A stand-in checkout answering each request with the next of `answers`.
    fn serve(answers: Vec<(u16, &'static str)>) -> (Checkout, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let t = thread::spawn(move || {
            let mut asked = Vec::new();
            for (status, body) in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut got = Vec::new();
                let mut buf = [0u8; 2048];
                loop {
                    let n = s.read(&mut buf).unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .map_or(0, |n| n.trim().parse::<usize>().unwrap());
                        if got.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(reply.as_bytes()).unwrap();
                asked.push(String::from_utf8(got).unwrap());
            }
            asked
        });
        (Checkout::new(&format!("{base}/")), t)
    }

    #[test]
    fn collects_a_purchase_once_paid() {
        let (c, t) = serve(vec![
            (404, r#"{"error":"not-paid"}"#),
            (200, r#"{"key":"CLKX-ABCDE-FGHJK-MNPQR-STVWX"}"#),
            (410, r#"{"error":"collected"}"#),
        ]);
        let code = "0123456789abcdef0123456789abcdef";
        assert_eq!(c.claim(code), Ok(None));
        assert_eq!(
            c.claim(code),
            Ok(Some("CLKX-ABCDE-FGHJK-MNPQR-STVWX".into()))
        );
        assert_eq!(c.claim(code), Err(Refusal::Collected));
        let asked = t.join().unwrap();
        assert!(asked[0].starts_with(&format!("GET /claim/{code} HTTP/1.1")));
        assert!(c.buy_url(code).ends_with(&format!("/buy?c={code}")));
    }

    #[test]
    fn activates_frees_and_checks() {
        let (c, t) = serve(vec![
            (
                200,
                r#"{"activation":"payload.sig","key":"CLKX-ABCDE-FGHJK-MNPQR-STVWX"}"#,
            ),
            (409, r#"{"error":"in-use"}"#),
            (409, r#"{"error":"moved-recently","until":1800000000}"#),
            (200, r#"{"ok":true}"#),
            (200, r#"{"ok":false,"reason":"moved"}"#),
            (200, r#"{"ok":true}"#),
            (409, r#"{"error":"not-here"}"#),
            (403, r#"{"error":"revoked"}"#),
            (400, r#"{"error":"key"}"#),
            (429, r#"{"error":"too-many"}"#),
        ]);
        let pc = "ab".repeat(32);
        let key = "CLKX-ABCDE-FGHJK-MNPQR-STVWX";
        assert_eq!(c.activate(key, &pc, false), Ok("payload.sig".into()));
        assert_eq!(c.activate(key, &pc, false), Err(Refusal::InUse));
        assert_eq!(
            c.activate(key, &pc, true),
            Err(Refusal::MovedRecently {
                until: Some(1_800_000_000)
            })
        );
        assert_eq!(c.check(key, &pc), Ok(Standing::Here));
        assert_eq!(c.check(key, &pc), Ok(Standing::Moved));
        assert_eq!(c.release(key, &pc), Ok(()));
        assert_eq!(c.release(key, &pc), Err(Refusal::NotHere));
        assert_eq!(c.activate(key, &pc, false), Err(Refusal::Revoked));
        assert_eq!(c.activate("CLKX-typo", &pc, false), Err(Refusal::NoKey));
        assert_eq!(c.check(key, &pc), Err(Refusal::Busy));
        let asked = t.join().unwrap();
        assert!(asked[0].starts_with("POST /activate HTTP/1.1"));
        assert!(asked[0].ends_with(&format!(r#"{{"key":"{key}","move":false,"pc":"{pc}"}}"#)));
        assert!(asked[2].contains(r#""move":true"#));
        assert!(asked[5].starts_with("POST /release "));
    }

    #[test]
    fn an_unreachable_checkout_says_so() {
        // Nothing listens on a port just freed.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let c = Checkout::new(&format!("http://127.0.0.1:{port}"));
        assert!(matches!(
            c.check("CLKX-ABCDE-FGHJK-MNPQR-STVWX", &"ab".repeat(32)),
            Err(Refusal::Unreachable(_))
        ));
    }
}

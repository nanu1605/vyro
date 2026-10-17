//! Pairing and sessions. A device pairs with a short-lived code shown in the app and gets a
//! random 256-bit session token; only the token's SHA-256 hash is kept, so the saved file
//! cannot sign anyone in. Time and randomness come from the caller, so this is tested as is.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    Pairing, RemoteDevice, RemoteSettings, CODE_SECS, CODE_TRIES, DAY_CHOICES, DEFAULT_DAYS,
    MAX_DEVICES,
};

/// No 0/O or 1/I, so a code read off the screen is typed right.
const ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
pub const CODE_LEN: usize = 8;
/// Longest device name kept.
pub const NAME_MAX: usize = 40;
/// `last_seen` is saved again only after this long, so a watching phone does not rewrite the
/// file every second.
const SEEN_STEP: i64 = 60;

/// A paired device as saved in `remote.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredDevice {
    pub id: String,
    pub name: String,
    /// SHA-256 of the session token, hex.
    pub token_hash: String,
    pub paired: i64,
    pub expires: i64,
    #[serde(default)]
    pub last_seen: Option<i64>,
    #[serde(default)]
    pub last_address: Option<String>,
}

/// `remote.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stored {
    pub version: u32,
    pub settings: RemoteSettings,
    pub devices: Vec<StoredDevice>,
    /// Reach from anywhere (1.4). Versions before 1.4 ignore it.
    pub relay: Option<super::relay::StoredRelay>,
}

/// 2: `relay` (1.4).
pub const STORED_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Code {
    /// Canonical, without the dash.
    code: [u8; CODE_LEN],
    expires: i64,
    tries_left: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// No code is open: the owner has to show one in the app.
    NoCode,
    Expired,
    /// Wrong code; this many tries are left on it.
    Wrong(u8),
    /// Too many wrong tries: the code is spent.
    Spent,
    /// [`MAX_DEVICES`] are paired already.
    Full,
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCode => f.write_str(
                "No pairing code is open. In Vyro, open Settings → Remote → Pair a device.",
            ),
            Self::Expired => f.write_str("That code has expired. Show a new one in Vyro."),
            Self::Wrong(1) => f.write_str("That code is wrong. One try left."),
            Self::Wrong(n) => write!(f, "That code is wrong. {n} tries left."),
            Self::Spent => f.write_str("Too many wrong tries. Show a new code in Vyro."),
            Self::Full => write!(
                f,
                "{MAX_DEVICES} devices are paired already. Sign one out in Vyro first."
            ),
        }
    }
}

/// Paired devices and the open pairing code.
#[derive(Debug, Clone, Default)]
pub struct Auth {
    devices: Vec<StoredDevice>,
    code: Option<Code>,
    /// Devices changed in a way worth saving.
    dirty: bool,
}

pub fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Five random bytes → eight code characters (5 bits each).
fn code_from(random: [u8; 5]) -> [u8; CODE_LEN] {
    let mut bits = random.iter().fold(0u64, |acc, &b| acc << 8 | u64::from(b));
    let mut out = [0u8; CODE_LEN];
    for c in out.iter_mut().rev() {
        *c = ALPHABET[(bits & 31) as usize];
        bits >>= 5;
    }
    out
}

/// `K7QM-4XPA`.
pub fn show(code: &[u8; CODE_LEN]) -> String {
    let s = String::from_utf8_lossy(code);
    format!("{}-{}", &s[..4], &s[4..])
}

/// What someone typed, canonical: upper case, without spaces or dashes. `None` when it cannot
/// be a code.
pub fn parse(typed: &str) -> Option<[u8; CODE_LEN]> {
    let mut out = [0u8; CODE_LEN];
    let mut n = 0;
    for c in typed
        .chars()
        .filter(|c| !matches!(c, '-' | ' ' | '\u{2010}'..='\u{2015}'))
    {
        let c = c.to_ascii_uppercase();
        if n == CODE_LEN || !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
            return None;
        }
        out[n] = c as u8;
        n += 1;
    }
    (n == CODE_LEN).then_some(out)
}

/// Same length, compared without an early exit.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A device's name, as the page sent it: printable, trimmed, at most [`NAME_MAX`] characters.
pub fn clean_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s: String = s.chars().take(NAME_MAX).collect();
    if s.is_empty() {
        "Device".into()
    } else {
        s
    }
}

impl Auth {
    /// From the saved devices, dropping expired ones and any that do not look right.
    pub fn new(devices: Vec<StoredDevice>, now: i64) -> Self {
        let mut a = Self {
            devices: devices
                .into_iter()
                .filter(|d| {
                    d.token_hash.len() == 64 && d.token_hash.bytes().all(|b| b.is_ascii_hexdigit())
                })
                .take(MAX_DEVICES)
                .collect(),
            code: None,
            dirty: false,
        };
        a.prune(now);
        a
    }

    pub fn stored(&self) -> &[StoredDevice] {
        &self.devices
    }

    /// Whether anything worth saving changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Opens a new pairing code (replacing any open one).
    pub fn start_pairing(&mut self, now: i64, random: [u8; 5]) -> Pairing {
        let code = Code {
            code: code_from(random),
            expires: now + CODE_SECS,
            tries_left: CODE_TRIES,
        };
        let shown = pairing_of(&code);
        self.code = Some(code);
        shown
    }

    pub fn cancel_pairing(&mut self) {
        self.code = None;
    }

    /// The open code, if it is still usable.
    pub fn pairing(&self, now: i64) -> Option<Pairing> {
        self.code
            .as_ref()
            .filter(|c| c.expires > now && c.tries_left > 0)
            .map(pairing_of)
    }

    /// Pairs a device with a typed code. On success the code is used up and the device's token
    /// (hex of `token`) is returned with its id.
    #[allow(clippy::too_many_arguments)]
    pub fn pair(
        &mut self,
        now: i64,
        typed: &str,
        name: &str,
        address: &str,
        days: u32,
        token: [u8; 32],
        id: [u8; 4],
    ) -> Result<(String, StoredDevice), PairError> {
        self.prune(now);
        let Some(code) = self.code.as_mut() else {
            return Err(PairError::NoCode);
        };
        if code.tries_left == 0 {
            return Err(PairError::Spent);
        }
        if code.expires <= now {
            self.code = None;
            return Err(PairError::Expired);
        }
        let ok = parse(typed).is_some_and(|t| same(&t, &code.code));
        if !ok {
            code.tries_left -= 1;
            return Err(match code.tries_left {
                0 => {
                    self.code = None;
                    PairError::Spent
                }
                n => PairError::Wrong(n),
            });
        }
        if self.devices.len() >= MAX_DEVICES {
            return Err(PairError::Full);
        }
        self.code = None;
        let days = if DAY_CHOICES.contains(&days) {
            days
        } else {
            DEFAULT_DAYS
        };
        let token = hex(&token);
        let device = StoredDevice {
            id: hex(&id),
            name: clean_name(name),
            token_hash: hash(&token),
            paired: now,
            expires: now + i64::from(days) * 86_400,
            last_seen: Some(now),
            last_address: Some(address.into()),
        };
        self.devices.push(device.clone());
        self.dirty = true;
        Ok((token, device))
    }

    /// The device a session token belongs to, noting when and from where it was seen.
    pub fn check(&mut self, now: i64, token: &str, address: &str) -> Option<StoredDevice> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let h = hash(&token.to_ascii_lowercase());
        let d = self
            .devices
            .iter_mut()
            .find(|d| same(d.token_hash.as_bytes(), h.as_bytes()))?;
        if d.expires <= now {
            self.prune(now);
            return None;
        }
        let moved = d.last_address.as_deref() != Some(address);
        let stale = d.last_seen.is_none_or(|t| now - t >= SEEN_STEP);
        if moved || stale {
            d.last_seen = Some(now);
            d.last_address = Some(address.into());
            self.dirty = true;
        }
        Some(d.clone())
    }

    /// Signs a device out; `false` when there was no such device.
    pub fn sign_out(&mut self, id: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.id != id);
        let gone = self.devices.len() != before;
        self.dirty |= gone;
        gone
    }

    pub fn sign_out_all(&mut self) {
        self.dirty |= !self.devices.is_empty();
        self.devices.clear();
    }

    /// Drops expired devices; `true` when any went.
    pub fn prune(&mut self, now: i64) -> bool {
        let before = self.devices.len();
        self.devices.retain(|d| d.expires > now);
        let gone = self.devices.len() != before;
        self.dirty |= gone;
        gone
    }

    pub fn devices(&self, watching: &[String]) -> Vec<RemoteDevice> {
        self.devices
            .iter()
            .map(|d| RemoteDevice {
                id: d.id.clone(),
                name: d.name.clone(),
                paired: d.paired,
                expires: d.expires,
                last_seen: d.last_seen,
                last_address: d.last_address.clone(),
                watching: watching.contains(&d.id),
                anywhere: false,
            })
            .collect()
    }

    pub fn is_paired(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id == id)
    }
}

fn pairing_of(c: &Code) -> Pairing {
    Pairing {
        code: show(&c.code),
        expires: c.expires,
        tries_left: c.tries_left,
        url: None,
        qr: None,
        qr_size: 0,
    }
}

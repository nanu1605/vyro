//! Clokrix Free and Pro (2.5, docs/pro.md).
//!
//! **Pro** is everything; **Free** is the Dashboard and Settings' own sections. A new install, or
//! one updated to 2.5, is Pro for a 7-day trial. After it, Pro needs a key from the checkout
//! (`licence/`): paid once, for life, on one PC. The checkout binds the key to this PC and signs an
//! *activation* saying so, which this checks offline at every start with the public key built in,
//! so Pro needs the internet only to be bought, moved, and looked at again now and then.
//!
//! A build without the checkout's settings (a fork, a local or CI build) is Pro: the code is open
//! source, and the lock is for the official builds.

pub mod checkout;
mod https;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

pub const DAY: i64 = 86_400;
/// The trial: Pro for this long from the first start of 2.5.
pub const TRIAL_DAYS: i64 = 7;
/// An activation is looked at again with the checkout this often, when online.
pub const CHECK_EVERY: i64 = 7 * DAY;
/// What Pro costs, as said in the app (the checkout charges it).
pub const PRICE: &str = "$100";

/// What this PC is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Tier {
    /// Paid for, on this PC; or a build without the checkout.
    Pro,
    /// The 7 days' trial: everything, as Pro.
    Trial,
    /// The Dashboard and Settings' own sections.
    Free,
}

impl Tier {
    /// Everything is open.
    pub fn full(self) -> bool {
        self != Self::Free
    }
}

/// What the app shows of it (`get_edition`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Edition {
    pub tier: Tier,
    /// When the trial ends (or ended), Unix seconds; none for a build without the checkout.
    #[ts(type = "number | null")]
    pub trial_ends: Option<i64>,
    /// The key in use, all but its last part hidden.
    pub key: Option<String>,
    /// Said once something changed: Pro moved to another PC, a refund, a key refused.
    pub note: Option<String>,
    /// A purchase was started here and its key is waited for.
    pub buying: bool,
    /// This build can be bought from (it carries the checkout's address and key).
    pub can_buy: bool,
    /// The trial has ended and the app has not said so yet.
    pub ended_unseen: bool,
    pub price: String,
}

/// A purchase the app opened in the browser, waited for until `until`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub code: String,
    pub until: i64,
}

/// What is kept (`edition.json` in the app's folder).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stored {
    /// The first start of 2.5 or later.
    pub trial_started: Option<i64>,
    /// The key bought, as the checkout gave it.
    pub key: Option<String>,
    /// The checkout's signed word that the key is Pro on this PC.
    pub activation: Option<String>,
    /// When the activation was last looked at with the checkout.
    pub checked: Option<i64>,
    pub note: Option<String>,
    pub claim: Option<Claim>,
    /// The trial's end has been said.
    pub ended_seen: bool,
    /// This PC's id where Windows' own could not be read: made once, then kept.
    pub own_pc: Option<String>,
}

impl Stored {
    /// Starts the trial clock the first time.
    pub fn start(&mut self, now: i64) -> bool {
        if self.trial_started.is_some() {
            return false;
        }
        self.trial_started = Some(now);
        true
    }

    pub fn trial_ends(&self) -> Option<i64> {
        self.trial_started.map(|t| t + TRIAL_DAYS * DAY)
    }
}

/// What an activation says (signed by the checkout, `licence/src/rules.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Activation {
    pub v: u32,
    pub lic: String,
    pub pc: String,
    pub edition: String,
    pub issued: i64,
}

/// The public key built in (base64url, 32 bytes), if this build has one.
pub fn public_key(text: Option<&str>) -> Option<[u8; 32]> {
    let t = text?.trim();
    if t.is_empty() {
        return None;
    }
    URL_SAFE_NO_PAD.decode(t).ok()?.try_into().ok()
}

/// The activation's word, if `key` signed it and it names this PC and Pro.
pub fn verify(token: &str, key: &[u8; 32], pc: &str) -> Option<Activation> {
    let (payload, sig) = token.split_once('.')?;
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let sig = URL_SAFE_NO_PAD.decode(sig).ok()?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(&payload, &sig)
        .ok()?;
    let a: Activation = serde_json::from_slice(&payload).ok()?;
    (a.v == 1 && a.edition == "pro" && a.pc == pc).then_some(a)
}

/// What this PC is now: Pro with an activation for it, else the trial while it lasts, else Free.
/// With no public key built in, Pro.
pub fn tier(s: &Stored, now: i64, pc: &str, key: Option<&[u8; 32]>) -> Tier {
    let Some(key) = key else { return Tier::Pro };
    if s.activation
        .as_deref()
        .is_some_and(|t| verify(t, key, pc).is_some())
    {
        return Tier::Pro;
    }
    match s.trial_ends() {
        Some(end) if now < end => Tier::Trial,
        // Not started: the app starts it before asking; until then, as a trial.
        None => Tier::Trial,
        _ => Tier::Free,
    }
}

/// This PC, as the checkout knows it: a salted hash of Windows' own id for the installation
/// (`MachineGuid`), never the id itself.
pub fn pc_id(machine_guid: &str) -> String {
    let d = Sha256::digest(format!(
        "clokrix-pc-1:{}",
        machine_guid.trim().to_ascii_lowercase()
    ));
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Windows' id for this installation (`HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`); off
/// Windows, `/etc/machine-id`, for tests and examples.
pub fn machine_guid() -> Option<String> {
    #[cfg(windows)]
    {
        use crate::winutil::{Key, HKEY_LOCAL_MACHINE};
        Key::open(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography")?.string("MachineGuid")
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string("/etc/machine-id")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }
}

/// A new purchase's code: 32 random hex digits.
pub fn new_claim() -> String {
    let mut b = [0u8; 16];
    // A failed draw leaves zeroes, which the checkout takes as a code like any other; it only
    // names this purchase, and is never a secret worth more than the purchase it waits for.
    let _ = getrandom::fill(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The key with all but its last part hidden.
pub fn mask(key: &str) -> String {
    match key.rsplit_once('-') {
        Some((_, last)) => format!("CLKX-•••••-•••••-•••••-{last}"),
        None => "CLKX-…".into(),
    }
}

/// What the app shows, from what is kept.
pub fn edition(s: &Stored, now: i64, pc: &str, key: Option<&[u8; 32]>, can_buy: bool) -> Edition {
    let tier = tier(s, now, pc, key);
    let trial_ends = key.and(s.trial_ends());
    Edition {
        tier,
        trial_ends,
        key: s.key.as_deref().filter(|_| tier == Tier::Pro).map(mask),
        note: s.note.clone(),
        buying: s.claim.as_ref().is_some_and(|c| c.until > now),
        can_buy: can_buy && key.is_some(),
        ended_unseen: tier == Tier::Free && !s.ended_seen && trial_ends.is_some_and(|e| e <= now),
        price: PRICE.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A constructed activation, signed as the checkout signs one (WebCrypto Ed25519 over the
    // JSON's bytes), with a key pair made only for this test.
    const PUB: &str = "ZUeIFJaQy2ojvL8dLaqjvXUjj25kTZA3B0YSCqdwbhE";
    const TOKEN: &str = "eyJ2IjoxLCJsaWMiOiIwMTIzNDU2Nzg5YWJjZGVmIiwicGMiOiJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiYWJhYmFiIiwiZWRpdGlvbiI6InBybyIsImlzc3VlZCI6MTc5NDYwMDAwMH0.tHunaFpsN3KZkY9gLmPqSTaDzl9YuMX2UMAB_hUQMynj6PD-ebqgsPXbTVI_epskI2Wtdj2zBFCQl8GE_7pkAQ";

    fn pc() -> String {
        "ab".repeat(32)
    }

    #[test]
    fn takes_the_checkouts_signature_for_this_pc_only() {
        let key = public_key(Some(PUB)).unwrap();
        let a = verify(TOKEN, &key, &pc()).unwrap();
        assert_eq!(a.lic, "0123456789abcdef");
        assert_eq!(a.issued, 1_794_600_000);
        // Another PC, a changed word, another key: refused.
        assert!(verify(TOKEN, &key, &"cd".repeat(32)).is_none());
        let (payload, sig) = TOKEN.split_once('.').unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
        bytes[5] ^= 1;
        let forged = format!("{}.{sig}", URL_SAFE_NO_PAD.encode(&bytes));
        assert!(verify(&forged, &key, &pc()).is_none());
        let mut other = key;
        other[0] ^= 1;
        assert!(verify(TOKEN, &other, &pc()).is_none());
        assert!(verify("not.a-token", &key, &pc()).is_none());
        assert!(public_key(Some("too-short")).is_none());
        assert!(public_key(Some("")).is_none());
        assert!(public_key(None).is_none());
    }

    #[test]
    fn a_trial_of_seven_days_then_free_unless_activated() {
        let key = public_key(Some(PUB)).unwrap();
        let mut s = Stored::default();
        let t0 = 1_800_000_000;
        assert!(s.start(t0));
        assert!(!s.start(t0 + 50), "the trial starts once");
        assert_eq!(tier(&s, t0, &pc(), Some(&key)), Tier::Trial);
        assert_eq!(tier(&s, t0 + 7 * DAY - 1, &pc(), Some(&key)), Tier::Trial);
        assert_eq!(tier(&s, t0 + 7 * DAY, &pc(), Some(&key)), Tier::Free);
        s.activation = Some(TOKEN.into());
        assert_eq!(tier(&s, t0 + 400 * DAY, &pc(), Some(&key)), Tier::Pro);
        // On another PC (Windows reinstalled), the activation is not this PC's.
        assert_eq!(
            tier(&s, t0 + 400 * DAY, &"cd".repeat(32), Some(&key)),
            Tier::Free
        );
        // A build without the checkout is Pro.
        assert_eq!(
            tier(&Stored::default(), t0 + 400 * DAY, &pc(), None),
            Tier::Pro
        );
    }

    #[test]
    fn says_what_the_app_shows() {
        let key = public_key(Some(PUB)).unwrap();
        let t0 = 1_800_000_000;
        let mut s = Stored::default();
        s.start(t0);
        let e = edition(&s, t0 + DAY, &pc(), Some(&key), true);
        assert_eq!(e.tier, Tier::Trial);
        assert_eq!(e.trial_ends, Some(t0 + 7 * DAY));
        assert!(e.can_buy && !e.ended_unseen && e.key.is_none());
        let e = edition(&s, t0 + 8 * DAY, &pc(), Some(&key), true);
        assert_eq!(e.tier, Tier::Free);
        assert!(e.ended_unseen);
        s.ended_seen = true;
        assert!(!edition(&s, t0 + 8 * DAY, &pc(), Some(&key), true).ended_unseen);
        s.key = Some("CLKX-ABCDE-FGHJK-MNPQR-STVWX".into());
        s.activation = Some(TOKEN.into());
        let e = edition(&s, t0 + 8 * DAY, &pc(), Some(&key), true);
        assert_eq!(e.tier, Tier::Pro);
        assert_eq!(e.key.as_deref(), Some("CLKX-•••••-•••••-•••••-STVWX"));
        s.claim = Some(Claim {
            code: new_claim(),
            until: t0 + 9 * DAY,
        });
        assert!(edition(&s, t0 + 8 * DAY, &pc(), Some(&key), true).buying);
        assert!(!edition(&s, t0 + 10 * DAY, &pc(), Some(&key), true).buying);
        // No checkout built in: Pro, nothing to buy, no trial.
        let e = edition(&s, t0, &pc(), None, true);
        assert_eq!((e.tier, e.can_buy, e.trial_ends), (Tier::Pro, false, None));
    }

    #[test]
    fn names_this_pc_by_a_hash_of_its_id() {
        let a = pc_id("6F1B0C4E-0000-4000-8000-123456789ABC");
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(a, pc_id(" 6f1b0c4e-0000-4000-8000-123456789abc\n"));
        assert_ne!(a, pc_id("6F1B0C4E-0000-4000-8000-123456789ABD"));
        assert!(!a.contains("6f1b0c4e"));
        let c = new_claim();
        assert_eq!(c.len(), 32);
        assert_ne!(c, new_claim());
    }
}

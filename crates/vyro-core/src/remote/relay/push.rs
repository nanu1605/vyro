//! Notifications on a phone while its app is closed (2.0), through Web Push. The phone's browser
//! subscribes with its push service (Google's, Apple's, Mozilla's or Microsoft's) and gives the
//! PC, sealed, the address it got and two keys. The PC seals each notification to those keys
//! (RFC 8291, `aes128gcm`) and hands it to the relay, which signs for the push service (VAPID,
//! RFC 8292) and passes it on: neither the relay nor the push service can read it.
//!
//! A phone chooses what it hears ([`PushKinds`]): alerts as Windows shows them, a device's
//! battery running low, a game's summary when it ends, and this PC going quiet without saying it
//! would. That last one is a notification the PC seals ahead of time and leaves with the relay,
//! which sends it only when the PC's connection ends unannounced and it does not come back.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use ts_rs::TS;

use crate::devices::{DeviceInfo, LowBattery};
use crate::gaming::GameSession;
use crate::remote::random;
use crate::units::{temps_in, TempUnit};

/// The record size written in the header: one record holds a whole notification.
const RS: u32 = 4096;
/// The header: salt (16), record size (4), key length (1) and the PC's one-time key (65).
const HEADER: usize = 16 + 4 + 1 + 65;
/// Push services take bodies of at most 4096 bytes; the header, the record's end mark and the
/// tag leave this much for the notification itself.
pub const MAX_PLAIN: usize = 4096 - HEADER - 1 - 16;
/// A push service's address, at most.
pub const MAX_ENDPOINT: usize = 1024;
/// A game shorter than this sends no summary.
pub const MIN_GAME_SECS: i64 = 5 * 60;
/// A notification's title and text are cut to these many characters.
const MAX_TITLE: usize = 80;
const MAX_BODY: usize = 240;

/// What a phone wants to hear about. Saved with its subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PushKinds {
    /// Alerts that fire, as Windows would show them.
    pub alerts: bool,
    /// A device's battery running low.
    pub batteries: bool,
    /// A game's summary when it ends.
    pub games: bool,
    /// The PC stopped answering without shutting down or going to sleep, and when it is back.
    pub offline: bool,
}

impl Default for PushKinds {
    fn default() -> Self {
        Self {
            alerts: true,
            batteries: true,
            games: false,
            offline: true,
        }
    }
}

/// A phone's push subscription: where its push service takes notifications, and its keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Push {
    pub endpoint: String,
    /// The browser's public key (uncompressed P-256), base64url.
    pub p256dh: String,
    /// Its authentication secret (16 bytes), base64url.
    pub auth: String,
    pub kinds: PushKinds,
}

impl Push {
    /// A subscription as the phone gives it, checked: an `https` address (plain `http` only on
    /// this computer, for a test), a key that is a point on the curve, a 16-byte secret.
    pub fn new(endpoint: &str, p256dh: &str, auth: &str, kinds: PushKinds) -> Result<Self, String> {
        let local =
            endpoint.starts_with("http://127.0.0.1:") || endpoint.starts_with("http://localhost:");
        if endpoint.len() > MAX_ENDPOINT
            || !(endpoint.starts_with("https://") || local)
            || endpoint
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err("That is not a push service's address.".into());
        }
        let push = Self {
            endpoint: endpoint.to_owned(),
            p256dh: p256dh.to_owned(),
            auth: auth.to_owned(),
            kinds,
        };
        push.keys()
            .ok_or("The phone's notification keys did not read.")?;
        Ok(push)
    }

    fn keys(&self) -> Option<(PublicKey, [u8; 16])> {
        let key = super::proto::unb64url(&self.p256dh)?;
        let auth = super::proto::unb64url(&self.auth)?;
        if key.len() != 65 {
            return None;
        }
        Some((
            PublicKey::from_sec1_bytes(&key).ok()?,
            auth.try_into().ok()?,
        ))
    }

    /// `plain` sealed for this phone's browser: the body to post to its push service.
    pub fn seal(&self, plain: &[u8]) -> Option<Vec<u8>> {
        let (ua, auth) = self.keys()?;
        let key = loop {
            // Almost every 32 random bytes is a valid key; the rare one that is not is drawn again.
            if let Ok(k) = SecretKey::from_slice(&random::<32>()?) {
                break k;
            }
        };
        seal_with(&key, &ua, &auth, &random::<16>()?, plain)
    }
}

/// RFC 8291 with the PC's one-time key and the salt given (the tests give the RFC's own).
fn seal_with(
    pc: &SecretKey,
    ua: &PublicKey,
    auth: &[u8; 16],
    salt: &[u8; 16],
    plain: &[u8],
) -> Option<Vec<u8>> {
    if plain.len() > MAX_PLAIN {
        return None;
    }
    let pc_public = pc.public_key().to_encoded_point(false);
    let ua_public = ua.to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(pc.to_nonzero_scalar(), ua.as_affine());
    let mut info = Vec::with_capacity(14 + 65 + 65);
    info.extend_from_slice(b"WebPush: info\0");
    info.extend_from_slice(ua_public.as_bytes());
    info.extend_from_slice(pc_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
        .expand(&info, &mut ikm)
        .ok()?;
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).ok()?;
    let mut nonce = [0u8; 12];
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
    // One record, the last: its end mark is 2, and no padding follows.
    let mut record = Vec::with_capacity(plain.len() + 1);
    record.extend_from_slice(plain);
    record.push(2);
    let sealed = Aes128Gcm::new_from_slice(&cek)
        .ok()?
        .encrypt(Nonce::from_slice(&nonce), record.as_slice())
        .ok()?;
    let mut out = Vec::with_capacity(HEADER + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&RS.to_be_bytes());
    out.push(65);
    out.extend_from_slice(pc_public.as_bytes());
    out.extend_from_slice(&sealed);
    Some(out)
}

/// What a notification is about, which decides which phones get it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Alert,
    Battery,
    Game,
    /// Sent by the relay when the PC went quiet ([`PushKinds::offline`]).
    Offline,
    /// The PC is back after that.
    Back,
    /// The phone asked for one, to see that notifications work.
    Test,
}

impl Kind {
    pub fn wanted(self, k: &PushKinds) -> bool {
        match self {
            Self::Alert => k.alerts,
            Self::Battery => k.batteries,
            Self::Game => k.games,
            Self::Offline | Self::Back => k.offline,
            Self::Test => true,
        }
    }
}

/// One notification, before it is sealed for each phone.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    /// A newer notification with the same tag replaces the older one on the phone.
    pub tag: String,
    /// Unix seconds; 0 for the relay's offline notification (the phone shows when it arrived).
    pub ts: i64,
}

fn cut(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_owned(),
    }
}

impl Note {
    pub fn new(kind: Kind, title: &str, body: &str, tag: &str, ts: i64) -> Self {
        Self {
            kind,
            title: cut(title, MAX_TITLE),
            body: cut(body, MAX_BODY),
            tag: cut(tag, 64),
            ts,
        }
    }

    /// The relay's notification for a PC that went quiet, sealed ahead of time.
    pub fn offline(pc: &str) -> Self {
        Self::new(
            Kind::Offline,
            &format!("{pc} stopped answering"),
            "Its connection ended without it shutting down or going to sleep: a crash, a power cut \
             or a lost network.",
            "offline",
            0,
        )
    }

    /// The PC is back after the relay told its phones it went quiet `away` seconds before.
    pub fn back(pc: &str, away: i64, ts: i64) -> Self {
        let mins = (away + 30) / 60;
        let away = match mins {
            0 | 1 => "a minute".to_owned(),
            m if m < 120 => format!("{m} minutes"),
            m => format!("{} hours", m / 60),
        };
        Self::new(
            Kind::Back,
            &format!("{pc} is back"),
            &format!("It answers again, after about {away} without a word."),
            "offline",
            ts,
        )
    }

    /// A device's battery needs charging; `None` while it does not.
    pub fn battery(d: &DeviceInfo, ts: i64) -> Option<Self> {
        let low = d.low?;
        let title = match low {
            LowBattery::Critical => format!("{} is nearly out of battery", d.name),
            LowBattery::Low => format!("{}'s battery is low", d.name),
        };
        let body = match d.battery.as_ref().and_then(|b| b.percent) {
            Some(p) => format!("{p} % left. Charge it soon."),
            None => "Charge it soon.".to_owned(),
        };
        Some(Self::new(
            Kind::Battery,
            &title,
            &body,
            &format!("battery-{}", d.id),
            ts,
        ))
    }

    /// A game's summary when it ends; `None` for one too short to be worth it.
    pub fn game(g: &GameSession, unit: TempUnit, ts: i64) -> Option<Self> {
        if g.secs < MIN_GAME_SECS {
            return None;
        }
        let mins = g.secs / 60;
        let mut parts = vec![if mins >= 60 {
            format!("{} h {} min", mins / 60, mins % 60)
        } else {
            format!("{mins} min")
        }];
        match (g.avg_fps, g.low1_fps) {
            (Some(a), Some(l)) => parts.push(format!("{a:.0} fps average, 1 % low {l:.0}")),
            (Some(a), None) => parts.push(format!("{a:.0} fps average")),
            _ => {}
        }
        if let Some(t) = g.cpu_temp_max {
            parts.push(format!("CPU up to {t:.0} °C"));
        }
        if let Some(t) = g.gpu_temp_max {
            parts.push(format!("GPU up to {t:.0} °C"));
        }
        Some(Self::new(
            Kind::Game,
            &format!("{} ended", g.app),
            &temps_in(&parts.join(" · "), unit),
            &format!("game-{}", g.started),
            ts,
        ))
    }

    /// As a phone shows it: the PC's name before the text, where the title does not say it.
    pub fn from_pc(&self, name: &str) -> Self {
        match self.kind {
            Kind::Offline | Kind::Back => self.clone(),
            _ => Self {
                body: cut(&format!("{name} · {}", self.body), MAX_BODY),
                ..self.clone()
            },
        }
    }

    /// What the phone's service worker reads: the PC's room picks the PC a tap opens.
    pub fn payload(&self, room: &str) -> Vec<u8> {
        serde_json::json!({
            "v": 1,
            "pc": room,
            "kind": self.kind,
            "title": self.title,
            "body": self.body,
            "tag": self.tag,
            "ts": self.ts,
        })
        .to_string()
        .into_bytes()
    }
}

/// How long a push service keeps a notification for a phone that is off, in seconds.
pub fn ttl(kind: Kind) -> u32 {
    match kind {
        Kind::Alert | Kind::Offline | Kind::Back => 12 * 3600,
        Kind::Battery | Kind::Game => 6 * 3600,
        Kind::Test => 600,
    }
}

/// Whether the phone's push service should wake it at once (`high`) or may wait (`normal`).
pub fn urgency(kind: Kind) -> &'static str {
    match kind {
        Kind::Alert | Kind::Offline | Kind::Test => "high",
        Kind::Battery | Kind::Game | Kind::Back => "normal",
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::remote::relay::proto::{b64url, unhex};

    fn bytes(h: &str) -> Vec<u8> {
        unhex(h).unwrap()
    }

    /// RFC 8291's own example (its appendix A, as Mozilla's `ece` crate checks it).
    #[test]
    fn sealed_as_rfc_8291_seals_its_example() {
        let pc = SecretKey::from_slice(&bytes(
            "c9f58f89813e9f8e872e71f42aa64e1757c9254dcc62b72ddc010bb4043ea11c",
        ))
        .unwrap();
        let ua = PublicKey::from_sec1_bytes(&bytes(
            "042571b2becdfde360551aaf1ed0f4cd366c11cebe555f89bcb7b186a53339173168ece2ebe018597bd30479b86e3c8f8eced577ca59187e9246990db682008b0e",
        ))
        .unwrap();
        let auth: [u8; 16] = bytes("05305932a1c7eabe13b6cec9fda48882")
            .try_into()
            .unwrap();
        let salt: [u8; 16] = bytes("0c6bfaadad67958803092d454676f397")
            .try_into()
            .unwrap();
        let body = seal_with(
            &pc,
            &ua,
            &auth,
            &salt,
            b"When I grow up, I want to be a watermelon",
        )
        .unwrap();
        assert_eq!(
            crate::remote::auth::hex(&body),
            "0c6bfaadad67958803092d454676f397000010004104fe33f4ab0dea71914db55823f73b54948f41306d920732dbb9a59a53286482200e597a7b7bc260ba1c227998580992e93973002f3012a28ae8f06bbb78e5ec0ff297de5b429bba7153d3a4ae0caa091fd425f3b4b5414add8ab37a19c1bbb05cf5cb5b2a2e0562d558635641ec52812c6c8ff42e95ccb86be7cd"
        );
    }

    fn browser() -> (SecretKey, Push) {
        let ua = SecretKey::from_slice(&[0x5a; 32]).unwrap();
        let key = ua.public_key().to_encoded_point(false);
        let push = Push::new(
            "https://fcm.googleapis.com/fcm/send/abc",
            &b64url(key.as_bytes()),
            &b64url(&[9; 16]),
            PushKinds::default(),
        )
        .unwrap();
        (ua, push)
    }

    /// What a browser does with a body: the same steps from its side.
    pub(crate) fn open(ua: &SecretKey, auth: &[u8; 16], body: &[u8]) -> Vec<u8> {
        let (salt, rest) = body.split_at(16);
        assert_eq!(&rest[..4], &RS.to_be_bytes());
        assert_eq!(rest[4], 65);
        let pc = PublicKey::from_sec1_bytes(&rest[5..70]).unwrap();
        let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), pc.as_affine());
        let mut info = b"WebPush: info\0".to_vec();
        info.extend_from_slice(ua.public_key().to_encoded_point(false).as_bytes());
        info.extend_from_slice(&rest[5..70]);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes())
            .expand(&info, &mut ikm)
            .unwrap();
        let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
            .unwrap();
        hk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
        let mut plain = Aes128Gcm::new_from_slice(&cek)
            .unwrap()
            .decrypt(Nonce::from_slice(&nonce), &rest[70..])
            .unwrap();
        assert_eq!(plain.pop(), Some(2), "one record, the last");
        plain
    }

    #[test]
    fn each_notification_has_its_own_key_and_salt_and_opens_on_the_phone() {
        let (ua, push) = browser();
        let note = Note::new(
            Kind::Alert,
            "CPU at 97 °C",
            "DESKTOP · held for 30 s",
            "cpu-temp",
            5,
        );
        let a = push.seal(&note.payload("ab")).unwrap();
        let b = push.seal(&note.payload("ab")).unwrap();
        assert_ne!(a[..16], b[..16], "a new salt each time");
        assert_ne!(a[21..86], b[21..86], "a new key each time");
        let plain: serde_json::Value = serde_json::from_slice(&open(&ua, &[9; 16], &a)).unwrap();
        assert_eq!(
            plain,
            serde_json::json!({"v": 1, "pc": "ab", "kind": "alert", "title": "CPU at 97 °C",
                "body": "DESKTOP · held for 30 s", "tag": "cpu-temp", "ts": 5})
        );
    }

    #[test]
    fn a_notification_fits_what_push_services_take() {
        let (ua, push) = browser();
        let long = "é".repeat(5000);
        let note = Note::new(Kind::Game, &long, &long, &long, 1);
        assert_eq!(note.title.chars().count(), MAX_TITLE + 1);
        assert!(note.title.ends_with('…'));
        let body = push.seal(&note.payload(&"f".repeat(32))).unwrap();
        assert!(body.len() <= 4096);
        assert!(!open(&ua, &[9; 16], &body).is_empty());
        assert!(push
            .seal(&[b'x'; MAX_PLAIN])
            .is_some_and(|b| b.len() == 4096));
        assert!(push.seal(&[b'x'; MAX_PLAIN + 1]).is_none());
    }

    #[test]
    fn subscriptions_are_checked() {
        let (_, good) = browser();
        let new = |endpoint: &str, key: &str, auth: &str| {
            Push::new(endpoint, key, auth, PushKinds::default())
        };
        assert!(new("http://127.0.0.1:9000/p", &good.p256dh, &good.auth).is_ok());
        for bad in [
            "http://push.example.com/p",
            "ftp://x",
            "https://a b",
            "javascript:1",
        ] {
            assert!(new(bad, &good.p256dh, &good.auth).is_err(), "{bad}");
        }
        assert!(new(
            &format!("https://{}", "a".repeat(MAX_ENDPOINT)),
            &good.p256dh,
            &good.auth
        )
        .is_err());
        assert!(
            new(&good.endpoint, &b64url(&[4; 65]), &good.auth).is_err(),
            "not on the curve"
        );
        assert!(new(&good.endpoint, &good.p256dh, &b64url(&[1; 15])).is_err());
        assert!(new(&good.endpoint, "%%", &good.auth).is_err());
    }

    #[test]
    fn batteries_and_games_read_well() {
        use crate::devices::{Battery, BatterySource};
        let mut d: DeviceInfo = serde_json::from_value(serde_json::json!({
            "id": "abc", "name": "G502", "kind": "mouse", "manufacturer": null,
            "connection": "receiver", "wireless": true, "connected": true, "state": "connected",
            "battery": null, "low": null, "problem": null, "parent": null, "control": null
        }))
        .unwrap();
        assert_eq!(Note::battery(&d, 1), None);
        d.low = Some(LowBattery::Low);
        d.battery = Some(Battery {
            percent: Some(15),
            level: None,
            charging: Some(false),
            source: BatterySource::Logitech,
        });
        let n = Note::battery(&d, 1).unwrap();
        assert_eq!(
            (n.title.as_str(), n.body.as_str(), n.tag.as_str()),
            (
                "G502's battery is low",
                "15 % left. Charge it soon.",
                "battery-abc"
            )
        );
        d.low = Some(LowBattery::Critical);
        assert_eq!(
            Note::battery(&d, 1).unwrap().title,
            "G502 is nearly out of battery"
        );

        let mut g = GameSession {
            app: "Cyberpunk 2077".into(),
            started: 77,
            secs: 4 * 60,
            avg_fps: Some(97.6),
            low1_fps: Some(70.9),
            cpu_temp_max: Some(78.0),
            gpu_temp_max: Some(71.4),
            ..GameSession::default()
        };
        assert_eq!(Note::game(&g, TempUnit::C, 1), None, "too short");
        g.secs = 72 * 60 + 30;
        let n = Note::game(&g, TempUnit::C, 1).unwrap();
        assert_eq!(n.title, "Cyberpunk 2077 ended");
        assert_eq!(
            n.body,
            "1 h 12 min · 98 fps average, 1 % low 71 · CPU up to 78 °C · GPU up to 71 °C"
        );
        assert_eq!(n.tag, "game-77");
        assert!(Note::game(&g, TempUnit::F, 1)
            .unwrap()
            .body
            .ends_with("GPU up to 160 °F"));
        g.avg_fps = None;
        g.cpu_temp_max = None;
        assert_eq!(
            Note::game(&g, TempUnit::C, 1).unwrap().body,
            "1 h 12 min · GPU up to 71 °C"
        );
    }

    #[test]
    fn kinds_pick_the_phones_and_the_relays_notes_read_well() {
        let k = PushKinds::default();
        assert!(Kind::Alert.wanted(&k) && Kind::Battery.wanted(&k) && !Kind::Game.wanted(&k));
        let none = PushKinds {
            alerts: false,
            batteries: false,
            games: false,
            offline: false,
        };
        assert!(Kind::Test.wanted(&none) && !Kind::Back.wanted(&none));
        assert_eq!(Note::offline("DESK").title, "DESK stopped answering");
        assert_eq!(
            Note::back("DESK", 40, 9).body,
            "It answers again, after about a minute without a word."
        );
        assert_eq!(
            Note::back("DESK", 25 * 60, 9).body,
            "It answers again, after about 25 minutes without a word."
        );
        assert_eq!(
            Note::back("DESK", 3 * 3600, 9).body,
            "It answers again, after about 3 hours without a word."
        );
    }
}

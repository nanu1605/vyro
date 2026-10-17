//! End-to-end sealing between the PC and one phone (1.4).
//!
//! Each phone has its own 32-byte secret. HKDF-SHA256 (salt [`SALT`]) turns it into two keys:
//! - `auth`: the phone shows it to the relay, which keeps only its SHA-256 hash;
//! - `seal`: AES-256-GCM for every message between the PC and that phone. The relay never has it.
//!
//! A sealed frame is `version | nonce (12) | ciphertext and tag (16)`. The associated data names
//! the room, the phone and the direction, so a frame cannot be replayed to another phone or
//! reflected back to its sender. `vectors.json` holds the same cases for the phone's WebCrypto
//! code (`src/phone/seal.ts`), so both sides are checked against the same bytes.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::remote::auth::hex;

pub const SALT: &[u8] = b"vyro-relay-1";
pub const VERSION: u8 = 1;
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;
/// Bytes a seal adds to a message.
pub const OVERHEAD: usize = 1 + NONCE_LEN + TAG_LEN;
pub const SECRET_LEN: usize = 32;
pub const ROOM_LEN: usize = 16;
pub const PHONE_LEN: usize = 4;
const AAD_TAG: &[u8] = b"vyro1";
pub const AAD_LEN: usize = AAD_TAG.len() + ROOM_LEN + PHONE_LEN + 1;

pub type Secret = [u8; SECRET_LEN];
/// A PC's room on the relay.
pub type Room = [u8; ROOM_LEN];
pub type PhoneId = [u8; PHONE_LEN];

/// Who a frame is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    ToPhone,
    ToPc,
}

/// What a frame is bound to: the room, the phone and the direction.
pub fn aad(room: &Room, phone: &PhoneId, dir: Dir) -> [u8; AAD_LEN] {
    let mut a = [0u8; AAD_LEN];
    let (tag, rest) = a.split_at_mut(AAD_TAG.len());
    tag.copy_from_slice(AAD_TAG);
    let (r, rest) = rest.split_at_mut(ROOM_LEN);
    r.copy_from_slice(room);
    let (p, d) = rest.split_at_mut(PHONE_LEN);
    p.copy_from_slice(phone);
    d[0] = match dir {
        Dir::ToPhone => 0,
        Dir::ToPc => 1,
    };
    a
}

/// One phone's two keys.
#[derive(Clone, PartialEq, Eq)]
pub struct Keys {
    /// Shown to the relay.
    pub auth: [u8; 32],
    seal: [u8; 32],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(..)")
    }
}

fn expand(hk: &Hkdf<Sha256>, info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    // 32 bytes is far below HKDF-SHA256's limit (8160), so this cannot fail.
    let _ = hk.expand(info, &mut out);
    out
}

impl Keys {
    pub fn new(secret: &Secret) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(SALT), secret);
        Self {
            auth: expand(&hk, b"auth"),
            seal: expand(&hk, b"seal"),
        }
    }

    /// SHA-256 of the auth key, hex: what the relay keeps for this phone.
    pub fn auth_hash(&self) -> String {
        hex(&Sha256::digest(self.auth))
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(&self.seal.into())
    }

    /// Seals `plain` with a fresh `nonce` (12 random bytes; never reuse one with a key).
    pub fn seal(&self, aad: &[u8], nonce: [u8; NONCE_LEN], plain: &[u8]) -> Option<Vec<u8>> {
        let sealed = self
            .cipher()
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad })
            .ok()?;
        let mut frame = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
        frame.push(VERSION);
        frame.extend_from_slice(&nonce);
        frame.extend_from_slice(&sealed);
        Some(frame)
    }

    /// The message in a frame, or `None` when it was not sealed with this key for this place
    /// and direction, or was changed on the way.
    pub fn open(&self, aad: &[u8], frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < OVERHEAD || frame[0] != VERSION {
            return None;
        }
        let (nonce, sealed) = frame[1..].split_at(NONCE_LEN);
        self.cipher()
            .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad })
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Case {
        secret: String,
        room: String,
        phone: String,
        dir: String,
        nonce: String,
        plain: String,
        auth: String,
        auth_hash: String,
        seal: String,
        aad: String,
        frame: String,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Refused {
        why: String,
        /// The case whose keys try to open it.
        case: usize,
        room: String,
        phone: String,
        dir: String,
        frame: String,
    }

    #[derive(Serialize, Deserialize)]
    struct Vectors {
        about: String,
        salt: String,
        cases: Vec<Case>,
        refused: Vec<Refused>,
    }

    const FILE: &str = "src/remote/relay/vectors.json";

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn arr<const N: usize>(s: &str) -> [u8; N] {
        unhex(s).try_into().unwrap()
    }

    fn dir_of(s: &str) -> Dir {
        match s {
            "toPhone" => Dir::ToPhone,
            "toPc" => Dir::ToPc,
            _ => panic!("{s}"),
        }
    }

    fn dir_name(d: Dir) -> &'static str {
        match d {
            Dir::ToPhone => "toPhone",
            Dir::ToPc => "toPc",
        }
    }

    /// The cases, computed by this code. `vectors.json` was written from them once and checked
    /// by an independent implementation (OpenSSL, through Node's `crypto`); it is compared here.
    fn computed() -> Vectors {
        let inputs: [(&str, &str, &str, Dir, &str, &str); 3] = [
            (
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
                "00112233445566778899aabbccddeeff",
                "0a1b2c3d",
                Dir::ToPhone,
                "a0a1a2a3a4a5a6a7a8a9aaab",
                r#"{"t":"view","view":{"ts":1760000000,"cpu":{"usage":37.4,"temp":92}}}"#,
            ),
            (
                "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100",
                "0f0e0d0c0b0a09080706050403020100",
                "deadbeef",
                Dir::ToPc,
                "000000000000000000000001",
                r#"{"t":"hello","name":"iPhone · Safari"}"#,
            ),
            (
                "5555555555555555555555555555555555555555555555555555555555555555",
                "ffffffffffffffffffffffffffffffff",
                "00000000",
                Dir::ToPhone,
                "ffffffffffffffffffffffff",
                "",
            ),
        ];
        let cases: Vec<Case> = inputs
            .iter()
            .map(|&(secret, room, phone, dir, nonce, plain)| {
                let keys = Keys::new(&arr(secret));
                let a = aad(&arr(room), &arr(phone), dir);
                let frame = keys.seal(&a, arr(nonce), plain.as_bytes()).unwrap();
                Case {
                    secret: secret.into(),
                    room: room.into(),
                    phone: phone.into(),
                    dir: dir_name(dir).into(),
                    nonce: nonce.into(),
                    plain: plain.into(),
                    auth: hex(&keys.auth),
                    auth_hash: keys.auth_hash(),
                    seal: hex(&keys.seal),
                    aad: hex(&a),
                    frame: hex(&frame),
                }
            })
            .collect();
        let first = &cases[0];
        let frame = unhex(&first.frame);
        let mut flipped = frame.clone();
        flipped[1 + NONCE_LEN] ^= 1;
        let mut tag = frame.clone();
        *tag.last_mut().unwrap() ^= 0x80;
        let mut version = frame.clone();
        version[0] = 2;
        let same = |why: &str, frame: &[u8]| Refused {
            why: why.into(),
            case: 0,
            room: first.room.clone(),
            phone: first.phone.clone(),
            dir: first.dir.clone(),
            frame: hex(frame),
        };
        let refused = vec![
            same("a bit of the message changed", &flipped),
            same("a bit of the tag changed", &tag),
            same("an unknown version", &version),
            same("too short to be a frame", &frame[..OVERHEAD - 1]),
            Refused {
                dir: "toPc".into(),
                ..same("sent back to its sender (the other direction)", &frame)
            },
            Refused {
                phone: "0a1b2c3e".into(),
                ..same("replayed to another phone", &frame)
            },
            Refused {
                room: "00112233445566778899aabbccddeefe".into(),
                ..same("replayed into another room", &frame)
            },
            Refused {
                case: 1,
                ..same("sealed with another phone's key", &frame)
            },
        ];
        Vectors {
            about: "Relay sealing (1.4): checked by crates/vyro-core/src/remote/relay/seal.rs and \
                    src/phone/seal.test.ts. Hex throughout; plain is UTF-8."
                .into(),
            salt: String::from_utf8_lossy(SALT).into(),
            cases,
            refused,
        }
    }

    #[test]
    fn the_vectors_file_matches_this_code() {
        let want = serde_json::to_string_pretty(&computed()).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(FILE);
        if std::env::var_os("VYRO_WRITE_VECTORS").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            have, want,
            "run with VYRO_WRITE_VECTORS=1 to rewrite {FILE}"
        );
    }

    #[test]
    fn every_case_opens_and_every_refused_frame_does_not() {
        let v = computed();
        for c in &v.cases {
            let keys = Keys::new(&arr(&c.secret));
            let a = aad(&arr(&c.room), &arr(&c.phone), dir_of(&c.dir));
            let opened = keys.open(&a, &unhex(&c.frame)).unwrap();
            assert_eq!(opened, c.plain.as_bytes());
            assert_eq!(unhex(&c.frame).len(), c.plain.len() + OVERHEAD);
        }
        for r in &v.refused {
            let keys = Keys::new(&arr(&v.cases[r.case].secret));
            let a = aad(&arr(&r.room), &arr(&r.phone), dir_of(&r.dir));
            assert_eq!(keys.open(&a, &unhex(&r.frame)), None, "{}", r.why);
        }
    }

    #[test]
    fn auth_and_seal_keys_differ_and_only_the_hash_of_auth_is_shown() {
        let keys = Keys::new(&[7; 32]);
        assert_ne!(keys.auth, keys.seal);
        assert_eq!(keys.auth_hash().len(), 64);
        assert!(!keys.auth_hash().contains(&hex(&keys.seal)));
        assert_eq!(format!("{keys:?}"), "Keys(..)");
    }
}

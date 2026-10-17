//! The phones paired through the relay (1.4), and the PC's own place on it. Time and randomness
//! come from the caller, so this is tested as is.
//!
//! Pairing: the owner opens a pairing in the app; its link carries a fresh secret for one phone.
//! The phone's first `hello` brings a secret of its own, which replaces the link's, so the link
//! works once: a photo of the QR code is useless afterwards. A phone stays paired while it is
//! used at least once in the owner's chosen number of days (Settings → Remote).

use serde::{Deserialize, Serialize};

use super::proto::Allowed;
use super::seal::{Keys, PhoneId, Room, Secret, PHONE_LEN, ROOM_LEN, SECRET_LEN};
use super::{vault, PAIR_SECS};
use crate::remote::auth::{clean_name, hex};
use crate::remote::{RemoteDevice, DAY_CHOICES, DEFAULT_DAYS, MAX_DEVICES};

/// `last_seen` is saved again only after this long.
const SEEN_STEP: i64 = 60;

/// A paired phone as saved in `remote.json` (only phones that finished pairing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredPhone {
    pub id: String,
    pub name: String,
    /// Its secret, sealed ([`vault`]).
    pub secret: String,
    pub paired: i64,
    pub expires: i64,
    #[serde(default)]
    pub last_seen: Option<i64>,
}

/// `remote.json`'s `relay`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StoredRelay {
    pub enabled: bool,
    /// The owner's own relay; none: the one built into Vyro.
    pub url: Option<String>,
    /// This PC's room, hex.
    pub room: String,
    /// The key this PC shows the relay, sealed ([`vault`]).
    pub pc_key: String,
    pub phones: Vec<StoredPhone>,
}

#[derive(Clone)]
struct Phone {
    id: PhoneId,
    name: String,
    keys: Keys,
    secret: Secret,
    paired: i64,
    expires: i64,
    last_seen: Option<i64>,
    /// Still on the pairing link's secret.
    pending: bool,
}

/// What a phone's `hello` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hello {
    /// It finished pairing on its own secret: answer `rekeyed`, sealed with these (the link's)
    /// keys, since the phone's socket still uses them; then send the relay the new list.
    Rekeyed(Keys),
    /// A paired phone came back.
    Known,
    /// A pairing link without a secret to replace it, or an unknown phone.
    Refused,
}

/// The phones, and this PC's room and key.
pub struct Phones {
    room: Room,
    pc_key: Secret,
    phones: Vec<Phone>,
    dirty: bool,
    /// Bumped whenever the list the relay holds must change.
    generation: u64,
}

fn arr<const N: usize>(hex_text: &str) -> Option<[u8; N]> {
    super::proto::unhex(hex_text)?.try_into().ok()
}

fn days_or_default(days: u32) -> i64 {
    i64::from(if DAY_CHOICES.contains(&days) {
        days
    } else {
        DEFAULT_DAYS
    }) * 86_400
}

impl Phones {
    /// A new place on the relay: a random room and key.
    pub fn fresh(room: Room, pc_key: Secret) -> Self {
        Self {
            room,
            pc_key,
            phones: Vec::new(),
            dirty: true,
            generation: 1,
        }
    }

    /// From `remote.json`. `None` when the room or key do not open here (another PC or account):
    /// the caller starts afresh. Phones whose secret does not open, or that expired, are dropped.
    pub fn load(stored: &StoredRelay, now: i64) -> Option<Self> {
        let room = arr::<ROOM_LEN>(&stored.room)?;
        let pc_key: Secret = vault::unprotect(&stored.pc_key)?.try_into().ok()?;
        let phones: Vec<Phone> = stored
            .phones
            .iter()
            .filter(|p| p.expires > now)
            .filter_map(|p| {
                let secret: Secret = vault::unprotect(&p.secret)?.try_into().ok()?;
                Some(Phone {
                    id: arr::<PHONE_LEN>(&p.id)?,
                    name: clean_name(&p.name),
                    keys: Keys::new(&secret),
                    secret,
                    paired: p.paired,
                    expires: p.expires,
                    last_seen: p.last_seen,
                    pending: false,
                })
            })
            .take(MAX_DEVICES)
            .collect();
        let dropped = phones.len() != stored.phones.len();
        Some(Self {
            room,
            pc_key,
            phones,
            dirty: dropped,
            generation: 1,
        })
    }

    /// What `remote.json` keeps (`enabled` and `url` are the caller's): phones that finished
    /// pairing, with their secrets sealed.
    pub fn stored(&self, enabled: bool, url: Option<String>) -> StoredRelay {
        StoredRelay {
            enabled,
            url,
            room: hex(&self.room),
            pc_key: vault::protect(&self.pc_key).unwrap_or_default(),
            phones: self
                .phones
                .iter()
                .filter(|p| !p.pending)
                .filter_map(|p| {
                    Some(StoredPhone {
                        id: hex(&p.id),
                        name: p.name.clone(),
                        secret: vault::protect(&p.secret)?,
                        paired: p.paired,
                        expires: p.expires,
                        last_seen: p.last_seen,
                    })
                })
                .collect(),
        }
    }

    pub fn room(&self) -> &Room {
        &self.room
    }

    pub fn pc_key(&self) -> &Secret {
        &self.pc_key
    }

    /// Whether anything worth saving changed since the last call.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.generation += 1;
    }

    /// The phones the relay lets in, the open pairing among them.
    pub fn allowed(&self) -> Vec<Allowed> {
        self.phones
            .iter()
            .map(|p| Allowed {
                id: hex(&p.id),
                hash: p.keys.auth_hash(),
            })
            .collect()
    }

    /// Opens a pairing for one phone (replacing any open one). `id` must not be in use; the
    /// caller draws again when it is ([`Phones::id_free`]).
    pub fn start_pairing(&mut self, now: i64, secret: Secret, id: PhoneId) -> Result<(), String> {
        self.cancel_pairing();
        if self.phones.len() >= MAX_DEVICES {
            return Err(format!(
                "{MAX_DEVICES} phones are paired already. Sign one out first."
            ));
        }
        self.phones.push(Phone {
            id,
            name: String::new(),
            keys: Keys::new(&secret),
            secret,
            paired: now,
            expires: now + PAIR_SECS,
            last_seen: None,
            pending: true,
        });
        self.generation += 1;
        Ok(())
    }

    pub fn id_free(&self, id: &PhoneId) -> bool {
        !self.phones.iter().any(|p| &p.id == id)
    }

    pub fn cancel_pairing(&mut self) {
        let before = self.phones.len();
        self.phones.retain(|p| !p.pending);
        if self.phones.len() != before {
            self.generation += 1;
        }
    }

    /// The open pairing, while it lasts: the phone's id, the link's secret, and when it ends.
    pub fn pairing(&self, now: i64) -> Option<(PhoneId, Secret, i64)> {
        self.phones
            .iter()
            .find(|p| p.pending && p.expires > now)
            .map(|p| (p.id, p.secret, p.expires))
    }

    pub fn keys(&self, id: &PhoneId) -> Option<&Keys> {
        self.phones.iter().find(|p| &p.id == id).map(|p| &p.keys)
    }

    /// A phone that finished pairing.
    pub fn is_paired(&self, id: &PhoneId) -> bool {
        self.phones.iter().any(|p| &p.id == id && !p.pending)
    }

    /// The phone's name and when its pairing ends, for its `info`.
    pub fn about(&self, id: &PhoneId) -> Option<(String, i64)> {
        self.phones
            .iter()
            .find(|p| &p.id == id && !p.pending)
            .map(|p| (p.name.clone(), p.expires))
    }

    /// A phone's `hello`.
    pub fn hello(
        &mut self,
        id: &PhoneId,
        name: &str,
        next: Option<&str>,
        now: i64,
        days: u32,
    ) -> Hello {
        let Some(p) = self.phones.iter_mut().find(|p| &p.id == id) else {
            return Hello::Refused;
        };
        if !p.pending {
            self.seen(id, now, days);
            return Hello::Known;
        }
        let Some(next) = next.and_then(super::proto::secret_from) else {
            return Hello::Refused;
        };
        if p.expires <= now || next == p.secret || next == [0u8; SECRET_LEN] {
            return Hello::Refused;
        }
        let old = std::mem::replace(&mut p.keys, Keys::new(&next));
        p.secret = next;
        p.name = clean_name(name);
        p.paired = now;
        p.expires = now + days_or_default(days);
        p.last_seen = Some(now);
        p.pending = false;
        self.changed();
        Hello::Rekeyed(old)
    }

    /// A paired phone was heard from: its pairing runs another full span from now.
    pub fn seen(&mut self, id: &PhoneId, now: i64, days: u32) {
        let Some(p) = self.phones.iter_mut().find(|p| &p.id == id && !p.pending) else {
            return;
        };
        p.expires = now + days_or_default(days);
        if p.last_seen.is_none_or(|t| now - t >= SEEN_STEP) {
            p.last_seen = Some(now);
            self.dirty = true;
        }
    }

    /// Signs a phone out; `false` when there was no such phone.
    pub fn sign_out(&mut self, id: &str) -> bool {
        let before = self.phones.len();
        self.phones.retain(|p| hex(&p.id) != id);
        let gone = self.phones.len() != before;
        if gone {
            self.changed();
        }
        gone
    }

    pub fn sign_out_all(&mut self) {
        if !self.phones.is_empty() {
            self.phones.clear();
            self.changed();
        }
    }

    /// Drops phones not used within their span, and a pairing nobody claimed; `true` when any
    /// went.
    pub fn prune(&mut self, now: i64) -> bool {
        let before = self.phones.len();
        self.phones.retain(|p| p.expires > now);
        let gone = self.phones.len() != before;
        if gone {
            self.changed();
        }
        gone
    }

    /// Settings' list (paired phones only), `watching`: ids with the app open now.
    pub fn devices(&self, watching: &[String]) -> Vec<RemoteDevice> {
        self.phones
            .iter()
            .filter(|p| !p.pending)
            .map(|p| {
                let id = hex(&p.id);
                RemoteDevice {
                    watching: watching.contains(&id),
                    id,
                    name: p.name.clone(),
                    paired: p.paired,
                    expires: p.expires,
                    last_seen: p.last_seen,
                    last_address: None,
                    anywhere: true,
                }
            })
            .collect()
    }
}

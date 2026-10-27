//! The phones paired through the relay (1.4), and the PC's own place on it. Time and randomness
//! come from the caller, so this is tested as is.
//!
//! Pairing: the owner opens a pairing in the app; its link carries a fresh secret for one phone.
//! The phone's first `hello` brings a secret of its own, which replaces the link's, so the link
//! works once: a photo of the QR code is useless afterwards. A phone stays paired while it is
//! used at least once in the owner's chosen number of days (Settings → Remote).
//!
//! Each phone may only look, or also control the PC (1.5): the owner chooses when pairing and
//! can change it later, on the PC only. A phone may also give the PC its push subscription
//! (2.0), so it hears about alerts with its app closed; only the phone turns that on or off.

use serde::{Deserialize, Serialize};

use super::proto::Allowed;
use super::push::{Kind, Push};
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
    /// May control the PC (1.5); phones paired before may only look.
    #[serde(default)]
    pub control: bool,
    /// May see and use the PC's screen (1.7); off until the owner turns it on.
    #[serde(default)]
    pub screen: bool,
    /// Its push subscription and what it wants to hear about (2.0), sealed ([`vault`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<String>,
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
    /// "Wake from phone" (1.5): phones are told this PC's wired address.
    pub wake: bool,
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
    control: bool,
    screen: bool,
    push: Option<Push>,
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
    /// Phones whose `info` changed (their permission): tell them while they are here.
    info_due: Vec<PhoneId>,
    /// Bumped whenever a phone's push subscription changes: the relay's offline notification
    /// is sealed again.
    push_generation: u64,
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
            info_due: Vec::new(),
            push_generation: 1,
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
                    control: p.control,
                    screen: p.screen,
                    push: p
                        .push
                        .as_deref()
                        .and_then(vault::unprotect)
                        .and_then(|b| serde_json::from_slice(&b).ok()),
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
            info_due: Vec::new(),
            push_generation: 1,
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
                        control: p.control,
                        screen: p.screen,
                        push: p
                            .push
                            .as_ref()
                            .and_then(|x| vault::protect(&serde_json::to_vec(x).ok()?)),
                    })
                })
                .collect(),
            wake: false,
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

    /// The phones the relay lets in changed; with them, perhaps who hears that the PC went
    /// quiet.
    fn changed(&mut self) {
        self.dirty = true;
        self.generation += 1;
        self.push_generation += 1;
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
            control: false,
            screen: false,
            push: None,
        });
        self.generation += 1;
        Ok(())
    }

    /// Lets a phone (or the open pairing) control the PC, or only look; `false` when there is
    /// no such phone. Only the owner, on the PC, calls this.
    pub fn set_control(&mut self, id: &str, on: bool) -> bool {
        let Some(p) = self.phones.iter_mut().find(|p| hex(&p.id) == id) else {
            return false;
        };
        if p.control != on {
            p.control = on;
            self.dirty = true;
            if !p.pending && !self.info_due.contains(&p.id) {
                self.info_due.push(p.id);
            }
        }
        true
    }

    /// May the phone control the PC.
    pub fn may_control(&self, id: &PhoneId) -> bool {
        self.phones
            .iter()
            .any(|p| &p.id == id && !p.pending && p.control)
    }

    /// Lets a phone see and use the PC's screen, or not; `false` when there is no such phone.
    /// Only the owner, on the PC, calls this.
    pub fn set_screen(&mut self, id: &str, on: bool) -> bool {
        let Some(p) = self.phones.iter_mut().find(|p| hex(&p.id) == id) else {
            return false;
        };
        if p.screen != on {
            p.screen = on;
            self.dirty = true;
            if !p.pending && !self.info_due.contains(&p.id) {
                self.info_due.push(p.id);
            }
        }
        true
    }

    /// May the phone see and use the PC's screen.
    pub fn may_screen(&self, id: &PhoneId) -> bool {
        self.phones
            .iter()
            .any(|p| &p.id == id && !p.pending && p.screen)
    }

    /// Gives a phone's push subscription, or takes it away (`None`); `false` when there is no
    /// such paired phone. Only the phone itself asks for this, or the relay when its push
    /// service says the subscription is gone.
    pub fn set_push(&mut self, id: &PhoneId, push: Option<Push>) -> bool {
        let Some(p) = self.phones.iter_mut().find(|p| &p.id == id && !p.pending) else {
            return false;
        };
        if p.push != push {
            p.push = push;
            self.dirty = true;
            self.push_generation += 1;
        }
        true
    }

    pub fn push(&self, id: &PhoneId) -> Option<&Push> {
        self.phones
            .iter()
            .find(|p| &p.id == id && !p.pending)
            .and_then(|p| p.push.as_ref())
    }

    /// The phones that want to hear about `kind`, with their subscriptions.
    pub fn pushes(&self, kind: Kind) -> Vec<(PhoneId, &Push)> {
        self.phones
            .iter()
            .filter(|p| !p.pending)
            .filter_map(|p| p.push.as_ref().map(|x| (p.id, x)))
            .filter(|(_, x)| kind.wanted(&x.kinds))
            .collect()
    }

    pub fn push_generation(&self) -> u64 {
        self.push_generation
    }

    /// Phones to send their `info` again, since it changed.
    pub fn take_info_due(&mut self) -> Vec<PhoneId> {
        std::mem::take(&mut self.info_due)
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
                    control: p.control,
                    screen: p.screen,
                    push: p.push.is_some(),
                }
            })
            .collect()
    }
}

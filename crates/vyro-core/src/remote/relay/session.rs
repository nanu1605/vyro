//! One connection's conversation with the relay (1.4), without I/O: what comes in, what goes
//! out. The client ([`super::client`]) moves the bytes; this decides.

use std::collections::BTreeSet;

use super::phones::{Hello, Phones};
use super::proto::{self, FromPhone, FromRelay};
use super::seal::{aad, Dir, Keys, PhoneId, PHONE_LEN};
use crate::remote::auth::hex;
use crate::remote::{random, RemoteInfo};

/// What to send the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Text(String),
    Binary(Vec<u8>),
}

/// What the app should hear about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    /// The phones changed in a way worth saving.
    pub saved: bool,
    /// The number of phones watching changed.
    pub watching: bool,
}

/// The state of one connection: which phones are here and which watch.
#[derive(Debug, Default)]
pub struct Session {
    /// The phone list last sent to the relay.
    sent: Option<u64>,
    here: BTreeSet<String>,
    watching: BTreeSet<String>,
    /// The view last sent to the watching phones.
    view: u64,
}

/// What a phone may be told now.
pub struct Now<'a> {
    pub at: i64,
    pub days: u32,
    /// PC name, version, theme, accent and unit.
    pub info: &'a dyn Fn() -> RemoteInfo,
    /// The latest view in JSON, if any.
    pub view: Option<&'a str>,
}

fn seal_to(room: &[u8; 16], id: &PhoneId, keys: &Keys, msg: &str) -> Option<Out> {
    let nonce = random::<12>()?;
    let sealed = keys.seal(&aad(room, id, Dir::ToPhone), nonce, msg.as_bytes())?;
    let mut out = Vec::with_capacity(PHONE_LEN + sealed.len());
    out.extend_from_slice(id);
    out.extend_from_slice(&sealed);
    Some(Out::Binary(out))
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ids of the phones watching.
    pub fn watching(&self) -> Vec<String> {
        self.watching.iter().cloned().collect()
    }

    /// The phone list, when the relay does not have the latest.
    pub fn sync(&mut self, phones: &Phones) -> Option<Out> {
        (self.sent != Some(phones.generation())).then(|| {
            self.sent = Some(phones.generation());
            Out::Text(proto::phones_message(&phones.allowed()))
        })
    }

    /// A text message from the relay.
    pub fn on_text(&mut self, text: &str) -> Effects {
        let Ok(FromRelay::Here { phones }) = serde_json::from_str::<FromRelay>(text) else {
            return Effects::default();
        };
        self.here = phones.into_iter().collect();
        let before = self.watching.len();
        let here = &self.here;
        self.watching.retain(|id| here.contains(id));
        Effects {
            saved: false,
            watching: self.watching.len() != before,
        }
    }

    /// A frame from a phone: `[phone id][sealed]`.
    pub fn on_frame(
        &mut self,
        phones: &mut Phones,
        frame: &[u8],
        now: &Now<'_>,
    ) -> (Vec<Out>, Effects) {
        let mut fx = Effects::default();
        let Some((id, sealed)) = frame.split_first_chunk::<PHONE_LEN>() else {
            return (Vec::new(), fx);
        };
        let id = *id;
        let room = *phones.room();
        let Some(keys) = phones.keys(&id).cloned() else {
            return (Vec::new(), fx);
        };
        let Some(plain) = keys.open(&aad(&room, &id, Dir::ToPc), sealed) else {
            log::debug!("relay: a frame from phone {} did not open", hex(&id));
            return (Vec::new(), fx);
        };
        let Ok(msg) = serde_json::from_slice::<FromPhone>(&plain) else {
            return (Vec::new(), fx);
        };
        let name = hex(&id);
        let mut out = Vec::new();
        match msg {
            FromPhone::Hello { name: shown, next } => {
                match phones.hello(&id, &shown, next.as_deref(), now.at, now.days) {
                    Hello::Rekeyed(old) => {
                        log::info!(
                            "relay: paired \"{}\"",
                            phones.about(&id).map(|a| a.0).unwrap_or_default()
                        );
                        out.extend(seal_to(&room, &id, &old, &proto::rekeyed_message()));
                        out.extend(self.sync(phones));
                        fx.saved = true;
                    }
                    Hello::Known => {
                        fx.saved = phones.take_dirty();
                        out.extend(self.info(phones, &id, &keys, now));
                    }
                    Hello::Refused => {}
                }
            }
            FromPhone::Watch if phones.is_paired(&id) => {
                phones.seen(&id, now.at, now.days);
                fx.watching = self.watching.insert(name);
                if let Some(view) = now.view {
                    out.extend(seal_to(&room, &id, &keys, &proto::view_message(view)));
                }
            }
            FromPhone::Unwatch => fx.watching = self.watching.remove(&name),
            FromPhone::Bye if phones.is_paired(&id) => {
                phones.sign_out(&name);
                log::info!("relay: a phone signed itself out");
                fx.watching = self.watching.remove(&name);
                fx.saved = true;
                out.extend(self.sync(phones));
            }
            _ => {}
        }
        if !fx.saved {
            fx.saved = phones.take_dirty();
        }
        (out, fx)
    }

    fn info(&self, phones: &Phones, id: &PhoneId, keys: &Keys, now: &Now<'_>) -> Option<Out> {
        let (device, expires) = phones.about(id)?;
        let info = RemoteInfo {
            device,
            expires,
            ..(now.info)()
        };
        seal_to(phones.room(), id, keys, &proto::info_message(&info))
    }

    /// A new view (`seq` counts them) for the phones watching.
    pub fn views(&mut self, phones: &Phones, seq: u64, json: &str) -> Vec<Out> {
        if seq == self.view {
            return Vec::new();
        }
        self.view = seq;
        let msg = proto::view_message(json);
        self.watching
            .iter()
            .filter_map(|name| {
                let id: PhoneId = proto::unhex(name)?.try_into().ok()?;
                seal_to(phones.room(), &id, phones.keys(&id)?, &msg)
            })
            .collect()
    }

    /// Phones signed out while connected stop watching at once.
    pub fn forget_gone(&mut self, phones: &Phones) -> bool {
        let before = self.watching.len();
        self.watching.retain(|name| {
            proto::unhex(name)
                .and_then(|b| PhoneId::try_from(b).ok())
                .is_some_and(|id| phones.is_paired(&id))
        });
        self.watching.len() != before
    }
}

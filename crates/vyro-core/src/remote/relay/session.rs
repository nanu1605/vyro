//! One connection's conversation with the relay (1.4), without I/O: what comes in, what goes
//! out. The client ([`super::client`]) moves the bytes; this decides. A phone's requests (1.5)
//! are checked here (its connection's session and count, the request's access) and handed to
//! the app; the app's answers come back through [`Session::answer`].

use std::collections::{BTreeSet, HashMap};

use super::api::{self, Access};
use super::phones::{Hello, Phones};
use super::proto::{self, b64url, FromPhone, FromRelay, Stamp};
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

/// A request let through, for the app to answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The phone's id, hex.
    pub phone: String,
    /// The phone's name, for the record of what phones did.
    pub name: String,
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

/// A phone's connection on protocol 2: its session, and the highest count it used.
#[derive(Debug, Clone)]
struct Conn {
    session: [u8; 16],
    last: Option<u64>,
}

/// The state of one connection: which phones are here and which watch.
#[derive(Debug, Default)]
pub struct Session {
    /// The phone list last sent to the relay.
    sent: Option<u64>,
    /// The phone list the relay last confirmed it has.
    listed: Option<u64>,
    here: BTreeSet<String>,
    watching: BTreeSet<String>,
    /// The view last sent to the watching phones.
    view: u64,
    /// Phones on protocol 2, by their latest `hello`.
    conns: HashMap<PhoneId, Conn>,
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

    /// The phone list the relay has (its generation, [`Phones::generation`]).
    pub fn listed(&self) -> Option<u64> {
        self.listed
    }

    /// A text message from the relay.
    pub fn on_text(&mut self, text: &str) -> Effects {
        let Ok(FromRelay::Here { phones, listed }) = serde_json::from_str::<FromRelay>(text) else {
            return Effects::default();
        };
        // The relay answers lists in the order they were sent, and its answer to the latest one
        // comes last, so `listed` may lag a list just sent, never run ahead of it.
        if listed {
            self.listed = self.sent;
        }
        self.here = phones.into_iter().collect();
        let before = self.watching.len();
        let here = &self.here;
        self.watching.retain(|id| here.contains(id));
        // A phone that left starts a new session when it comes back.
        self.conns.retain(|id, _| here.contains(&hex(id)));
        Effects {
            saved: false,
            watching: self.watching.len() != before,
        }
    }

    /// A frame from a phone: `[phone id][sealed]`. Gives what to send, what changed, and the
    /// requests for the app to answer.
    pub fn on_frame(
        &mut self,
        phones: &mut Phones,
        frame: &[u8],
        now: &Now<'_>,
    ) -> (Vec<Out>, Effects, Vec<Request>) {
        let mut fx = Effects::default();
        let mut requests = Vec::new();
        let Some((id, sealed)) = frame.split_first_chunk::<PHONE_LEN>() else {
            return (Vec::new(), fx, requests);
        };
        let id = *id;
        let room = *phones.room();
        let Some(keys) = phones.keys(&id).cloned() else {
            return (Vec::new(), fx, requests);
        };
        let Some(plain) = keys.open(&aad(&room, &id, Dir::ToPc), sealed) else {
            log::debug!("relay: a frame from phone {} did not open", hex(&id));
            return (Vec::new(), fx, requests);
        };
        let Ok(msg) = serde_json::from_slice::<FromPhone>(&plain) else {
            return (Vec::new(), fx, requests);
        };
        let name = hex(&id);
        if !matches!(msg, FromPhone::Hello { .. }) && !self.fresh(&id, &plain, &msg) {
            log::debug!("relay: an old or repeated message from phone {name} was dropped");
            return (Vec::new(), fx, requests);
        }
        let mut out = Vec::new();
        match msg {
            FromPhone::Hello {
                name: shown,
                next,
                protocol,
            } => {
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
                        // Each connection of a protocol 2 phone gets a session of its own.
                        match (protocol >= 2).then(random::<16>).flatten() {
                            Some(session) => {
                                self.conns.insert(
                                    id,
                                    Conn {
                                        session,
                                        last: None,
                                    },
                                );
                            }
                            None => {
                                self.conns.remove(&id);
                            }
                        }
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
                self.conns.remove(&id);
                out.extend(self.sync(phones));
            }
            FromPhone::Req { id: rid, m, p } if phones.is_paired(&id) => {
                let refused = match api::access(&m) {
                    None => Some(api::UNKNOWN),
                    Some(Access::Control) if !phones.may_control(&id) => Some(api::VIEW_ONLY),
                    Some(_) => None,
                };
                match refused {
                    Some(why) => out.extend(self.answer(phones, &name, rid, &Err(why.into()))),
                    None => requests.push(Request {
                        phone: name,
                        name: phones.about(&id).map(|a| a.0).unwrap_or_default(),
                        id: rid,
                        method: m,
                        params: p,
                    }),
                }
            }
            _ => {}
        }
        if !fx.saved {
            fx.saved = phones.take_dirty();
        }
        (out, fx, requests)
    }

    /// Whether a message after `hello` may be taken. A protocol 2 phone's must carry its
    /// connection's session and a count above any it used: the relay cannot replay one. A
    /// request needs protocol 2; a protocol 1 phone's other messages only change what it is
    /// sent.
    fn fresh(&mut self, id: &PhoneId, plain: &[u8], msg: &FromPhone) -> bool {
        let Some(conn) = self.conns.get_mut(id) else {
            return !matches!(msg, FromPhone::Req { .. });
        };
        let stamp: Stamp = serde_json::from_slice(plain).unwrap_or_default();
        let ours = stamp.s.as_deref() == Some(b64url(&conn.session).as_str());
        match stamp.n {
            Some(n) if ours && conn.last.is_none_or(|last| n > last) => {
                conn.last = Some(n);
                true
            }
            _ => false,
        }
    }

    fn info(&self, phones: &Phones, id: &PhoneId, keys: &Keys, now: &Now<'_>) -> Option<Out> {
        let (device, expires) = phones.about(id)?;
        let info = RemoteInfo {
            device,
            expires,
            ..(now.info)()
        };
        let access = self.conns.get(id).map(|c| {
            let control = phones.may_control(id);
            proto::Access {
                session: b64url(&c.session),
                control,
                methods: api::allowed(control),
            }
        });
        seal_to(
            phones.room(),
            id,
            keys,
            &proto::info_message(&info, access.as_ref()),
        )
    }

    /// The app's answer to a request: sealed for the phone that asked, while it is paired.
    pub fn answer(
        &self,
        phones: &Phones,
        phone: &str,
        id: u64,
        result: &Result<serde_json::Value, String>,
    ) -> Vec<Out> {
        let Some(pid) = proto::unhex(phone).and_then(|b| PhoneId::try_from(b).ok()) else {
            return Vec::new();
        };
        let Some(keys) = phones.keys(&pid).filter(|_| phones.is_paired(&pid)) else {
            return Vec::new();
        };
        proto::answer_messages(id, result)
            .iter()
            .filter_map(|m| seal_to(phones.room(), &pid, keys, m))
            .collect()
    }

    /// Phones here whose permission changed get their `info` again.
    pub fn refresh(&mut self, phones: &mut Phones, now: &Now<'_>) -> Vec<Out> {
        phones
            .take_info_due()
            .into_iter()
            .filter(|id| self.conns.contains_key(id))
            .filter_map(|id| {
                let keys = phones.keys(&id)?.clone();
                self.info(phones, &id, &keys, now)
            })
            .collect()
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

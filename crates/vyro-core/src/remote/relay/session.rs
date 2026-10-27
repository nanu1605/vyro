//! One connection's conversation with the relay (1.4), without I/O: what comes in, what goes
//! out. The client ([`super::client`]) moves the bytes; this decides. A phone's requests (1.5)
//! are checked here (its connection's session and count, the request's access) and handed to
//! the app; the app's answers come back through [`Session::answer`]. Notifications (2.0) are
//! the exception: a phone's `push.*` requests are about its own subscription and are answered
//! here, and [`Session::notes`] seals what the app has to tell for each phone that wants it.

use std::collections::{BTreeSet, HashMap};

use super::api::{self, Access};
use super::phones::{Hello, Phones};
use super::proto::{self, b64url, FromPhone, FromRelay, Sealed, Stamp};
use super::push::{self, Kind, Note, Push, PushKinds};
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
    /// The relay told the phones this PC went quiet, at this time (ms since 1970), while it was
    /// away: they hear that it is back.
    pub back: Option<i64>,
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
    /// The relay sends notifications (it said so in its greeting).
    push: bool,
    /// The relay has the offline notifications of this [`Phones::push_generation`].
    will: Option<u64>,
    /// Notifications for phones that asked for a test.
    tests: Vec<PhoneId>,
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
    pub fn on_text(&mut self, store: &mut Phones, text: &str) -> Effects {
        let (phones, listed) = match serde_json::from_str::<FromRelay>(text) {
            Ok(FromRelay::Here {
                phones,
                listed,
                push,
                back,
            }) => {
                self.push |= push;
                if back.is_some() {
                    return Effects {
                        back,
                        ..self.here(phones, listed)
                    };
                }
                (phones, listed)
            }
            Ok(FromRelay::Pushed { to, gone: true }) => {
                // The browser unsubscribed, or the push service forgot it: the phone turns
                // notifications on again the next time it opens this PC.
                let saved = proto::unhex(&to)
                    .and_then(|b| PhoneId::try_from(b).ok())
                    .is_some_and(|id| store.push(&id).is_some() && store.set_push(&id, None));
                if saved {
                    log::info!("relay: a phone's push subscription is gone");
                }
                return Effects {
                    saved: saved && store.take_dirty(),
                    ..Effects::default()
                };
            }
            _ => return Effects::default(),
        };
        self.here(phones, listed)
    }

    fn here(&mut self, phones: Vec<String>, listed: bool) -> Effects {
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
            back: None,
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
                    Some(Access::Screen) if !phones.may_screen(&id) => Some(api::NO_SCREEN),
                    Some(_) => None,
                };
                match refused {
                    Some(why) => out.extend(self.answer(phones, &name, rid, &Err(why.into()))),
                    None if m.starts_with("push.") => {
                        let result = self.push_request(phones, &id, &m, &p);
                        out.extend(self.answer(phones, &name, rid, &result));
                    }
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
            let screen = phones.may_screen(id);
            proto::Access {
                session: b64url(&c.session),
                control,
                screen,
                methods: api::allowed(control, screen),
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

    /// A phone's `push.*` request: its own subscription, answered here.
    fn push_request(
        &mut self,
        phones: &mut Phones,
        id: &PhoneId,
        method: &str,
        p: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let state = |phones: &Phones| {
            let push = phones.push(id);
            serde_json::json!({
                "on": push.is_some(),
                "kinds": push.map(|x| x.kinds).unwrap_or_default(),
                "endpoint": push.map(|x| x.endpoint.as_str()),
                "relay": self.push,
            })
        };
        match method {
            "push.state" => Ok(state(phones)),
            "push.set" => {
                let s = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("");
                let kinds: PushKinds = p
                    .get("kinds")
                    .cloned()
                    .and_then(|k| serde_json::from_value(k).ok())
                    .unwrap_or_default();
                let push = Push::new(s("endpoint"), s("p256dh"), s("auth"), kinds)?;
                phones.set_push(id, Some(push));
                Ok(state(phones))
            }
            "push.off" => {
                phones.set_push(id, None);
                Ok(state(phones))
            }
            "push.test" if !self.push => Err(
                "This relay can't send notifications. Update the relay, or use Vyro's own.".into(),
            ),
            "push.test" if phones.push(id).is_none() => Err("Turn notifications on first.".into()),
            "push.test" => {
                if !self.tests.contains(id) {
                    self.tests.push(*id);
                }
                Ok(state(phones))
            }
            _ => Err(api::UNKNOWN.into()),
        }
    }

    /// Phones that asked for a test notification since the last call: the client seals one
    /// for each ([`Session::notes`]).
    pub fn take_tests(&mut self) -> Vec<PhoneId> {
        std::mem::take(&mut self.tests)
    }

    /// `note` sealed for one phone's browser; `name` is this PC's.
    fn seal_note(room: &str, id: &PhoneId, push: &Push, note: &Note, name: &str) -> Option<Sealed> {
        let note = note.from_pc(name);
        Some(Sealed {
            to: hex(id),
            endpoint: push.endpoint.clone(),
            body: b64url(&push.seal(&note.payload(room))?),
            ttl: push::ttl(note.kind),
        })
    }

    /// The notifications for the phones that want each, sealed for the relay to pass on; `to`,
    /// only that phone (a test). Nothing while the relay does not send them.
    pub fn notes(
        &self,
        phones: &Phones,
        notes: &[Note],
        name: &str,
        to: Option<&PhoneId>,
    ) -> Vec<Out> {
        if !self.push {
            return Vec::new();
        }
        let room = hex(phones.room());
        notes
            .iter()
            .flat_map(|note| {
                phones
                    .pushes(note.kind)
                    .into_iter()
                    .filter(|(id, _)| to.is_none_or(|t| t == id))
                    .filter_map(|(id, push)| Self::seal_note(&room, &id, push, note, name))
                    .map(|sealed| Out::Text(proto::push_message(&sealed, push::urgency(note.kind))))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Whether the relay should be given the offline notifications again: the phones that want
    /// them changed, or this is a new connection.
    pub fn will_due(&self, phones: &Phones) -> bool {
        self.push && self.will != Some(phones.push_generation())
    }

    /// The offline notifications for the relay to keep, sealed now for each phone that wants
    /// them; `name` is this PC's.
    pub fn will(&mut self, phones: &Phones, name: &str) -> Option<Out> {
        if !self.will_due(phones) {
            return None;
        }
        self.will = Some(phones.push_generation());
        let room = hex(phones.room());
        let note = Note::offline(name);
        let list: Vec<Sealed> = phones
            .pushes(Kind::Offline)
            .into_iter()
            .filter_map(|(id, push)| Self::seal_note(&room, &id, push, &note, name))
            .collect();
        Some(Out::Text(proto::will_message(&list)))
    }

    /// Tells the relay the PC is about to go (it quits, sleeps or shuts down), so it does not
    /// tell the phones it went quiet.
    pub fn leaving(&self) -> Option<Out> {
        self.push.then(|| Out::Text(proto::leaving_message()))
    }

    /// The relay sends notifications.
    pub fn pushes(&self) -> bool {
        self.push
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

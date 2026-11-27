//! What the phone wheel sends and hears (2.6), on its own data channel: unordered and never
//! resent, so a late packet is no use and is dropped, never waited for. Little-endian.
//!
//! Phone to PC, 12 bytes:
//! `[1, kind, seq u16, t u16, steer i16, throttle u8, brake u8, buttons u16]`
//! - kind 0: the controller as it is now; kind 1: let everything go (the phone's page closed);
//! - `seq` counts packets (wrapping), so an older one that comes late is dropped;
//! - `t` is the phone's clock (ms, wrapping), echoed back for the delay it shows;
//! - `steer` is -32767 (full left) to 32767; `throttle` and `brake` 0 to 255;
//! - `buttons` are an Xbox controller's ([`buttons`]).
//!
//! PC to phone: `[1, 2, seq u16, t u16]` echoes the newest packet now and then;
//! `[1, 3, large, small]` is the game's rumble, each motor 0 to 255.

/// This layout.
pub const VERSION: u8 = 1;
pub const LEN: usize = 12;

pub mod kind {
    pub const STATE: u8 = 0;
    pub const NEUTRAL: u8 = 1;
    pub const ECHO: u8 = 2;
    pub const RUMBLE: u8 = 3;
}

/// An Xbox controller's buttons, as Windows' XInput numbers them.
pub mod buttons {
    pub const UP: u16 = 0x0001;
    pub const DOWN: u16 = 0x0002;
    pub const LEFT: u16 = 0x0004;
    pub const RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LB: u16 = 0x0100;
    pub const RB: u16 = 0x0200;
    /// Opens Windows' Game Bar: never pressed from a phone.
    pub const GUIDE: u16 = 0x0400;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
    /// Every button a phone may press.
    pub const ALLOWED: u16 = !(GUIDE | 0x0800);
}

/// The controller as the phone holds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pad {
    pub steer: i16,
    pub throttle: u8,
    pub brake: u8,
    pub buttons: u16,
}

/// A packet from the phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packet {
    State { seq: u16, t: u16, pad: Pad },
    Neutral { seq: u16, t: u16 },
}

impl Packet {
    pub fn seq(&self) -> u16 {
        match self {
            Self::State { seq, .. } | Self::Neutral { seq, .. } => *seq,
        }
    }

    pub fn t(&self) -> u16 {
        match self {
            Self::State { t, .. } | Self::Neutral { t, .. } => *t,
        }
    }
}

/// A packet as the phone sent it; Guide (and the unused bit) never pressed, steering within
/// its range.
pub fn parse(b: &[u8]) -> Result<Packet, &'static str> {
    if b.len() != LEN {
        return Err("not a wheel packet's length");
    }
    if b[0] != VERSION {
        return Err("another wheel packet version");
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let (seq, t) = (u16_at(2), u16_at(4));
    match b[1] {
        kind::STATE => Ok(Packet::State {
            seq,
            t,
            pad: Pad {
                steer: i16::from_le_bytes([b[6], b[7]]).max(-i16::MAX),
                throttle: b[8],
                brake: b[9],
                buttons: u16_at(10) & buttons::ALLOWED,
            },
        }),
        kind::NEUTRAL => Ok(Packet::Neutral { seq, t }),
        _ => Err("not a wheel packet's kind"),
    }
}

/// A packet as the phone sends it (its tests, and the example PC's).
pub fn encode(p: &Packet) -> [u8; LEN] {
    let mut b = [0u8; LEN];
    b[0] = VERSION;
    let (k, seq, t, pad) = match *p {
        Packet::State { seq, t, pad } => (kind::STATE, seq, t, pad),
        Packet::Neutral { seq, t } => (kind::NEUTRAL, seq, t, Pad::default()),
    };
    b[1] = k;
    b[2..4].copy_from_slice(&seq.to_le_bytes());
    b[4..6].copy_from_slice(&t.to_le_bytes());
    b[6..8].copy_from_slice(&pad.steer.to_le_bytes());
    b[8] = pad.throttle;
    b[9] = pad.brake;
    b[10..12].copy_from_slice(&pad.buttons.to_le_bytes());
    b
}

/// The newest packet, echoed: the phone times the round trip by it.
pub fn echo(seq: u16, t: u16) -> [u8; 6] {
    let mut b = [VERSION, kind::ECHO, 0, 0, 0, 0];
    b[2..4].copy_from_slice(&seq.to_le_bytes());
    b[4..6].copy_from_slice(&t.to_le_bytes());
    b
}

/// The game's rumble, for the phone to vibrate.
pub fn rumble(large: u8, small: u8) -> [u8; 4] {
    [VERSION, kind::RUMBLE, large, small]
}

/// `a` came after `b`, counting round the wrap: the later of two within half the range.
pub fn newer(a: u16, b: u16) -> bool {
    (a.wrapping_sub(b) as i16) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_phone_sends_and_masks_guide() {
        let pad = Pad {
            steer: -12_345,
            throttle: 200,
            brake: 3,
            buttons: buttons::A | buttons::RB | buttons::GUIDE | 0x0800,
        };
        let b = encode(&Packet::State {
            seq: 7,
            t: 65_000,
            pad,
        });
        let Ok(Packet::State { seq, t, pad: got }) = parse(&b) else {
            panic!("not read");
        };
        assert_eq!((seq, t), (7, 65_000));
        assert_eq!(got.buttons, buttons::A | buttons::RB, "Guide never pressed");
        assert_eq!((got.steer, got.throttle, got.brake), (-12_345, 200, 3));
        // i16::MIN would be one past full left.
        let mut b = b;
        b[6..8].copy_from_slice(&i16::MIN.to_le_bytes());
        let Ok(Packet::State { pad, .. }) = parse(&b) else {
            panic!("not read");
        };
        assert_eq!(pad.steer, -32_767);
        assert_eq!(
            parse(&encode(&Packet::Neutral { seq: 1, t: 2 })),
            Ok(Packet::Neutral { seq: 1, t: 2 })
        );
    }

    #[test]
    fn refuses_what_is_not_a_wheel_packet() {
        let good = encode(&Packet::Neutral { seq: 1, t: 2 });
        assert!(parse(&good[..11]).is_err());
        assert!(parse(&[good.as_slice(), &[0]].concat()).is_err());
        let mut v = good;
        v[0] = 2;
        assert!(parse(&v).is_err());
        let mut k = good;
        k[1] = kind::ECHO;
        assert!(parse(&k).is_err());
        assert!(parse(b"{\"t\":\"here\"}").is_err());
    }

    #[test]
    fn counts_round_the_wrap() {
        assert!(newer(1, 0));
        assert!(!newer(0, 1));
        assert!(!newer(5, 5));
        assert!(newer(0, 65_535), "0 follows 65535");
        assert!(newer(10, 65_530));
        assert!(!newer(65_530, 10));
        assert_eq!(echo(0x0102, 0x0304), [1, 2, 2, 1, 4, 3]);
        assert_eq!(rumble(9, 8), [1, 3, 9, 8]);
    }
}

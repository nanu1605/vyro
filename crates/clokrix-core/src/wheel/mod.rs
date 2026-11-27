//! The phone wheel (2.6; the owner, 2026-11-27: "can we use the mobile phones gyro behaviour and
//! drive any driving games like Euro truck simulator 2 or Snowrunner, etc using a phone just like
//! it's a Moza wheel? Also there should be no lag."). The phone, held like a wheel, sends its
//! steering, pedals and buttons straight to the PC ([`packet`], on a WebRTC data channel that
//! never resends); the PC turns them into an Xbox controller's report, which a virtual controller
//! (ViGEmBus, [`vigem`]) gives the game. Games see a controller as any other.
//!
//! Safe by default: the newest packet wins and a late one is dropped; when packets stop for
//! [`DEAD_MAN`] (Wi-Fi dropped, the phone locked), the pedals and buttons let go at once and the
//! steering eases back to centre, so a truck never keeps driving on its own.

pub mod packet;
#[cfg(windows)]
pub mod vigem;

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use packet::{newer, Packet, Pad};

/// No packet for this long: let go.
pub const DEAD_MAN: Duration = Duration::from_millis(400);
/// The steering then eases back to centre over this long.
pub const RECENTRE: Duration = Duration::from_millis(200);

/// Where the pedals go on the controller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Pedals {
    /// Throttle on the right trigger, brake on the left: as most driving games take them.
    #[default]
    Triggers,
    /// Both on the right stick, up and down: for games that read the triggers as one axis
    /// (DirectInput).
    RightStick,
}

/// How the wheel is set on the PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct WheelOptions {
    /// 0 to 30: a game's own dead zone in the middle of the stick, in percent, made up for, so
    /// the slightest turn of the phone already steers.
    pub anti_deadzone: u8,
    pub pedals: Pedals,
}

impl Default for WheelOptions {
    fn default() -> Self {
        Self {
            anti_deadzone: 0,
            pedals: Pedals::Triggers,
        }
    }
}

impl WheelOptions {
    pub fn clamped(mut self) -> Self {
        self.anti_deadzone = self.anti_deadzone.min(30);
        self
    }
}

/// An Xbox controller's state, as the virtual controller takes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Report {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

/// What takes the reports: the virtual controller, or a recording for the tests.
pub trait PadSink {
    fn update(&mut self, r: &Report) -> Result<(), String>;
}

/// The reports, kept (tests and the example PC).
#[derive(Debug, Default)]
pub struct Recording {
    pub reports: Vec<Report>,
}

impl PadSink for Recording {
    fn update(&mut self, r: &Report) -> Result<(), String> {
        self.reports.push(*r);
        Ok(())
    }
}

/// `v` past a dead zone of `dz` percent at each side of centre: what is left of the stick's
/// travel is spread over the rest, so a game that ignores the middle `dz` still sees the
/// smallest turn.
fn past_deadzone(v: i16, dz: u8) -> i16 {
    if v == 0 || dz == 0 {
        return v;
    }
    let full = f32::from(i16::MAX);
    let dz = f32::from(dz.min(30)) / 100.0;
    let x = f32::from(v) / full;
    let out = x.signum() * (dz + x.abs() * (1.0 - dz));
    (out * full).round().clamp(-full, full) as i16
}

/// The controller as the phone holds it, packet after packet.
#[derive(Debug, Default)]
pub struct Wheel {
    pub options: WheelOptions,
    /// The newest packet taken, and when it came.
    seq: Option<u16>,
    t: u16,
    at: Option<Instant>,
    pad: Pad,
    /// Where the steering was when packets stopped, to ease it back from.
    released: Option<(Instant, i16)>,
}

impl Wheel {
    pub fn new(options: WheelOptions) -> Self {
        Self {
            options: options.clamped(),
            ..Self::default()
        }
    }

    /// Takes a packet that came at `now`; false (and nothing changes) if one sent after it came
    /// first.
    pub fn take(&mut self, p: &Packet, now: Instant) -> bool {
        if self.seq.is_some_and(|s| !newer(p.seq(), s)) {
            return false;
        }
        self.seq = Some(p.seq());
        self.t = p.t();
        self.at = Some(now);
        self.released = None;
        self.pad = match *p {
            Packet::State { pad, .. } => pad,
            Packet::Neutral { .. } => Pad::default(),
        };
        true
    }

    /// The newest packet's number and phone time, to echo.
    pub fn newest(&self) -> Option<(u16, u16)> {
        self.seq.map(|s| (s, self.t))
    }

    /// Packets have stopped: let go now (the session ended).
    pub fn let_go(&mut self, now: Instant) {
        self.at = Some(now - DEAD_MAN);
    }

    /// The report for `now`: the phone's controller while its packets come; once they stop for
    /// [`DEAD_MAN`], no pedals or buttons and the steering easing back to centre.
    pub fn report(&mut self, now: Instant) -> Report {
        let live = self
            .at
            .is_some_and(|t| now.saturating_duration_since(t) < DEAD_MAN);
        let pad = if live {
            self.pad
        } else {
            let (since, from) = *self.released.get_or_insert((now, self.pad.steer));
            let k =
                1.0 - now.saturating_duration_since(since).as_secs_f32() / RECENTRE.as_secs_f32();
            Pad {
                steer: (f32::from(from) * k.clamp(0.0, 1.0)).round() as i16,
                ..Pad::default()
            }
        };
        let mut r = Report {
            buttons: pad.buttons & packet::buttons::ALLOWED,
            thumb_lx: past_deadzone(pad.steer, self.options.anti_deadzone),
            ..Report::default()
        };
        match self.options.pedals {
            Pedals::Triggers => {
                r.right_trigger = pad.throttle;
                r.left_trigger = pad.brake;
            }
            Pedals::RightStick => {
                let v =
                    (i32::from(pad.throttle) - i32::from(pad.brake)) * i32::from(i16::MAX) / 255;
                r.thumb_ry = v as i16;
            }
        }
        r
    }

    /// Packets have stopped and the steering is back at centre: nothing more changes.
    pub fn settled(&self, now: Instant) -> bool {
        self.released
            .is_some_and(|(since, _)| now.saturating_duration_since(since) >= RECENTRE)
    }
}

#[cfg(test)]
mod tests {
    use super::packet::{buttons, Packet, Pad};
    use super::*;

    fn state(seq: u16, steer: i16, throttle: u8, brake: u8, b: u16) -> Packet {
        Packet::State {
            seq,
            t: seq,
            pad: Pad {
                steer,
                throttle,
                brake,
                buttons: b,
            },
        }
    }

    #[test]
    fn the_newest_packet_wins_and_a_late_one_is_dropped() {
        let t0 = Instant::now();
        let mut w = Wheel::new(WheelOptions::default());
        assert!(w.take(&state(10, 1000, 50, 0, buttons::A), t0));
        assert!(
            !w.take(&state(9, -5000, 0, 0, 0), t0),
            "sent before, came after"
        );
        assert!(!w.take(&state(10, -5000, 0, 0, 0), t0), "the same again");
        let r = w.report(t0);
        assert_eq!(
            (r.thumb_lx, r.right_trigger, r.left_trigger, r.buttons),
            (1000, 50, 0, buttons::A)
        );
        assert_eq!(w.newest(), Some((10, 10)));
        // Across the wrap.
        let mut w = Wheel::new(WheelOptions::default());
        assert!(w.take(&state(65_535, 1, 0, 0, 0), t0));
        assert!(w.take(&state(0, 2, 0, 0, 0), t0));
        assert!(!w.take(&state(65_534, 3, 0, 0, 0), t0));
        assert_eq!(w.report(t0).thumb_lx, 2);
    }

    #[test]
    fn packets_stopping_let_go_of_the_pedals_at_once_and_centre_the_steering() {
        let t0 = Instant::now();
        let mut w = Wheel::new(WheelOptions::default());
        w.take(&state(1, 20_000, 255, 0, buttons::B | buttons::RB), t0);
        assert_eq!(w.report(t0 + Duration::from_millis(390)).right_trigger, 255);
        let gone = t0 + DEAD_MAN;
        let r = w.report(gone);
        assert_eq!((r.right_trigger, r.left_trigger, r.buttons), (0, 0, 0));
        assert_eq!(r.thumb_lx, 20_000, "steering eases back, from where it was");
        let half = w.report(gone + RECENTRE / 2).thumb_lx;
        assert!((9_000..=11_000).contains(&half), "{half}");
        assert!(!w.settled(gone + RECENTRE / 2));
        assert_eq!(w.report(gone + RECENTRE).thumb_lx, 0);
        assert!(w.settled(gone + RECENTRE));
        // A packet brings it back at once.
        w.take(&state(2, -100, 10, 0, 0), gone + RECENTRE * 2);
        assert_eq!(w.report(gone + RECENTRE * 2).thumb_lx, -100);
        // The phone's own let-go: all at neutral.
        w.take(&Packet::Neutral { seq: 3, t: 3 }, gone + RECENTRE * 2);
        assert_eq!(w.report(gone + RECENTRE * 2), Report::default());
        // The session's end: let go now.
        w.take(&state(4, 9000, 255, 0, 0), gone + RECENTRE * 3);
        w.let_go(gone + RECENTRE * 3);
        assert_eq!(w.report(gone + RECENTRE * 3).right_trigger, 0);
    }

    #[test]
    fn makes_up_for_a_games_dead_zone_when_asked() {
        let t0 = Instant::now();
        let mut w = Wheel::new(WheelOptions {
            anti_deadzone: 24,
            ..WheelOptions::default()
        });
        w.take(&state(1, 328, 0, 0, 0), t0); // 1% of a turn
        let small = w.report(t0).thumb_lx;
        assert!((8_100..8_300).contains(&small), "{small}: just past 24%");
        w.take(&state(2, -32_767, 0, 0, 0), t0);
        assert_eq!(w.report(t0).thumb_lx, -32_767, "full lock stays full lock");
        w.take(&state(3, 0, 0, 0, 0), t0);
        assert_eq!(w.report(t0).thumb_lx, 0, "centre stays centre");
        assert_eq!(
            WheelOptions {
                anti_deadzone: 90,
                ..WheelOptions::default()
            }
            .clamped()
            .anti_deadzone,
            30
        );
    }

    #[test]
    fn pedals_on_the_triggers_or_the_right_stick() {
        let t0 = Instant::now();
        let mut w = Wheel::new(WheelOptions {
            pedals: Pedals::RightStick,
            ..WheelOptions::default()
        });
        w.take(&state(1, 0, 255, 0, 0), t0);
        let r = w.report(t0);
        assert_eq!(
            (r.thumb_ry, r.right_trigger, r.left_trigger),
            (32_767, 0, 0)
        );
        w.take(&state(2, 0, 0, 255, 0), t0);
        assert_eq!(w.report(t0).thumb_ry, -32_767);
        w.take(&state(3, 0, 255, 255, 0), t0);
        assert_eq!(w.report(t0).thumb_ry, 0);
        let mut rec = Recording::default();
        rec.update(&w.report(t0)).unwrap();
        assert_eq!(rec.reports.len(), 1);
    }
}

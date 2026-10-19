//! The rules of a screen session that need no Windows (1.7): what the PC's banner shows, what the
//! phone is told over the data channel, when an unused session ends, and how much of the slow
//! view through the relay a day allows.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::producer::Report;

/// A session nobody uses this long (no input, no keyframe asked) ends.
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// The slow view through the relay: this many seconds a day (UTC), counted per PC.
pub const SLOW_DAILY_SECS: u32 = 20 * 60;
/// The slow view's pictures are at most this size.
pub const SLOW_MAX: (u32, u32) = (960, 600);
/// The phone asks for the slow view's next picture no sooner than this.
pub const SLOW_INTERVAL: Duration = Duration::from_millis(500);

/// The screen shared now, for the PC's banner and tray.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScreenShare {
    /// The phone's name.
    pub phone: String,
    /// The phone's id (hex).
    pub id: String,
    /// Unix seconds.
    pub since: i64,
    /// Through the relay, a picture at a time (no direct path).
    pub slow: bool,
}

/// The screen's state for the PC's window: the banner and Settings → Remote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScreenStatus {
    /// The screen shared now, if it is.
    pub share: Option<ScreenShare>,
    /// Lock the PC when a session ends (not when it is stopped at the PC).
    pub lock_on_end: bool,
    /// Seconds of the slow view left today.
    pub slow_left: u32,
}

/// A monitor as the phone can choose it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorChoice {
    pub index: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

/// What the PC tells the phone over the data channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Note {
    /// The picture is `w`×`h` (the shape the phone maps touches onto).
    Sending { w: u32, h: u32 },
    /// Windows shows its secure desktop (sign-in, lock screen, an administrator prompt).
    Secure,
    /// The session ended, and why.
    End { why: String },
    /// What went out lately ([`Report`]): sizes as `[w, h]`, rates a second, bits a second.
    Report {
        capture: &'static str,
        from: [u32; 2],
        sent: [u32; 2],
        taken: u32,
        encoded: u32,
        keyframes: u32,
        bitrate: u32,
        luma: u8,
        spread: u8,
        asked: u32,
    },
}

impl Note {
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn report(r: &Report) -> Self {
        Self::Report {
            capture: r.capture,
            from: [r.from.0, r.from.1],
            sent: [r.sent.0, r.sent.1],
            taken: r.taken,
            encoded: r.encoded,
            keyframes: r.keyframes,
            bitrate: r.bitrate,
            luma: r.luma,
            spread: r.spread,
            asked: r.asked,
        }
    }
}

impl std::fmt::Display for Report {
    /// One line for the app's log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}x{} through {}, sent as {}x{}: {} new and {} encoded a second, {} keyframes, \
             {} kbit/s, brightness {} varying {}, {} keyframes asked for",
            self.from.0,
            self.from.1,
            self.capture,
            self.sent.0,
            self.sent.1,
            self.taken,
            self.encoded,
            self.keyframes,
            self.bitrate / 1000,
            self.luma,
            self.spread,
            self.asked
        )
    }
}

/// Whether a session last used `since_use` ago has been left.
pub fn idle(since_use: Duration) -> bool {
    since_use >= IDLE
}

/// How much of today's slow view is spent, kept in `remote.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SlowAllowance {
    /// Days since 1970 (UTC).
    pub day: i64,
    pub used_secs: u32,
}

impl SlowAllowance {
    fn today(&mut self, now: i64) {
        let day = now.div_euclid(86_400);
        if day != self.day {
            *self = Self { day, used_secs: 0 };
        }
    }

    /// Seconds of slow view left today.
    pub fn left(&mut self, now: i64) -> u32 {
        self.today(now);
        SLOW_DAILY_SECS.saturating_sub(self.used_secs)
    }

    /// Spends `secs`; `false` (and nothing spent) when today's allowance is gone.
    pub fn spend(&mut self, now: i64, secs: u32) -> bool {
        if self.left(now) == 0 {
            return false;
        }
        self.used_secs = self.used_secs.saturating_add(secs).min(SLOW_DAILY_SECS);
        true
    }
}

/// Why the slow view was refused when today's allowance is gone.
pub const SLOW_SPENT: &str =
    "Today's 20 minutes of the slow view are used up. A direct connection has no limit.";

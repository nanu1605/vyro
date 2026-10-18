//! What the phone sends over the screen session's data channel: the pointer, buttons, the wheel,
//! text and a few keys (1.7). Positions are fractions of the shared monitor, so the phone never
//! needs its size. Everything is checked here; the app turns it into Windows input.

use serde::Deserialize;

/// Longest text one message types.
pub const MAX_TEXT: usize = 200;
/// Most wheel notches one message scrolls.
pub const MAX_NOTCHES: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// The keys the phone's key bar offers; anything else is typed as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Key {
    Escape,
    Tab,
    Enter,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    /// The Windows key.
    Meta,
    Control,
    Alt,
    Shift,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Input {
    /// The pointer to (x, y), fractions of the monitor from its top left.
    Move {
        x: f64,
        y: f64,
    },
    /// A button pressed or released where the pointer is.
    Button {
        b: Button,
        down: bool,
    },
    /// Wheel notches: positive scrolls down (dy) or right (dx).
    Scroll {
        dx: f64,
        dy: f64,
    },
    /// Text typed as it is.
    Text {
        s: String,
    },
    Key {
        k: Key,
        down: bool,
    },
}

/// Reads and checks one message.
pub fn parse(msg: &str) -> Result<Input, String> {
    let i: Input = serde_json::from_str(msg).map_err(|e| format!("not an input message: {e}"))?;
    match &i {
        Input::Move { x, y } if !(0.0..=1.0).contains(x) || !(0.0..=1.0).contains(y) => {
            Err("position outside the screen".into())
        }
        Input::Scroll { dx, dy }
            if !dx.is_finite()
                || !dy.is_finite()
                || dx.abs() > MAX_NOTCHES
                || dy.abs() > MAX_NOTCHES =>
        {
            Err("too much scrolling at once".into())
        }
        Input::Text { s } if s.chars().count() > MAX_TEXT => Err("too much text at once".into()),
        Input::Text { s } if s.chars().any(|c| c.is_control() && c != '\n' && c != '\t') => {
            Err("control characters are sent as keys".into())
        }
        _ => Ok(i),
    }
}

/// A rectangle of the desktop, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

/// Where (x, y), fractions of `monitor`, lands in the 0–65535 coordinates Windows uses for the
/// whole desktop (`desktop`: every monitor together).
pub fn absolute(x: f64, y: f64, monitor: Rect, desktop: Rect) -> (i32, i32) {
    let px = f64::from(monitor.left) + x.clamp(0.0, 1.0) * f64::from(monitor.width - 1);
    let py = f64::from(monitor.top) + y.clamp(0.0, 1.0) * f64::from(monitor.height - 1);
    let span = |v: f64, start: i32, len: i32| {
        let len = f64::from((len - 1).max(1));
        (((v - f64::from(start)) * 65535.0 / len).round() as i32).clamp(0, 65535)
    };
    (
        span(px, desktop.left, desktop.width),
        span(py, desktop.top, desktop.height),
    )
}

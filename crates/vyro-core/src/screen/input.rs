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
    /// A letter or digit key pressed and released, as a key rather than as text: with Ctrl, Alt
    /// or the Windows key held, a shortcut (Ctrl+C).
    Press {
        c: char,
    },
    /// The phone still shows the screen (sent every 30 s while it does): a session the phone
    /// stopped showing ends after a while.
    Here,
    /// The phone's picture stopped (it lost part of one, or has none yet): send a whole one.
    Picture,
    /// HD or SD from now on (1.7).
    Quality {
        q: super::producer::Quality,
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
        Input::Press { c } if !c.is_ascii_alphanumeric() => {
            Err("only letters and digits are pressed as keys".into())
        }
        _ => Ok(i),
    }
}

/// What a session's input holds down on the PC: keys and buttons pressed and not let go. The
/// phone holds Ctrl, Alt, Shift and Win until its next key, and the left button through a drag;
/// a session that ends between (the phone gone, Stop at the PC, the idle rule) would leave them
/// down for whoever uses the PC next, so its end lets them go.
#[derive(Debug, Default)]
pub struct Held {
    keys: Vec<Key>,
    buttons: Vec<Button>,
    ended: bool,
}

impl Held {
    /// Notes `i` before it is sent; false once the session ended, when it is not sent.
    pub fn note(&mut self, i: &Input) -> bool {
        if self.ended {
            return false;
        }
        match *i {
            Input::Key { k, down } => hold(&mut self.keys, k, down),
            Input::Button { b, down } => hold(&mut self.buttons, b, down),
            _ => {}
        }
        true
    }

    /// The session ended: the input that lets go of what it holds. Later input is not sent.
    pub fn end(&mut self) -> Vec<Input> {
        self.ended = true;
        let buttons = self
            .buttons
            .drain(..)
            .map(|b| Input::Button { b, down: false });
        let keys = self
            .keys
            .drain(..)
            .rev()
            .map(|k| Input::Key { k, down: false });
        buttons.chain(keys).collect()
    }
}

fn hold<T: PartialEq>(held: &mut Vec<T>, x: T, down: bool) {
    held.retain(|h| *h != x);
    if down {
        held.push(x);
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

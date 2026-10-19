//! The phone's input as Windows input (1.7), through `SendInput` in the signed-in user's
//! session. Windows keeps two limits on it: windows of programs running as administrator do not
//! take input from a program that is not, and Ctrl+Alt+Del can never be sent.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
    VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_LWIN, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use super::input::{absolute, Button, Input, Key, Rect};

/// Every monitor together, as Windows lays them out.
fn desktop() -> Rect {
    // SAFETY: plain metric queries.
    unsafe {
        Rect {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        }
    }
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn vk(k: Key) -> (VIRTUAL_KEY, bool) {
    // The second value: an extended key (the arrows and the block above them).
    match k {
        Key::Escape => (VK_ESCAPE, false),
        Key::Tab => (VK_TAB, false),
        Key::Enter => (VK_RETURN, false),
        Key::Backspace => (VK_BACK, false),
        Key::Delete => (VK_DELETE, true),
        Key::ArrowLeft => (VK_LEFT, true),
        Key::ArrowRight => (VK_RIGHT, true),
        Key::ArrowUp => (VK_UP, true),
        Key::ArrowDown => (VK_DOWN, true),
        Key::Home => (VK_HOME, true),
        Key::End => (VK_END, true),
        Key::PageUp => (VK_PRIOR, true),
        Key::PageDown => (VK_NEXT, true),
        Key::Meta => (VK_LWIN, true),
        Key::Control => (VK_CONTROL, false),
        Key::Alt => (VK_MENU, false),
        Key::Shift => (VK_SHIFT, false),
    }
}

/// The Windows input for one phone message on `monitor`.
fn inputs(i: &Input, monitor: Rect, desk: Rect) -> Vec<INPUT> {
    match i {
        Input::Move { x, y } => {
            let (dx, dy) = absolute(*x, *y, monitor, desk);
            vec![mouse(
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                dx,
                dy,
                0,
            )]
        }
        Input::Button { b, down } => {
            let f = match (b, down) {
                (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
                (Button::Left, false) => MOUSEEVENTF_LEFTUP,
                (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
                (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
            };
            vec![mouse(f, 0, 0, 0)]
        }
        Input::Scroll { dx, dy } => {
            let mut v = Vec::new();
            // Positive dy scrolls down: a negative wheel turn.
            if *dy != 0.0 {
                v.push(mouse(MOUSEEVENTF_WHEEL, 0, 0, (-dy * 120.0).round() as i32));
            }
            if *dx != 0.0 {
                v.push(mouse(MOUSEEVENTF_HWHEEL, 0, 0, (dx * 120.0).round() as i32));
            }
            v
        }
        Input::Text { s } => {
            let mut v = Vec::new();
            for c in s.chars() {
                match c {
                    '\n' => {
                        v.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                        v.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
                    }
                    '\t' => {
                        v.push(key(VK_TAB, 0, KEYBD_EVENT_FLAGS(0)));
                        v.push(key(VK_TAB, 0, KEYEVENTF_KEYUP));
                    }
                    _ => {
                        let mut units = [0u16; 2];
                        for u in c.encode_utf16(&mut units) {
                            v.push(key(VIRTUAL_KEY(0), *u, KEYEVENTF_UNICODE));
                            v.push(key(VIRTUAL_KEY(0), *u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                        }
                    }
                }
            }
            v
        }
        Input::Key { k, down } => {
            let (code, extended) = vk(*k);
            let mut f = if *down {
                KEYBD_EVENT_FLAGS(0)
            } else {
                KEYEVENTF_KEYUP
            };
            if extended {
                f |= KEYEVENTF_EXTENDEDKEY;
            }
            vec![key(code, 0, f)]
        }
        Input::Press { c } => {
            // Letters and digits are their own virtual-key codes, in upper case.
            let code = VIRTUAL_KEY(u16::from(c.to_ascii_uppercase() as u8));
            vec![
                key(code, 0, KEYBD_EVENT_FLAGS(0)),
                key(code, 0, KEYEVENTF_KEYUP),
            ]
        }
        Input::Here => Vec::new(),
    }
}

/// Sends one phone message to Windows, on `monitor`.
pub fn apply(i: &Input, monitor: Rect) -> Result<(), String> {
    let v = inputs(i, monitor, desktop());
    if v.is_empty() {
        return Ok(());
    }
    // SAFETY: a slice of fully initialised INPUT structures.
    let sent = unsafe { SendInput(&v, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == v.len() {
        Ok(())
    } else {
        Err(
            "Windows did not take the input (a window running as administrator has the focus)"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_becomes_unicode_key_presses() {
        let r = Rect {
            left: 0,
            top: 0,
            width: 100,
            height: 100,
        };
        let v = inputs(
            &Input::Text {
                s: "hé😀\n".into()
            },
            r,
            r,
        );
        // h, é, and the emoji's two UTF-16 units: down and up each; then Enter.
        assert_eq!(v.len(), 2 * 4 + 2);
        // SAFETY: reading the keyboard member of keyboard inputs.
        unsafe {
            assert_eq!(v[0].Anonymous.ki.wScan, u16::from(b'h'));
            assert_eq!(
                v[1].Anonymous.ki.dwFlags,
                KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
            );
            assert_eq!(v[8].Anonymous.ki.wVk, VK_RETURN);
        }
        let c = inputs(&Input::Press { c: 'c' }, r, r);
        // SAFETY: reading the keyboard member of keyboard inputs.
        unsafe {
            assert_eq!(c[0].Anonymous.ki.wVk, VIRTUAL_KEY(u16::from(b'C')));
            assert_eq!(c[1].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
        }
        let s = inputs(&Input::Scroll { dx: 0.0, dy: 2.0 }, r, r);
        // SAFETY: reading the mouse member of a mouse input.
        unsafe {
            assert_eq!(
                s[0].Anonymous.mi.mouseData as i32, -240,
                "down is a negative turn"
            );
        }
    }
}

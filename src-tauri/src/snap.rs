//! Desktop widgets snap into place while dragged (1.2.0): to the edges of the screen's work
//! area, beside another widget with a small gap, or in line with its edges. Holding Shift
//! places a widget freely.
//!
//! A widget is dragged by Windows' own move loop (`startDragging`), which asks the window with
//! `WM_MOVING` where each step goes. A window subclass answers with [`snap`]: the widget moves
//! in steps that already land in place, and the place saved afterwards (`widgets.rs`, on
//! `Moved`) is the snapped one. The other widgets' rectangles are read once as the drag
//! starts. Nothing runs outside a drag.

#[cfg(windows)]
pub use imp::{attach, smoke_snap};

/// A rectangle in physical screen px, as Windows gives it (right and bottom exclusive).
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Rect {
    fn moved(self, dx: i32, dy: i32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }
}

/// Where a widget dragged to `r` goes. On each axis on its own, the nearest of these within
/// `reach` wins, else it stays where the pointer put it:
/// - the work area's edges, flush;
/// - beside a neighbour, `gap` from it;
/// - in line with a neighbour's edges.
///
/// A neighbour counts on one axis only when it is near on the other (overlapping, or within
/// `gap + reach`), so a widget across the screen never pulls. The size never changes.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn snap(r: Rect, area: Rect, others: &[Rect], reach: i32, gap: i32) -> Rect {
    let near = |a0: i32, a1: i32, b0: i32, b1: i32| a0 < b1 + gap + reach && b0 < a1 + gap + reach;
    let dx = nearest(
        [area.left - r.left, area.right - r.right]
            .into_iter()
            .chain(
                others
                    .iter()
                    .filter(|o| near(r.top, r.bottom, o.top, o.bottom))
                    .flat_map(|o| {
                        [
                            o.right + gap - r.left,
                            o.left - gap - r.right,
                            o.left - r.left,
                            o.right - r.right,
                        ]
                    }),
            ),
        reach,
    );
    let dy = nearest(
        [area.top - r.top, area.bottom - r.bottom]
            .into_iter()
            .chain(
                others
                    .iter()
                    .filter(|o| near(r.left, r.right, o.left, o.right))
                    .flat_map(|o| {
                        [
                            o.bottom + gap - r.top,
                            o.top - gap - r.bottom,
                            o.top - r.top,
                            o.bottom - r.bottom,
                        ]
                    }),
            ),
        reach,
    );
    r.moved(dx, dy)
}

fn nearest(moves: impl Iterator<Item = i32>, reach: i32) -> i32 {
    moves
        .filter(|d| d.abs() <= reach)
        .min_by_key(|d| d.abs())
        .unwrap_or(0)
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;

    use tauri::{AppHandle, Manager, WebviewWindow};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_SHIFT};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetWindowRect, GetWindowTextW, IsWindowVisible, SendMessageW,
        WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_MOVING, WM_NCDESTROY,
    };

    use super::{snap, Rect};

    /// The subclass's id on a widget window (tao's own is 0).
    const SUBCLASS_ID: usize = 0x5659_524f;
    /// Logical px: how close an edge must come to snap, and the space left between widgets.
    const REACH: i32 = 10;
    const GAP: i32 = 8;

    thread_local! {
        /// The other widgets, read as a drag starts (the subclass runs on the main thread).
        static OTHERS: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
    }

    /// Snaps a widget window while it is dragged. Main thread (the window's own).
    pub fn attach(win: &WebviewWindow) {
        let Ok(hwnd) = win.hwnd() else {
            return;
        };
        // SAFETY: subclasses a window of this thread; setting it again only updates it.
        let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) };
        if !ok.as_bool() {
            log::warn!("snap: a widget window could not be subclassed; it moves freely");
        }
    }

    unsafe extern "system" fn subclass(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_ENTERSIZEMOVE => OTHERS.with(|o| *o.borrow_mut() = others(hwnd)),
            WM_MOVING if lparam.0 != 0 => {
                // SAFETY: Windows passes the proposed rectangle, to be changed in place.
                let r = unsafe { &mut *(lparam.0 as *mut RECT) };
                if let Some(to) = snapped(hwnd, *r) {
                    *r = to;
                }
            }
            WM_NCDESTROY => {
                // SAFETY: removes this subclass from the window going away.
                unsafe {
                    let _ = RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID);
                }
            }
            _ => {}
        }
        // SAFETY: the next handler (tao's) for every message.
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }

    /// Where a drag step to `r` goes, or None to leave it (Shift held, no monitor).
    fn snapped(hwnd: HWND, r: RECT) -> Option<RECT> {
        // SAFETY: reads the keyboard state of this thread's input.
        if unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0 {
            return None;
        }
        let area = work_area(&r)?;
        // SAFETY: reads the window's DPI.
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
        let px = |logical: i32| logical * dpi / 96;
        let to = OTHERS.with(|o| snap(rect(r), area, &o.borrow(), px(REACH), px(GAP)));
        Some(RECT {
            left: to.left,
            top: to.top,
            right: to.right,
            bottom: to.bottom,
        })
    }

    fn rect(r: RECT) -> Rect {
        Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }

    /// The work area (the screen less the taskbar) of the monitor nearest `r`.
    fn work_area(r: &RECT) -> Option<Rect> {
        // SAFETY: looks up a monitor and reads its information into a sized struct.
        unsafe {
            let monitor = MonitorFromRect(r, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info)
                .as_bool()
                .then(|| rect(info.rcWork))
        }
    }

    /// The other shown widget windows' rectangles. Widgets are made on the main thread, as tao
    /// makes every window, so they are this thread's windows.
    fn others(me: HWND) -> Vec<Rect> {
        struct Found {
            me: HWND,
            rects: Vec<Rect>,
        }
        unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
            // SAFETY: `data` is the `Found` below, alive for the whole enumeration.
            let found = unsafe { &mut *(data.0 as *mut Found) };
            if hwnd != found.me && is_widget(hwnd) {
                let mut r = RECT::default();
                // SAFETY: reads a window's rectangle.
                if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
                    found.rects.push(rect(r));
                }
            }
            true.into()
        }
        let mut found = Found {
            me,
            rects: Vec::new(),
        };
        // SAFETY: enumerates this thread's top-level windows with a callback that only reads.
        unsafe {
            let _ = EnumThreadWindows(
                GetCurrentThreadId(),
                Some(each),
                LPARAM(&mut found as *mut Found as isize),
            );
        }
        found.rects
    }

    fn is_widget(hwnd: HWND) -> bool {
        let mut title = [0u16; 32];
        // SAFETY: reads a window's visibility and title into the buffer.
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return false;
            }
            let n = usize::try_from(GetWindowTextW(hwnd, &mut title)).unwrap_or(0);
            String::from_utf16_lossy(&title[..n.min(title.len())]) == crate::widgets::TITLE
        }
    }

    /// CI's smoke test (`smoke.rs`): plays a drag's messages to the widget's window, as Windows'
    /// move loop sends them. A step 4 px from the work area's corner must come back flush with
    /// it; one 40 px away must come back as it was. Runs on the smoke test's thread.
    pub fn smoke_snap(app: &AppHandle, label: &str) -> String {
        let Some(hwnd) = app.get_webview_window(label).and_then(|w| w.hwnd().ok()) else {
            return "failed: no widget window".into();
        };
        let mut now = RECT::default();
        // SAFETY: reads the widget window's rectangle and DPI.
        let (dpi, read) = unsafe {
            (
                GetDpiForWindow(hwnd).max(96) as i32,
                GetWindowRect(hwnd, &mut now),
            )
        };
        if read.is_err() {
            return "failed: the widget window's rectangle could not be read".into();
        }
        let Some(area) = work_area(&now) else {
            return "skipped: no monitor".into();
        };
        let (w, h) = (now.right - now.left, now.bottom - now.top);
        let at = |off: i32| RECT {
            left: area.left + off * dpi / 96,
            top: area.top + off * dpi / 96,
            right: area.left + off * dpi / 96 + w,
            bottom: area.top + off * dpi / 96 + h,
        };
        let (mut close, mut far) = (at(4), at(40));
        let asked_far = far;
        // SAFETY: the messages Windows' move loop sends, to a window of this process; each
        // rectangle outlives its (synchronous) SendMessage. The window is not moved.
        unsafe {
            SendMessageW(hwnd, WM_ENTERSIZEMOVE, None, None);
            SendMessageW(
                hwnd,
                WM_MOVING,
                None,
                Some(LPARAM(&mut close as *mut RECT as isize)),
            );
            SendMessageW(
                hwnd,
                WM_MOVING,
                None,
                Some(LPARAM(&mut far as *mut RECT as isize)),
            );
            SendMessageW(hwnd, WM_EXITSIZEMOVE, None, None);
        }
        if (close.left, close.top) != (area.left, area.top) {
            return format!(
                "failed: a widget 4 px from the corner went to {},{} (the corner is {},{})",
                close.left, close.top, area.left, area.top
            );
        }
        if far != asked_far {
            return format!(
                "failed: a widget 40 px from the corner moved to {},{}",
                far.left, far.top
            );
        }
        "passed".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    fn at(left: i32, top: i32, w: i32, h: i32) -> Rect {
        Rect {
            left,
            top,
            right: left + w,
            bottom: top + h,
        }
    }

    #[test]
    fn edges_of_the_work_area_pull_within_reach() {
        assert_eq!(
            snap(at(7, 1040 - 104 - 9, 220, 104), AREA, &[], 10, 8),
            at(0, 936, 220, 104)
        );
        assert_eq!(
            snap(at(1920 - 220 - 4, 3, 220, 104), AREA, &[], 10, 8),
            at(1700, 0, 220, 104)
        );
        // Out of reach: left where it was.
        assert_eq!(
            snap(at(11, 500, 220, 104), AREA, &[], 10, 8),
            at(11, 500, 220, 104)
        );
    }

    #[test]
    fn a_neighbour_pulls_beside_it_with_the_gap_and_in_line() {
        let a = at(400, 300, 220, 104);
        // To its right, 13 px off and 5 px lower: beside it (gap 8) and tops in line.
        assert_eq!(
            snap(at(633, 305, 220, 104), AREA, &[a], 10, 8),
            at(628, 300, 220, 104)
        );
        // Under it, 3 px left of its edge: under it (gap 8) and lefts in line.
        assert_eq!(
            snap(at(397, 410, 200, 90), AREA, &[a], 10, 8),
            at(400, 412, 200, 90)
        );
        // Under it with its right edge 2 px off: in line on the right.
        assert_eq!(
            snap(at(422, 420, 200, 90), AREA, &[a], 10, 8),
            at(420, 412, 200, 90)
        );
    }

    #[test]
    fn a_far_widget_never_pulls() {
        // Lefts nearly in line, but 400 px apart up and down.
        let a = at(400, 100, 220, 104);
        assert_eq!(
            snap(at(405, 600, 220, 104), AREA, &[a], 10, 8),
            at(405, 600, 220, 104)
        );
    }

    #[test]
    fn the_nearest_wins() {
        // The screen's left edge 4 px away, a neighbour's right edge (with the gap) 2 px away.
        let a = at(-300, 500, 290, 104);
        assert_eq!(
            snap(at(-4, 520, 220, 104), AREA, &[a], 10, 8),
            at(-2, 520, 220, 104)
        );
    }
}

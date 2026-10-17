//! Desktop widgets snap into place while dragged (1.2.0): to the edges of the screen's work
//! area, beside another widget with a small gap, or in line with its edges. Holding Shift
//! places a widget freely.
//!
//! A widget is dragged by Windows' own move loop (`startDragging`), which asks the window with
//! `WM_MOVING` where each step goes. A window subclass answers with [`snap`]: the widget moves
//! in steps that already land in place, and the place saved afterwards (`widgets.rs`, on
//! `Moved`) is the snapped one. The other widgets' rectangles are read once as the drag
//! starts. Nothing runs outside a drag.
//!
//! Windows builds each step from the last one answered plus the mouse's movement, so snapping
//! what it proposes would carry each snap into the next step: a slow drag never got further
//! than the pull and a snapped widget stayed stuck (1.2's first field check). Each step is
//! worked out from the pointer instead ([`follow`]), where the window was grabbed.

#[cfg(windows)]
pub use imp::{attach, smoke_drag};

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

    fn width(self) -> i32 {
        self.right - self.left
    }

    fn height(self) -> i32 {
        self.bottom - self.top
    }
}

/// Where a window grabbed at `grab` (the pointer's place in it, as a share of its width and
/// height, from its top left) goes with the pointer at `cursor`: the pointer's own place, never
/// the step answered before.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn follow(cursor: (i32, i32), grab: (f64, f64), width: i32, height: i32) -> Rect {
    let left = cursor.0 - (grab.0 * f64::from(width)).round() as i32;
    let top = cursor.1 - (grab.1 * f64::from(height)).round() as i32;
    Rect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

/// Where the pointer took hold of `r`, as [`follow`] wants it (0 to 1 on each axis).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn grab_of(cursor: (i32, i32), r: Rect) -> (f64, f64) {
    let share = |at: i32, from: i32, size: i32| {
        if size <= 0 {
            0.5
        } else {
            (f64::from(at - from) / f64::from(size)).clamp(0.0, 1.0)
        }
    };
    (
        share(cursor.0, r.left, r.width()),
        share(cursor.1, r.top, r.height()),
    )
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::WebviewWindow;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS, VK_SHIFT,
    };
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetCursorPos, GetWindowRect, GetWindowTextW, IsWindowVisible,
        SetCursorPos, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WM_ENTERSIZEMOVE,
        WM_EXITSIZEMOVE, WM_MOVING, WM_NCDESTROY,
    };

    use super::{follow, grab_of, snap, Rect};

    /// The subclass's id on a widget window (tao's own is 0).
    const SUBCLASS_ID: usize = 0x5659_524f;
    /// Logical px: how close an edge must come to snap, and the space left between widgets.
    const REACH: i32 = 10;
    const GAP: i32 = 8;

    /// A drag under way: where the pointer took hold, and the other widgets.
    struct Drag {
        grab: Option<(f64, f64)>,
        others: Vec<Rect>,
    }

    thread_local! {
        /// Read as a drag starts (the subclass runs on the main thread).
        static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
    }

    /// For CI's smoke test: whether a widget is being dragged, and the last step Windows
    /// proposed and the one answered.
    static DRAGGING: AtomicBool = AtomicBool::new(false);
    static LAST: Mutex<Option<(Rect, Rect)>> = Mutex::new(None);

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
            WM_ENTERSIZEMOVE => {
                let grab = match (cursor(), window_rect(hwnd)) {
                    (Some(c), Some(r)) => Some(grab_of(c, r)),
                    _ => None,
                };
                let others = others(hwnd);
                DRAG.with(|d| *d.borrow_mut() = Some(Drag { grab, others }));
                DRAGGING.store(true, Ordering::SeqCst);
            }
            WM_MOVING if lparam.0 != 0 => {
                // SAFETY: Windows passes the proposed rectangle, to be changed in place.
                let r = unsafe { &mut *(lparam.0 as *mut RECT) };
                if let Some(to) = step(hwnd, *r) {
                    *r = to;
                }
            }
            WM_EXITSIZEMOVE => {
                DRAG.with(|d| *d.borrow_mut() = None);
                DRAGGING.store(false, Ordering::SeqCst);
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

    /// Where a drag step Windows proposed as `r` goes: under the pointer where it took hold,
    /// snapped unless Shift is held. None leaves it (no drag recorded, no monitor).
    fn step(hwnd: HWND, r: RECT) -> Option<RECT> {
        let proposed = rect(r);
        let to = DRAG.with(|d| {
            let d = d.borrow();
            let d = d.as_ref()?;
            let free = match (d.grab, cursor()) {
                (Some(grab), Some(c)) => follow(c, grab, proposed.width(), proposed.height()),
                _ => proposed,
            };
            // SAFETY: reads the keyboard state of this thread's input.
            if unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0 {
                return Some(free);
            }
            let area = work_area(&r)?;
            // SAFETY: reads the window's DPI.
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            let px = |logical: i32| logical * dpi / 96;
            Some(snap(free, area, &d.others, px(REACH), px(GAP)))
        })?;
        if let Ok(mut last) = LAST.lock() {
            *last = Some((proposed, to));
        }
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

    fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        // SAFETY: reads the pointer's place (physical px: the app is per-monitor DPI aware).
        unsafe { GetCursorPos(&mut p) }.ok().map(|()| (p.x, p.y))
    }

    fn window_rect(hwnd: HWND) -> Option<Rect> {
        let mut r = RECT::default();
        // SAFETY: reads a window's rectangle.
        unsafe { GetWindowRect(hwnd, &mut r) }
            .ok()
            .map(|()| rect(r))
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
                if let Some(r) = window_rect(hwnd) {
                    found.rects.push(r);
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

    fn mouse(flags: MOUSE_EVENT_FLAGS) -> bool {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: flags,
                    ..Default::default()
                },
            },
        };
        // SAFETY: one mouse input, sized as the call wants.
        unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 1 }
    }

    /// Puts the pointer at a place, with a mouse move there for whoever has the mouse.
    fn point(x: i32, y: i32) {
        // SAFETY: moves the pointer; a move of nothing makes Windows report where it is.
        unsafe {
            let _ = SetCursorPos(x, y);
        }
        mouse(MOUSEEVENTF_MOVE);
        std::thread::sleep(Duration::from_millis(12));
    }

    fn wait(limit: Duration, done: impl Fn() -> bool) -> bool {
        let since = Instant::now();
        while !done() {
            if since.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        true
    }

    fn last() -> Option<(Rect, Rect)> {
        LAST.lock().ok().and_then(|l| *l)
    }

    /// CI's smoke test (`smoke.rs`): drags the widget with the mouse, as a person would, on its
    /// own thread. The widget must be unlocked and on top of every window. It is put 150 px from
    /// the work area's corner and pressed in its middle; moved slowly to 5 px from the corner,
    /// it must snap flush; moved on 30 px, 1 px at a time, it must follow the pointer out of the
    /// snap; released, it must stay where it was last put.
    pub fn smoke_drag(hwnd: HWND) -> String {
        let Some(now) = window_rect(hwnd) else {
            return "failed: the widget window's rectangle could not be read".into();
        };
        let Some(area) = work_area(&RECT {
            left: now.left,
            top: now.top,
            right: now.right,
            bottom: now.bottom,
        }) else {
            return "skipped: no monitor".into();
        };
        // SAFETY: reads the widget window's DPI.
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
        let px = |logical: i32| logical * dpi / 96;
        let (x0, y0) = (area.left + px(150), area.top + px(150));
        // SAFETY: moves a window of this process; it is not resized, raised or activated.
        let placed = unsafe {
            SetWindowPos(
                hwnd,
                None,
                x0,
                y0,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        if let Err(e) = placed {
            return format!("failed: the widget could not be put in place ({e})");
        }
        if !wait(Duration::from_secs(2), || {
            window_rect(hwnd).is_some_and(|r| (r.left, r.top) == (x0, y0))
        }) {
            return "failed: the widget did not go where it was put".into();
        }
        std::thread::sleep(Duration::from_millis(300));
        let start = window_rect(hwnd).unwrap_or(now);
        let (cx, cy) = (
            start.left + start.width() / 2,
            start.top + start.height() / 2,
        );
        if let Ok(mut l) = LAST.lock() {
            *l = None;
        }
        point(cx, cy);
        if !mouse(MOUSEEVENTF_LEFTDOWN) {
            return "skipped: this desktop refuses mouse input".into();
        }
        if !wait(Duration::from_secs(3), || DRAGGING.load(Ordering::SeqCst)) {
            mouse(MOUSEEVENTF_LEFTUP);
            return "failed: pressing the widget did not start a drag".into();
        }
        let result = (|| {
            // Towards the corner, 2 px a step, until the widget is 5 px from it.
            let (dx, dy) = (area.left + px(5) - start.left, area.top + px(5) - start.top);
            let steps = dx.abs().max(dy.abs()) / 2;
            for k in 1..=steps {
                point(cx + dx * k / steps, cy + dy * k / steps);
            }
            std::thread::sleep(Duration::from_millis(250));
            let Some((_, near)) = last() else {
                return "failed: Windows asked nothing of the widget while it was dragged".into();
            };
            if (near.left, near.top) != (area.left, area.top) {
                return format!(
                    "failed: 5 px from the corner the widget went to {},{} (the corner is {},{})",
                    near.left, near.top, area.left, area.top
                );
            }
            // On, 1 px a step, well out of reach of the corner.
            let (ax, ay) = (cx + dx, cy + dy);
            point(ax + 1, ay + 1);
            std::thread::sleep(Duration::from_millis(150));
            // What Windows proposed for the first step after the snap tells how its move loop
            // works (the cause of the field check's stuck widgets), for the record: one pixel on
            // from the snapped place if it builds each step on the last one answered, or one
            // pixel on from where the pointer put the widget.
            let first = last().map(|(proposed, _)| (proposed.left, proposed.top));
            let built = if first == Some((near.left + 1, near.top + 1)) {
                "Windows built each step on the last one answered".to_owned()
            } else if first == Some((area.left + px(5) + 1, area.top + px(5) + 1)) {
                "Windows built each step from the pointer".to_owned()
            } else {
                format!("after the snap Windows proposed {first:?}")
            };
            let on = px(30);
            for k in 2..=on {
                point(ax + k, ay + k);
            }
            std::thread::sleep(Duration::from_millis(250));
            let Some((_, away)) = last() else {
                return "failed: no step answered".into();
            };
            let want = (area.left + px(5) + on, area.top + px(5) + on);
            if ((away.left - want.0).abs(), (away.top - want.1).abs()) > (1, 1) {
                return format!(
                    "failed: moved on from the corner, the widget went to {},{} (the pointer \
                     was at {},{}; {built})",
                    away.left, away.top, want.0, want.1
                );
            }
            format!("passed ({built})")
        })();
        mouse(MOUSEEVENTF_LEFTUP);
        let ended = wait(Duration::from_secs(2), || !DRAGGING.load(Ordering::SeqCst));
        if !result.starts_with("passed") {
            return result;
        }
        if !ended {
            return "failed: the drag did not end when the mouse was let go".into();
        }
        let (Some((_, put)), Some(end)) = (last(), window_rect(hwnd)) else {
            return "failed: the widget's place could not be read".into();
        };
        if (end.left, end.top) != (put.left, put.top) {
            return format!(
                "failed: let go, the widget went to {},{} (it was put at {},{})",
                end.left, end.top, put.left, put.top
            );
        }
        result
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
    fn a_widget_follows_the_pointer_where_it_was_grabbed() {
        let r = at(100, 200, 220, 104);
        let grab = grab_of((210, 252), r);
        assert_eq!(grab, (0.5, 0.5));
        assert_eq!(follow((310, 352), grab, 220, 104), at(200, 300, 220, 104));
        // Grabbed outside (it can't be, but never more than its own size away).
        assert_eq!(grab_of((0, 400), r), (0.0, 1.0));
    }

    /// Windows' move loop as it works: each step it proposes is the last one answered moved by
    /// the mouse's movement. The pointer took hold of `start` at `grab`; `answer` is the
    /// window's answer to a proposal, with the pointer's place. Returns each step's place.
    fn drag(
        start: Rect,
        grab: (f64, f64),
        moves: &[(i32, i32)],
        answer: impl Fn(Rect, (i32, i32)) -> Rect,
    ) -> Vec<Rect> {
        let mut cursor = (
            start.left + (grab.0 * f64::from(start.width())).round() as i32,
            start.top + (grab.1 * f64::from(start.height())).round() as i32,
        );
        let mut last = start;
        let mut out = Vec::new();
        for &(dx, dy) in moves {
            cursor = (cursor.0 + dx, cursor.1 + dy);
            last = answer(last.moved(dx, dy), cursor);
            out.push(last);
        }
        out
    }

    #[test]
    fn a_snapped_widget_comes_free_when_dragged_slowly_away() {
        // Flush with the screen's left edge and in line with a neighbour above, then 1 px at a
        // time rightwards for 30 px, as a slow hand drags.
        let start = at(0, 500, 220, 104);
        let neighbour = at(0, 300, 220, 192);
        let moves = [(1, 0); 30];
        let grab = (0.5, 0.5);
        let followed = drag(start, grab, &moves, |p, c| {
            snap(
                follow(c, grab, p.width(), p.height()),
                AREA,
                &[neighbour],
                10,
                8,
            )
        });
        // Held by the edge for 10 px, then with the pointer.
        assert_eq!(followed[9].left, 0);
        assert_eq!(followed[10].left, 11);
        assert_eq!(followed.last().unwrap().left, 30);
        // The way 1.2's first build answered, snapping what Windows proposed: each answer was
        // carried into the next proposal, so the widget never got away (the field check).
        let carried = drag(start, grab, &moves, |p, _| {
            snap(p, AREA, &[neighbour], 10, 8)
        });
        assert!(carried.iter().all(|r| r.left == 0), "{carried:?}");
    }

    #[test]
    fn a_widget_dragged_off_its_neighbour_comes_free() {
        // 8 px beside a neighbour, tops in line; dragged away down and right, slowly.
        let a = at(400, 300, 220, 104);
        let start = at(628, 300, 220, 104);
        let moves = [(1, 1); 40];
        let grab = (0.3, 0.6);
        let followed = drag(start, grab, &moves, |p, c| {
            snap(follow(c, grab, p.width(), p.height()), AREA, &[a], 10, 8)
        });
        let end = *followed.last().unwrap();
        assert_eq!((end.left, end.top), (668, 340));
        let carried = drag(start, grab, &moves, |p, _| snap(p, AREA, &[a], 10, 8));
        let stuck = *carried.last().unwrap();
        assert_eq!((stuck.left, stuck.top), (628, 300), "stuck the old way");
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

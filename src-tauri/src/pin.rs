//! Keeps "Behind windows" desktop widgets in view through Windows' Show desktop (Win+D, or the
//! corner of the taskbar), as Rainmeter's "On desktop" skins stay (1.1.1).
//!
//! Show desktop lifts the window that holds the desktop icons above every window that is not
//! always-on-top, so a widget kept at the bottom disappears under it. Vyro keeps a hidden probe
//! window at the bottom too: when the icons' window sits above the probe, the desktop is being
//! shown, and the widgets go on top of it until an app comes forward again. The check runs
//! for a moment after each change of foreground window (the shell lifts the desktop a little
//! after), and every 100 ms while the desktop shows, from a hook set only while a widget sits
//! behind windows; otherwise nothing runs. Vyro's windows are never put inside Explorer's: that
//! would tie the desktop's input to Vyro's main thread.

#[cfg(windows)]
pub use imp::{desktop_shown, smoke_show_desktop, watch};

/// What the foreground window is, as [`next`] needs it.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Front {
    /// The desktop (the icons' window, Progman or a WorkerW).
    Desktop,
    /// The taskbar, Start, search, the notification area, task switching.
    Shell,
    /// One of Vyro's windows (a widget, the main window).
    Vyro,
    /// Any other window.
    App,
    /// None, for a moment while windows switch.
    Nothing,
}

/// Whether the desktop counts as shown after a check. `raised`: the icons' window is above the
/// probe. Show desktop starts with the desktop in front; it ends when an app comes forward, or
/// when the desktop drops back (Win+D again). Clicking the desktop doesn't lift it, so the
/// widgets don't jump over open windows.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn next(shown: bool, front: Front, raised: bool) -> bool {
    match (shown, front) {
        (_, Front::App) => false,
        (false, Front::Desktop) => raised,
        (false, _) => false,
        (true, _) => raised,
    }
}

/// The foreground window's kind by its class (Vyro's own windows are told by their process).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn front_of_class(class: &str) -> Front {
    match class {
        "Progman" | "WorkerW" => Front::Desktop,
        "Shell_TrayWnd"
        | "Shell_SecondaryTrayWnd"
        | "Windows.UI.Core.CoreWindow"
        | "NotifyIconOverflowWindow"
        | "TopLevelWindowForOverflowXamlIsland"
        | "XamlExplorerHostIslandWindow"
        | "MultitaskingViewFrame"
        | "ForegroundStaging"
        | "TaskListThumbnailWnd" => Front::Shell,
        _ => Front::App,
    }
}

#[cfg(not(windows))]
pub fn watch(_app: &tauri::AppHandle, _on: bool) {}

#[cfg(not(windows))]
pub fn desktop_shown() -> bool {
    false
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Manager};
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, FindWindowExW, GetClassNameW, GetForegroundWindow,
        GetShellWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, KillTimer,
        RegisterClassExW, SetTimer, SetWindowPos, EVENT_SYSTEM_FOREGROUND, GWL_EXSTYLE,
        HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSENDCHANGING, SWP_NOSIZE,
        WINEVENT_OUTOFCONTEXT, WNDCLASSEXW, WS_DISABLED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP,
    };

    use super::{front_of_class, next, Front};

    const PROBE_CLASS: PCWSTR = w!("VyroDesktopProbe");
    const TIMER_ID: usize = 1;
    /// Checks this far apart: after a change of foreground window, and while the desktop shows.
    const TICK_MS: u32 = 100;
    /// Checks after a change of foreground window (the shell lifts the desktop a little after).
    const TICKS: u32 = 6;

    // Set and read on the main thread (the hook and the timer call back there); atomics only
    // because statics need them. The smoke test reads them from its own thread.
    static APP: OnceLock<AppHandle> = OnceLock::new();
    static PROBE: AtomicIsize = AtomicIsize::new(0);
    static HOOK: AtomicIsize = AtomicIsize::new(0);
    static LEFT: AtomicU32 = AtomicU32::new(0);
    static SHOWN: AtomicBool = AtomicBool::new(false);

    /// Show desktop is on: "Behind windows" widgets are kept on top for now.
    pub fn desktop_shown() -> bool {
        SHOWN.load(Ordering::SeqCst)
    }

    /// Watches for Show desktop while `on` (a widget sits behind windows). Main thread only.
    pub fn watch(app: &AppHandle, on: bool) {
        let _ = APP.set(app.clone());
        let hooked = HOOK.load(Ordering::SeqCst) != 0;
        if on && !hooked {
            if probe().is_none() {
                log::warn!("pin: no probe window; widgets behind windows hide on Show desktop");
                return;
            }
            // SAFETY: an out-of-context hook: Windows posts its events to this thread, whose
            // message loop calls `on_foreground`.
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(on_foreground),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_invalid() {
                log::warn!("pin: no foreground hook; widgets behind windows hide on Show desktop");
                return;
            }
            HOOK.store(hook.0 as isize, Ordering::SeqCst);
            // The desktop may be showing already.
            burst();
        } else if !on && hooked {
            let hook = HOOK.swap(0, Ordering::SeqCst);
            // SAFETY: the hook this module set, removed once.
            unsafe {
                let _ = UnhookWinEvent(HWINEVENTHOOK(hook as _));
            }
            stop();
            if SHOWN.swap(false, Ordering::SeqCst) {
                if let Some(probe) = probe_hwnd() {
                    to_bottom(probe);
                }
            }
        }
    }

    fn probe_hwnd() -> Option<HWND> {
        let p = PROBE.load(Ordering::SeqCst);
        (p != 0).then_some(HWND(p as _))
    }

    /// The probe, made the first time: a hidden, disabled tool window at the bottom of the
    /// stack. It also carries the check's timer.
    fn probe() -> Option<HWND> {
        if let Some(probe) = probe_hwnd() {
            return Some(probe);
        }
        // SAFETY: registers a class and makes a window of this thread; the window is never shown.
        unsafe {
            let instance = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(probe_proc),
                hInstance: instance,
                lpszClassName: PROBE_CLASS,
                ..Default::default()
            };
            // 0 when the class is there already, which is fine.
            let _ = RegisterClassExW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PROBE_CLASS,
                w!("Vyro desktop probe"),
                WS_POPUP | WS_DISABLED,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .ok()?;
            PROBE.store(hwnd.0 as isize, Ordering::SeqCst);
            to_bottom(hwnd);
            Some(hwnd)
        }
    }

    unsafe extern "system" fn probe_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: the default handling, for the probe's own messages.
        unsafe { DefWindowProcW(hwnd, msg, w, l) }
    }

    fn to_bottom(probe: HWND) {
        // SAFETY: moves the probe (this process's window) in the stack only.
        unsafe {
            let _ = SetWindowPos(
                probe,
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSENDCHANGING,
            );
        }
    }

    unsafe extern "system" fn on_foreground(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _object: i32,
        _child: i32,
        _thread: u32,
        _time: u32,
    ) {
        burst();
    }

    /// Checks now and for a moment after.
    fn burst() {
        LEFT.store(TICKS, Ordering::SeqCst);
        if let Some(probe) = probe_hwnd() {
            // SAFETY: a timer on the probe, a window of this thread; setting it again restarts it.
            unsafe {
                let _ = SetTimer(Some(probe), TIMER_ID, TICK_MS, Some(tick));
            }
        }
        check();
    }

    unsafe extern "system" fn tick(_hwnd: HWND, _msg: u32, _id: usize, _time: u32) {
        check();
        // While the desktop shows, keep checking: it can end without a change of foreground
        // window (Win+D again with nothing else open).
        if SHOWN.load(Ordering::SeqCst) {
            return;
        }
        let left = LEFT.load(Ordering::SeqCst).saturating_sub(1);
        LEFT.store(left, Ordering::SeqCst);
        if left == 0 {
            stop();
        }
    }

    fn stop() {
        if let Some(probe) = probe_hwnd() {
            // SAFETY: the timer this module set on the probe.
            unsafe {
                let _ = KillTimer(Some(probe), TIMER_ID);
            }
        }
    }

    fn check() {
        let Some(probe) = probe_hwnd() else {
            return;
        };
        let host = host();
        let was = SHOWN.load(Ordering::SeqCst);
        let now = next(was, front(host), host.is_some_and(|h| raised(h, probe)));
        if now == was {
            return;
        }
        SHOWN.store(now, Ordering::SeqCst);
        if !now {
            to_bottom(probe);
        }
        log::info!(
            "pin: {}",
            if now {
                "Show desktop: widgets behind windows kept over the desktop"
            } else {
                "the desktop is covered again: widgets back behind windows"
            }
        );
        if let Some(app) = APP.get() {
            crate::widgets::restack(app);
        }
    }

    /// The top-level window holding the desktop icons (`SHELLDLL_DefView`): the shell window
    /// (Progman) on Windows 11 24H2 and normally before it, or the WorkerW that Show desktop
    /// lifts on earlier versions. Found afresh each time, so a restarted Explorer is followed.
    fn host() -> Option<HWND> {
        // SAFETY: looks up top-level windows by class; reads their process ids.
        unsafe {
            let shell = GetShellWindow();
            if shell.is_invalid() {
                return None;
            }
            if holds_icons(shell) {
                return Some(shell);
            }
            let mut shell_pid = 0u32;
            GetWindowThreadProcessId(shell, Some(&mut shell_pid));
            let mut after = None;
            while let Ok(worker) = FindWindowExW(None, after, w!("WorkerW"), PCWSTR::null()) {
                let mut pid = 0u32;
                GetWindowThreadProcessId(worker, Some(&mut pid));
                if pid == shell_pid && holds_icons(worker) {
                    return Some(worker);
                }
                after = Some(worker);
            }
            None
        }
    }

    fn holds_icons(window: HWND) -> bool {
        // SAFETY: looks up a child window by class.
        unsafe { FindWindowExW(Some(window), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok() }
    }

    /// The icons' window is shown and above the probe: Show desktop lifted it over the other
    /// windows (normally it sits at the very bottom, under the probe).
    fn raised(host: HWND, probe: HWND) -> bool {
        // SAFETY: reads window state; FindWindowEx walks the top-level windows after `host` in
        // the stack.
        unsafe {
            IsWindowVisible(host).as_bool()
                && FindWindowExW(None, Some(host), PROBE_CLASS, PCWSTR::null())
                    .is_ok_and(|found| found == probe)
        }
    }

    fn front(host: Option<HWND>) -> Front {
        // SAFETY: reads the foreground window's process and class.
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() {
                return Front::Nothing;
            }
            if Some(fg) == host {
                return Front::Desktop;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid == std::process::id() {
                return Front::Vyro;
            }
            let mut class = [0u16; 64];
            let n = usize::try_from(GetClassNameW(fg, &mut class)).unwrap_or(0);
            front_of_class(&String::from_utf16_lossy(&class[..n.min(class.len())]))
        }
    }

    fn topmost(window: HWND) -> bool {
        // SAFETY: reads a window's extended style.
        let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) };
        style & WS_EX_TOPMOST.0 as isize != 0
    }

    fn wait(limit: Duration, done: impl Fn() -> bool) -> bool {
        let since = Instant::now();
        loop {
            if done() {
                return true;
            }
            if since.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// CI's smoke test (`smoke.rs`): presses Show desktop through Explorer, checks the widget
    /// (behind windows) stays over the desktop, presses it again and checks the widget goes back.
    /// "skipped: …" where the runner has no desktop to show. Runs on the smoke test's thread.
    pub fn smoke_show_desktop(app: &AppHandle, label: &str) -> String {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellDispatch4, Shell};

        let Some(widget) = app.get_webview_window(label).and_then(|w| w.hwnd().ok()) else {
            return "failed: no widget window".into();
        };
        if HOOK.load(Ordering::SeqCst) == 0 {
            return "failed: no watch for Show desktop with a widget behind windows".into();
        }
        let Some(probe) = probe_hwnd() else {
            return "failed: no probe window".into();
        };
        if host().is_none() {
            return "skipped: no desktop shell".into();
        }
        let lifted = || host().is_some_and(|h| raised(h, probe));
        // SAFETY: COM on this thread; the shell object is released before COM is uninitialised.
        unsafe {
            if let Err(e) = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() {
                return format!("skipped: COM ({e})");
            }
            let result = (|| {
                let shell: IShellDispatch4 = match CoCreateInstance(&Shell, None, CLSCTX_ALL) {
                    Ok(shell) => shell,
                    Err(e) => return format!("skipped: no Shell.Application ({e})"),
                };
                if let Err(e) = shell.ToggleDesktop() {
                    return format!("skipped: Show desktop could not be pressed ({e})");
                }
                if !wait(Duration::from_secs(3), lifted) {
                    return "skipped: Show desktop did not lift the desktop (no interactive shell)"
                        .into();
                }
                let kept = wait(Duration::from_secs(2), || {
                    desktop_shown() && topmost(widget) && IsWindowVisible(widget).as_bool()
                });
                let _ = shell.ToggleDesktop();
                if !kept {
                    return "failed: the widget did not stay over the shown desktop".into();
                }
                if !wait(Duration::from_secs(3), || {
                    !desktop_shown() && !topmost(widget)
                }) {
                    return "failed: the widget did not go back behind windows".into();
                }
                "passed".into()
            })();
            CoUninitialize();
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_desktop_starts_with_the_desktop_in_front_and_lifted() {
        assert!(next(false, Front::Desktop, true));
        // Clicking the desktop brings it forward without lifting it.
        assert!(!next(false, Front::Desktop, false));
        // Lifted with anything else in front is not counted as a start.
        for front in [Front::Shell, Front::Vyro, Front::App, Front::Nothing] {
            assert!(!next(false, front, true), "{front:?}");
        }
    }

    #[test]
    fn it_lasts_through_the_shell_and_vyro_and_ends_with_an_app_or_the_desktop_dropping() {
        for front in [Front::Desktop, Front::Shell, Front::Vyro, Front::Nothing] {
            assert!(next(true, front, true), "{front:?}");
            assert!(!next(true, front, false), "{front:?}");
        }
        // An app in front ends it, even where the desktop stays above the probe.
        assert!(!next(true, Front::App, true));
    }

    #[test]
    fn foreground_classes() {
        assert_eq!(front_of_class("Progman"), Front::Desktop);
        assert_eq!(front_of_class("WorkerW"), Front::Desktop);
        assert_eq!(front_of_class("Shell_TrayWnd"), Front::Shell);
        assert_eq!(front_of_class("Windows.UI.Core.CoreWindow"), Front::Shell);
        assert_eq!(front_of_class("XamlExplorerHostIslandWindow"), Front::Shell);
        assert_eq!(front_of_class("Chrome_WidgetWin_1"), Front::App);
        assert_eq!(front_of_class(""), Front::App);
    }
}

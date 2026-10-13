//! Which process to capture: the app whose window fills its monitor in the foreground (a game
//! in fullscreen or borderless mode), a game the owner saved (also in a window), or, when the
//! owner turned it on, an app in front that keeps the GPU busy. Runs in the desktop app, which
//! (unlike the service) sees the signed-in user's desktop. A game stays the target while it
//! runs, also while alt-tabbed, until another game takes the foreground.

use super::CaptureTarget;

/// A window's or monitor's rectangle, physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The foreground window's owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    pub pid: u32,
    /// Executable file name, if the process could be opened.
    pub exe: Option<String>,
    pub class: String,
    pub window: Rect,
    pub monitor: Rect,
}

/// Desktop, shell and video / browser windows are fullscreen without being games.
const NOT_GAMES: &[&str] = &[
    "explorer.exe",
    "vyro.exe",
    "msedgewebview2.exe",
    "applicationframehost.exe",
    "lockapp.exe",
    "searchhost.exe",
    "searchapp.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "textinputhost.exe",
    "screenclippinghost.exe",
    "dwm.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "opera.exe",
    "brave.exe",
    "vivaldi.exe",
    "vlc.exe",
    "mpc-hc64.exe",
    "mpc-be64.exe",
    "potplayermini64.exe",
    "wmplayer.exe",
    "video.ui.exe",
    "microsoft.media.player.exe",
    "powerpnt.exe",
];
const NOT_GAME_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Windows.UI.Core.CoreWindow",
];

/// The window covers the whole monitor (exclusive fullscreen windows may overhang it).
pub fn covers(window: Rect, monitor: Rect) -> bool {
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
        && monitor.right > monitor.left
        && monitor.bottom > monitor.top
}

/// What the owner's saved games say about an executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// Saved as a game: detected in a window too.
    Game,
    /// Marked "Not a game".
    NotGame,
    Unknown,
}

/// A foreground app that may be a game: fullscreen, saved as a game, or (`busy`) keeping the
/// GPU busy. Never Vyro, the shell, browsers and players, or an app marked "Not a game".
pub fn is_candidate(fg: &Foreground, own_pid: u32, known: Known, busy: bool) -> bool {
    if fg.pid == 0 || fg.pid == own_pid || known == Known::NotGame {
        return false;
    }
    if NOT_GAME_CLASSES.contains(&fg.class.as_str()) {
        return false;
    }
    if known == Known::Game {
        return true;
    }
    let excluded = fg
        .exe
        .as_deref()
        .is_some_and(|e| NOT_GAMES.contains(&e.to_ascii_lowercase().as_str()));
    !excluded && (covers(fg.window, fg.monitor) || busy)
}

/// Picks the capture target from foreground samples taken about once a second.
#[derive(Debug, Default)]
pub struct Watcher {
    current: Option<CaptureTarget>,
    /// A new candidate must stay in front for two samples (a splash screen or a brief
    /// fullscreen flash does not switch).
    pending: Option<u32>,
}

impl Watcher {
    pub fn current(&self) -> Option<&CaptureTarget> {
        self.current.as_ref()
    }

    /// Feeds one sample; returns the new target when it changes. `game` tells whether the
    /// foreground app may be a game ([`is_candidate`]), `alive` whether a process still runs.
    pub fn step(
        &mut self,
        fg: Option<&Foreground>,
        game: impl Fn(&Foreground) -> bool,
        alive: impl Fn(u32) -> bool,
    ) -> Option<Option<CaptureTarget>> {
        if let Some(cur) = &self.current {
            if !alive(cur.pid) {
                self.current = None;
                self.pending = None;
                return Some(None);
            }
        }
        let candidate = fg.filter(|f| game(f));
        let Some(c) = candidate else {
            self.pending = None;
            return None;
        };
        if self.current.as_ref().is_some_and(|t| t.pid == c.pid) {
            self.pending = None;
            return None;
        }
        if self.pending != Some(c.pid) {
            self.pending = Some(c.pid);
            return None;
        }
        self.pending = None;
        let target = CaptureTarget {
            pid: c.pid,
            name: c
                .exe
                .clone()
                .unwrap_or_else(|| format!("Process {}", c.pid)),
        };
        self.current = Some(target.clone());
        Some(Some(target))
    }

    /// Stops following the current target (capture turned off).
    pub fn reset(&mut self) {
        self.current = None;
        self.pending = None;
    }
}

#[cfg(windows)]
pub use win::{alive, foreground};

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::{CloseHandle, RECT, STILL_ACTIVE};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    };

    use super::{Foreground, Rect};

    fn rect(r: RECT) -> Rect {
        Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }

    /// The foreground window's process, class and placement.
    pub fn foreground() -> Option<Foreground> {
        // SAFETY: plain Win32 queries on the foreground window handle; every out-pointer is a
        // local of the right type, and buffers are passed with their lengths.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
            let mut window = RECT::default();
            GetWindowRect(hwnd, &raw mut window).ok()?;
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: u32::try_from(std::mem::size_of::<MONITORINFO>()).ok()?,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &raw mut info).as_bool() {
                return None;
            }
            let mut class = [0u16; 256];
            let n = usize::try_from(GetClassNameW(hwnd, &mut class)).unwrap_or(0);
            Some(Foreground {
                pid,
                exe: exe_name(pid),
                class: String::from_utf16_lossy(&class[..n.min(class.len())]),
                window: rect(window),
                monitor: rect(info.rcMonitor),
            })
        }
    }

    fn exe_name(pid: u32) -> Option<String> {
        // SAFETY: the handle is checked and closed; the buffer length is passed in and updated.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = u32::try_from(buf.len()).ok()?;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &raw mut len,
            );
            let _ = CloseHandle(h);
            ok.ok()?;
            let path = String::from_utf16_lossy(&buf[..usize::try_from(len).ok()?.min(buf.len())]);
            path.rsplit(['\\', '/']).next().map(str::to_owned)
        }
    }

    /// Whether the process still runs.
    pub fn alive(pid: u32) -> bool {
        // SAFETY: the handle is checked and closed; the exit code is a local.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &raw mut code).is_ok();
            let _ = CloseHandle(h);
            ok && code == STILL_ACTIVE.0 as u32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect {
        left: 0,
        top: 0,
        right: 2560,
        bottom: 1440,
    };

    fn fg(pid: u32, exe: &str, window: Rect) -> Foreground {
        Foreground {
            pid,
            exe: Some(exe.into()),
            class: "UnrealWindow".into(),
            window,
            monitor: MON,
        }
    }

    #[test]
    fn fullscreen_apps_that_are_not_the_shell() {
        assert!(covers(MON, MON));
        let overhang = Rect {
            left: -8,
            top: -8,
            right: 2568,
            bottom: 1448,
        };
        assert!(covers(overhang, MON));
        let windowed = Rect {
            left: 100,
            top: 100,
            right: 1380,
            bottom: 820,
        };
        assert!(!covers(windowed, MON));
        let plain = |f: &Foreground| is_candidate(f, 1, Known::Unknown, false);
        assert!(plain(&fg(10, "Game.exe", MON)));
        assert!(!plain(&fg(10, "Game.exe", windowed)));
        assert!(!plain(&fg(10, "EXPLORER.EXE", MON)));
        assert!(!plain(&fg(10, "chrome.exe", MON)));
        assert!(!plain(&fg(1, "Game.exe", MON)), "Vyro itself");
        let mut desktop = fg(10, "Game.exe", MON);
        desktop.class = "WorkerW".into();
        assert!(!plain(&desktop));
        let mut unknown = fg(10, "x", MON);
        unknown.exe = None;
        assert!(plain(&unknown), "protected process: still a game");
    }

    #[test]
    fn saved_games_in_a_window_ignored_apps_and_gpu_load() {
        let windowed = Rect {
            left: 100,
            top: 100,
            right: 1380,
            bottom: 820,
        };
        let game = fg(10, "Game.exe", windowed);
        assert!(
            is_candidate(&game, 1, Known::Game, false),
            "saved: windowed counts"
        );
        assert!(
            !is_candidate(&fg(10, "Game.exe", MON), 1, Known::NotGame, true),
            "not a game"
        );
        assert!(
            is_candidate(&game, 1, Known::Unknown, true),
            "busy GPU in a window"
        );
        assert!(
            !is_candidate(&fg(10, "chrome.exe", windowed), 1, Known::Unknown, true),
            "browsers never"
        );
        assert!(
            !is_candidate(&fg(1, "Game.exe", windowed), 1, Known::Game, true),
            "Vyro never"
        );
    }

    #[test]
    fn follows_a_game_until_it_exits_or_another_takes_over() {
        let mut w = Watcher::default();
        let game = fg(10, "Game.exe", MON);
        let alive = |_| true;
        let g = |f: &Foreground| is_candidate(f, 1, Known::Unknown, false);
        assert_eq!(w.step(Some(&game), g, alive), None, "first sighting");
        let t = w.step(Some(&game), g, alive).unwrap().unwrap();
        assert_eq!((t.pid, t.name.as_str()), (10, "Game.exe"));
        assert_eq!(w.step(Some(&game), g, alive), None);
        // Alt-tab to the desktop: the game stays the target.
        assert_eq!(w.step(Some(&fg(2, "explorer.exe", MON)), g, alive), None);
        assert_eq!(w.step(None, g, alive), None);
        assert_eq!(w.current().map(|t| t.pid), Some(10));
        // A brief flash of another fullscreen app does not switch…
        let other = fg(20, "Other.exe", MON);
        assert_eq!(w.step(Some(&other), g, alive), None);
        assert_eq!(w.step(Some(&game), g, alive), None);
        // …staying in front does.
        assert_eq!(w.step(Some(&other), g, alive), None);
        assert_eq!(w.step(Some(&other), g, alive).unwrap().unwrap().pid, 20);
        // The game exits.
        assert_eq!(w.step(None, g, |pid| pid != 20), Some(None));
        assert!(w.current().is_none());
    }
}

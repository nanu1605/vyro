# Field check: 1.1.1 (widgets through Show desktop)

Status: **not run yet.**

1.1.1 changes one thing: widgets set "Behind windows" stay in view when Windows shows the
desktop (Win+D, or the far right corner of the taskbar). Before, they disappeared until a
window was switched to. ROADMAP → 1.1.1 has how it works.

The test build is the `vyro-windows-<commit>` artifact of the latest CI run on
`claude/blissful-knuth-begnu5` (Actions → the run → Artifacts). Install the MSI over 1.1.0. About
15 minutes.

## Before you start

- Two or three widgets **Behind windows** and one **On top** (the Widgets page → Sits).
- A few apps open over part of the widgets (a browser, Explorer).

## 1. Show desktop

- **Win+D.** The behind-windows widgets stay in view, over the desktop. The on-top widget too.
- **Win+D again.** Your windows come back, and the behind-windows widgets are under them as
  before.
- **The corner of the taskbar** (click the far right end) does the same as Win+D, both ways.
- **Ending it by opening an app** (taskbar or Alt+Tab) while the desktop shows: the widgets go
  back under your windows.

## 2. Things that should not change

- **Clicking the desktop** (not Win+D) with windows open: the widgets stay under your windows
  and don't jump over them.
- **While the desktop shows**, open Start, click the taskbar, then drag a widget somewhere
  else. It stays in view the whole time and keeps its new place.
- **Hovering the corner of the taskbar** ("peek"): windows turn see-through. Widgets stay where
  they are. That is fine, and unchanged.
- **On top** widgets stay over everything, as in 1.1.0.

## 3. Explorer restarted

- Task Manager → Windows Explorer → Restart. Then Win+D and Win+D again: the same as in
  section 1.

## 4. Footprint

- With the widgets showing and Vyro in the tray, Task Manager's CPU for Vyro (the app and its
  WebView2 processes) at rest is as before (no extra work while nothing happens on the screen).

## Recording results

- **Passes:** ROADMAP → 1.1.1 gets "Field check (`<PC>`, `<date>`, test build
  `vyro-windows-<commit>`)".
- **A check fails:** note what you saw (and your Windows version, from `winver`). The fix goes
  on the branch, and the checks it touches are done again.
- **Release:** after the owner's go, the branch goes into `main` and the Release workflow builds
  1.1.1.

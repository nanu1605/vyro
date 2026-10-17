# Field check: 1.1 (P28 – P30) in one pass

Status: **passed (2026-10-17, `vyro-windows-d48c153`); 1.1.0 released.** It took three rounds:
- The first pass (2026-10-16, `vyro-windows-123a938`) found four problems: widgets did not
  appear, Hardware and Sensors waited forever, Quit did nothing, and scrolling was laggy.
- The second report (`vyro-windows-227e896`): the sidebar's highlight sat off the chosen view
  after widening a small window.
- All were fixed (ROADMAP → P31) before the passing run.

P28 to P30 are built on one branch, `claude/blissful-knuth-begnu5`, so one test build holds all
three:
- the `vyro-windows-<commit>` artifact of the latest CI run on that branch (Actions → the run →
  Artifacts);
- unzip it and install the MSI over 1.0.1.

Results go into each phase's ROADMAP entry (see the end). 1.1.0 is released after the checks
pass and the owner says go.

## What to have at hand

- Fan control on, as you use it.
- Task Manager.
- A game you play with the overlay on.
- A laptop, if you have one (Vyro installed on it the same way).
- A second monitor, if you have one.

## 0. Install over 1.0.1 (5 min)

- The installer is under 60 MB (decision 28).
- Settings, the Dashboard layouts, alert rules and fan curves are as before.
- Vyro opens on the Dashboard, or on the last view if that was your choice.

## 1. The new look (P28, 20 min)

- **Sidebar.** Four groups (Overview, Performance, This PC, Vyro). The highlight slides to the
  chosen view. Folding it to icons is remembered after a restart, and a narrow window
  (under 1100 px) folds it by itself. Health, Insights and Hardware keep their dots. After a
  small window is made full screen, or the sidebar is expanded, the highlight sits on the
  chosen view.
- **Ctrl+K.** It finds a view, a setting (opens the card), a sensor (opens it in History), a
  theme or accent, and "Return fans to BIOS".
- **Accent.** Change it in Settings → Appearance. The sidebar, buttons, switches, the gauges
  and the 3-D model all change with it, in every theme.
- **The whole width.** On your monitor nothing is left empty at the sides. At 1500 px or wider
  the Dashboard's live rail shows, and folding it is remembered.
- **Motion.** Cards lift under the mouse, views rise in, and toasts and dialogs spring in.
  The profile's "calmer" shortens all of it and "off" stops it. Windows' "Animation effects"
  off stops it too.
- **Overlay.** In Settings → Overlay, the colour "App accent" follows the app's accent.

## 2. My PC (P29, 20 min)

- **Every fan turns:** the cooler, the two front fans, the rear fan and the GPU fans. A fan
  without a reading turns slowly and says "Speed not reported". A real 0 RPM shows as
  stopped.
- **Tools.** "Explode" pulls the parts apart and back. "Airflow" shows air from the front to
  the back, faster when the fans are faster. Hovering a part names it, and double-clicking
  flies to it ("Reset view" goes back). The GPU view and the thermal map work.
- **Motion.** It runs smoothly at full animation. "Calmer" drops the bloom and the turning,
  and "off" draws only when a reading changes. Leaving My PC frees the GPU (Task Manager →
  the GPU process).
- **Laptop**, if you have one:
  - My PC shows a laptop without being told.
  - The lid's screen shows live readings.
  - "See inside" shows the heat pipes and blowers.
  - Settings → My PC → Model switches between laptop and desktop.

## 3. A game with the overlay (1.0.1's graph, P28's colour; 15 min)

- The overlay's graph scrolls smoothly, with no jumps between readings.
- The "App accent" colour is readable on the game.

## 4. Desktop widgets (P30, 30 min)

- **Adding.** Add one of each kind from the Widgets page. Each appears down the right edge of
  your main monitor without taking focus from what you were typing in.
- **Moving.** Drag each one somewhere else and close Vyro to the tray. Then quit and start it
  again: they are where you left them. Locked, a widget no longer moves when dragged.
- **Right-click.** Lock or Unlock, Edit… (opens the Widgets page on the main window), Remove.
- **Settings.** Size, background, style and readings change the widget on the desktop within
  a second. "Behind windows" stays under other windows; "On top" stays over them.
- **Win+D.** Note what happens to "Behind windows" and "On top" widgets, and when the first
  ones come back. The README says they can hide until you switch to a window.
- **The tray.** "Desktop widgets" hides and shows them all. With the main window in the tray,
  they still update every second (Desktop profile).
- **Sign out and in** (with "Start with Windows" on): the widgets come back with Vyro in the
  tray.
- **Look.** A theme or accent change reaches the widgets. Glass reads well on your wallpaper.
- **Second monitor**, if you have one: put a widget on it, then unplug the monitor. The widget
  comes back on the main monitor. At 125 % or 150 % display scaling, widgets are sharp and
  the right size.

## 5. Footprint for 1.1 (30 min, after a restart)

Follow [`footprint.md`](footprint.md) → "Field measurement for 1.1". The figures go into the
README's Performance table.

## Defaults to confirm

Each was chosen without you. Say yes or what to change:

| Phase | Most worth a look |
|---|---|
| P28 | The four groups and the order of the views; the live rail from 1500 px; the motion durations |
| P29 | 60 fps on My PC with full animation; two front fans and a rear one always drawn; the slow turn of a fan with no reading; bloom on the lights |
| P30 | Up to 8 widgets; a new widget's look (the app's card, 85 % background, behind windows); new widgets down the right edge; no desktop pinning yet (the Win+D limit) |

## Recording results

- **A phase passes.** Its ROADMAP entry gets
  "Field check (`<PC>`, `<date>`, test build `vyro-windows-<commit>`)".
- **A check fails.** Note it in that phase's entry. The fix goes on the branch, and the checks
  it touches are done again.
- **Release.** With all three passed and the owner's go, the branch merges into `main` and
  the Release workflow builds 1.1.0.

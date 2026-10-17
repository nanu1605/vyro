# Field check: 1.2 (widgets that snap, let clicks through, three new kinds)

Status: **not run yet.**

1.2 adds to the desktop widgets (ROADMAP → 1.2.0 has how each works):
- they snap to the screen's edges and to each other while you drag them;
- a locked widget can let clicks through;
- three new kinds: Network, Game FPS and Top processes.

**Order.** Do the 1.1.1 check ([`field-check-1.1.1.md`](field-check-1.1.1.md)) first, on its own
build. This one is for the `vyro-windows-<commit>` artifact of the latest CI run on
`claude/blissful-knuth-begnu5` (Actions → the run → Artifacts), version 1.2.0. Install the MSI
over what you have. About 20 minutes.

## Before you start

- On the Widgets page, "Unlock all to arrange", and have three or four widgets on the desktop.
- If you have a second monitor, keep it connected.

## 1. Snapping

- **Screen edges.** Drag a widget toward each edge of the screen. Within about 10 px it jumps
  flush to the edge. At the bottom, it stops at the taskbar, not under it.
- **Beside another widget.** Drag one widget next to another. It stops a small gap (8 px)
  away, and its top lines up with the other's when they are close.
- **Under another widget.** It stops the same gap below, with the left or right edges in line.
- **Far away.** A widget across the screen does not pull.
- **Shift.** Hold Shift while dragging: no snapping, it goes exactly where you leave it.
- **Second monitor** (if you have one): the same against that screen's edges.
- **Kept.** Quit Vyro from the tray and start it again: each widget is back where it snapped.
- **Display scaling** other than 100 % (Settings → Display → Scale), if you use it: the pull
  and the gap feel the same.

## 2. Clicks pass through

- Pick a widget over part of a window. On the Widgets page, lock it and turn on
  **Clicks pass through**.
- Click, scroll and select text through the widget: it all reaches the window under it.
  Right-click on the widget opens that window's menu, not the widget's.
- The widget keeps updating, and still stays in view through Win+D (1.1.1).
- **Unlock it on the Widgets page:** the widget catches the mouse again, and you can drag it
  and right-click it. While it is unlocked, the switch is greyed out.
- **"Unlock all to arrange"** does the same for every widget.

## 3. The new kinds

Add one of each from the gallery.
- **Network:** during a download (or a speed test) the down figure and its line rise, with the
  adapter's name on top. Unplug the cable, or turn Wi-Fi off: "Not connected" and dashes.
- **Game FPS:** in a game Vyro measures (the Gaming page shows it), the game's name, FPS,
  1% low and frame time, close to the overlay's figures. With no game: "No game running".
- **Top processes:** with a browser open, it shows as one row with "×N". Switch "Busiest by"
  between CPU, Memory and GPU: the list and the title follow. Compare with Task Manager: the
  same programs near the top.
- Each kind at 50 % and 200 % size, in the Glass and Solid styles: nothing is cut off.

## 4. Footprint

- With the widgets showing and Vyro in the tray, Task Manager's CPU for Vyro at rest is as
  with 1.1 (snapping only runs during a drag).

## Recording results

- **Passes:** ROADMAP → 1.2.0 gets "Field check (`<PC>`, `<date>`, test build
  `vyro-windows-<commit>`)".
- **A check fails:** note what you saw (and your Windows version, from `winver`, and display
  scaling). The fix goes on the branch, and the checks it touches are done again.
- **Release:** after the owner's go, the branch goes into `main` and the Release workflow builds
  1.2.0.

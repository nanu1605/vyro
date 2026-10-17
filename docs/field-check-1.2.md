# Field check: 1.2 (widgets through Show desktop, snapping, clicks passing through, three new kinds; My PC fixes; text inside widgets and the Dashboard's packing)

Status: **check 1 run on 2026-10-17, with six findings, all fixed. On the b4748f3 build the owner
then found three more (widget text past the edge, the thread bars, blank cards), all fixed.
Check 2 not run yet.**

1.2 is one release that also carries 1.1.1, at the owner's request. ROADMAP → 1.1.1 and
1.2.0 have how each part works. It adds to the desktop widgets:
- widgets set "Behind windows" stay in view through Show desktop (1.1.1);
- widgets snap to the screen's edges and to each other while you drag them;
- a locked widget can let clicks through;
- three new kinds: Network, Game FPS and Top processes.

After check 1 it also fixes:
- new widgets overlapping, and snapped widgets getting stuck;
- My PC's 3-D view: its size, the white haze, front case fans standing still, and airflow,
  which now shows as wind-tunnel smoke lines.

After the b4748f3 build it also fixes:
- text running past a widget's edge (the clock with two GPU readings);
- the Dashboard's thread bars filling half their card, and cards stretched into blank space.

**Build.** Use the `vyro-windows-<commit>` artifact of the latest CI run on
`claude/blissful-knuth-begnu5` (Actions → the run → Artifacts), version 1.2.0. Install the MSI
over what you have. It takes about 40 minutes.

## Before you start

- On the Widgets page, "Unlock all to arrange".
- Have two or three widgets **Behind windows** and one **On top** (the Widgets page → Sits).
- Open a few apps over part of the widgets (a browser, Explorer).
- If you have a second monitor, keep it connected.

## 1. Show desktop (from 1.1.1)

- **Win+D.** The behind-windows widgets stay in view, over the desktop. The on-top widget
  stays too.
- **Win+D again.** Your windows come back, and the behind-windows widgets are under them again.
- **The far right corner of the taskbar** (click it) does the same as Win+D, both ways.
- **Opening an app** (taskbar or Alt+Tab) while the desktop shows: the widgets go back under
  your windows.
- **Clicking the desktop** (not Win+D) with windows open: the widgets stay under your windows.
- **Switching** a widget between "Behind windows" and "On top": it stays shown, and moves under
  or over your windows.

## 2. Placing and snapping

These retest check 1's findings 1 and 2.
- **Adding widgets.** Add three widgets one after another from the gallery. Each appears in a
  free spot down the right edge of the screen, under the last one. None overlaps another.
- **Screen edges.** Drag a widget toward each edge of the screen. Within about 10 px it jumps
  flush to the edge. At the bottom it stops at the taskbar, not under it.
- **Beside and under another widget.** It stops a small gap (8 px) away, with the edges in line
  when they are close.
- **Coming away again.** Drag a snapped widget away *slowly*: from an edge, and from a
  neighbour it sits beside. It follows the mouse as soon as you are about 10 px away. It never
  sticks. Do this with two widgets side by side, each in turn.
- **Far away.** A widget across the screen does not pull.
- **Shift.** Hold Shift while dragging: no snapping, and it goes exactly where you leave it.
- **Second monitor** (if you have one): the same against that screen's edges.
- **Kept.** Quit Vyro from the tray and start it again: each widget is back where you left it.

## 3. Clicks pass through

- Pick a widget over part of a window. On the Widgets page, lock it and turn on
  **Clicks pass through**.
- Click, scroll and select text through the widget: it all reaches the window under it.
- **Unlock it on the Widgets page:** the widget catches the mouse again, and can be dragged and
  right-clicked. While the widget is unlocked, the switch is greyed out.

## 4. The new kinds, and text inside every widget

Add one of each from the gallery.
- **Network:** during a download, the down figure and its line rise, with the adapter's name
  above them. With the cable unplugged or Wi-Fi off, it shows "Not connected".
- **Game FPS:** in a game Vyro measures, it shows the game's name, FPS, 1% low and frame time.
  With no game, it shows "No game running".
- **Top processes:** a browser shows as one row with "×N". "Busiest by" switches between CPU,
  Memory and GPU.
- **Text inside** (the b4748f3 finding): on each widget, nothing runs past its edge. A name too
  long for the room ends in "…".
  - A clock with "GPU temp (GPU 1)" and "GPU hotspot (GPU 1)" shows them one a row, the values
    on the right.
  - A stack, a gauge and a reading of your second GPU (the processor's graphics) also show in
    full or end in "…".
  - Try 150 % and 50 % sizes too.

## 5. My PC

These retest check 1's findings 3 to 6.
- **The whole PC shows.** Maximise the window, then make it small: the 3-D view fills the space
  below the bar, and the whole case fits, top to bottom. "Reset view" frames it again after
  you turn or zoom. Explode: the parts fit too.
- **Seeing inside.** Turn the PC to look straight at the glass, then from above and from below.
  The parts inside are clear and the inside of the case stays dark, with no white haze. Check
  the PC view and the Thermal map. Under the map, each part shows its colour (blue, green,
  amber, red) plainly.
- **Case fans.**
  - The two front fans and the rear fan turn. Ones the board doesn't read turn slowly, and
    their details say "Speed not reported".
  - A fan the board does read turns at its speed.
  - A fan that has turned this session and then stops, as a GPU's fans do at idle, stands
    still.
- **Airflow** (the button on, PC view or Thermal map):
  - Thin smoke lines run in at the front fans, curve round the graphics card and the RAM, and
    leave by the rear fan and through the radiator (or the cooler, on a tower cooler), with
    bright streaks running along them.
  - Under load, the lines past the CPU's cooler and the GPU turn orange.
  - Explode fades them out.
- **Laptop** (Settings → My PC → Model → Laptop, if you want to look): it is framed whole, and
  the airflow runs through each blower and out of the back.

## 6. Dashboard

These retest the b4748f3 findings on your own layout, at your usual window size and maximised.
- **Logical processors:** the 16 bars run across the whole card, in one row on a wide card or
  in even rows on a narrow one. On CPU sensors, the per-core power reads 4 and 4.
- **No blank cards:** each card is as tall as what it shows.
  - CPU sensors is short, and Power and Board sit right under it.
  - Memory's card ends after its timings, and Vyro itself sits under it.
  - A row of reading tiles, or the two gauges, stays in line.
- **GPUs:** the AMD graphics' card shows what it reports, then one line "Not reported: …"
  instead of rows of "—". Made 8 or 12 columns wide, the two GPUs sit side by side, and a card's
  lists run in two or three columns.
- **Editing:** "Edit layout" shows the same arrangement. Resize a card taller: it keeps that
  height and the cards under it move down. Move a card: the others close the gap.

## 7. Footprint

- With widgets showing and Vyro in the tray, Task Manager's CPU for Vyro at rest is the same as
  with 1.1. Snapping only runs during a drag.
- On My PC with Airflow on, the app's CPU is no higher than in 1.1. The lines are drawn on the
  graphics card; each frame only moves them.

## Recording results

- **Passes:** ROADMAP → 1.2.0 gets "Field check 2 (`<PC>`, `<date>`, test build
  `vyro-windows-<commit>`)".
- **A check fails:** note what you saw, with your Windows version (from `winver`) and display
  scaling. The fix goes on the branch, and the checks it affects are run again.
- **Release:** after the owner's go, the branch goes into `main`, and the Release workflow
  builds 1.2.0.

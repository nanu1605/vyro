# Footprint audit (P27)

Status: **audited 2026-10-16 from the code and each phase's declared budget**. The numbers to
sign off are measured on the owner's PC at the P27 field check (procedure below). The spec §8
budget still holds:
- under 1.5 % CPU on average with the window visible;
- under 120 MB with the window visible;
- under 2 s to first data;
- an installer under 20 MB (under 60 MB from 1.1, decision 28).

V2_PLAN's rule for every V2 feature: render on demand, suspend when hidden, and stay off the 1 Hz
tick.

## What each V2 feature costs, and when

"Hidden" means the window is closed to the tray. WebView2 is then suspended (P9a), so no page
timer, animation or WebGL runs; the engine keeps polling at the tray rate (5 s).

| Feature | Threads | Per tick | While hidden | Declared budget | Verified |
|---|---|---|---|---|---|
| Fan control (P12) | `vyro-fans` (service; only on the allow-listed board) | The decision in the poller; writes off the tick | Keeps driving, at most every 2 s | — | P12 field: 125 MB, 0.46 % CPU |
| Game profiles (P13) | None (the app's `vyro-game-watch`, from P10) | One check a second | The game's or the profile's rate | ±2 MB | P13 field |
| Overlay 2.0 (P14) | None (the overlay window only while a captured game is in front) | Redraw ≤ 2 Hz | n/a | A few MB, only while shown | P14 field |
| Session replay (P15) | None | One row a second while a game runs | Same | Inside the 200 MB history cap | Tests |
| Hardware changes (P19) | `vyro-changes` (app, once at start and on device changes) | None | None | — | P19 field |
| Insights (P16) | `vyro-intel`: short-lived, at most every 10 min | The minute digest, under 0.1 ms | Same | +2 MB, under 250 ms a pass | Tests and timing |
| Compound alerts (P18) | None | Rules × conditions; rate buffers ≤ 10 min | Same | +1 MB | Tests |
| Performance Score (P17) | In the intel pass, once a day | None | Same | Under 300 ms a day | Tests and timing |
| Benchmark (P20) | Only during a run, all joined after it | None | None | Nothing when idle | P20 build |
| Diagnostics (P21) | A blocking task, about 1 s, on demand | None | None | Nothing when idle | P21 build |
| Themes, cards (P25) | None | None | None | Main bundle +6 kB | Bundle check |
| Devices (P22) | One parked thread waiting for device changes | None | None (reads only while the tab is open) | Main bundle +2 kB | P22 build |
| Dashboard builder (P24) | None | Widgets use the tick's snapshot | Suspended | +1 MB with 40 widgets | P24 build |
| My PC 3-D (P23) | None (WebGL in the page) | None | Context freed; nothing runs | ≤ 30 fps while shown, GPU process +40 MB | P23 build |
| Remote companion (P26) | `vyro-companion` (on only) and one per connection | One shared view per reading | 1 Hz while a phone watches | Under 1 MB on, nobody watching | P26 build |
| Screen on the phone (1.7) | Only during a session: `vyro-screen-rtc`, `vyro-screen` (capture and H.264), `vyro-screen-idle` | None | Same | Nothing without a session; during one, measured at field check 1.7 | Tests; field check 1.7 |

The frontend's repeating timers were audited too:
- History, Insights, Score, the Dashboard's history and events widgets, Devices, Benchmark,
  System and the Remote card each run a timer only while their tab or card is on screen, and
  the timer stops when it is taken off.
- None runs while the window is hidden, because WebView2 is suspended.

## What CI now holds (P27)

- **Installer:** under 20 MB (as before; 60 MB from 1.1).
- **Page scripts and styles at start** (`scripts/bundle_budget.py`): the app's window under
  125 kB gzipped (118.2 kB at P27; 200 kB from 1.1) and the phone companion under 32 kB
  (29.2 kB). Lazy chunks (the 3-D scene, Benchmark, the dashboard editor, …) are listed but not
  budgeted.
- **The installed service** on CI's Windows runner (`vyro-probe`): its CPU and memory over 30 s
  go to the run's summary. They are reported, not gated, because a CI virtual machine has few
  sensors.

## Field measurement for 1.0

On the reference PC, Desktop profile, after a restart. Use the "Vyro itself" card, with Task
Manager to cross-check:

1. **Idle.** Window visible on the Dashboard for 5 minutes: average CPU, memory, startup.
2. **Hidden.** Closed to the tray for 5 minutes: memory (WebView2 suspended).
3. **Every tab.** Visit each tab once, then reset the peaks and open History at 30 min, 24 h and
   7 days: the peak.
4. **On demand.** My PC in 3-D for a minute (CPU, GPU process); a benchmark run; a diagnostics
   run; the companion with a phone watching for 5 minutes.
5. **Gaming.** A game with the overlay in Full mode for 10 minutes.

Each result goes into the README's Performance table under 1.0.

## 1.1 (P28–P30)

Decision 28 raised the installer budget to 60 MB and the window's start bundle to 200 kB
gzipped; the RAM and CPU budgets above are unchanged.

| Feature | Threads | Per tick | While hidden | Declared budget | Verified |
|---|---|---|---|---|---|
| New frame and motion (P28) | None | None; animations run only on a change (a view, a hover, a toast) | Suspended | Start bundle 139.1 of 200 kB | Bundle check, sweep |
| My PC 3-D (P29) | None (WebGL in the page) | Drawn each frame while shown: 60 fps with full animation, 30 "calmer", on change with animation off | Context freed; nothing runs | GPU process as P23 plus bloom; lazy chunk 162.8 kB | P29 build |
| Desktop widgets (P30) | One short-lived thread per drag (saves the place once still) | The snapshot is also sent to the widget windows | Widgets keep the profile's rate; the main window stays suspended | One WebView2 view per widget in the shared browser process | P30 build |

## Field measurement for 1.1

As for 1.0, plus:

1. **My PC.** One minute on the tower and on the GPU view with full animation, then "calmer":
   CPU and the GPU process.
2. **Widgets.** Three widgets on the desktop with the window closed to the tray for 5 minutes:
   memory of the WebView2 processes and the app; then with eight.

Each result goes into the README's Performance table under 1.1.0.

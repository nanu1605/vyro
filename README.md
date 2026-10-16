# Vyro

A low-footprint Windows hardware monitor: animated CPU/GPU speedometers, per-subsystem
sensors, history, health and alerts, with direct sensor access (no third-party monitoring
app or library).

- **Build spec:** [`PROJECT_PLAN.md`](PROJECT_PLAN.md) — what to build and why.
- **Roadmap:** [`ROADMAP.md`](ROADMAP.md) — milestones, phase tracker, decision log.

**Status:** 1.0 (milestone M13; V2 complete). Live CPU, GPU, board, fan, voltage, RAM, storage
and network sensors on Windows (low-level ones through PawnIO, read by the Vyro Sensor Service),
history, System Health, alerts, game frame rates (PresentMon), tray and an MSI installer. New in
1.0: Insights with a Performance Score, alerts with several conditions and actions, a benchmark,
one-click diagnostics and reports, themes, a dashboard builder, a My PC view, a Devices tab and
a phone companion (the sections marked (1.0) below). 0.3 added automatic game profiles, a
configurable in-game overlay and a replay of each game session (see [Gaming](#gaming)). 0.2
added fan and pump control, so far only on the MSI B650M GAMING PLUS WIFI (MS-7E24). See
[`ROADMAP.md`](ROADMAP.md) for what is verified on which hardware.
Contributions: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Install

Releases are published by the Release workflow (see [CONTRIBUTING](CONTRIBUTING.md#releasing)):
install `Vyro_<version>_x64_en-US.msi` from the latest release. Installed copies look for a new
release daily and update themselves from Settings → Updates (one administrator prompt; app and
service are both replaced). Every CI run also uploads a test build, `vyro-windows-<commit>`,
which never updates itself.

The MSI installs Vyro for all users with a Start-menu entry, installs the signed
[PawnIO](https://github.com/namazso/PawnIO) driver silently if it is missing (it reads CPU
temperature and power, fans, voltages and RAM sensors; it stays installed when Vyro is removed),
and installs the **Vyro Sensor Service**. The service runs the sensor engine with the rights
PawnIO and SMART need, keeps history while the app is closed, and serves the app over a local
named pipe; the app itself runs without administrator rights. Intel's
[PresentMon](https://github.com/GameTechDev/PresentMon) console app is installed beside the
service, which runs it to measure the game in front (nothing is injected into games). Without the service the app still
starts, reads what it can, and says what is missing. "Start with Windows" (Settings) opens it
in the tray at sign-in. Builds are not code-signed yet, so SmartScreen will warn.

## Fan and pump control

The **Cooling** tab sets each fan header to the BIOS curve, a fixed speed or Vyro's own curve
on any temperature sensor, per profile. It is off until you turn it on and confirm. Control runs
in the service, so it keeps going with the app closed. Safeguards:

- Fans never go below 20 % and pumps never below 60 %. A pump header is driven only after you
  confirm a pump is on it, and a pump that stops reporting a speed ends control.
- CPU at 95 °C or GPU at 90 °C (both adjustable), or a source sensor without a reading, sends
  every controlled fan to 100 %.
- The BIOS settings are saved before the first change. They are put back on Return to BIOS
  (Cooling tab, header chip or tray), when the service stops, before sleep, on uninstall, when
  a write doesn't read back, when another program takes over the fans, and at the next start
  after a crash. If the control loop stalls for 10 s, the BIOS takes over until it recovers.

Only boards that have been checked on real hardware can be controlled: today the MSI B650M
GAMING PLUS WIFI (MS-7E24, Nuvoton NCT6687D). Every other board shows its fans read-only.
[`docs/control-safety.md`](docs/control-safety.md) lists every safety rule with the code that
enforces it, its tests and its result on that board.

## Gaming

- **Automatic game profiles.** A fullscreen or borderless game switches Vyro to the Gaming
  profile, or to the settings saved for that game (profile or "Don't switch", whose fan
  settings, overlay, update rate 0.5–5 s), and everything goes back when it closes. A saved
  game is also recognised in a window; an opt-in trigger also counts an app using at least 60 %
  of the GPU for 10 s. A profile you pick during a game wins until the game closes; the Gaming
  tab offers "Keep Desktop" and "Not a game". Settings → Gaming turns it off.
- **Overlay.** Minimal, Performance, Full and Graph modes; corner and offset, size, background
  opacity, your own choice of readings, refresh every 0.5 / 1 / 2 s, colour and a compact
  layout (Settings → Overlay, with a live preview). It is click-through, never takes focus,
  and cannot show over exclusive fullscreen.
- **Session replay.** The Gaming tab keeps every reading of the last 100 sessions, inside the
  200 MB history cap: frame rate, frame time, use, temperatures and power on one time axis,
  with markers for frame-rate drops, stutters, thermal and power limits, switching away and
  back, profile changes and the overlay shown or hidden. Any moment also shows a minute either
  side of the PC's sensor history, while that history is kept (7 days).

0.3 moves the history database (`%ProgramData%\Vyro\history.sqlite`) to a new format that 0.2
cannot read: after going back to 0.2 its history stays off until that file is deleted. 0.2's
session log (`games.json`) is imported once, as summaries without a replay.

1.0 adds tables to the history database (schema v3), which 0.3 cannot read: after going back to
0.3 its history stays off until that file is deleted. If any alert rule uses 1.0's conditions
or actions, 0.3 moves `alerts.json` aside (to `alerts.json.bad`) and uses its default rules.

## Insights, Performance Score and alerts (1.0)

- **Insights.** A tab after Health that compares the last half hour with this PC's own past
  week at the same load, and says what changed:
  - running hotter than usual, or a fan slower than usual;
  - slowed by heat, or close to a temperature limit;
  - a game slower than its earlier sessions, or what set its frame rate;
  - a program busy in the background during a game.

  Each finding shows its evidence ("Show in History"), whether it was measured or inferred,
  and how sure Vyro is. A cause is called likely only when it can produce the effect, came
  just before it began, and is the only candidate. Hardware and driver changes, profile
  switches, fan settings and busy programs are the candidates. Anything else that merely
  happened around the same time is listed apart. A change you made yourself, such as the
  Silent profile, is shown as the explanation, not as a warning. The check runs every 10
  minutes and puts a dot on the tab when something is worth a look; it never notifies. To
  spot background load, the names of the five busiest programs of each minute are kept with
  the history for 7 days (names only).
- **Performance Score.** At the top of Insights. One number from:
  - thermal headroom and sustained clocks;
  - throttling and cooling;
  - drive health and free space;
  - memory use and the network;
  - stability: unexpected shutdowns and hardware (WHEA) errors in the System log.

  Each input shows its sub-score, its weight and the values behind it. The score appears
  after about a day of history, is recorded daily, and shows a year's trend. The formula is
  fixed and versioned: [`docs/performance-score.md`](docs/performance-score.md).
- **Alerts.** A rule (Health tab) can combine up to five conditions, all of them or any one:
  - above or below a limit;
  - rising or falling by an amount within a set time;
  - above or below the sensor's usual reading. For a CPU or GPU temperature, "usual" means
    usual for the load at that moment.

  A hold time, a cooldown and a clearing margin keep a reading that hovers at the limit from
  flapping. Besides a notification (with or without sound, or none), a rule can:
  - put its name in the tray tooltip;
  - switch the profile;
  - raise the fans Vyro drives by 5–50 points;
  - save a diagnostic record: the readings and the five busiest program names.

  The profile, tray and fan actions end 30 s after the conditions clear, and a profile you
  pick meanwhile wins. Fans are raised only while fan control is on, never on a header left
  to the BIOS, and the safety full-speed rules still come first. Two alerts about the same
  part within a minute notify once; a hardware problem always notifies.

## Benchmark and diagnostics (1.0)

- **Benchmark.** A tab that measures this PC against itself. It covers:
  - CPU single- and multi-thread;
  - memory and storage;
  - GPU compute and graphics (Direct3D 11 on the high-performance GPU).

  Quick takes about 3 minutes. Full, about 13 minutes, adds ten one-minute loops of CPU and
  GPU load together to show whether performance holds. Each area runs a warm-up and three
  measured passes, and reports the median and the spread, with temperatures, power and
  throttling. The run is compared with your last one, and anything that changed in between is
  listed: setup, drivers, hardware. Every result is checked against known checksums and data
  read back, so an unstable PC shows up as errors, not as a fast score.

  A run stops on Stop, on closing the window, at Health's problem limits, when a game starts,
  or when readings go missing; all load ends within a second. The storage test uses a 1 GiB
  temporary file, deleted afterwards. Scores compare this PC with itself, not with other tools
  or PCs: [`docs/benchmark.md`](docs/benchmark.md).
- **Diagnostics.** "Run diagnostics" in Health checks 13 areas in about a second, without
  loading the PC: thermals, clock stability, power limits, throttling, fans and pump, memory,
  storage, network, hardware errors, changes from usual, drivers, sensors and the service.
  - Each area is Pass, Attention, Info or Unavailable.
  - Every Attention cites the readings behind it, with a link into History.
  - A report exports as HTML, JSON, Markdown or PDF to `Documents\Vyro Reports`.
  - Serial numbers are left out unless you tick "Include serial numbers".

  More in [`docs/diagnostics.md`](docs/diagnostics.md).

## Looks, layouts and your PC (1.0)

- **Themes.** Settings → Appearance offers Midnight, AMOLED, Cyber, Glass, Minimal and
  Daylight, each with its own accent or one of five more. OK, warning and problem colours keep
  their meaning in every theme, and every theme meets WCAG AA contrast. A thin rail under the
  header shows the PC's state: idle, gaming, warning or critical. Critical pulses four times,
  then holds; it does not pulse with reduced motion. Vyro opens on the Dashboard, or on the
  last tab if you choose.
- **Dashboard builder.** "Edit layout" on the Dashboard adds, moves, resizes, duplicates and
  removes widgets, with the mouse or the keyboard. Widgets:
  - gauges, and any of about 35 readings;
  - a minute's graph, or an hour of history;
  - the game's frame times;
  - Health, events and the familiar cards.

  Up to 12 layouts can be saved, with a picker. Seven presets ship: Default, Gaming, Work,
  Streaming, Benchmark, Monitoring and Minimal. A layout can be locked, and a preset reset.
- **My PC.** A tab after Hardware with three views:
  - your PC: a stylized tower with your cooler, RAM sticks, GPU, drives and fans;
  - your GPU;
  - a thermal map, coloured by how close each part is to its Health limits.

  Fans spin at their reported speed, and clicking a part shows its details. If the GPU does
  not look like your card, you can correct it. The models are Vyro's own, with no brand
  logos. The 3-D view uses WebGL only while the tab is shown, and a flat 2-D plan is there
  instead if you prefer.
- **Devices.** A tab after Displays lists keyboards, mice, controllers, headsets and other
  USB and Bluetooth devices, each once, with how it connects. It shows the battery where the
  device reports one (Bluetooth, Bluetooth LE and Xbox controllers; 2.4 GHz receivers usually
  do not). It also lists the sound outputs and inputs, with the defaults. The list updates
  within a second when a device arrives or leaves. Device names stay on this PC: they are not
  put in reports, cards or the history.
- **Share card.** In Hardware: a picture of your PC's specs and Performance Score, in 16:9,
  square or story size. It is saved to `Pictures\Vyro` or copied to the clipboard, and never
  shows the computer's name or serial numbers.

## Desktop widgets (1.1)

The Widgets page (in the sidebar, under Vyro) puts live readings on your desktop, up to eight
widgets:

- **Gauge:** one reading on a dial.
- **Reading:** one reading, large.
- **Graph:** one reading and its last minute.
- **Stack:** up to six readings, one under the other, with bars.
- **Clock:** the time and date, with two readings.

Any reading the Dashboard has can go on a widget. Each widget has a size (50–200 %), a
background strength, a style (the app's card, see-through glass, or solid) and a place: behind
your windows (the default) or on top of them. Drag an unlocked widget to move it; Vyro
remembers where it is. Right-click a widget to lock it, edit it or remove it. "Desktop
widgets" in the tray icon's menu shows or hides them all, and they come back when Vyro starts.

Widgets follow your theme and accent, and update at your profile's rate while the window is
in the tray. Windows' Show desktop (Win+D) can hide widgets that sit behind windows until you
switch back to a window; choose "On top" to keep one in view.

## Phone companion (1.0)

**Off by default.** Settings → Remote turns on a read-only page that a phone or another
computer on your local network can open. It shows CPU, GPU, memory, drives, fans,
temperatures, power, FPS, the network, Health and active alerts, refreshed every second.
Nothing on it changes the PC.

- **Pairing.** "Pair a device" shows a QR code and an 8-character code.
  - The code is valid for 5 minutes and for one device; five wrong tries spend it.
  - A paired device stays signed in for 30 days (1, 7 or 90 can be chosen), up to 8 devices.
  - Settings lists the devices and signs any of them out at once.
- **Local network only.**
  - Only private addresses and this PC can connect; anything else is dropped. The port is
    8976 and can be changed.
  - Windows asks about the firewall the first time: allow Private networks only.
  - It is plain HTTP, not encrypted: HTTPS would need a self-signed certificate, which puts a
    warning on every phone. Use it on a network you trust.
- **Never sent:** processes, serial numbers, IP or MAC addresses, user or device names, and
  Health notes.
- **While it is on**, a "Remote" chip in the header shows how many devices are watching, and
  the tray tooltip says so.
- **It runs in the app, not the service.** It answers only while Vyro runs (the tray counts),
  and nothing running as LocalSystem listens on the network. Turning it off closes the port
  and every connection at once.

## Reliability

- **Upgrades keep your data.** Settings, alert rules, fan settings, the history and game
  sessions from every release since 0.1 carry over. Tests open files written by each
  release's own code (`crates/vyro-core/tests/migration.rs`).
- **A crash or power cut never leaves half a file.** Every state file is written to a temporary
  file, flushed to disk and then renamed.
- **A damaged file is set aside, not overwritten.** It is kept as `<name>.bad.<ext>` (for
  example `settings.bad.json` or `history.bad.sqlite`) and the defaults are used. In
  `settings.json` a bad value resets only that one setting.
- **Fan control after a crash.** The saved BIOS settings go back before anything else. If that
  write fails, Vyro keeps trying with them. If they cannot be read, the fans Vyro may have
  driven go to full speed until a restart.

## Documentation

| | |
|---|---|
| [`docs/control-safety.md`](docs/control-safety.md) | Fan and pump control: every safety rule, its code, tests and field result |
| [`docs/anti-cheat.md`](docs/anti-cheat.md) | What an anti-cheat can see of Vyro, what Vyro never does, and what to turn off |
| [`docs/footprint.md`](docs/footprint.md) | What each feature costs and when; how 1.0 is measured |
| [`docs/performance-score.md`](docs/performance-score.md) | The Performance Score formula |
| [`docs/benchmark.md`](docs/benchmark.md) | The benchmark's method |
| [`docs/diagnostics.md`](docs/diagnostics.md) | The diagnostics checks |
| [`docs/field-check.md`](docs/field-check.md) | The owner's field check of 1.0 (P16–P27) on the reference PC, in one pass |

`vyro-remote.exe` in the same artifact checks the service from the outside (state, connection,
sensors; `--capture Game.exe` measures a running game for 15 s). The service logs to `%ProgramData%\Vyro\service.log`.

## License

Vyro is licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Third-party components and their licenses, including PawnIO and PresentMon, are listed in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md), with the full texts in
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) (regenerate both with
`python3 scripts/third_party.py`; CI checks they are current).

## Layout

| Path | What |
|---|---|
| `crates/vyro-core/` | Sensor engine, platform-neutral: registry, provider trait, poller, SQLite history, simulator, data contracts, service IPC |
| `crates/vyro-service/` | The Windows service that runs the engine with the rights PawnIO and SMART need |
| `src-tauri/` | Tauri 2 shell: connects to the service (or starts the engine), emits the `sensors` event, serves commands |
| `src/` | Svelte 5 + TypeScript UI; canvas gauges and sparklines, no chart library |
| `src/bindings/` | TypeScript types **generated** from the Rust structs by ts-rs — never edit by hand |

## Development

Prerequisites: Rust stable (≥ 1.88), Node 22 + pnpm 10. On Windows, the WebView2 runtime
(preinstalled on Windows 11) and the MSVC build tools. On Linux, the
[Tauri system dependencies](https://v2.tauri.app/start/prerequisites/) (webkit2gtk 4.1).

```sh
pnpm install
pnpm tauri dev          # desktop app with hot reload
pnpm dev                # UI only, in a browser, fed by an in-page simulator
```

The app uses the installed sensor service when it runs and otherwise starts its own engine
(`VYRO_LOCAL=1` forces that). To develop against a service you build yourself:

```sh
cargo run -p vyro-service -- --console          # Windows: as administrator; Linux: simulated sensors
VYRO_DEV_PIPE=1 pnpm tauri dev                  # Windows: trust the console service's pipe
VYRO_SOCKET=/run/user/$UID/vyro-engine.sock pnpm tauri dev   # Linux
```

Checks (what CI runs):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p vyro-core   # also regenerates src/bindings — commit any changes
cargo test -p vyro-service
pnpm check && pnpm test && pnpm build
python3 scripts/bundle_budget.py --check   # what each page loads at start, gzipped
pnpm a11y                 # axe-core on every tab, theme and width (needs a Playwright Chromium)
```

On Windows, CI also installs the MSI on its runner, checks the service with `vyro-probe`, and
uninstalls it again.

The core crate builds and tests on any OS without WebView dependencies
(`cargo test` at the root targets it by default). Windows-only code can be type-checked from
Linux with `rustup target add x86_64-pc-windows-gnu` plus mingw-w64:
`cargo clippy --workspace --target x86_64-pc-windows-gnu`.

**No toolchain?** Every CI run uploads a `vyro-windows-<sha>` artifact (Actions → run →
Artifacts) with the MSI, `vyro-inventory.exe` (double-click; saves `vyro-inventory.json` beside
it), `vyro-live.exe` (samples the sensors in-process, as administrator), `vyro-remote.exe`
(checks the installed service), `vyro-probe.exe` (asks the installed service for readings and
history and reports its footprint) and the bare `vyro.exe` / `vyro-service.exe`.

To see exactly what Vyro reads on a machine (e.g. to compare with `msinfo32`):

```sh
cargo run -p vyro-core --example inventory              # board serial redacted
cargo run -p vyro-core --example inventory -- --show-serial
```

## Architecture in brief

The **Vyro Sensor Service** (`crates/vyro-service`, LocalSystem) runs the engine; the app
(`src-tauri`, unelevated) receives its snapshots and calls it over `\\.\pipe\vyro-engine`
(`vyro_core::ipc`, one commands and one snapshot connection per app). Without the service the
app runs the same engine in-process. Providers register sensors in a unified registry and fill a `SensorSnapshot` each tick. A
background poller runs at 1 Hz (5 s while the window is hidden), isolates failing providers,
records history to SQLite (raw ≤ 30 min, 10 s ≤ 24 h, 1 min ≤ 7 days, ≤ 200 MB; the same
file keeps the game replays) and emits the snapshot. The UI eases gauges toward each new value on a single `requestAnimationFrame` loop
that sleeps when values settle or the window is hidden. The service never listens on the
network: the optional phone companion is served by the app, as the signed-in user, and
benchmarks run in the app too, so quitting Vyro always ends them.

## Performance

Measured against the spec §8 budget with the built-in "Vyro itself" card (Dashboard),
which counts the sensor service, the app and its WebView2 processes. Reference PC: Ryzen 7
9700X, RTX 5060 Ti, Windows 11, 144 Hz display; Desktop profile (1 s polling).

| Metric | Target | 0.1.4 | 0.2.0 | 0.3.0 | 1.0.0 |
|---|---|---|---|---|---|
| CPU (average, window visible) | < 1.5 % | 0.68 % | 0.46 % | not re-measured | within target, not written down |
| RAM (private working set, window visible) | < 120 MB | 120 MB | 125 MB | not re-measured | within budget, not written down |
| RAM peak | — | 132 MB after visiting every tab | 125–150 MB with History open | not re-measured | not written down |
| RAM (closed to the tray) | — | ≈ 40 MB (service 33 MB, app 7 MB, WebView2 suspended) | not re-measured | not re-measured | not written down |
| Startup to first data | < 2 s | 0.76 s | 1.02 s | not re-measured | within target, not written down |
| Installer | < 60 MB (< 20 MB before 1.1) | 9.5 MB | 9.7 MB | 9.9 MB | 11.3 MB |

In 0.2.0, of the visible figure, WebView2 takes 85 MB (five processes), the service 33 MB and
the app 7 MB. Animation runs at most 60 fps and only while values move. For 0.3.0 the field
checks of automatic game profiles and the overlay re-measured the footprint within their
budgets (within 2 MB of 0.2.0; the overlay window only while it is shown), but the figures
were not written down. 1.0.0 was measured at its field check (2026-10-16) by the procedure in
[`docs/footprint.md`](docs/footprint.md), which also lists what each V2 feature costs and when.
It was within the budgets there; the figures go into the table when they are written down.

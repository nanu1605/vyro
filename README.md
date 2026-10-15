# Vyro

A low-footprint Windows hardware monitor: animated CPU/GPU speedometers, per-subsystem
sensors, history, health and alerts, with direct sensor access (no third-party monitoring
app or library).

- **Build spec:** [`PROJECT_PLAN.md`](PROJECT_PLAN.md) — what to build and why.
- **Roadmap:** [`ROADMAP.md`](ROADMAP.md) — milestones, phase tracker, decision log.

**Status:** 0.3 (milestone M8). Live CPU, GPU, board, fan, voltage, RAM, storage and network
sensors on Windows (low-level ones through PawnIO, read by the Vyro Sensor Service), history,
System Health, alerts, game frame rates (PresentMon), tray and an MSI installer. New in 0.3:
games switch Vyro's profile, fan settings, overlay and update rate on their own, a configurable
in-game overlay, and a replay of each game session showing what happened when (see
[Gaming](#gaming)). 0.2 added fan and pump control, so far only on the MSI B650M GAMING PLUS
WIFI (MS-7E24). See [`ROADMAP.md`](ROADMAP.md) for what is verified on which hardware.
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
```

The core crate builds and tests on any OS without WebView dependencies
(`cargo test` at the root targets it by default). Windows-only code can be type-checked from
Linux with `rustup target add x86_64-pc-windows-gnu` plus mingw-w64:
`cargo clippy --workspace --target x86_64-pc-windows-gnu`.

**No toolchain?** Every CI run uploads a `vyro-windows-<sha>` artifact (Actions → run →
Artifacts) with the MSI, `vyro-inventory.exe` (double-click; saves `vyro-inventory.json` beside
it), `vyro-live.exe` (samples the sensors in-process, as administrator), `vyro-remote.exe`
(checks the installed service) and the bare `vyro.exe` / `vyro-service.exe`.

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
that sleeps when values settle or the window is hidden.

## Performance

Measured against the spec §8 budget with the built-in "Vyro itself" card (Dashboard),
which counts the sensor service, the app and its WebView2 processes. Reference PC: Ryzen 7
9700X, RTX 5060 Ti, Windows 11, 144 Hz display; Desktop profile (1 s polling).

| Metric | Target | 0.1.4 | 0.2.0 | 0.3.0 |
|---|---|---|---|---|
| CPU (average, window visible) | < 1.5 % | 0.68 % | 0.46 % | not re-measured |
| RAM (private working set, window visible) | < 120 MB | 120 MB | 125 MB | not re-measured |
| RAM peak | — | 132 MB after visiting every tab | 125–150 MB with History open | not re-measured |
| RAM (closed to the tray) | — | ≈ 40 MB (service 33 MB, app 7 MB, WebView2 suspended) | not re-measured | not re-measured |
| Startup to first data | < 2 s | 0.76 s | 1.02 s | not re-measured |
| Installer | < 20 MB | 9.5 MB | 9.7 MB | 9.9 MB |

In 0.2.0, of the visible figure, WebView2 takes 85 MB (five processes), the service 33 MB and
the app 7 MB. Animation runs at most 60 fps and only while values move. For 0.3.0 the field
checks of automatic game profiles and the overlay re-measured the footprint within their
budgets (within 2 MB of 0.2.0; the overlay window only while it is shown), but the figures
were not written down.

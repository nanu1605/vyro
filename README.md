# Vyro

A low-footprint Windows hardware monitor: animated CPU/GPU speedometers, per-subsystem
sensors, history, health and alerts, with direct sensor access (no third-party monitoring
app or library).

- **Build spec:** [`PROJECT_PLAN.md`](PROJECT_PLAN.md) — what to build and why.
- **Roadmap:** [`ROADMAP.md`](ROADMAP.md) — milestones, phase tracker, decision log.

**Status:** early release (P11) with the privilege split (P8). Live CPU, GPU, board, fan,
voltage, RAM, storage and network sensors on Windows (low-level ones through PawnIO, read by the
Vyro Sensor Service), history, System Health, alerts, game frame rates (PresentMon), tray and an
MSI installer. See [`ROADMAP.md`](ROADMAP.md) for what is verified on which hardware.
Contributions: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Install (test builds)

Every CI run on this branch uploads `vyro-windows-<commit>` with `Vyro_<version>_x64_en-US.msi`.
The MSI installs Vyro for all users with a Start-menu entry, installs the signed
[PawnIO](https://github.com/namazso/PawnIO) driver silently if it is missing (it reads CPU
temperature and power, fans, voltages and RAM sensors; it stays installed when Vyro is removed),
and installs the **Vyro Sensor Service**. The service runs the sensor engine with the rights
PawnIO and SMART need, keeps history while the app is closed, and serves the app over a local
named pipe; the app itself runs without administrator rights. Intel's
[PresentMon](https://github.com/GameTechDev/PresentMon) console app is installed beside the
service, which runs it to measure the fullscreen game in front (nothing is injected into games). Without the service the app still
starts, reads what it can, and says what is missing. "Start with Windows" (Health → Alerts)
opens it in the tray at sign-in. Builds are not code-signed yet, so SmartScreen will warn.

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
records history to SQLite (raw ≤ 30 min, 10 s ≤ 24 h, 1 min ≤ 7 days, ≤ 200 MB) and emits the
snapshot. The UI eases gauges toward each new value on a single `requestAnimationFrame` loop
that sleeps when values settle or the window is hidden.

## Performance

Measured against the spec §8 budget from Phase 9 on, using the built-in self-monitoring
widget.

| Metric | Target | Measured |
|---|---|---|
| CPU (steady, window visible) | < 1.5 % | — |
| RAM (working set) | < 120 MB | — |
| Startup to first data | < 2 s | — |

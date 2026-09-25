# Vyro

A low-footprint Windows hardware monitor: animated CPU/GPU speedometers, per-subsystem
sensors, history, health and alerts, with direct sensor access (no third-party monitoring
app or library).

- **Build spec:** [`PROJECT_PLAN.md`](PROJECT_PLAN.md) — what to build and why.
- **Roadmap:** [`ROADMAP.md`](ROADMAP.md) — milestones, phase tracker, decision log.

**Status:** Phase 0. The sensor engine, history store and UI run end-to-end on simulated
hardware. Real Windows sensors start in Phase 1.

## Layout

| Path | What |
|---|---|
| `crates/vyro-core/` | Sensor engine, platform-neutral: registry, provider trait, poller, SQLite history, simulator, data contracts |
| `src-tauri/` | Tauri 2 shell: starts the engine, emits the `sensors` event, serves commands |
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

Checks (what CI runs):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p vyro-core   # also regenerates src/bindings — commit any changes
pnpm check && pnpm test && pnpm build
```

The core crate builds and tests on any OS without WebView dependencies
(`cargo test` at the root targets it by default).

## Architecture in brief

Providers register sensors in a unified registry and fill a `SensorSnapshot` each tick. A
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

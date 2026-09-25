# Vyro — Roadmap & Decision Log

Execution tracker for [`PROJECT_PLAN.md`](PROJECT_PLAN.md) (the build spec, v2). The spec is the
source of truth for *what* to build; this file records *in what order*, what is done, and which
open decisions (spec §12) have been answered.

## Decision log (spec §12)

| # | Decision | Answer | Status |
|---|---|---|---|
| 1 | Product name / codename | **Vyro** (repo name) | decided |
| 2 | First release cut | **Early no-driver release** after P5 → P11 split into **11a** (app) and **11b** (driver) | decided |
| 3 | Gaming module (P10) in v1? | Decide at milestone M5 | open |
| 4 | Fan control (v2 write path) | Not built without explicit go-ahead | default: no |
| 5 | Ping target host | `1.1.1.1`, user-configurable | default |
| 6 | History tiers / cap | raw ≤ 30 min · 10 s avg ≤ 24 h · 1 min avg ≤ 7 d · ≤ 200 MB | default |
| 7 | Frontend framework | **Svelte 5** + strict TS + Vite | decided |
| 8 | Installer | **WiX (MSI)** — better service/driver install story | default |
| 9 | Extra Super-I/O coverage | None beyond one Nuvoton NCT67xx + one ITE IT87xx | default |

Other: the speedometer prototype referenced in spec §3 was not available, so it is rebuilt from
the §4.3 animation spec in Phase 0.

## Milestones

| Milestone | Phases | Output | Driver | Signing |
|---|---|---|:-:|---|
| **M1** Foundation | P0 | Pipeline + animated gauges on simulated data | – | – |
| **M2** No-driver app | P1 → P5 | Inventory, usage, GPU, displays, storage, memory, network, self-monitoring | – | – |
| **M3** Early release | P11a | EV-signed app + MSI + updater (runs elevated) | – | EV cert |
| **M4** Driver | P6 → P11b | CPU temps/power/voltage, Super-I/O fans/temps, SPD timings + DIMM temps | test-sign → attestation | Partner Center |
| **M5** Insight | P7, P8, P9 | History/health/alerts/explorer, privilege split, profiles, analysis, polish | – | – |
| **M6** Gaming | P10 | PresentMon FPS + companion overlay (deferrable) | – | – |

P7 has no hard dependency on P6 (it operates on whatever sensors are registered) and can be
pulled ahead of P6 if the early release should include history/alerts — decide at M3.

## Cross-cutting conventions

- **Provider = parse + win.** Byte/struct decoders (EDID, SMBIOS, NVMe health log, ATA SMART,
  SPD, Super-I/O dumps) live in platform-neutral modules with fixture tests; a thin
  `#[cfg(windows)]` module fetches the bytes. Most logic is testable on any OS.
- Every provider implements `vyro_core::Provider`, registers its sensors in the unified
  `Registry`, and fills its slice of `SensorSnapshot`. A failing sensor becomes `null` and is
  logged once — it never aborts the tick.
- Static inventory is read once at startup (WMI only there) and served by commands.
- Rust structs are the single source of truth; TypeScript types are generated with `ts-rs` into
  `src/bindings/` and CI fails on drift.
- Non-Windows builds (and `VYRO_SIM=1`) use `SimProvider` so the UI can be developed anywhere.

## Phase tracker

### P0 — Scaffold, sensor engine, animation, history skeleton (M1)
Built and verified on Linux (unit tests; app run under Xvfb). Pending: first run on a Windows machine
(`pnpm tauri dev`) and a first footprint measurement there.

- [x] Workspace: `crates/vyro-core` (pure Rust) + `src-tauri` (Tauri 2 shell)
- [x] Unified sensor `Registry` with min/max/avg; `Provider` trait
- [x] `SensorSnapshot` + aux structs, generated TS bindings
- [x] Poller (1 Hz, 5 s when hidden) → registry → history → `sensors` event
- [x] `SimProvider`
- [x] SQLite history: tiered write path, `query_history`, retention + size cap
- [x] Svelte UI: canvas speedometers (usage + MHz), sparklines, stat tiles, eased 60 fps,
      `prefers-reduced-motion`, pause when hidden; dark theme; stacks < 720 px
- [x] CI: Linux (core + frontend) and Windows (Tauri shell build)

### P1 — Static inventory, hardware ID, system info (M2)
Built; verified by unit tests (Linux), Windows type-check + clippy (cross target), a Wine smoke
run, and Windows CI (real-machine sanity tests + inventory dump in the log). Field-checked on a physical PC
(Ryzen 7 9700X / MSI B650M / 2×16 GB DDR5-5600 / RTX 5060 Ti + Radeon iGPU / Windows 11 25H2):
CPU, caches, topology, board/BIOS, DIMMs, GPUs, OS, security and startup apps all correct. Gaps
found there and fixed: DIMM vendor from part number when firmware leaves it blank, AMD chipset
from the board name, NVIDIA GPU BIOS display format.

- [x] CPUID decode: vendor, brand, family/model/stepping, ISA extensions, hybrid, hypervisor,
      leaf 0x16 base/max clock
- [x] `GetLogicalProcessorInformationEx` decoder: cores, threads, packages, P/E map via
      `EfficiencyClass`, aggregated caches (offsets asserted against Windows headers at compile time)
- [x] SMBIOS parser: BIOS, system, baseboard (+ serial, local only, redacted from `Debug`),
      processor socket/clocks, memory arrays and modules (JEDEC vendor names, channel heuristic)
- [x] GPUs via DXGI + display-class registry (driver / vendor driver version, date, BIOS, memory type)
- [x] Chipset from the PCI LPC/eSPI bridge description (Intel; AMD → unknown)
- [x] OS name/edition/version/build/install date (Windows 11 naming fix), boot time, uptime
- [x] UEFI, Secure Boot, TPM (TBS), virtualization + hypervisor, BitLocker (WMI, admin only),
      elevation
- [x] Startup apps: Run keys, Startup folders, non-Microsoft logon/boot scheduled tasks, with
      Explorer's enabled/disabled state. Startup *impact* omitted (no public API — spec §2.10)
- [x] Power history from the System event log (boot, shutdown, unexpected shutdown, sleep, wake
      + wake source, modern standby)
- [x] Process / thread / handle counts
- [x] Collected once on a background thread (`Inventory`), never in the poll loop
- [x] Hardware and System views (tabs), browser-mode sample data

### P2 — Live usage, clocks, GPU, per-process (M2)
PDH CPU/GPU-engine counters, top-N processes, interim MHz via `CallNtPowerInformation`.
GPU: NVML → NVAPI (hotspot/mem temp, voltage) → ADLX → IGCL. Tensor/RT → N/A.

### P3 — Displays (M2)
DisplayConfig + DXGI + EDID/CTA/DisplayID parser, HDR/SDR white level, VRR range + state,
G-SYNC/FreeSync via vendor SDK, hot-plug refresh, arrangement view.

### P4 — Storage & memory runtime (M2)
NVMe health log + SATA SMART decoders, PDH disk counters, memory counters, elevated manifest
with graceful unelevated degrade.

### P5 — Network, power (partial), self-monitoring (M2)
IP Helper + PDH + WLAN API, ICMP ping/loss thread, app + WebView2 footprint, power panel (GPU
now, CPU after P6).

### P11a — Early release (M3)
EV code-signing, WiX MSI, Tauri updater, elevated autostart via Task Scheduler, perf pass
against spec §8.

### P6 — Kernel driver (M4)
KMDF, semantic read-only IOCTLs (whitelisted MSRs, validated Super-I/O banks, SMBus reads to
0x18–0x1F / 0x50–0x57 only, whitelisted AMD SMN); all port writes internal to the driver;
coexistence mutexes; rejection tests. Order: skeleton → Rust client → Intel CPU → AMD CPU →
NCT67xx → IT87xx → SPD/TSOD.

### P11b — Driver attestation (M4)
Partner Center, attestation signing, driver in MSI, verify on clean Win11 24H2+ with HVCI.

### P7 — History, health, alerts, sensor explorer (M5)
### P8 — Privilege separation: Windows service + ACL'd named pipe (M5)
### P9 — Performance hardening, profiles, intelligent analysis, polish (M5)
### P10 — Gaming capture + companion overlay (M6, deferrable)

**Deferred to v2:** fan/pump control (driver write path), injected in-game overlay.

## Risks

- Spec §8 RAM target (< 120 MB) is tight — WebView2 alone is typically 60–100 MB. Measure on
  Windows as soon as P0 runs there.
- Hardware acceptance criteria require real machines from the §9 test matrix; CI can only prove
  compilation and unit tests.
- EV certificate issuance and Partner Center onboarding take weeks — start during M2.

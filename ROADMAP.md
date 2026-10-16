# Vyro — Roadmap & Decision Log

Execution tracker for [`PROJECT_PLAN.md`](PROJECT_PLAN.md) (the build spec, v2). The spec is the
source of truth for *what* to build; this file records *in what order*, what is done, and which
open decisions (spec §12) have been answered.

V2 (phases P12–P27, milestones M7–M13) follows [`V2_PLAN.md`](V2_PLAN.md), the V2 spec; its
"roadmap §…" references are the rules spelled out in that file. V2 is built on the `v2` branch
and merged into `main` (the released line) at each milestone.

Deviation from the spec: the custom kernel driver (spec P6) and its attestation signing (spec
P11, driver part) are replaced by **PawnIO** — see decisions 10–11 and the P6 section.

## Decision log (spec §12)

| # | Decision | Answer | Status |
|---|---|---|---|
| 1 | Product name / codename | **Vyro** (repo name) | decided |
| 2 | First release cut | **Early release** after P5 (P11), before the low-level sensors; the former P11b (own driver signing) is dropped | decided |
| 3 | Gaming module (P10) in v1? | **Yes**: frame capture with the bundled PresentMon (MIT, Intel-signed) run by the service; companion overlay, no injection | decided |
| 4 | Fan control (v2 write path) | Superseded by 18 | superseded |
| 5 | Ping target host | `1.1.1.1`, user-configurable | default |
| 6 | History tiers / cap | raw ≤ 30 min · 10 s avg ≤ 24 h · 1 min avg ≤ 7 d · ≤ 200 MB | default |
| 7 | Frontend framework | **Svelte 5** + strict TS + Vite | decided |
| 8 | Installer | **WiX (MSI)**; bundles `PawnIO_setup.exe` and runs `-install -silent` when PawnIO is missing or older | default |
| 9 | Extra Super-I/O coverage | Nuvoton NCT6687D first (MSI AM5), then NCT67xx + ITE IT87xx | default |
| 10 | Low-level hardware access | **PawnIO** (signed, free, HVCI-compatible) replaces our own KMDF driver | decided |
| 11 | Code signing | **SignPath Foundation** or **Certum Open Source** (no EV certificate) | decided |
| 12 | Source availability | **Open source**; license MIT OR Apache-2.0 (LICENSE files added in P11; can still change before the repo goes public) | default |
| 13 | Privilege split (P8) | **LocalSystem service `VyroService`** runs the whole engine (sensors, history, health, alerts) and serves it over `\\.\pipe\vyro-engine`; the app runs unelevated and falls back to an in-process engine without the privileged sensors when the service is missing | decided |
| 14 | GPU visual | Own stylized parameterized **WebGL** model; no third-party photos; offline | decided |
| 15 | 3-D renderer | **Three.js** bundled locally; render on demand only | decided |
| 16 | Devices battery sources | BLE GATT 0x180F + PnP battery devnode property + XInput; vendor HID++ later | decided |
| 17 | Open on Dashboard | Default on, with an "Open on: Dashboard / Last tab" setting | decided |
| 18 | Fan / pump control | **Yes, the V2 flagship**, safety-gated (V2_PLAN "Hardware-control safety"); read-only by default; per-board allow-list | decided |
| 19 | Remote / web companion | In V2 (P26): served by the desktop app (not the service) over plain HTTP on the LAN, off by default, QR + code pairing, hashed session tokens that expire, sign-out at once | default |
| 20 | Versions | 0.2 / 0.3 / 0.4 / 0.5 at milestone merges; **1.0** at M13 | decided |
| 21 | Themes | Several themes + accent colour, accessibility preserved | decided |
| 22 | Performance Score method | Formula v1 ([`docs/performance-score.md`](docs/performance-score.md)): nine observed inputs, fixed weights and breakpoints, versioned, inputs shown | decided (signed off 2026-10-15) |
| 23 | Benchmark positioning | Self-relative (own engine), variance reported | decided |
| 24 | Fan-control launch boards | **MSI B650M GAMING PLUS WIFI (MS-7E24, NCT6687D) only**; each further board after its own validation | decided |
| 25 | Tagline | "Know your machine." unless changed | open |
| 26 | Navigation (new IA) | Adopted incrementally; V1 tabs keep working each release | decided |

Other: the speedometer prototype referenced in spec §3 was not available, so it is rebuilt from
the §4.3 animation spec in Phase 0.

## Milestones

| Milestone | Phases | Output | Driver | Signing |
|---|---|---|:-:|---|
| **M1** Foundation | P0 | Pipeline + animated gauges on simulated data | – | – |
| **M2** No-driver app | P1 → P5 | Inventory, usage, GPU, displays, storage, memory, network, self-monitoring | – | – |
| **M3** Early release | P11 | Signed app + MSI (+ PawnIO prerequisite) + updater (elevated until P8) | PawnIO | SignPath / Certum OSS |
| **M4** Low-level sensors | P6 | CPU temps/power/voltage, Super-I/O fans/temps/voltages, SPD + DIMM temps, RAM timings | PawnIO | – |
| **M5** Insight | P7, P8, P9 | History/health/alerts/explorer, privilege split (service + unelevated app), profiles, analysis, polish | PawnIO (via the service) | – |
| **M6** Gaming | P10 | PresentMon FPS + companion overlay (deferrable) | – | – |
| **M7** Control → 0.2 | P12 | Fan and pump control with failsafe, watchdog and restore | PawnIO (writes) | – |
| **M8** Gaming automation → 0.3 | P13 – P15 | Auto game profiles, Overlay 2.0, session replay | – | – |
| **M9** Intelligence → 0.4 | P16 – P19 | Intelligence engine, Performance Score, compound alerts, hardware changes | – | – |
| **M10** Benchmark & diagnostics → 0.4 | P20, P21 | Benchmark, one-click diagnostics, reports | – | – |
| **M11** Visualization & UX → 0.5 | P22 – P25 | Devices + battery, 3-D / My PC / thermal map, dashboard builder, themes | – | – |
| **M12** Remote → 0.5 | P26 | LAN-only companion, off by default | – | – |
| **M13** V2 polish → 1.0 | P27 | Footprint, accessibility, migration, control-safety validation | – | – |

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
Status: **done**, field-verified (2a field-checked on the Ryzen 7 9700X / RTX 5060 Ti + Radeon iGPU PC: all
providers up, poll ≈ 9 ms/tick, NVML matches the card, iGPU temperature and clocks via WDDM).
2b field check under load (Rocket League): RTX 5060 Ti at 60–64 % 3D, 2805 MHz, 51 °C, NVAPI
memory 58 °C and core 1.04 V, hotspot absent as expected on RTX 50; the game tops the process
list with its GPU share. Follow-ups: NVAPI fan RPM via cooler status (tach read unsupported on
RTX 50); ADL kept headless iGPU adapters and now reports why it did not attach; `vyro-live`
records engine log lines. Second gaming run: NVIDIA fan 1358 RPM @ 38 % via cooler status ✓;
ADL loaded but listed no adapters with connected-only enumeration → enumerate all adapters
(headless iGPU) and log every raw record. Third run's records showed the real cause: ADL encodes
the vendor id as decimal 1002 (0x3EA), not 0x1002 — accepted now; iGPU on bus 20 matches D3DKMT.
Fourth run: ADL attached to the iGPU (clocks, edge temperature via ADL). Its power (42–87 W while
idle) and voltage (1.2 V) track the whole APU package, not the GPU, so they are dropped for
integrated adapters. Phase 2 field-verified on NVIDIA (NVML + NVAPI) and AMD iGPU (ADL + WDDM);
Intel IGCL unverified (no hardware). Cost per tick under load: GPU ≈ 4–11 ms (PDH engine counters),
processes ≈ 7 ms (≈ 450 processes) — revisit in P9.

- [x] CPU total + per-logical-processor utilization from PDH `% Processor Utility` (Task
      Manager's counter), falling back to `% Processor Time` and the legacy `Processor` object
- [x] Effective clock = `% Processor Performance` × nominal `Processor Frequency` (Task
      Manager's "Speed"), `CallNtPowerInformation` as the interim fallback; rated base/boost from
      CPUID / SMBIOS
- [x] GPU usage + per-engine (3D / decode / encode / compute / copy) from `GPU Engine` counters,
      aggregated like Task Manager; dedicated VRAM from `GPU Adapter Memory`; tensor/RT → N/A
- [x] NVIDIA via NVML (dynamic load): core/mem clock, temperature, VRAM, power + limit, fan %,
      PCIe gen/width, throttle reasons; matched to DXGI adapters by PCI bus (D3DKMT)
- [x] Vendor-neutral fallback: WDDM adapter / node perf data (temperature, fan RPM, clocks)
- [x] Top processes (union of top 25 by CPU, RAM, GPU) from one `NtQuerySystemInformation` call
- [x] Basic memory totals (used / available / committed / cached); full set in P4
- [x] Dashboard gauges on live data (`VYRO_SIM=1` forces the simulator), per-core and GPU engine
      cards, **Processes** view
- [x] 2b NVIDIA NVAPI (by `nvapi_QueryInterface` id): hotspot + memory-junction temperature
      (per-generation channel map; RTX 50 exposes no hotspot), core voltage, fan RPM
- [x] 2b AMD via ADL PMLog (`ADL2_New_QueryPMLogData_Get`): clocks, edge / hotspot / memory
      temperature, fan, core voltage, board / ASIC power. ADL rather than ADLX: same sensors, plain
      C ABI; ADLX's COM-style vtables would add risk without new data
- [x] 2b Intel IGCL (`ControlLib.dll`, matched by LUID): clocks, GPU / VRAM temperature,
      voltage, power from the energy counter, fan, power / thermal limiters
- [x] Vendor DLLs loaded from System32 only; struct layouts verified against the vendor headers
      compiled for x64 Windows; every call degrades to "unavailable" on failure

### P3 — Displays (M2)
Status: **done** (see field checks below). `display/edid.rs`: EDID 1.3/1.4 + CTA-861 (HDR static
metadata, colorimetry, FreeSync / HDMI Forum VRR / NVIDIA vendor blocks) + DisplayID 1.x/2.x
(Type I/VII timings, Adaptive-Sync) decoder, tested on three real monitor dumps (serials
replaced). `display/win.rs`: `QueryDisplayConfig` paths (connection, rotation, refresh, desktop
rect, primary), advanced color info (24H2 `_2` query first), SDR white level, DXGI `GetDesc1`
(bit depth, color space, luminance), registry EDID, per-monitor DPI, DDC/CI or WMI brightness,
VRR state via NVAPI `Disp_GetVRRInfo` / ADL `FreeSyncState_Get`; hot-plug through a hidden
window (`WM_DISPLAYCHANGE`/`SETTINGCHANGE`/`DEVICECHANGE`, debounced) → `displays-changed`.
Displays tab: arrangement view, per-display signal / HDR / VRR / panel / EDID, serial hidden by
default. `vyro-inventory` now includes `displays` and collector log lines.
Field check 1 (LG UltraGear GSM5BB4, DP on the RTX 5060 Ti, HDR on): 2560×1440 @ 143.97 Hz,
10-bit RGB, BT.2100 PQ, SDR white 240 nits, peak 351 nits, 27.0″, made week 11 2023, DDC/CI
brightness 100 %, collection 366 ms (DDC/CI dominates). NVAPI reported enabled=0 possible=1
inVrrMode=1 with G-SYNC Compatible on and the desktop idle: "display in VRR mode" is now the
enabled state and the old "enabled" bit is reported as *varying now*. EDID kept as a fixture.
Field check 2 (Rocket League running, GPU 63 %): VRR on ✓, HDR on ✓, all values stable. "Varying
now" read false, most likely because the console window had focus when sampled (G-SYNC only
engages for the foreground fullscreen app); `vyro-inventory --delay N` allows switching back
first. Phase 3 **done**: field-verified on a single HDR / G-SYNC Compatible DP monitor;
multi-monitor, laptop panels (WMI brightness) and AMD FreeSync state (ADL) unverified here.

### P4 — Storage & memory runtime (M2)
Status: **built, awaiting field check**. `storage/nvme.rs` (SMART / Health log page 0x02,
Identify Controller) and `storage/ata.rs` (IDENTIFY DEVICE, SMART data + thresholds, attribute
names, vendor units for host writes, SSD wear attributes) decode raw sectors; `storage::build`
merges them with the device descriptor and grades health like CrystalDiskInfo (bad: attribute at
threshold or NVMe critical warning; caution: reallocated / pending / uncorrectable sectors, media
errors, ≤ 10 % life, temperature warning). `storage/win.rs`: `\\.\PhysicalDriveN` property
queries (descriptor, seek penalty, temperature), NVMe log pages via
`StorageDeviceProtocolSpecificProperty`, ATA pass-through (legacy `SMART_RCV_DRIVE_DATA`
fallback), volumes → disks, `\PhysicalDisk(*)` rates / IOPS / active time. Health refreshes
every 5 s; spinning disks only every 5 min and never while spun down (CHECK POWER MODE). USB,
RAID/RST and virtual disks list without SMART and say why. Memory: standby / modified / free
lists, pools, page file (`SystemPageFileInformation`), page / hard faults, compression store
(`MemCompression` working set); "Cached" now matches Task Manager (standby + modified).
Release builds carry a `requireAdministrator` manifest; debug builds (`pnpm tauri dev`) stay
unelevated and show a banner. Storage tab + dashboard Memory card. Drive serials are local-only.
Field check 1 (unelevated; Ryzen 7 9700X, 2× Crucial NVMe + Seagate 2.5" HDD): both NVMe drives
report the full health log without admin (P3 39 °C / 5 % used, E100 38 °C / 1 % used), volumes
map to disks, rates and memory lists match Task Manager's shape; the SATA HDD correctly asks for
admin. The compression process is named "Memory Compression" in the process list, not
"MemCompression": fixed (now 1361 MB).
Field check 2 (elevated): Seagate ST1000LM048 2.5" HDD over ATA pass-through — 5400 RPM,
SATA 6 Gb/s, 31 °C (attr 194), 12,998 power-on hours (low 32 bits of attr 9; Seagate packs
minutes above), 3,747 power cycles, 53.6 TB written (241 in LBAs), all 24 attributes above
threshold → good. Table now shows packed hour attributes (9, 240) as hours.

### P5 — Network, power (partial), self-monitoring (M2)
Status: **built, awaiting field check**. `network/mod.rs`: counter → Mbit/s rates (reset-safe),
utilization (busier direction vs link), Wi-Fi band / channel from centre frequency, PHY →
Wi-Fi generation, auth → security label, RSSI → quality, rolling 60-probe ping window (last /
avg / min / max / jitter / loss). `network/win.rs`: `GetIfTable2` (counters, link, state; filter
shadows dropped; VPNs with a default route kept), `GetAdaptersAddresses` (IPs, gateways, DNS,
DHCP), `GetBestInterface` for the primary adapter, WLAN API (RSSI, channel, SSID / BSSID / signal
/ rates / security; notes when Windows location access hides them), ICMP echo on its own thread
(1 / s, default 1.1.1.1, host names resolved, changeable from the UI and remembered). MAC,
IPv6, SSID and BSSID are local-only. Self-monitoring: app + descendants (WebView2) CPU / RAM /
GPU from the process walk. Power: GPU sum now; CPU and total stay "needs driver". Network tab,
dashboard Power and "Vyro itself" cards.
Field check 1 (Ryzen desktop, Realtek 2.5GbE at 1 Gbps + Radmin VPN + idle RZ616 Wi-Fi 6E):
primary route found, ping to 1.1.1.1 10.4 ms / 0 % loss / 0.4 ms jitter, VPN listed (has a
default route), disconnected Wi-Fi listed; network poll ≈ 2 ms. Fixes: MTU from the IP layer
(the interface row reported the NIC's 16114-byte jumbo maximum), duplicate DNS entries removed,
APIPA address hidden on a disconnected adapter, global IPv6 DNS / gateways redacted in the
inventory. Wi-Fi link details and throughput under load still to check.

### P11 — Early release (M3)
In progress. Done: WiX MSI (`pnpm tauri build`, uploaded by CI) with the pinned PawnIO 2.2.0
installer (`scripts/fetch-pawnio.ps1`, SHA-256 checked) run silently and elevated when PawnIO
is missing or 1.x (`src-tauri/wix/vyro.wxs`; a PawnIO failure never fails the install);
Start-menu shortcut carrying the toast app id; fixed upgrade code; "Start with Windows" as an
elevated logon task (a per-user Run entry since P8); uninstall removes that task and the user's toast
registration and keeps settings and history; single instance; license MIT OR Apache-2.0
(decision 12); `THIRD_PARTY_NOTICES.md` and full texts in `THIRD_PARTY_LICENSES.md`, generated
from Cargo.lock and the UI packages and checked in CI; `CONTRIBUTING.md`; spec §8 budget
readout in the "Vyro itself" card (average CPU, peak memory, startup to first data) and a CI
installer-size check (< 20 MB); updates: a Release workflow (Actions tab or a `vX.Y.Z` tag)
publishes the MSI with `latest.json` and checksums, and only those builds carry the Tauri updater
(daily check, Settings → Updates, dashboard notice, a notification once per version; install runs
the MSI passively); `scripts/set-version.py` keeps the manifests in step. Field-checked on the reference PC: install with PawnIO, test
notification, close to tray, "Start with Windows" (tray at sign-in, no UAC prompt), amd-cpu
poll ≈ 1 ms under full load. Downloads go to a public releases-only repository
(`RELEASES_REPO`), so the source can stay private. Field-checked: releases 0.1.2 and 0.1.3
published; 0.1.2 found 0.1.3 and updated itself, app and service, and reopened. §8 numbers
from the reference PC in the README. Open: code signing (needs the repo public and a SignPath / Certum
account); clean-machine test with Memory Integrity on.

- Signing: SignPath Foundation or Certum Open Source (requires the repo public under an OSI
  license) for vyro.exe and the MSI.
- WiX MSI; bundles the unmodified signed `PawnIO_setup.exe`, run as an elevated custom action
  with `-install -silent` when the uninstall key `PawnIO\DisplayVersion` is missing or older.
- Tauri updater (signed manifest); elevated autostart via a Task Scheduler "highest privileges"
  task; perf pass against spec §8; verify on a clean Win11 24H2+ with Memory Integrity on.
- LICENSE, THIRD_PARTY_NOTICES (PawnIO driver GPL-2.0 with IOCTL exception, PawnIO modules
  LGPL-2.1, with source links), CONTRIBUTING.
- Uninstall removes the per-user toast registration Vyro writes at start
  (`HKCU\Software\Classes\AppUserModelId\app.vyro.monitor`); history and alert rules
  (`%ProgramData%\Vyro` since P8) and settings (`%APPDATA%\app.vyro.monitor`) stay.

### P6 — Low-level sensors via PawnIO (M4)
Our own KMDF driver and its attestation signing are dropped. Vyro talks to the PawnIO driver
(`\\?\GLOBALROOT\Device\PawnIO`) through its IOCTLs and loads the signed modules it needs,
embedded from `crates/vyro-core/pawnio/` (PawnIO.Modules release, LGPL-2.1). Missing PawnIO →
those sensors stay `null` and the UI says "needs PawnIO". Vyro only issues reads, except SMBus
page-select writes that SPD reads require, and holds the `Access_ISABUS` / `Access_SMBUS`
coexistence mutexes around bus access.

PawnIO check (2026-09):

| Point | Result |
|---|---|
| License | Driver GPL-2.0 with an exception for programs using only its IOCTLs; modules and PawnIOLib LGPL-2.1. Shipping the signed setup and module binaries with license texts and source links is allowed; Vyro's own license is unconstrained. |
| Silent install | `PawnIO_setup.exe -install -silent` (`-uninstall -silent`); installed version in the Uninstall registry key. |
| Ryzen 9700X (family 1Ah, model 44h) | `AMDFamily17` accepts families 17h–1Ah: whitelisted MSRs (RAPL, P-state, APERF/MPERF) and SMN reads → Tctl, CCD temps, package/core power, VID, effective clocks. SoC rails via `RyzenSMU` (later). |
| MSI B650M (MS-7E24) | `LpcIO` Super-I/O access at 0x2E/0x4E with chip-ID check and HWM BAR discovery → NCT6687D. `SmbusPIIX4` covers the AMD FCH SMBus → DDR5 SPD / hub temperature; UMC timings via SMN. |
| Defender / HVCI | Signed, not on the vulnerable-driver blocklist, works with Memory Integrity on. |
| Anti-cheat | FACEIT blocks PawnIO (its certificate also signed cheats). Riot Vanguard: Valorant runs with PawnIO 2.2 installed (field test, 2026-09). |

Order: **6a** Rust client (status, module loading, execute, mutexes) → **6b** AMD CPU (Zen 2–5)
→ **6c** NCT6687D fans/temps/voltages, then other NCT67xx / IT87xx → **6d** SPD + DIMM
temperatures (DDR5 hub, DDR4 TSOD) and AMD memory timings → Intel CPU when a test machine is
available.

Status: 6a and 6b done and field-checked on the Ryzen 7 9700X against HWMonitor (Tctl, CCD
temperature and core VID match; the Zen 5 VID uses the SVI3 formula; package and core power match
at the 88 W PPT under Cinebench). 6c reads the NCT6687 read-only (no monitor-enable or fan writes),
field-checked on the MSI B650M GAMING PLUS WIFI: fans, board temperatures and voltages match
HWMonitor. Other boards show +12V/+5V plus raw channels until checked. GPU hotspot on RTX 50 (NVAPI
no longer reports it): read from the GB20x thermal registers through PawnIO's `Nvidia` module
(channel 0, HWMonitor's "NVIDIA I/O" Hot Spot), field-checked on the RTX 5060 Ti. 6d field-checked
on the same PC: DDR5 module temperatures (38.5 / 37.75 °C vs HWMonitor's PMIC 39 / 38 °C) and
timings (40-40-40-77 at 5600 MT/s, the kit's XMP). It reads module temperatures from the SPD5 hub
sensor (read-only: hub registers only, no page or NVM access, skipped while another tool holds the
SMBus mux on another port) and timings from the Zen 4/5 memory controller over SMN, shown in the
Memory card. DDR4 TSOD and DDR4 timings wait for a test machine; DIMM PMIC rails need writes and
stay out.

### P7 — History, health, alerts, sensor explorer (M5)
Built; awaiting a run of the desktop app on Windows (the pieces are unit-tested and previewed in
the browser build).

- [x] **Sensor Explorer** (Sensors tab): every registered sensor by group with value, min, max,
      avg, source; filter, collapsible groups, "Reset min/max"; history-tracked sensors link to
      their history.
- [x] **History** tab: 5 min / 30 min / 1 h / 6 h / 24 h / 7 days from the SQLite store; SVG
      charts (average + min–max band, gaps where the app was closed, hover readout) for usage,
      temperatures, power, clocks, RAM/VRAM, board + DIMM temps, drive temps, fans, core
      voltages, disk and network throughput, ping. Board voltages are now history-tracked.
- [x] **System Health** (`vyro_core::health`, Health tab): per-subsystem status with fixed,
      hysteresis-damped limits; event log of the last 200 changes; dashboard strip and tab dot
      when something needs attention. CPU limit 90 °C warning / 99 °C problem, since Ryzen
      7000/9000 hold 95 °C under full load by design.
- [x] **Alerts** (`vyro_core::alerts`, Health tab): sensor-threshold rules (above / below, hold
      time, cooldown, sound) and System Health forwarding; saved to `alerts.json` in the app
      config folder (a bad file is moved aside, defaults used); Windows toast notifications via
      `tauri-plugin-notification`, silent unless the rule asks for sound (the app registers its
      toast id per user at start so a bare `vyro.exe` can notify); in-app toasts; test button. Defaults: CPU > 95 °C and GPU > 90 °C for 30 s, hardware problems; warnings off.
- [x] Health events and fired alerts persist in the history database (`event` table, 7 days,
      ≤ 1000 per kind), so the logs survive restarts.
- [x] Tray icon (left-click shows, menu: Show / Quit). Closing the window hides it to the tray
      so monitoring and alerts continue (setting, on by default, `settings.json`); a one-time
      notification says so.
### P8 — Privilege separation: Windows service + ACL'd named pipe (M5)
Done. Built and tested (Linux, Wine, CI). Field-checked on the reference PC: MSI upgrade from
the elevated build, app unelevated and connected to the service, the full sensor set through it
(PawnIO ready; CPU, board, fans, voltages, DIMMs, drives all live), alert rules carried over,
history kept while the app is closed, reconnect across `sc stop` / `sc start`, "Start with
Windows" into the tray without a prompt.
Footprint with the service: 185 MB (service + app + WebView2), above the §8 120 MB target — P9.

- [x] `vyro_core::engine`: the engine as one object (`LocalEngine`: registry, poller and
      providers, history, health, alert rules) behind `EngineApi`, moved out of the Tauri shell.
- [x] `vyro_core::ipc`: protocol v1, length-prefixed JSON. Per client session one commands
      connection (request → reply) and one snapshot stream (server → client), so both ends use
      plain blocking I/O. The server polls at 1 Hz while any client window is visible, at the
      tray rate otherwise, and counts client processes in the app footprint. Unknown methods
      and other protocol versions are refused; requests are capped at 1 MB and connections at
      32.
- [x] Pipe security: protected DACL (SYSTEM, administrators and owner full control; interactive
      users read/write only, without `FILE_CREATE_PIPE_INSTANCE`), `PIPE_REJECT_REMOTE_CLIENTS`,
      first instance claimed with `FILE_FLAG_FIRST_PIPE_INSTANCE`; clients open it with
      identification-level impersonation only and check that the server process is the
      running `VyroService` (`GetNamedPipeServerProcessId` vs the SCM's process id).
- [x] `crates/vyro-service`: SCM glue (stop / shutdown), `--console` mode, rotating
      `service.log`; data in `%ProgramData%\Vyro`, created with a protected ACL (users read
      only); a pre-existing folder not owned by SYSTEM / administrators, or a link, is moved
      aside. Retries the pipe for 10 s at start so a restart never races a stale client handle.
- [x] App: `asInvoker`; connects to the service (waits up to 5 s while it starts), reconnects by
      itself, and otherwise runs the engine in-process with a banner explaining what is
      missing. BitLocker status, drive health and PawnIO status come from the service. "Start
      with Windows" is now an `HKCU\…\Run` entry. Alert rules from before the service are
      handed to it once.
- [x] MSI: installs and starts the service (start failures never fail the install), restart
      on failure (5 s, 5 s, 60 s), stops it before upgrades, removes it on uninstall; removes
      the legacy elevated logon task; removes the user's Run entry on uninstall.
- [x] Tests: framing and wire shapes; end-to-end over a Unix socket and, on Windows CI, a named
      pipe (snapshots, every command, visibility, reconnect after a service restart, version
      refusal); the service binary end to end. Checked under Wine with the real SCM: install,
      start, client verification, stop.
- [x] Field check: upgrade from the elevated build, sensors through the service,
      `vyro-remote.exe`.
- [x] Field check: history with the app closed; `sc stop` / `sc start VyroService` while the app
      runs; "Start with Windows" re-enabled after the upgrade.
### P9 — Performance hardening, profiles, intelligent analysis, polish (M5)
In progress.

- [x] P9a Footprint breakdown per executable (service, app, WebView2 helpers) in "Vyro itself"
      (IPC protocol 2). Hidden to the tray: no snapshots sent to the page, engine at the tray
      rate, WebView2 invisible + low memory target + suspended; resumed before showing.
      `--minimized` starts park the page after 5 s.
- [x] P9a Measured on the reference PC. In the tray: service 32 MB + app 7 MB, WebView2
      suspended (≈ 40 MB). Visible, before → after the 60 fps animation cap and
      `--in-process-gpu` (0.1.4): 131 → 120 MB (WebView2 92 MB ×6 → 80 MB ×5), peak 208 →
      132 MB, average CPU 1.0 → 0.68 %, UI 144 → ≤ 60 fps.
- [x] P9a Accepted as is (owner's call): 120 MB average and a 132 MB peak with the window
      open; unloading left tabs kept as an option if it grows.
- [x] P9b Settings tab: keep running in the tray, start with Windows (moved from Alerts),
      °C/°F everywhere (tiles, cards, Sensors, History, alert limits, health and alert texts,
      Windows notifications; the engine stays in °C), hidden sensors (× in Sensors, listed in
      Settings to bring back, left out of History presets), sensor service status. Saved in the
      app's `settings.json`; field-checked: °F and a hidden sensor survive a restart.
- [x] P9b Empty/error states and accessibility pass: one loading / error / nothing-found block
      with Try again for Hardware, System, Storage, Network and Displays; Sensors and History
      say why a list is empty (filter, all hidden, first reading) and announce errors; the
      dashboard says it is waiting for the first reading or the service. Tabs take Home/End;
      tile warnings and Health statuses are written out, not colour only; heading order and a
      low-contrast disabled-rule text fixed. axe-core: no violations on any tab.
- [x] P9c Profiles (`vyro_core::profiles`): Desktop (1 s, all alerts, full animation, full
      dashboard), Gaming (2 s, hardware problems only, reduced, compact), Work (2 s, silent
      alerts, reduced, full), Benchmark (0.5 s, silent alerts, no animation, full), Silent (5 s,
      no notifications but alerts logged, no animation, compact). Poll rate goes to the service
      (`SetPollInterval`, clamped 250 ms–10 s, re-sent after reconnects); notifications and
      in-window toasts follow the policy (Test always shows); chosen in Settings or the tray's
      Profile menu. Field-checked: Gaming from the tray switches the dashboard to compact and History to 2 s readings.
- [x] P9d Insights (`vyro_core::analysis`, pure and tested; `EngineApi::insights`, Health
      tab, refreshed every minute): CPU / GPU running hotter than usual for their load (a
      straight-line fit of temperature on load over the past week vs the last 30 min); heat
      throttling (full load, high temperature, clock ≤ 90 % of the usual full-load clock);
      fans much slower than usual at the current CPU temperature; sustained CPU + GPU power
      steps explained by the component and load that changed. Each links to History.
- [ ] Field check: insights after a few days of history; the footprint breakdown visible and
      in the tray.
### P10 — Gaming capture + companion overlay (M6)
In progress.

- [x] P10a Frame capture (`vyro_core::gaming`): the app watches for a fullscreen, non-shell app
      in the foreground (two 1 s samples; it stays the target while it runs, also alt-tabbed,
      until another fullscreen app takes over) and hands its process id to the service
      (`SetCaptureTarget`, IPC protocol 3; a client's target ends with its session). The service
      runs the bundled `PresentMon.exe` 2.6.0 (`--process_id --output_stdout --v2_metrics`,
      ETW session `VyroCapture`, ended on stop) and parses its CSV by column name (8-bit or
      UTF-16). Live, every tick: FPS and frame time over the last second, 1 % / 0.1 % lows (FPS
      at the 99th / 99.9th percentile frame time) over 10 s, CPU / GPU busy, latency, displayed
      FPS, what limits the frame rate (GPU or CPU busy ≥ 85 % of the frame, else capped), and
      a frame-time timeline (last 10 s in 100 ms buckets on the capture clock: average and
      worst frame; IPC protocol 4) drawn as a steady, scrolling graph. `game.fps`, `game.frametime`, `game.low1` are history sensors.
      Sessions ≥ 30 s are logged (`games.json`, last 100) with averages, lows from a 0.1 ms
      histogram, max frame time, max CPU / GPU temperature and average load. Gaming tab (live
      figures, frame-time graph, limiter, session log), a Game FPS dashboard tile, a Settings
      switch; PresentMon fetched with a pinned SHA-256 and installed by the MSI.
      `vyro-remote --capture <exe|pid>` checks capture through the service without the app.
- [x] P10a Field-checked: FPS matches the game's own counter.
- [ ] P10a Field check: anti-cheat games (FACEIT excepted, as with PawnIO) unaffected.
- [x] P10b Companion overlay (off by default; Settings → Gaming or the tray's "Game overlay"):
      a transparent, always-on-top, click-through window in a chosen corner of the game's
      monitor with FPS, 1 % low and frame time, optionally a frame-time graph and CPU / GPU
      temperature, load and power. Created only while the captured game is in front, shown with
      `SW_SHOWNOACTIVATE` (never takes focus), destroyed afterwards; while it shows, the engine
      polls at the profile's rate instead of the tray rate. Its window may only listen for its
      readings. A desktop window, not injected: it shows over borderless and windowed games,
      not exclusive fullscreen. Re-raised every tick (a fullscreen game activated after the
      overlay appeared sits above it); the Gaming tab says when a game is in exclusive
      fullscreen (present mode "Hardware: Legacy …") and the overlay cannot show.
- [x] P10b Field-checked: overlay shows over Rocket League in borderless mode; in its
      exclusive "Fullscreen" mode it cannot, as expected.
- [x] P10b Field-checked: never steals focus; sessions are logged after quitting.
- [x] P10b Field-checked: frame-time graph smooth and scrolls continuously in the overlay.

**Deferred to v2:** fan/pump control (now P12), injected in-game overlay (research only).

## V2 phase tracker

Built in this order (V2_PLAN.md has each phase's scope and acceptance). Every phase: branch
`v2-<phase>` off `v2`, a footprint budget written here before coding, pure logic with tests plus
a thin Windows layer, CI green, then merge into `v2`. Hardware writes never ship without the
owner's field check on the reference PC.

P16 to P27 are stacked, so one test build holds them all. [`docs/field-check.md`](docs/field-check.md)
orders their field checks into sittings for that build. Results are recorded in each
phase's entry below.

### P12 — Fan & pump control (M7 → 0.2)
All control runs in the service; the app only asks over the pipe. Writes go through PawnIO
`LpcIO` port I/O on the NCT6687 EC (same page/index/data transaction as the reads), only on
allow-listed chip + revision + board + header, each read back and bounded.

- [x] 12a Write path + capability detection (`sio::control`, tested against a simulated EC):
      `write_ec` (same free/page/index/data/free transaction as reads, never forces a busy EC),
      mode bit + request/done handshake + duty command, readback of every write, allow-list
      keyed by chip, revision (0x92), board (MS-7E24) and header, floors (fan ≥ 20 %, pump
      ≥ 60 %, an unknown request = 100 %), BIOS state capture / save / restore (mode register
      written last, also when a duty write failed). `vyro-fans` (CI download): read-only dump by
      default; `--try HEADER PERCENT` supervised 10 s test with restore; `--restore`.
- [x] 12a Field dump (read only) on MS-7E24: chip 0xD5 rev 0x92, EC base 0xA20, allow-listed;
      mode 0xA00 = 0 (all BIOS), request 0xA01 idle, commands 0xA28+ a fixed 0x66, outputs
      0x160+ follow the BIOS curve (CPU 58→65 % in 3 s), RPM as in the monitor path (CPU 1461,
      pump 4444 at 100 %). System Fan 1–3 have nothing connected (0 RPM at 60 %).
- [x] 12a Field-checked: `vyro-fans --try 0 100` (readback 255, CPU fan 1550 → 2010 RPM) and
      `--try 0 30` (readback 77, 1371 → 968 RPM, settled in ~7 s); only header 0 changed; after
      restore mode 0 and the output back on the BIOS curve.
- [x] 12b Controller (`vyro_core::fan`): speeds decided in the poller tick right after the
      providers (curve on any temperature sensor with hysteresis and ramp limits, or fixed; floors
      per header), written by a worker thread so EC waits never delay a tick. Failsafe core from
      12d built in: CPU / GPU emergency limit or a source without a reading → 100 %; readback
      mismatch or 3 I/O errors → restore and turn off; no tick for 10 s → BIOS until ticks
      resume; `fan-restore.json` saved before the first write and restored at the next start
      after a crash (the MSI already restarts the service). Tray interval capped at 2 s while
      fans are driven. Service only (the app's fallback engine reports it unavailable); IPC
      protocol 5. Cooling tab: per-header BIOS / Fixed / Curve, SVG curve editor (drag,
      double-click to add, right-click or Delete to remove, arrow keys, presets, copy to another
      header, live marker), smoothing and safety limits, apply / discard, confirmation before
      turning on; "Fan control active" chip with Return to BIOS in the header.
- [x] 12b Field check: CPU fan Fixed 40 % holds; a curve follows a stress test; Return to BIOS
      (mode 0 in `vyro-fans`); kill `vyro-service.exe` with control on → BIOS curve back after the
      restart; emergency limit lowered to 50 °C → 100 %; footprint re-measured.
      Fixed 40 % passed (2026-09-30). HWiNFO Sensors shows the same two fan speeds as Vyro
      (CPU 1,246 RPM, PUMPFAN 4,428 RPM); the owner's other 8 fans are on splitters or a hub
      without a speed wire, or a SATA-powered hub. System Fan 1 at Fixed 100 % made the case fans
      louder (2026-10-01), so the case hub is on SYS_FAN1 with no speed wire: Vyro drives it but
      cannot see it spin, so a stalled fan there is not detectable. Return to BIOS passed
      (2026-10-13): `vyro-fans` reads mode 0x00 afterwards. Curve passed (2026-10-13): the fans
      followed a stress test and slowed after it. Killing `vyro-service.exe` with control on
      brought the BIOS curve back (2026-10-13; the restarted service restores the saved state),
      and control resumed after the restart. With the CPU limit at 50 °C under load every
      controlled fan went to 100 % (2026-10-13). Footprint (2026-10-13, after 12e): 125 MB
      (WebView2 85 MB ×5, service 33 MB, app 7 MB), average CPU 0.46 %, startup 1.02 s. That
      is close to 0.1.4's 120 MB (WebView2 80 MB ×5) and below the 175 MB of 2026-09-30. But
      the peak since start was 296 MB (0.1.4: 132 MB). In the browser preview the Cooling tab
      adds about 1.7 MB of JS heap and 1,000 DOM nodes, steady over 2 minutes with nothing
      left behind, so it is not the source.
- [x] The peak was the History tab. Its series (about 58 sensors × 600 points) sat in deep
      `$state`, so Svelte wrapped every point in a proxy and rebuilt them on each reload
      (every 5 s at 30 min). In the browser preview, History took the page's JS heap from
      2.4 MB to 53.5 MB (after GC) and all Chromium processes from 147 MB to 295 MB at the
      highest. Every list that is only ever replaced (History series and sensor trees, the
      Sensors, Settings and Alerts trees, alert log, Health insights and events, game
      sessions, the Cooling status, sources, profiles and events) is now `$state.raw`, as the
      snapshot already was. After the fix the History JS heap is 8.6 MB. The highest total in
      a sweep of all 13 tabs, sampled every 250 ms, is 167 MB (295 MB before), and History's
      own highest is 154 MB. Field-confirmed (2026-10-13, build 70be0db): after resetting
      peaks and opening History at 30 min, 24 h and 7 days, the owner reports the peak
      stayed within 125–150 MB.
- [x] Board names match HWiNFO: temperature channel 4 is "VR" (was "CPU Socket"; now
      `temp_vrm`, id `board.temp4` unchanged); voltage channel 13 (0x13A) added as "VTT" (2.046 V).
- [x] False voltage alarm (2026-10-13): on the owner's working PC, Health kept reporting
      "+3.3V Standby at 0.00 V" and "CMOS battery low: 0.00 V" at every start. The chip reads
      0 on those inputs while it isn't sampling them. Earlier field reads of 3.37 V and 3.09 V
      were taken with HWMonitor open, which likely turned their monitoring on. Exactly 0 V is
      now "no reading" (`sio::win`), and Health skips 0 V inputs. Vyro does not turn the
      monitoring on itself: that is a chip write and would need its own field check.
      Field-confirmed (2026-10-13): Voltages show OK.
- [x] 12c Pump control (built 2026-10-13): the pump header is driven only
      after the owner confirms a pump is on it (`FanConfig.pumps`); a pump on a fan header can be
      marked the same way. Pumps never go below 60 %. Quiet 60 / Standard 80 / Full 100 % and a
      pump curve (40 °C 60 % → 85 °C 100 %). Stall guard: a driven pump that has reported a speed
      and then reads below 300 RPM on 3 ticks in a row ends control (every header back to the
      BIOS). A pump without a speed reading can be driven, with a warning that a stop can't be
      seen.
- [x] 12c Field check (2026-10-13, MS-7E24): confirm Pump Fan, Quiet 60 % → the pump dropped
      from about 4,400 to about 3,300 RPM and held (well above the 300 RPM stall threshold);
      Full 100 % → back up to about 4,400 RPM; Return to BIOS → stayed at about 4,400 RPM (the
      BIOS runs the pump at 100 %).
- [x] 12d Failsafe (built 2026-10-13): emergency override, watchdog, restore on
      stop / shutdown / crash (service restart restores first) came with 12b. Added: the service
      takes power events and hands the fans to the BIOS before the PC sleeps (it waits up to
      1.5 s), then captures the BIOS state afresh after waking before the next write. The chip is
      re-read every few seconds: a driven header that lost Vyro's setting (mode bit cleared or
      another duty, read twice) is written again; three rewrites within 2 minutes mean another
      program is driving the fans, and control ends. Control-event log moves to 12e.
- [x] 12d Field check (2026-10-13, MS-7E24): sleep and wake with control on (settings back
      after waking), reboot with control on (control resumes), uninstall with control on (BIOS
      curve after removal) all passed. Not run: MSI Center fighting Vyro (covered by tests).
- [x] 12e (built 2026-10-13):
      - Per-profile fan settings: `FanConfig.profiles` holds the header settings of profiles
        that don't use the shared ones. The app sends the profile in use (`SetFanProfile`) on
        start and on every change. The service keeps it in `fan-profile.json` for boot before
        sign-in. On, off, the limits, pumps and names apply to every profile.
      - Stored in `fan-control.json` with the rest of the configuration (one atomic file)
        instead of a `fan_profiles` table.
      - Fan event log: on and off, Return to BIOS, full speed and back, faults, watchdog,
        crash recovery, rewrites, sleep and wake, profile changes. Kept in the history store's
        `event` table under kind `fan` (7 days, 1,000 entries), not a separate
        `fan_control_events` table, so no migration is needed. Shown in the Cooling tab.
      - Header names ("Case fans") in `FanConfig.labels`.
      - Each snapshot carries `fanControl { active, fullSpeed }`. The header chip and the tray
        follow it: the tooltip reads "fan control on" or "fans at full speed", and the tray
        menu has "Return fans to BIOS". The app's 5 s fan status poll is gone.
      - Control is off by default and needs confirmation (from 12b).
- [x] 12e Field check (2026-10-13, MS-7E24), all passed: name System Fan 1; give Gaming its
      own settings and switch to Gaming from the tray (the fans follow it, and an event is
      logged); tray tooltip and "Return fans to BIOS" while control is on; events kept after a
      restart.
- [x] Field matrix on MS-7E24 (each header 30/50/100 %, curve under load, kill the service,
      forced override, sleep/resume, reboot, uninstall with control on); footprint re-measured.
      Done through the checks above. Headers: CPU at 30 / 40 / 100 %, pump at 60 / 100 %,
      System Fan 1 at 100 % (heard, no speed wire); System Fan 2–3 have nothing connected,
      and 50 % was not run as its own step. The rest passed as listed, and the footprint is
      125 MB with the peak fixed. M7 is complete.
- [x] **0.2.0 released** (2026-10-13): `v2` merged into `main` (117d5eb); Release run
      38372955933 (started from the Actions tab on `main`) tagged `v0.2.0` and published the
      9.7 MB MSI, `latest.json` and checksums here and to `nanu1605/vyro-releases`.

### P13 — Automatic game profiles (M8 → 0.3)
Owner's choices (2026-10-13): games without their own settings get the Gaming profile; the
per-game "refresh Hz" is Vyro's update rate (the monitor's refresh rate is not touched).
Budget: no new process or thread; one more check per second in the existing game watcher;
footprint within ±2 MB of 0.2.0. While a game runs, the engine keeps its update rate with the
window hidden (the profile's or the game's own; Gaming = 2 s instead of the 5 s tray rate).

- [x] Built (2026-10-13, branch `v2-p13-game-profiles`, 4723a88):
      - `vyro_core::gaming::auto`: per-game `GameRule` (profile, "Don't switch", whose fan
        settings, overlay on/off, update rate 0.5/1/2/5 s, "Not a game"), `GameAutomation`
        (on by default, default profile Gaming, opt-in GPU-load trigger, rules) kept in the app's
        `settings.json`, and the `Switcher`. The owner's profile is never changed by a game, so
        closing it restores everything. A profile picked in the tray or Settings while a game
        runs becomes the owner's and keeps the game's settings out until it closes ("Keep
        Desktop" does the same without changing the profile).
      - Detection (`foreground.rs`): fullscreen or borderless as before; a saved game also in a
        window; with the opt-in trigger, an app in front using ≥ 60 % of the GPU for 10 s;
        never Vyro, the shell, browsers and players, or an app marked "Not a game". Frame-rate
        capture follows its own setting, so profiles also switch with capture off.
      - App: `apply_effective` puts the profile's update rate, the fan settings (12e's
        per-profile fans) and the overlay choice in force and moves the tray check mark;
        `profile-state` tells the window (motion and dashboard density follow the profile in
        force). IPC protocol 6: `SetGameContext` (in front, profile, automatic, overlay) for
        the service; sessions now log their profile.
      - UI: Gaming tab strip ("Gaming profile while Game.exe runs… Keep Desktop / Not a
        game"), "Game profiles" card (the game in front, saved and recently played games),
        Settings → Gaming (on/off, default profile, GPU-load trigger), Settings → Profile shows
        the profile in force.
- [x] Field check (2026-10-13, MS-7E24), all passed: a game without settings switches to
      Gaming (tray check mark) and back to Desktop on exit; a saved game with Silent,
      Benchmark's fans and 1 s updates; a tray pick mid-game sticks after exit; a windowed
      saved game is detected and measured; "Not a game" on a fullscreen non-game; footprint
      re-measured.

### P14 — Gaming Overlay 2.0 (M8)
Budget: the same overlay window as P10b (no new process); the modes add at most a few MB to it
and nothing when no game is in front; the graph redraws only when readings arrive (≤ 2 Hz,
far below the 30 fps cap) with no animation between. In the browser preview the overlay
page's renderer is 42–46 MB in every mode (JS heap 2.1–2.3 MB, 68–174 DOM nodes), so the
mode changes about 4 MB; most of it is the WebView's fixed cost. The figure on Windows is
taken in the field check.

- [x] Built (2026-10-13, branch `v2-p14-overlay`, df67965; with 0d77a60, which marks fan control
      off before the BIOS settings go back, a race the Windows CI run found):
      - `vyro_core::gaming::overlay`: `OverlayMode` (Minimal, Performance, Full, Graph),
        `OverlayMetric` (13 readings: FPS, 1 % and 0.1 % lows, frame time, latency, CPU and
        GPU use, temperature and power, VRAM, what limits the frame rate), each mode's
        defaults, `rows` from the snapshot, and `size` for the window. P13's `GameRule` gains
        a per-game overlay mode.
      - Settings (`src-tauri/src/overlay.rs`): mode, corner plus offsets, size 75–200 %,
        background opacity, own choice of readings, graph, refresh every 0.5/1/2 s, colour,
        compact. A 0.2 `settings.json` keeps its look (`detailed` → Performance, otherwise
        Minimal); out-of-range values are brought back.
      - Window: sized as an upper bound for what it shows; the page pins the plate to the
        window's corner on the screen's corner, so spare room is invisible (the window is
        transparent and click-through). In the browser preview every mode fits its window
        at 100 % and 150 %, normal and compact (20 cases, including Minimal with all 13
        readings wrapping onto a second line). Readings go out at most once per refresh
        interval.
      - UI: `OverlayPlate` (shared by the overlay window and the preview), canvas
        `OverlayGraph`, Settings → Overlay card with a live preview over a stand-in game
        scene, and per-game mode in the Game profiles card.
- [x] Field check (2026-10-13, MS-7E24), all passed: the four modes over a borderless game;
      changes apply live; the game keeps input focus; exclusive fullscreen still reports
      "overlay unavailable"; "Vyro itself" with the overlay shown and hidden.

### P15 — Game session replay (M8)
Owner's choice (2026-10-13): full detail, every reading of the last 100 sessions, inside the
200 MB history cap. Budget: no new process or thread; one sample per tick while a game is
captured, written with the history store's existing 10 s batch (≤ 0.4 MB per hour played);
a 2 h replay keeps the page's JS heap ≤ 10 MB and its total within 25 MB of the Dashboard;
nothing is loaded until a replay is opened. The history database moves to schema v2 (three
new tables, added in place): going back to 0.2 would turn its history off, since 0.2 refuses
a newer schema.

- [x] Built (2026-10-13, branch `v2-p15-replay`, bd7b43a; 14df117 moves `games.json` aside even
      over an earlier import, which Windows will not rename over):
      - Store (`history::games`, schema v2): `game_session` (summary as JSON), `game_sample`
        (one row per tick; readings as scaled integers, about 44 bytes a sample: 80 KB per
        hour played at 2 s updates, 156 KB at 1 s, 312 KB at 0.5 s) and `game_event`, written
        with the history's 10 s batch. The running session's summary is refreshed with each
        batch; a session left open by a crash is closed at its last sample. The newest 100
        sessions are kept; over the 200 MB cap the oldest data goes first, a replay or history
        (a dropped replay keeps its summary). 0.2's `games.json` is imported once (summaries
        only) and renamed `games.json.imported`.
      - Markers (`gaming::markers`): a frame-rate drop below 60 % of the last 30 s median
        (after 10 s of frames, until back above 80 %); a stutter, a frame ≥ 3× the median and
        ≥ 25 ms; CPU and GPU thermal and power limits and other clock limits (CPU current
        limit, GPU hardware slowdown) when they come on, again only after 30 s off; switching
        away and back, profile changes, the overlay shown or hidden. Frame-rate markers are
        10 s apart at least; at most 200 per kind.
      - IPC protocol 7: session ids, `GameTimeline` (downsampled in the service to ≤ 1500
        samples, keeping each bucket's slowest stretch and worst frame) and `HistoryWindow`
        (`History::query_window`: raw within 30 min, 10 s within a day, 1 min within a week).
      - UI: Replay in the Gaming tab's session list (its code loads on first use): summary,
        frame rate, frame time, use, temperatures and power on one time axis with a shared
        crosshair; a marker lane and list with filters; any moment by clicking a chart or with
        the slider. A moment shows its marker, the game's readings then and a minute either
        side of the PC's sensor history (or that history older than 7 days is gone). Browser
        preview: a 2 h replay keeps the JS heap at 5.1–5.8 MB and the page 10–13 MB above the
        Dashboard; opening and closing it six times does not grow memory; axe clean, no
        overflow at 390 px.
- [x] Field check (2026-10-13, MS-7E24), all passed: two sessions with an alt-tab, a profile
      change, an overlay toggle and heavy load; markers at the right moments and their context
      matches History; sessions and replays kept after a service restart; disk growth
      measured. With P13 and P14 also field-checked, M8 is complete.
- [x] **0.3.0 released** (2026-10-15): `v2` merged into `main` (2cc7be0); Release run
      38685441589 (started from the Actions tab on `main`) tagged `v0.3.0` and published the
      9.9 MB MSI, `latest.json` and checksums here and to `nanu1605/vyro-releases`. Before the
      merge, a sweep of all 13 tabs in the browser preview, sampled every 250 ms with a replay
      and its context open on Gaming, peaked at 154 MB (0.2.0: 167 MB); the "Vyro itself"
      figures on the reference PC were not re-measured for the README.

### P19 — Hardware change detection (M9 → 0.4; first, P16 cites it)
Owner's choices (2026-10-15): a detected change goes into the list with a dot on the Hardware
tab, no notification; every Windows update counts, monthly builds included. Budget: no new
process or thread and nothing in the 1 Hz tick; the app builds a fingerprint of the PC
(identity and version fields only) after its inventory warm-up and when monitors or drives
change, under 50 ms on top of the warm-up it already does; the engine compares it with the last
one and keeps the list in the history database (under 50 KB); footprint within ±1 MB of 0.3.0.
Serial numbers are never stored: where one tells two identical devices apart, a salted hash of
it is part of the key. The history database moves to schema v3 (two new tables, added in place),
so going back to 0.3 turns its history off; IPC protocol 8.

- [x] Built (2026-10-15, branch `v2-p19-changes`):
      - `vyro_core::changes`: `fingerprint` (CPU; board and BIOS; each memory module by slot
        with its configured speed and voltage; GPUs with driver and VBIOS; internal drives
        with firmware; Windows edition and build; UEFI, Secure Boot, TPM, virtualization,
        hypervisor; and as hot-plug classes monitors by EDID, USB drives and physical network
        adapters) and `record`, which compares with what is known: added, removed, replaced
        (another device in the same slot, or one out and one in), a field changed with before →
        after, a hot-plug device seen for the first time (with that kind's devices before and
        now), and a monitor or adapter not seen for 30 days. What could not be read is not a
        change: missing fields keep their last value and a kind that came back empty is left
        as it was. Identities use raw numbers, so a change in how sizes are written never reads
        as another part.
      - Store (schema v3): `pc_inventory` (what is known, one row) and `hardware_change` (the
        newest 1000, never pruned by age or the size cap; about 200 bytes a change). Serials
        cross the local pipe but only a salted hash inside the key is kept; a test checks no
        serial or MAC reaches the database.
      - IPC protocol 8 (`RecordInventory`, `HardwareChanges`). The app checks after its
        inventory warm-up, when the set of monitors changes (not on a resolution or HDR
        switch) and when a drive comes or goes, on a short-lived thread; drives are compared
        only when the service reads them, since the app's own engine reads serials and firmware
        another way.
      - UI: "PC changes" card at the top of the Hardware tab (newest five, "Show all", category
        filters, day groups, "New" on entries not seen yet; the same change on several memory
        modules at once is one entry) and a dot on the Hardware tab until new changes are
        shown. Browser preview: axe clean, no overflow at 390 px; the main bundle grows 7.9 kB.
- [x] Field check (MS-7E24, 2026-10-15, test build `vyro-windows-d590d8c`): all five checks
      passed: "Tracking started" on the first start; a GPU driver update (or rollback) with
      before → after; a monitor unplugged and plugged back logs nothing, a different one logs
      "connected for the first time"; an EXPO / RAM setting changed in the BIOS logs a Settings
      change; the list survives a service restart.

### P16 — Intelligence engine (M9)
Owner's choices (2026-10-15): findings get their own Insights tab (Health keeps its limits and
events, with a link); a new finding worth a look puts a dot on that tab, from a check the engine
runs every 10 minutes, with no notification; to spot background interference the engine keeps
the names of the busiest programs of each minute (at most five, names only) in the history
database for 7 days, like the sensor history.

Budget: no new long-lived thread; the only per-tick work is the minute digest (one pass over
the top processes and the throttle flags, well under 0.1 ms). The analysis runs on a
short-lived thread at most every 10 minutes in the background and on demand while Insights is
open (cached 60 s), under 250 ms of one core on the reference PC. The digest table stays under
3 MB over 7 days; footprint within +2 MB of 0.3.0. History schema stays v3 (the new table is
created in place); IPC protocol 9.

Rules that keep a weak correlation from reading as a cause: a cause is "likely" only when a
fixed table says it can produce that effect, it came just before the effect began, the effect
lasted, and it is the only such candidate; timing alone is never high confidence. Several
candidates are each "possible"; anything else in the window is listed apart as "around the
same time, not shown to be the cause".

- [x] Built (2026-10-15, branch `v2-p16-intelligence`):
      - `vyro_core::intel` replaces P9d's `analysis`. Derived metrics: thermal headroom against
        the Health problem limits, temperature for the load, CPU fan speed for the CPU
        temperature, frames per watt per game session (sessions now log CPU + GPU power), and
        clock steadiness under full load. Baselines: the last half hour against the past week
        at the same load, on the profile in force when there is enough of it (else every
        profile, one step less sure), the same game's earlier sessions, and the time before a
        lasting shift: a change-point search over the week (hourly, 6 h each side) or the last
        day (3 h before, 30 min after) dates when it began, and a shift must hold from that
        date, so a recent one is not dated hours too early.
      - Findings: hotter than usual, a fan slower than usual, slowed by heat (measured from
        the CPU's and GPU's own limit flags where they report them, else inferred from clocks),
        close to the temperature limit, power steps, what set a game's frame rate, background
        load during a game (with frame drops tied to it only at 5 or more drops and 3 times
        the rate of the other minutes) and a game slower than its earlier sessions. Each has
        evidence (value against usual over a window, opening History at that moment), a basis
        and a confidence; under three days of history confidence drops a step.
      - Causes follow the rules above, from P19 changes, profile switches, fan control events
        and busy programs, with chains up to three deep (slowed by heat ← hotter ← fan slower ←
        BIOS update). A likely cause the owner chose (a profile, turning fan control on)
        makes the finding an explanation rather than a warning.
      - The poller builds a minute digest each tick (busiest programs, names only; seconds of
        heat, power and current limiting; profile; game and what bound it), kept 7 days in
        the new `digest` table. The analysis runs on a "vyro-intel" thread every 10 minutes
        and on demand (cached 60 s); `SensorSnapshot::insights` carries the keys of the
        findings worth a look. IPC protocol 9.
      - UI: Insights tab after Health (vital signs, then findings with Measured (solid) and
        Inferred (dashed, italic) badges that say which in words, confidence in words,
        evidence with "Show in History", "Why" chains and a separate "Around the same time
        (not shown to be the cause)" list) and a dot on the tab until new warnings are shown;
        Health keeps a one-line summary with a link. Tests: the five P9d cases and the
        acceptance scenarios (load alone, a dated shift with one able change and a coincident
        monitor, two changes at once, a change after the onset, thin history, fan control and
        the Silent profile as the owner's choice, background load with and without matching
        drops, a game slower after a driver update). Browser preview: axe clean, no overflow
        at 390 px (the tab bar's screen-reader notes no longer widen the page); the main
        bundle grows 9.5 kB.
- [ ] Field check (MS-7E24): quiet at normal desktop use; a CPU stress test gives headroom but
      no "hotter"; Silent or a lower fan curve under load is explained by the profile or fan
      change; a background job during a game names the program; badges, "Show in History"
      and the tab dot work; footprint within budget.

### P18 — Smart compound alerts (M9)
Started while P16 waits for its field check, on `v2-p18-alerts` stacked on
`v2-p16-intelligence` (it merges after P16). Defaults chosen without the owner, open to change
at the field check: an alert's profile and fan actions last while its conditions hold and end
30 s after they clear; a profile the owner picks meanwhile wins until that alert clears; fans
are only ever raised (5–50 points on top of what Vyro already drives, never a header left to
the BIOS) and only while fan control is on; two alerts about the same part within a minute
notify once (a hardware problem always notifies); the diagnostic record keeps readings and the
five busiest program names, nothing else.

Budget: no new thread; per tick, the rules' conditions plus a short buffer for each sensor a
rate condition watches (at most 10 minutes of readings); the usual values for "above usual"
come from the 10-minute intelligence pass (one week query per sensor a rule watches). A
diagnostic record adds about 1.5 KB to its fired alert in the existing event log. Footprint
within +1 MB of P16. History schema unchanged; IPC protocol 10.

- [x] Built (2026-10-15, branch `v2-p18-alerts`):
      - Rules (`vyro_core::alerts`): besides one sensor against a limit and System Health, a
        rule can hold up to five sensor conditions, all of them or any one: above or below a
        limit, rising or falling by an amount within 5–600 s, or an amount above or below the
        sensor's usual reading (a CPU or GPU temperature for the load of the moment, from the
        past week's fit; anything else its weekly median; nothing until about a day of history,
        the last hour left out). Each condition can have a clearing margin (hysteresis); the
        rule must hold for its hold time, then stays quiet for its cooldown; a missing reading
        neither trips nor clears a rule, but never keeps its actions going (they come back with
        the reading while the rule still holds).
      - Actions: a notification (with or without sound, or none: logged only), the rule's name
        in the tray tooltip, a profile, the fans Vyro drives raised by 5–50 points, and a
        diagnostic record (the rule's readings, key temperatures, loads, power and fan speeds,
        and the five busiest program names) kept with the fired alert. Tray, profile and fans
        last while the conditions hold and end 30 s after they clear; the first active rule's
        profile and the largest boost apply. Two alerts about the same part (CPU, GPU, fans,
        memory, storage) within a minute notify once and the second is logged as a repeat; a
        hardware problem always notifies.
      - Fan boost (service): added to each driven header's curve (ramped like any change) or
        fixed speed, capped at 100 %; safety full speed still wins; nothing with control off,
        and never a header left to the BIOS. The fan log records when an alert raised the
        fans and when they went back; Cooling says so while it lasts.
      - Profile (app): an alert's profile wins over a game's; a profile the owner picks while
        it holds wins until that alert clears. Settings and the Gaming strip say which alert
        put it in force.
      - IPC protocol 10 (`AlertUsual`, `SensorSnapshot::alert_state`, `FanStatus::boost`, rule
        conditions and actions). Rules saved before load unchanged; a single plain limit is
        still saved the old way, so going back to 0.3 only drops rules that use the new kinds
        (`alerts.json` is then moved aside and the defaults used).
      - UI: the rule editor (Health tab) with condition rows, all/any, the usual reading for
        "above usual", and the actions; "Active" on rules that hold; the log shows actions,
        repeats and the diagnostic record. Tests: compound and any-of rules, a missing reading,
        hysteresis on a hovering reading, a fast rise against a slow climb, above usual with
        and without history, actions ending 30 s after clearing, a lost sensor, a rule turned
        off, repeats within a minute, the diagnostic record, validation, old rule files; the
        fan boost on fixed and curve headers, its cap and its gate; the alert profile against
        games and manual picks; the tray tooltip. Browser preview: axe clean, no overflow at
        390 px; the main bundle grows 12.8 kB (the preview's rules and log load separately).
- [ ] Field check (MS-7E24): a CPU-only stress test leaves an "all of" CPU-and-GPU rule quiet
      and a game with a heavy CPU load fires it; "rises 15 °C within 30 s" fires on starting a
      stress test; with fan control on, a fan action raises only the driven fans (Cooling log
      and readings) and they return 30 s after the rule clears, and with control off nothing
      changes; a profile action switches the profile (tray check mark) and back, and picking
      a profile meanwhile keeps it; two CPU rules firing within a minute notify once (the second
      is logged as a repeat); the diagnostic record names the busy program.

### P17 — Performance Score (M9)
Owner's choices (2026-10-15): formula v1 signed off as drafted; Stability counts WHEA hardware
errors as well as unexpected shutdowns; the score shows at the top of Insights.

Budget: no new thread; the score is worked out by the intelligence pass once a day (and when
Insights asks, kept a minute): the week's 1-minute history of about ten metrics, the minute
digests, the drives (read at most every 10 minutes) and one System-log query; under 300 ms of one
core on the reference PC. One stored result a day, kept a year (under 400 KB). History schema
stays v3 (the `score` table is created in place); IPC protocol 11.

- [x] Formula v1 drafted (2026-10-15) and signed off the same day:
      [`docs/performance-score.md`](docs/performance-score.md).
- [x] Built (2026-10-15, branch `v2-p17-score`, stacked on `v2-p18-alerts`):
      - `vyro_core::score`: formula v1 as pure code. Each input's sub-score from the document's
        breakpoints (straight lines between them), the worst part counting; the weighted average
        over the inputs with data, none below 60 % of the weight; the band; every input with its
        parts, weight, basis and, when missing, why.
      - Measures (`score::measure`): thermal headroom from the median temperature in loaded
        minutes (the CPU's design limit from its CPUID brand: 100 °C for Intel Core, else
        95 °C; the GPU against Health's 85 °C), clock sag over full-load stretches of 5 minutes
        or more, heat and current limiting from the minute digests (missing where nothing
        reports flags), cooling and the change from usual from the intelligence pass (an
        explanation of the owner's own choice counts as 100), drive health, SSD life and system
        drive free space, RAM in use (p95), ping (p95) and lost replies, and unexpected
        shutdowns (Kernel-Power 41) and WHEA hardware errors (17, 18, 19, 47) from the System
        log over 30 days. The history inputs wait for about a day of history.
      - Store: the `score` table (one result a day, the whole result as JSON, kept a year;
        none kept without a score); `History::minutes` reads the 1-minute tier.
      - IPC protocol 11 (`PerformanceScore`); the app's `get_performance_score`.
      - UI: the Performance Score card at the top of Insights (the score, its band, how many
        inputs it rests on, the formula version, a one-year trend, and every input with its
        sub-score, weight, Measured / Inferred badge and the values behind it, or why it has no
        data). Tests: every breakpoint, the coverage rule, the worked example (98), a healthy
        synthetic week (≥ 90), throttling, a failing drive, crashes and a fatal hardware error
        moving it through Good, Fair and Needs attention, a first day without a score, the
        event-log counts, the daily store. Browser preview: axe clean, no overflow at 390 px;
        the main bundle grows 6.9 kB.
- [ ] Field check (MS-7E24): after a day of use the score appears with all inputs that apply;
      the thermal headroom matches the History of a gaming evening; a heavy stress test with a
      lowered fan curve shows in Throttling or Sustained clocks; the stability counts match the
      System log in Event Viewer (Kernel-Power 41, WHEA-Logger); the daily point appears in the
      trend the next day.

### P20 — Benchmark mode (M10 → 0.4)
On `v2-p20-bench`, stacked on `v2-p17-score`. Method v1 is in
[`docs/benchmark.md`](docs/benchmark.md).

Defaults chosen at build (2026-10-15). The owner can change them at the field check:
- **Modes.** Quick (about 3 min) covers CPU single/multi, memory, storage, GPU compute and
  graphics. Full (about 13 min) adds 10 one-minute loops of CPU and GPU load together.
- **Passes.** A warm-up, then 3 measured passes per area. The result is the median and the spread.
- **What runs where.**
  - The load runs in the app, so quitting Vyro always ends it.
  - The GPU work is Direct3D 11 on the high-performance GPU.
  - Storage uses a 1 GiB uncached temporary file in the Windows temporary folder.
  - The Benchmark profile is in force during a run.
- **Stops.** Stop, closing or quitting the window, Health's problem limits (CPU 99 °C, GPU 92 °C),
  a new Health problem, readings missing for 10 s, or twice the planned time. All load ends
  within a second.
- **Comparison.** The newest earlier completed run of the same method that was not busy. A change counts beyond the
  larger of 3 % and either run's spread, and setup differences are listed.

Budget:
- **Idle.** Nothing: no thread, no polling.
- **During a run.**
  - Threads: a runner thread plus one worker per logical processor (4 for storage, 1 for the GPU),
    all joined at the end.
  - Memory: up to 512 MiB of RAM for the memory area and about 320 MiB of video memory, freed
    after each area.
  - Disk: a 1 GiB temporary file, about 3 GiB written per run.
- **Stored.** One `benchmark_runs` row (about 8 KB) and about 25 `benchmark_metrics` rows per run,
  newest 100 runs kept (under 1 MB).
- **Versions.** History schema stays v3 (the tables are created in place); IPC protocol 12.
- **Bundle.** The main bundle grows under 15 kB.

- [x] Method v1 written (2026-10-15): [`docs/benchmark.md`](docs/benchmark.md).
- [x] Built (2026-10-15, branch `v2-p20-bench`, stacked on `v2-p17-score`):
      - **Workloads (`vyro_core::bench`):**
        - CPU rounds of four fixed kernels (hash, LZ match search, matrix product, sort), each
          round checked against a reference checksum.
        - Memory: read and copy over two 256 MiB buffers, with known sums, and latency along a
          Sattolo cycle over 256 MiB.
        - Storage: a 1 GiB uncached file, written whole first, then 3 capped passes each of
          sequential write and read (4 threads, 1 MiB blocks) and random 4K reads at QD1.
          Every sector carries its own offset, so data read back is checked. The file is
          deleted on drop, and stale files at the next run.
        - Direct3D 11 on the high-performance adapter: FP32 multiply-add chains, a 128 MiB
          video memory copy, an integer hash checked on the CPU, and an offscreen 2560 × 1440
          scene (400 instanced draws a frame, two frames in flight). GPU work goes out in
          batches of about 20 ms; a reset counts as a GPU driver reset.
      - **Runner.**
        - Settles for 3 s first (marks a busy PC), then runs each area on its own thread while
          watching the readings: peaks, average power and clocks over the measured passes,
          throttled seconds, and fans at full speed.
        - Stops on Health's problem limits (2 readings in a row), a new Health problem, a game
          starting, 10 s without readings, or twice the planned time. A test asserts every
          worker stops within a second.
        - Works out scaling, efficiency (points per W, frames per joule) and stability (wrong
          checksums, read-back errors, WHEA errors during the run, GPU resets).
        - Full adds 10 one-minute loops of CPU and graphics together: stability is the worst
          loop ÷ the best, plus clock sag.
        - EcoQoS is off for the process during a run.
      - **Store.** `benchmark_runs` (the run as JSON, plus mode, status, busy and errors) and
        `benchmark_metrics` (value and spread per metric), newest 100 kept. IPC protocol 12
        (`AddBenchmark`, `BenchmarkRuns`, `BenchmarkRun`, `DeleteBenchmark`).
      - **App.**
        - The run thread lives in the app.
        - Readings come from the snapshot sink; the engine keeps the window's rate while a
          run goes on.
        - The Switcher's benchmark layer puts the Benchmark profile in force: owner's pick >
          benchmark > alert > game > base.
        - A run stops on Stop, on the window closing or hiding to the tray, and on quit
          (waits up to 3 s so the stopped run is kept and the file deleted).
        - Each run records its setup, including the active power plan (`sysinfo::power_plan`).
      - **UI: the Benchmark tab, loaded on first visit.**
        - Quick or Full, start checks, live progress with Stop and the areas finished.
        - The result per area: median, ± spread, change against the baseline, and the readings.
        - The comparison with the newest earlier completed, non-busy run (or any run picked),
          what changed in between (setup differences and P19 hardware changes), and the Setup.
        - The list of runs with two-click delete.
        - Browser preview with past runs and a compressed live run.
      - **Checks.**
        - Rust tests: workloads, store, Switcher layer, IPC wire shapes. The Windows CI job also
          runs every GPU workload on WARP and checks the shader's hashes against the CPU's.
        - Browser preview: axe clean and no overflow at 1440 and 390 px.
        - The main bundle grows 3.1 kB; the tab is a 19 kB chunk.
- [ ] Field check (MS-7E24, RTX 5060 Ti):
      - A Quick run completes in about 3 minutes, then a second scores within noise of the first.
      - The CPU multi-thread peak temperature and power match HWMonitor.
      - The GPU areas name the RTX 5060 Ti (not the iGPU), and Graphics holds it at full load.
      - Stop, closing the window and quitting each end all load within a second (Task Manager)
        and leave no `vyro-bench-*.tmp` in `%TEMP%`.
      - A Full run's Sustained loops read steady.
      - A run after a GPU driver update lists the change.

### P21 — One-click diagnostics + reports (M10)
On `v2-p21-diag`, stacked on `v2-p20-bench`. Checks v1 are in
[`docs/diagnostics.md`](docs/diagnostics.md).

Defaults chosen at build (2026-10-15). The owner can change them at the field check:
- **Passive.** No load (the Benchmark does that). 13 checks read the week's history and digests,
  the Health log, SMART, the System event log and the readings now; the limits follow
  Performance Score formula v1 where they overlap.
- **Status.** Pass / Attention / Info / Unavailable; every Attention cites its telemetry.
- **Where it shows.** A Diagnostics card in Health, below each part's status.
- **Exports.** HTML, JSON, Markdown and PDF, written to `Documents\Vyro Reports` and shown in
  Explorer. Serial numbers are never stored, and are added to an export only when asked.

Budget:
- **No new thread.** The checks run on a blocking task of the app's async runtime, and the engine
  works the report out in one request (about a second: the week's minutes are already read for
  the score).
- **Stored.** The newest 50 reports, each about 10 KB of JSON plus 13 finding rows (under 1 MB).
- **Versions.** History schema stays v3 (tables created in place); IPC protocol 13.
- **Bundle.** The card loads on first visit, so the main bundle grows under 3 kB.

- [x] Checks v1 written (2026-10-15): [`docs/diagnostics.md`](docs/diagnostics.md).
- [x] Built (2026-10-15, branch `v2-p21-diag`, stacked on `v2-p20-bench`):
      - **Checks (`vyro_core::diag::checks`).** Pure, over formula v1's measures plus each
        drive, the Health log, the readings now, PawnIO, fan control and what the app sends
        (how it reaches the engine, the PC summary, the GPU drivers).
        - Thermals, clock stability, power limits (Info), throttling, fans and pump (the owner's
          own choice does not count), memory, storage, network, hardware errors, changes from
          usual (P16 warnings with their evidence), drivers (Info), sensors, service.
        - Each is Pass, Attention, Info or Unavailable (saying why), with citations: value,
          unit, source, sensor and how long ago. Summaries carry no temperatures, so the
          citations follow the chosen unit.
        - The score's measures now also count the power-limit shares, which formula v1 does
          not use.
      - **Store.** `diagnostic_runs` (the report as JSON) and `diagnostic_findings` (one row per
        check), newest 50 kept. IPC protocol 13 (`RunDiagnostics`, `DiagnosticRuns`,
        `DiagnosticRun`, `DeleteDiagnostic`).
      - **Exports from one report.**
        - HTML (self-contained, prints well), JSON, Markdown, and a PDF that Vyro writes
          itself: A4, standard Helvetica, WinAnsi, wrapped with Helvetica's widths, page
          numbers.
        - Citations format the same in Rust and TypeScript (halves round away from zero, as
          `toFixed` does), in °C or °F.
        - Serial numbers (board, drives) are read only for an export that asks for them, and
          never kept.
        - Saved to `Documents\Vyro Reports\vyro-diagnostics-<date>-<time>.<ext>`; "Show in
          folder" opens Explorer on it (files in that folder only).
      - **UI.** The Diagnostics card in Health (loaded on first visit).
        - Run diagnostics.
        - Counts and earlier reports.
        - Attention first, open: what to do, and the citations with links into History.
        - Export buttons and "Include serial numbers"; delete a report.
        - Browser preview with a report that has a stopped fan.
      - **Checks.**
        - Rust tests: every problem turns Attention and cites its telemetry, the owner's choice
          and missing data, the formatting, all four exports showing the same values in °C and
          °F, serials only when asked, and the PDF's structure (cross-reference offsets, page
          count). poppler reads the PDF and its text extracts cleanly.
        - Browser preview: axe clean, no overflow at 1440 and 390 px (°F).
        - The main bundle grows 1.5 kB; the card is an 8 kB chunk.
- [ ] Field check (MS-7E24):
      - A run completes in about a second, and its values match History, Storage and Event Viewer.
      - Unplugging a case fan for a minute turns Fans and pump to Attention after the Health
        event.
      - Each of the four exports opens (the PDF in Edge) and shows the on-screen values.
      - An export without serials contains none; with them it contains the drives' and the
        board's.
      - Running without the service turns Service and Sensors to Attention.

### P25 — Themes, open on Dashboard, shareable cards (M11 → 0.5; first, small)
On `v2-p25-themes`, stacked on `v2-p21-diag` (it touches the same Settings and Health files).

Defaults chosen at build (2026-10-15). The owner can change them at the field check:
- **Themes.** Six: Midnight (today's look, the default), AMOLED (pure black, no glow), Cyber
  (violet night, neon), Glass (translucent cards over a colour wash, no blur), Minimal (flat
  graphite) and Daylight (light).
- **Accents.** The theme's own, or Cyan, Blue, Violet, Magenta or Green. No amber or red
  accents, so an accent never reads as a warning. Each accent has a second colour for the GPU,
  and its own shades on Daylight.
- **Warning colours.** OK, warning and problem keep their meaning in every theme; only their
  shade changes, so they keep contrast.
- **Contrast.** Every theme and accent pair is unit-tested to WCAG AA: 4.5:1 for text, muted
  text and the colours used as text, on each surface; and axe-core runs on every tab in every
  theme.
- **Dynamic states.** Idle, Gaming (a game is measured), Warning and Critical (Health). A thin
  rail under the header and the background wash take the state's colour; Critical pulses
  four times, then stays lit (not at all with the motion off). The words are elsewhere already
  (the Health strip, the game tile), so colour is never the only sign.
- **Open on.** Dashboard by default, at launch and when the window is opened from the tray;
  "Last tab" returns to where you were.
- **Share cards.** In Hardware, "Share card": 16:9 (1920 × 1080), square (1080 × 1080) or
  story (1080 × 1920), each Full, Minimal or Transparent. CPU, GPU, RAM, board, storage,
  Windows and the Performance Score, with "Powered by Vyro". A name you type (default "My
  PC"); no computer name and no serial numbers. Saved as PNG to `Pictures\Vyro` (Show in
  folder), or copied to the clipboard. The card uses the current theme and accent.

Budget:
- **No new thread or polling.** Themes are CSS variables set once per change; canvases redraw
  only when the theme changes.
- **Animation.** The Critical pulse is a CSS opacity animation on a 2 px rail, four cycles then
  still; no backdrop blur anywhere.
- **Share cards.** Drawn on one canvas only while the card maker is open; the PNG goes to the
  app as raw bytes (no base64).
- **Bundle.** The main bundle grows under 6 kB; the card maker loads on first use.
- **Versions.** Settings gain `theme`, `accent` and `openOn` (old files load with the
  defaults); IPC protocol stays 13.

- [x] Built (2026-10-15, branch `v2-p25-themes`, stacked on `v2-p21-diag`):
      - **Themes (`src/themes.ts`).** Six themes as token sets put on the page root before the
        first paint (the page keeps a copy of the choice; `settings.json` stays the source).
        - Cards use `--card` / `--card-top`, so only Glass is see-through, and dialogs, toasts
          and sticky table heads stay opaque.
        - Canvases (the dials, sparklines) redraw once on a theme change.
        - The app paints the window in the theme's background with a matching title bar
          (`src-tauri/src/look.rs`), so Daylight does not open on a dark frame.
        - Unknown names in `settings.json` fall back to the defaults, keeping the rest of the
          file.
      - **Contrast.** A unit test checks all 36 theme and accent pairs to WCAG AA:
        - text, muted text, both accents, OK, warning and problem on every surface (Glass
          cards over the bare page and both washes);
        - text on the tinted strips and buttons at 7:1;
        - no accent hue near amber or red.

        Getting there darkened Daylight's green and Glass's card top, and lightened Glass's
        problem red. A second test checks that the window colours in `look.rs` match.
      - **States.** `data-state` idle / gaming / warning / critical from Health and the game,
        a 2 px rail under the header, and a wash at the top of the page. Critical pulses
        8 half-cycles (none with the profile's motion off or Windows' reduced motion).
      - **Open on.**
        - Dashboard at launch unless "Last tab" is chosen; the settings file corrects the
          page's copy without moving you once you have picked a tab.
        - The app emits `window-opened` when the tray shows a hidden window, and the page goes
          back to the Dashboard.
      - **Settings.** An Appearance card: theme tiles with swatches, accent dots (the theme's
        own or five more), Open on.
      - **Share cards (`src/card.ts`, `ShareCard.svelte` loaded on first use, in Hardware).**
        - A pure model from named inventory fields only (CPU and GPU short names, RAM, board,
          drives in decimal sizes with NVMe/SSD/HDD, Windows, the score), and a pure layout
          per size; `paint` draws it.
        - Values shrink to 72 % before they are shortened.
        - The PNG goes to the app as raw bytes. `save_share_card` writes only
          `vyro-card-*.png` names, PNG data, under 16 MB, into `Pictures\Vyro`; "Show in
          folder" uses the reports' Explorer helper.
        - Copy puts the PNG on the clipboard.
      - **Checks.**
        - Rust tests (now run in CI with `cargo test -p vyro`; that also caught a P20 GPU-name
          match, fixed on `v2-p20-bench`): settings fallback and 0.3 files, card file names.
        - Vitest: contrast, states, card model (no serials, given a board and drives that
          have them) and layout inside the picture for every size and style.
        - Browser preview:
          - axe clean on all 15 tabs in all 6 themes at 1440 px, and on Dashboard, Settings,
            Health and History for every accent on Midnight, Cyber, Glass and Daylight;
          - no overflow at 1440 or 390 px;
          - all nine cards download at 1920 × 1080, 1080 × 1080 and 1080 × 1920,
            transparent corners clear;
          - the theme and Open on survive a reload.
        - The main bundle grows 9.8 kB (3.5 kB gzipped), over the 6 kB budget: the theme
          tables and the Appearance card have to be there at first paint. The card maker is an
          11 kB chunk.
- [ ] Field check (MS-7E24):
      - Each theme looks right in the app, Daylight opens without a dark flash, and the title
        bar follows.
      - The dials change colour with the theme at once.
      - Vyro opens on the Dashboard from the tray; with "Last tab", where you left it.
      - A Critical Health problem pulses the rail four times, then holds.
      - Glass costs no more GPU than Midnight at idle (Task Manager).
      - A saved card opens in Photos at its size, Show in folder finds it, and Copy pastes into
        Discord / Paint.
      - The defaults above are what you want.

### P22 — Devices & wireless battery (M11)
On `v2-p22-devices`, stacked on `v2-p25-themes` (it adds a tab next to P25's changes).

Defaults chosen at build (2026-10-15). The owner can change them at the field check:
- **Where.** A Devices tab after Displays: input (keyboards, mice, controllers, receivers),
  audio (headsets, headphones, speakers, microphones), then the rest (cameras, printers,
  phones, USB storage, other USB devices). Below them, the sound outputs and inputs, with
  Windows' default and empty jacks.
- **What is listed.** Each physical device once, as Windows groups it (its device
  container). The PC's own parts, monitors (Displays has them), USB hubs and virtual devices
  (Print to PDF) are left out. A paired Bluetooth device that is off shows "Not connected";
  an unplugged USB device disappears.
- **Kind.** The device's own name first (a gaming mouse also has a keyboard function), then
  Windows' category, then what it does (audio endpoints, keyboard/mouse, camera, printer).
  Keyboard-and-mouse devices with no other clue show as "Keyboard / mouse".
- **Battery.** Windows' own Bluetooth battery reading (what Settings shows) first; the
  Bluetooth LE Battery Service (0x180F, Windows' cached value) where Windows has none;
  XInput's four levels for an Xbox controller. "—" where the device reports nothing (2.4 GHz
  receivers need vendor protocols, later). Battery is shown only while connected; under 20 %
  it is amber, under 10 % red, always with the number.
- **Refresh.** On opening the tab, within a second of a device arriving, leaving, connecting
  or disconnecting, and every 60 s while the tab is open (battery).
- **Privacy.** No Bluetooth addresses, device paths or serial numbers leave the collector; ids
  are hashes. Device names stay on this PC: not in reports, cards or history.
- **No low-battery notification yet.** It fits P18's alerts later.

Budget:
- **No service change.** Devices are read by the app (no administrator rights, no driver);
  IPC protocol stays 13.
- **No polling while the tab is closed.** One Windows device-change registration and one
  device-container query, both on Windows' own threads, and one parked thread that waits a
  short quiet moment before telling the window.
- **A read** is three device queries, Core Audio and XInput, under 100 ms, off the UI thread.
  Battery Service reads are cached by Windows; each device is read at most once a minute.
- **Bundle.** The tab loads on first visit; the main bundle grows under 2 kB.

- [x] Built (2026-10-15, branch `v2-p22-devices`, stacked on `v2-p25-themes`):
      - **Collector (`crates/vyro-core/src/devices/`).**
        - Device containers and present device nodes come from the device query API
          (`DevGetObjects`, with a present-only filter and a fallback without it), plus
          Windows' Bluetooth battery property on each node.
        - The Battery Service is read through `bluetoothapis.dll` and XInput 1.4; both are loaded
          at run time. Battery Service readings are cached for 55 s, with a 3 s budget per read.
        - Core Audio endpoints come with their state, form factor, container and the defaults.
        - `build` is pure and unit-tested: kinds, what is left out, connection, battery
          precedence, XInput matching, audio links, and hashed ids with nothing identifying in
          the output.
      - **Hot-plug.**
        - `CM_Register_Notification` covers every interface class, and a container query covers
          "connected".
        - A parked thread waits for 0.6 s of quiet (at most 3 s), then the app emits
          `devices-changed`. Both start with the tab's first read.
      - **App.**
        - `get_devices` runs off the UI thread; non-Windows builds and `VYRO_SIM` get the
          sample.
        - The inventory example prints the list too, and a Windows-only test reads the CI
          machine.
      - **Tab.**
        - Devices sits after Displays and loads on first visit.
        - It shows the groups, each device's state, and a battery bar and number (amber under
          20 %, red under 10 %), with a low-battery line above.
        - Sound shows playback and recording, the defaults, empty jacks, and which device each
          belongs to.
        - It refreshes on opening, on a change while shown, and every 60 s while shown.
      - **Checked:**
        - core tests (560), frontend tests (100) and app tests under Wine (13) pass;
        - axe is clean on Devices in all six themes at 1440 and 390 px, with no overflow;
        - all 16 tabs are reachable with the arrow keys.
      - **Bundle.** The main bundle grows 0.9 kB (0.2 kB gzipped). The tab is a 7.4 kB chunk
        (3.1 kB gzipped) plus 3 kB of CSS.
      - **Wine.** Wine has no device query API, so CI's Windows runner is the first real read,
        and the field check the second.
- [ ] Field check (MS-7E24):
      - Every keyboard, mouse, headset and controller in use is listed once, with the right
        kind and connection. Monitors, hubs and the PC's own parts are not listed.
      - Plug a USB device in and out: it appears and goes within a couple of seconds. Turn a
        Bluetooth device off and on: "Not connected" and "Connected" follow.
      - Battery matches Windows Settings › Bluetooth & devices for Bluetooth devices. An Xbox
        controller shows a level; a 2.4 GHz receiver shows "—".
      - Sound lists the outputs and inputs and marks the defaults. An empty front jack says
        "nothing plugged in".
      - With the tab closed, Vyro's CPU in Task Manager is unchanged; the tab opens at once.
      - The defaults above are what you want.

### P24 — Custom dashboard builder (M11)
On `v2-p24-dashboard`, stacked on `v2-p22-devices` (it replaces the Dashboard markup both
touch).

Depth decided at start (2026-10-15; the owner can change it at the field check):
- **A flow grid, not free placement.** Widgets sit in order on a 12-column grid. Each is 3,
  4, 6, 8 or 12 columns wide and 1 to 4 rows tall. Cards with more to show grow past their
  rows instead of scrolling. Under 900 px the grid has 6 columns, and under 560 px each widget
  takes the full width. Order instead of x/y keeps every layout tidy at any window size, and
  every edit works from the keyboard.
- **Editing.** "Edit layout" on the Dashboard shows each widget's bar:
  - drag to move it, or move it earlier or later;
  - wider, narrower, taller or shorter, or drag its corner;
  - its settings (which reading, which GPU);
  - duplicate and remove.
  A palette adds widgets. "Done" saves; "Cancel" drops the changes.
- **Lock and reset.** A layout can be locked (no editing until it is unlocked). "Reset"
  puts a layout back to the preset it came from.
- **Layouts.** A picker on the Dashboard switches between saved layouts. You can make a new
  one (a copy of the current), rename it, or delete it (not the last). Up to 12 layouts of
  up to 40 widgets each.
- **Presets.** Seven ship as layouts: Default (today's Dashboard, widget for widget),
  Gaming, Work, Streaming, Benchmark, Monitoring and Minimal.
- **Widgets.**
  - Gauge: CPU or a GPU.
  - Reading: any of about 35 live values. CPU usage, clock, temperature and power; GPU usage,
    clock, temperature, hotspot, power, VRAM and fan, for each GPU; memory; system power;
    each fan and pump; download, upload and ping; disk read, write and the hottest drive; the
    game's FPS, 1 % low and frame time.
  - Graph: the last minute of a reading.
  - History: the last hour of a recorded sensor, from the history store.
  - Game: FPS, 1 % low and the frame-time graph.
  - Health: the grade and the top findings.
  - Events: the latest health events.
  - Today's cards: CPU cores, CPU sensors, GPUs, memory, power, board and fans, and Vyro
    itself.
- **Profiles.** The compact profiles (Battery saver) still hide the detail cards (cores,
  memory, power, board, Vyro itself) in every layout, and say so.
- **Storage.** `dashboard_layouts` is kept as `dashboard.json` in the app's config folder,
  next to `settings.json` but apart from it, so a damaged layout file can never reset the
  other settings. It is checked on load: sizes clamped, counts capped, ids made unique.

Budget:
- **No new polling.** Widgets draw from the snapshot the Dashboard already gets each tick.
  A History widget reads the store when shown and then once a minute; Events reads once a
  minute while shown.
- **Edits.** Edits stay in memory until "Done": one small file write per save, none while
  dragging.
- **Memory.** The Default layout renders the same components as today. Each Graph widget
  keeps 60 points. Within +1 MB with 40 widgets.
- **Bundle.** The editor (palette, bars, settings) loads on first "Edit layout"; the main
  bundle grows under 8 kB.
- **Versions.** IPC protocol stays 13; the settings file is unchanged.

- [x] Built (2026-10-15, branch `v2-p24-dashboard`, stacked on `v2-p22-devices`):
      - **Model.** `vyro-core/src/dashboard.rs` checks what is stored: widths snap to the grid,
        rows are clamped, counts capped, ids made valid and unique, and kinds from a later
        version kept.
      - **Store.** `src-tauri/src/dashboard.rs` keeps `dashboard.json`: written through a
        temporary file, and a file that does not load is renamed `dashboard.bad.json`.
        `get_dashboard` and `set_dashboard` are its commands.
      - **UI.** `src/dashboard.ts` holds the readings, the Default preset and what to show.
        `dashboard-presets.ts` holds the other six presets; `dashboard-edit.ts` the palette,
        sources and edits. All are unit-tested:
        - every preset resolves on this PC;
        - Default matches the old Dashboard tile for tile;
        - each reading reads the snapshot;
        - move, resize, duplicate, remove, add, reset, new and delete behave.
      - **Components.**
        - `DashboardGrid` is the picker, Lock and the grid.
        - `DashWidget` draws gauges, readings and cards.
        - `DashExtra` (Graph, History, Game, Health, Events) loads with the first one shown.
        - `DashboardEditor` loads on the first "Edit layout". It has the bars, a menu per
          widget, the palette, drag by the bar, the corner (pointer, or arrow keys) and
          spoken results.
        - "Save as new layout" takes the edits; the original stays as it was.
      - **Fixed along the way:** the History widget read the store every tick (it now reads
        once a minute), and dragging a corner started a move (now only the bar moves a
        widget).
      - **Checked:**
        - frontend tests (111), core tests (567) and app tests under Wine (14) pass;
        - axe is clean on the Dashboard and the editor in all six themes, and on all seven
          presets;
        - no overflow at 1440, 800 or 390 px;
        - moving by the menu and by dragging works, and so does resizing by keys and by the
          corner;
        - a new layout survives a reload, and the lock survives a reload.
      - **Bundle: over budget.** The main bundle grows 11.9 kB (5.7 kB gzipped), over the
        8 kB budget: the readings table and the widget renderer have to be there at first
        paint. The editor (13.3 kB), the less-used widgets (7.3 kB) and the other presets
        (1.7 kB) load when needed.
- [ ] Field check (MS-7E24):
      - The Dashboard looks as before on Default (the tiles are now four to a row).
      - Build a layout: add, move (drag and keyboard), resize (corner and keys), duplicate,
        remove, change a reading. Press Done, restart Vyro: it is still there.
      - Switch between the presets while a game runs (Gaming shows the frame times).
      - Lock a layout: Edit is off until unlocked. Reset puts a preset back.
      - With 40 widgets on a layout, Vyro's memory and CPU in Task Manager stay near today's.
      - The depth chosen above (flow grid, layout-level lock) is what you want.

### P23 — 3-D GPU / My PC / thermal map (M11)
On `v2-p23-visual`, stacked on `v2-p24-dashboard` (it adds a tab next to the ones P22 and P24
touch).

Defaults chosen at build (2026-10-15). The owner can change them at the field check:
- **Where.** A "My PC" tab after Hardware with three views: PC (the default), GPU and Thermal
  map. Each can be drawn in 3-D or as a flat 2-D plan; the choice is remembered.
- **Stylized, our own models only.** No exact-board renders and no brand logos. The PC is a
  generic ATX tower with its side off. It holds:
  - the board, the CPU with a tower cooler (or an AIO block and radiator when a pump is
    reported) and the RAM sticks in the populated slots;
  - each discrete GPU in a PCIe slot;
  - M.2 drives on the board and SATA drives in the cage;
  - the case fans Vyro reads, and a PSU.
  Monitors are listed beside it (they have no temperatures).
- **GPU model.** Built from a bundled table:
  - The reference designs are keyed by PCI device id: NVIDIA Founders Edition (RTX 20 to
    50), AMD reference (RX 6000, 7000 and 9000) and Intel Arc Limited Edition.
  - Partner cards (another subsystem vendor) get a shape from the GPU's tier: fans, length,
    thickness and power connectors.
  - Anything else gets a generic two-fan card.
  - "Does this look like your card?" lets the owner set fans, length, thickness, power
    connectors and colour. That is kept in the settings per card.
  - Integrated graphics are noted on the CPU, not drawn as a card.
- **Live.** Fans spin at their reported RPM (scaled, so it reads as speed, not a blur). Parts
  carry their temperature on a label, and clicking a part or its label shows its details
  with a link to its tab.
- **Thermal map.** Each part is coloured by how close it is to its limits: blue when cool,
  green at normal, amber at the warning limit, red at the problem limit. The limits are the
  ones Health uses. Modes CPU, GPU, Board, Storage and Full colour only those parts (the
  rest grey).
- **Motion.** Fans spin at up to 30 fps with the profile's full motion and up to 10 fps with
  reduced motion; with motion off nothing spins. Without spinning or dragging, a frame is
  drawn only when a reading changes.

Budget:
- **On demand only.** Three.js (0.186.1, MIT) loads with the tab's first visit, in its own
  chunk; the main bundle grows under 2 kB.
- **Nothing when hidden.** One WebGL context, only while the tab shows. Leaving the tab, hiding
  the window or switching to 2-D stops the loop and frees the context (renderer disposed,
  context lost), so there is no GPU work or GPU memory while hidden.
- **Shown.** At most 30 fps, under 5 % of one CPU core while spinning (field check), and the
  WebView's GPU process within +40 MB.
- **Installer.** Grows under 0.3 MB.
- **Versions.** The settings gain `myPc` (2-D or 3-D, and the owner's GPU shapes); old files
  load with the defaults. IPC protocol stays 13.

- [x] Built (2026-10-15, branch `v2-p23-visual`, stacked on `v2-p24-dashboard`):
      - **Models.**
        - `src/gpuspecs.ts` holds the reference designs by device id and the partner names by
          subsystem vendor. A partner card's shape follows its tier, parsed from the model
          number (NVIDIA, AMD with RDNA 4's naming, and Intel Arc). Shapes read back from the
          settings are checked.
        - `src/pcmodel.ts` lays out the tower from the hardware list and the snapshot, reads
          each part's temperature against Health's limits, and holds the heat ramp. Both are
          unit-tested: every part fits the case, an AIO or a tower cooler, RAM by slot name,
          a second GPU below the first, integrated graphics left out, and heat and colours
          rising with the temperature.
      - **3-D (`src/scene3d.ts`, loaded with the tab).**
        - Stylized meshes: fins, fans with blades, the card with its fans, plugs and bracket.
        - Orbit and zoom, and a click picks a part.
        - Frames are drawn only while something spins or moves, or once per reading, at most
          30, 10 or 0 a second by the profile's motion.
        - `dispose` frees everything and loses the context. Each scene gets a fresh canvas,
          because a canvas whose context was freed cannot have another.
      - **Tab (`MyPcPanel`, `PcPlan`).**
        - PC, GPU and Thermal map views; label chips and a readings list; details with a link
          to the part's tab; the displays; "Does this look like your card?".
        - The 2-D plan is used when chosen or when WebGL is missing.
        - The settings gain `myPc` (a damaged section falls back to its defaults without
          touching the rest).
        - Three.js joins `THIRD_PARTY_NOTICES.md`.
      - **Checked:**
        - frontend tests (125), core tests (567) and app tests under Wine (15) pass;
        - axe is clean on PC, Thermal map and GPU in all six themes, and on the 2-D plan;
        - no overflow at 1440, 800 or 390 px;
        - spinning fans draw 10 to 11 frames a second under reduced motion;
        - with the fans still, it draws one frame per reading (4 in 4 s);
        - leaving the tab or hiding the window removes the canvas and loses its context;
        - correcting the card to two fans redraws it.
      - **Fixed along the way:**
        - thermal colours were lost (Three.js wants `rgb()` with commas);
        - switching views reused a freed canvas;
        - the card form overflowed;
        - the plan drew edge-on fans as discs;
        - the label chips crowd narrow views (they hide under 520 px; the list has the same
          readings).
      - **Bundle.** The main bundle grows 0.9 kB. The tab is a 27 kB chunk and Three.js with
        the scene a 566 kB chunk (142 kB gzipped), both loaded on first visit.
- [ ] Field check (MS-7E24):
      - The PC view shows your cooler type, RAM sticks, GPU, drives and fans, each where
        expected. Labels match the Sensors tab.
      - The GPU looks like your card, or can be corrected and stays corrected after a restart.
      - Fans spin faster under load. The thermal map turns amber or red near the Health limits
        (a stress test or a benchmark).
      - Task Manager: Vyro's CPU while the tab spins its fans; the WebView's GPU process memory
        before, while and after the tab (it drops back after leaving it).
      - The 3-D toggle off gives the plan. The defaults above are what you want.

### P26 — Remote companion (M12 → 0.5)
On `v2-p26-remote`, stacked on `v2-p23-visual` (it adds a Settings card and a header chip next
to the earlier phases' changes).

Defaults chosen at build (2026-10-15). The owner can change them at the field check:
- **Where it runs: the desktop app, not the service.** V2_PLAN put the server in
  `VyroService`; it is in the app instead. Reasons:
  - The code that reads from the network runs as the signed-in user, never as LocalSystem.
  - Windows' own firewall prompt asks the owner the first time.
  - It works with the in-process engine too.
  - The page is the app's own bundled build.

  The cost: the companion answers only while Vyro runs. Running in the tray counts.
- **Off by default.** Settings → Remote turns it on, after saying what that means:
  - local network only;
  - not encrypted;
  - Windows will ask about the firewall (allow Private networks only).

  It stays on across restarts until turned off. Turning it off closes the port and every
  connection at once. Paired devices are kept until they expire, so turning it back on needs
  no new pairing. "Sign out all devices" removes them all.
- **Transport: plain HTTP on the local network.** It uses IPv4 on port 8976, which can be
  changed. HTTPS was ruled out because a self-signed certificate puts a warning on every
  phone that the owner would have to click through. The page says "Local network · not
  encrypted". It is read-only: nothing on it changes the PC.
- **Who may connect:**
  - Peers in private ranges (10/8, 172.16/12, 192.168/16, 169.254/16) and this PC (127/8).
    Anyone else is dropped before a byte is read.
  - The Host header must be an IP address, `localhost` or this PC's name. A web page cannot
    reach the companion through DNS rebinding.
  - Sign-ins must come from the page itself (same Origin, JSON body). No CORS.
- **Pairing.** "Pair a device" shows a QR code and an 8-character code (`K7QM-4XPA`, 40
  bits).
  - The code is valid for 5 minutes and for one device. Five wrong tries spend it.
  - The QR code opens `http://<PC address>:8976/#pair=<code>`. The code travels in the URL
    fragment, which the browser does not send with the page request; the page posts it once
    and removes it from the address bar.
  - The device names itself. The name is guessed from its browser and can be edited.
- **Sessions:**
  - A paired device gets a random 256-bit token in an HttpOnly, SameSite=Strict cookie.
    Vyro keeps only the token's SHA-256 hash, in `remote.json` in the app's local data
    folder (not roamed).
  - Sessions end 30 days after pairing (choices: 1, 7, 30 or 90 days). At most 8 devices.
  - Settings lists each device: its name, when it was paired, when it expires, when it was
    last seen and from which address.
  - "Sign out" ends a session at once and closes its live view within a second.
  - A damaged `remote.json` turns the companion off and is kept as `remote.bad.json`.
- **What it shows:**
  - CPU, GPUs, memory, drives, fans, temperatures and power;
  - FPS while a game is measured;
  - network rates and ping;
  - Health and active alerts.

  It is refreshed every second while open, in the app's theme and temperature unit. Never
  sent: processes, serial numbers, IP or MAC addresses, user or device names, Health notes.
- **Remote alerts.** Active alerts and Health problems head the page and its title. Push
  notifications to a phone are not offered: they need HTTPS and an internet push service.
- **Indicators while on:**
  - a "Remote" chip in the app's header, with the number of devices watching;
  - the tray tooltip says so;
  - Settings shows the address and the Windows firewall's state for Vyro.

Budget:
- **Off.** No socket and no thread.
- **On, nobody watching.** One thread waiting on the port, using under 1 MB.
- **Watching:**
  - One short-lived thread per connection: at most 32 connections, 8 per address (a browser
    opens up to six at once for the page's files) and 6 live views.
  - Each reading is turned into the page's view once (about 3 kB) and shared by all viewers.
  - While anyone watches, the engine keeps its 1 Hz rate with the window hidden, as the
    overlay does.
- **No service change.** IPC protocol stays 13, and the history is unchanged.
- **Bundle.** The companion is its own page (`remote.html`), under 30 kB gzipped beyond what
  it shares with the app. The Settings card loads on first view, and the main bundle grows
  under 3 kB.
- **Installer.** Grows under 0.3 MB. `httparse` is already in the tree; `qrcodegen` is new.

- [x] Built (2026-10-15, branch `v2-p26-remote`, stacked on `v2-p23-visual`):
      - **Core (`crates/vyro-core/src/remote/`):**
        - `view` builds the page's view from each snapshot. A test checks that process names,
          drive ids, adapter names, Health notes and drive letters never reach it.
        - `auth` handles pairing codes (5 bits per character from an alphabet without
          look-alikes, compared in constant time) and sessions. It keeps only SHA-256 hashes,
          saves "last seen" at most once a minute, and drops expired devices.
        - `http` reads one request per connection through `httparse`:
          - head at most 8 kB and 32 headers;
          - body at most 4 kB, no chunked uploads;
          - 10 s to send the whole request;
          - every answer carries `nosniff`, `no-referrer` and `DENY`, and CR/LF are stripped
            from header values.
        - `net` checks peers and the Host header and orders this PC's addresses (the default
          route first, link-local last). `firewall` reads Windows Defender Firewall's rules
          for the app; it never writes one.
        - `qr` draws the pairing QR code as an SVG path.
        - `server` routes the page, its files and the API. The live view uses server-sent
          events, and signing a device out ends its live view at once. `stop` closes the
          port and every connection.
        - `examples/companion.rs` serves a built page with simulated readings, for working
          on the page; `pnpm dev` proxies `/api` to it.
      - **App (`src-tauri/src/remote.rs`):**
        - `remote.json` lives in the local app data folder. A damaged one turns the
          companion off and is kept as `remote.bad.json`.
        - Start, stop, port and length changes, pairing and sign-out run off the UI thread.
        - The page comes from the bundled build, never Tauri's `index.html` fallback.
        - A phone watching keeps the 1 Hz rate.
        - The tray tooltip says "remote on".
      - **UI:**
        - `remote.html` is its own Vite entry (`src/remote/`). It covers pairing (the code
          from the QR link, the device's name from its browser), live tiles, temperatures,
          fans, drives, alerts, sign-out, and stale and reconnect states.
        - Settings has a Remote card (loaded on first view): confirm before turning on, the
          addresses, the firewall state, the QR code with a countdown, devices with sign-out,
          and options.
        - A header chip shows while the companion is on and opens the card.
        - Health's limits moved to `src/limits.ts`, so the page does not load the 3-D model's
          code.
      - **Checked:**
        - core tests (607), app tests under Wine (17) and
          frontend tests (132) pass. The companion's server tests also pass under Wine;
          Windows' firewall and address read is checked on CI's Windows runner.
        - End to end in Chromium against the example server: a wrong code is refused with
          the tries left; the QR link fills the code and leaves the address bar clean;
          pairing survives a reload; a spent code is refused; signing out answers 401 after.
        - axe is clean on the pairing and live pages in all six themes at 390 and 1440 px,
          and on the Settings card (midnight 1440, daylight 390), with no overflow.
        - In the browser preview of the Settings card: a bad port is refused, a new port
          changes the addresses, the chip opens the card, and turning off removes the chip.
      - **Fixed along the way:**
        - 4 connections per address was too few: a browser opens six at once for a page's
          files, and the page never started. Now 8 per address and 32 in all.
        - A pairing link opened while the page already showed was ignored.
        - A live view whose device was signed out stayed open until its next heartbeat.
      - **Bundle.** The main bundle grows 1.4 kB gzipped (the second entry moved Svelte's
        runtime into a shared chunk). The phone loads 29 kB gzipped in all; the Settings
        card is a 3.6 kB chunk.
- [ ] Field check (MS-7E24 and a phone on the same Wi-Fi):
      - Settings → Remote → Turn on: Windows asks about the firewall; allow Private. The card
        then says the firewall allows Vyro, and the header chip shows.
      - Pair a device: scan the QR code with the phone and pair. The tiles update every
        second, in Vyro's theme and unit, and the chip says "1 watching".
      - Type the code by hand from a second device. Five wrong codes spend it.
      - Sign the phone out in Vyro: its page goes back to pairing within seconds.
      - Close the window to the tray: the phone keeps updating every second. Turn the
        companion off: the phone shows "Can't reach Vyro". Turn it back on: the phone
        reconnects without pairing again.
      - From a phone on mobile data (not the Wi-Fi), the address does not answer.
      - Task Manager: Vyro's memory and CPU with the companion off, on, and with a phone
        watching.
      - The defaults above are what you want: port, 30 days, plain HTTP, the app rather than
        the service.

### P27 — V2 polish & validation (M13 → 1.0)
On `v2-p27-polish`, stacked on `v2-p26-remote`. V2_PLAN asks for:
- a footprint audit;
- an accessibility pass;
- crash recovery;
- V1 → V2 migration testing;
- clean-machine testing;
- control-safety validation on every writable controller;
- anti-cheat notes and docs.

The 1.0 gate (V2_PLAN, roadmap §30):
- control is tested on every writable board, and unsupported hardware is blocked;
- failures restore safe cooling;
- migration keeps V1 data;
- the resource budget holds, and V1 features still work.

Defaults chosen at build (2026-10-16). The owner can change them at the field check:
- **What an upgrade keeps.** Upgrading from any released version (0.1.x, 0.2.0, 0.3.0) keeps:
  - settings;
  - alert rules;
  - fan settings and the profile in force;
  - the history: every tier, Health, alert and fan events, and game sessions;
  - the 0.1/0.2 game log.

  Going back to an older version is not supported for the history: an older Vyro leaves a
  newer history untouched and runs without one.

  This is checked with files written by each release's own code. Each tag was built in a
  scratch checkout, and a small generator (kept in `crates/vyro-core/tests/fixtures/gen/`) wrote
  its files. The tests open those files with today's code.
- **Damaged files.** Every file Vyro keeps follows one rule:
  - Writes go to a temporary file that is then renamed, so a crash or power cut never leaves
    half a file.
  - A file that cannot be read is set aside as `<name>.bad.<ext>` (only the newest is kept),
    the log says so, and defaults are used. The file is never silently overwritten.
  - Settings load one setting at a time, so a bad value resets only that setting.
- **A damaged history database** is set aside as `history.bad.sqlite` and a new one is
  started. A database written by a newer Vyro is left as it is, and history stays off (as now).
- **Fan control after a crash (a fix found in this audit).** At start, a BIOS state left by a
  crash is written back first, as before. Two cases change:
  - **The write-back fails.** The saved state is now kept and used for every later attempt.
    Before, Vyro's next capture could overwrite it with the settings Vyro itself had left on the
    chip, so "BIOS control" would have meant Vyro's stale speeds.
  - **The file cannot be read.** The allow-listed headers that the chip has under software
    control go to full speed, which is always safe. The file is kept as `fan-restore.bad.json`,
    and the Cooling log says to restart the PC, which gives the BIOS its settings back.

  This is a hardware write, so it ships only after the field check.
- **Accessibility.** The target is WCAG 2.1 AA as axe-core checks it, on:
  - every tab in all six themes, at 1440 and 390 px;
  - the main dialogs;
  - the companion page.

  Every control is reachable by keyboard and shows a focus ring. Reduced motion stops CSS
  animation. It runs in CI as its own job.
- **Footprint.**
  - CI gates the installer (< 20 MB, as before) and now also what each page loads at start
    (scripts and styles, gzipped): the app's window under 125 kB (118.2 kB now) and the
    companion under 32 kB (29.4 kB now).
  - CI also installs Vyro on its Windows runner and reports the service's memory and CPU. This
    is not gated, because a CI virtual machine has few sensors.
  - The real budget is measured on the owner's PC at the field check.
- **Clean machine.** CI's Windows runner has never had Vyro installed. CI installs the MSI there
  and checks:
  - the files;
  - the service and its restart-on-failure settings;
  - that the service answers on its pipe and writes history.

  It then uninstalls and checks what is removed and what is kept. The owner repeats this on a
  clean PC or virtual machine, and upgrades from 0.3.0.
- **Control safety.** `docs/control-safety.md` lists each rule, the code that enforces it, its
  tests and the field result on MS-7E24, the only writable board (decision 24).
- **Anti-cheat.** `docs/anti-cheat.md` says what Vyro does that an anti-cheat can see, and what it
  never does. Compatibility with a given game is a field result, not a promise.
- **Version.** It stays 0.3.0 on this branch. 1.0.0 is set when M13 merges into `main`
  (decision 20).

Budget:
- **No runtime cost.** No new thread, timer or setting. The IPC protocol stays 13 and the
  history schema stays 3.
- **Installer.** Unchanged; the app gains no dependency.
- **CI.**
  - One new parallel job (accessibility, about 5 minutes).
  - About 3 more minutes on the Windows job (install, probe, uninstall).
  - New dev dependencies, not shipped: Playwright and axe-core.

- [x] Built (2026-10-16, branch `v2-p27-polish`, stacked on `v2-p26-remote`):
      - **Migration.**
        - Each released tag (0.1.4, 0.2.0, 0.3.0) was built in a scratch checkout. A generator
          (`crates/vyro-core/tests/fixtures/gen/`) wrote that release's files with its own
          code: `settings.json`, `alerts.json`, `fan-control.json`, `fan-profile.json`,
          `games.json` and `history.sqlite`. It also wrote `expect.json`: what that release
          itself read back.
        - `tests/migration.rs` opens each set with today's code:
          - the History view's series match on every tier (to the last digit of an average);
          - Health, alert and fan events, game sessions and a replay timeline match;
          - the engine starts on the old folder: rules unchanged, the 0.1/0.2 game log moved
            into the history;
          - the fan controller loads the old settings and the profile in force.
        - The app's tests read each release's `settings.json`. Every setting carries over,
          including 0.1's "not detailed" overlay as Minimal mode.
        - No data loss was found.
      - **Crash recovery.**
        - `vyro_core::files`: atomic writes (flushed to disk, then renamed) and damaged files
          set aside as `<name>.bad.<ext>`. Settings, alert rules, fan settings and profile,
          the BIOS fan state, the game log, layouts and paired devices all use it.
        - Before, `settings.json` was written in place and a damaged one silently reset every
          setting. Now it is read one setting at a time.
        - A history database that is not a database is set aside and a new one started. A
          newer one is still left untouched.
      - **Fan restore fix (hardware write; field check needed).** A crash's saved BIOS state
        that fails to go back is now kept for every later attempt. Before, the next capture
        could replace it with Vyro's own leftover speeds. An unreadable one sends the
        software-driven allow-listed headers to full speed and says to restart.
        - Found in a review after the build: if the fan chip also did not answer at start (it
          gets three tries within about 150 ms), nothing went to full speed and nothing was
          recorded. The first capture with control on then saved the leftover speeds as the
          BIOS state. Now nothing is captured or driven until the chip answers, then full
          speed comes first. The damaged file stays until then, so a restart meanwhile tries
          again.
      - **CI.**
        - A new Accessibility job (`pnpm a11y`, 362 checks in about 3 minutes):
          - axe-core on all 17 tabs × 6 themes × 1440 and 390 px;
          - the dashboard editor, an alert rule editor, the fan control confirmation, the
            share card, and the remote card's confirmation and pairing;
          - the companion's pairing and live pages in every theme (its API answered by the
            test);
          - the keyboard: every focusable control shows a focus ring and focus never sticks;
          - reduced motion: no CSS animation runs.
        - The size budget (`scripts/bundle_budget.py`).
        - On the Windows runner: install the MSI, check files, service and recovery actions,
          ask the service with `vyro-probe` (readings, history, its CPU and memory), uninstall,
          and check the service and program files are gone and the history kept.
      - **Found and fixed by the sweep.** The System tab's startup-app table scrolled sideways
        at phone width with no keyboard access (WCAG 2.1.1). Scrolling tables (System,
        SMART, game profiles, sessions) now get a tab stop only while they scroll
        (`src/scrollable.ts`).
      - **Found and fixed by the sweep on CI.** In the Sensors tab, a row with no reading or
        a hidden sensor was dimmed with 50 % opacity, which put its numbers below 4.5:1 in
        every theme (WCAG 1.4.3). The sweep only caught it on CI, where a reading happened to
        be missing. Dimmed rows now use the muted colour. The sweep hides a sensor first, so
        a dimmed row is always checked.
      - **Docs.**
        - `docs/control-safety.md`: every safety rule → code → tests → MS-7E24 result.
        - `docs/anti-cheat.md`: what an anti-cheat sees and what Vyro never does. FACEIT's
          2025 block of PawnIO's old signature was reported fixed in PawnIO 2.1.0; Vyro ships
          2.2.0.
        - `docs/footprint.md`: every V2 feature's threads and costs, and the 1.0 measurement
          procedure.
        - `docs/field-check.md`: the field checks of P16 to P27, ordered into sittings for the
          one build that holds them all.
        - README: what 0.4 and 0.5 add, for users (Insights, the score, alerts, Benchmark,
          Diagnostics, themes, the dashboard builder, My PC, Devices, cards, and the phone
          companion with its security model). CONTRIBUTING updated.
        - `docs/anti-cheat.md` now records the owner's Valorant run from P6. The Risks entry
          now has the FACEIT certificate fix reported for PawnIO 2.1.0.
      - **Small polish.** The companion server's threads are now `vyro-companion`. They shared
        the name `vyro-remote` with the app's service-connection thread.
      - **Checked.** Core tests (614 + 3 migration), the service test, app tests under Wine
        (20) and frontend tests (132) pass, and `pnpm a11y` is clean. On CI, the Windows
        clean install (install, probe the service, uninstall) passes.
- [ ] Field check (MS-7E24; ideally also a clean Windows PC or virtual machine):
      - **Upgrade.** Install this build over 0.3.0 (or 0.2.0) as you use it: settings, alert
        rules, fan settings and profile, History (7 days back), Health and alert logs, game
        sessions and replays are all still there.
      - **Clean machine.** On a PC or virtual machine that never had Vyro:
        - install, run, and accept PawnIO;
        - the app shows readings within 2 s;
        - uninstall: the service and program files go, and `%ProgramData%\Vyro` stays.
      - **Damaged files.**
        - Quit Vyro, put `{` in `%APPDATA%\app.vyro.monitor\settings.json`, start: defaults,
          and `settings.bad.json` is beside it.
        - Put `"temperatureUnit": "k"` in a good file: only the unit resets.
      - **Fan restore after a crash** (fan control on; a hardware write):
        - `taskkill /F /IM vyro-service.exe`: the BIOS curve comes back after the restart
          (as in P12).
        - Then the damaged case. In an administrator prompt, pause the automatic restart
          (`sc failure VyroService reset= 0 actions= ""`) and kill the service again. Replace
          `%ProgramData%\Vyro\fan-restore.json` with `{` and run `sc start VyroService`.
          Expected:
          - the fans Vyro drove go to full speed;
          - Cooling says to restart the PC;
          - `fan-restore.bad.json` exists.

          Restart the PC: the BIOS curve is back. Put the restart actions back with
          `sc failure VyroService reset= 86400 actions= restart/5000/restart/5000/restart/60000`
          (reinstalling does it too).
      - **Control-safety matrix.** Confirm every MS-7E24 row of `docs/control-safety.md`
        still holds on this build, including the P18 boost row.
      - **Anti-cheat.** Play the games you have with anti-cheat (at least one each of what you
        own: Easy Anti-Cheat, BattlEye, Vanguard, FACEIT) with Vyro running, the overlay on
        and fan control on. Fill in the table in `docs/anti-cheat.md`.
      - **Footprint.** Follow "Field measurement for 1.0" in `docs/footprint.md`; the results
        go into the README's Performance table.
      - **V1 features still work.** One pass through every V1 tab: sensors, history, Health,
        alerts and a toast, processes, storage SMART, network and ping, displays, system,
        gaming capture and overlay, tray and updates.
      - **Keyboard and screen reader.** Narrator on two tabs; Tab through Settings and Cooling.
      - **Defaults.** Confirm the defaults above, especially the size budgets and the fan
        full-speed rule.

## Risks

- Spec §8 RAM target (< 120 MB) is tight — WebView2 alone is typically 60–100 MB. Measure on
  Windows as soon as P0 runs there.
- Hardware acceptance criteria require real machines from the §9 test matrix; CI can only prove
  compilation and unit tests.
- Anti-cheat: FACEIT refused to run with PawnIO loaded over its old signing certificate;
  PawnIO's author reports that fixed in 2.1.0 (Vyro ships 2.2.0), and FACEIT has not been seen
  confirming it. Valorant (Vanguard) runs with it. PawnIO has its own entry in Installed apps,
  so anyone whose game refuses it can remove it; Vyro then runs without the low-level sensors
  and says so. [`docs/anti-cheat.md`](docs/anti-cheat.md) has the details and the P27 field
  table.
- PawnIO has a single upstream maintainer; module ABI or signing changes need a Vyro update.
- SignPath / Certum OSS require a public repo with an OSI license and a verifiable build.
- Fan control (P12): some boards' fans are owned by the embedded controller and cannot be
  driven safely, pump headers are easy to mislabel, and another tool (MSI Center, FanControl)
  writing the same EC fights Vyro. Mitigations: allow-list, owner-confirmed pump header,
  readback on every write, restore on any fault, and the EC's free/busy page handshake.

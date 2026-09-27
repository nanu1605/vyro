# Vyro — Roadmap & Decision Log

Execution tracker for [`PROJECT_PLAN.md`](PROJECT_PLAN.md) (the build spec, v2). The spec is the
source of truth for *what* to build; this file records *in what order*, what is done, and which
open decisions (spec §12) have been answered.

Deviation from the spec: the custom kernel driver (spec P6) and its attestation signing (spec
P11, driver part) are replaced by **PawnIO** — see decisions 10–11 and the P6 section.

## Decision log (spec §12)

| # | Decision | Answer | Status |
|---|---|---|---|
| 1 | Product name / codename | **Vyro** (repo name) | decided |
| 2 | First release cut | **Early release** after P5 (P11), before the low-level sensors; the former P11b (own driver signing) is dropped | decided |
| 3 | Gaming module (P10) in v1? | **Yes**: frame capture with the bundled PresentMon (MIT, Intel-signed) run by the service; companion overlay, no injection | decided |
| 4 | Fan control (v2 write path) | Not built without explicit go-ahead | default: no |
| 5 | Ping target host | `1.1.1.1`, user-configurable | default |
| 6 | History tiers / cap | raw ≤ 30 min · 10 s avg ≤ 24 h · 1 min avg ≤ 7 d · ≤ 200 MB | default |
| 7 | Frontend framework | **Svelte 5** + strict TS + Vite | decided |
| 8 | Installer | **WiX (MSI)**; bundles `PawnIO_setup.exe` and runs `-install -silent` when PawnIO is missing or older | default |
| 9 | Extra Super-I/O coverage | Nuvoton NCT6687D first (MSI AM5), then NCT67xx + ITE IT87xx | default |
| 10 | Low-level hardware access | **PawnIO** (signed, free, HVCI-compatible) replaces our own KMDF driver | decided |
| 11 | Code signing | **SignPath Foundation** or **Certum Open Source** (no EV certificate) | decided |
| 12 | Source availability | **Open source**; license MIT OR Apache-2.0 (LICENSE files added in P11; can still change before the repo goes public) | default |
| 13 | Privilege split (P8) | **LocalSystem service `VyroService`** runs the whole engine (sensors, history, health, alerts) and serves it over `\\.\pipe\vyro-engine`; the app runs unelevated and falls back to an in-process engine without the privileged sensors when the service is missing | decided |

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
installer-size check (< 20 MB). Field-checked on the reference PC: install with PawnIO, test
notification, close to tray, "Start with Windows" (tray at sign-in, no UAC prompt), amd-cpu
poll ≈ 1 ms under full load. Open: §8 numbers from the reference PC into the README; code
signing and the Tauri updater (deferred: need the repo public, a SignPath / Certum account and
an update signing key); clean-machine test with Memory Integrity on.

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
- [ ] P9a Measure on the reference PC (visible and in the tray) and decide the next trims.
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
      FPS, what limits the frame rate (GPU or CPU busy ≥ 85 % of the frame, else capped), the
      last 240 frame times. `game.fps`, `game.frametime`, `game.low1` are history sensors.
      Sessions ≥ 30 s are logged (`games.json`, last 100) with averages, lows from a 0.1 ms
      histogram, max frame time, max CPU / GPU temperature and average load. Gaming tab (live
      figures, frame-time graph, limiter, session log), a Game FPS dashboard tile, a Settings
      switch; PresentMon fetched with a pinned SHA-256 and installed by the MSI.
      `vyro-remote --capture <exe|pid>` checks capture through the service without the app.
- [ ] P10a Field check: FPS and lows within a few % of PresentMon's or the game's own counter
      in a real game; session logged after quitting; anti-cheat games (FACEIT excepted, as with
      PawnIO) unaffected.
- [x] P10b Companion overlay (off by default; Settings → Gaming or the tray's "Game overlay"):
      a transparent, always-on-top, click-through window in a chosen corner of the game's
      monitor with FPS, 1 % low and frame time, optionally a frame-time graph and CPU / GPU
      temperature, load and power. Created only while the captured game is in front, shown with
      `SW_SHOWNOACTIVATE` (never takes focus), destroyed afterwards; while it shows, the engine
      polls at the profile's rate instead of the tray rate. Its window may only listen for its
      readings. A desktop window, not injected: it shows over borderless and windowed games,
      not exclusive fullscreen.
- [ ] P10b Field check: overlay appears over a borderless game in the chosen corner, never
      steals focus or clicks, disappears on alt-tab and when the game exits.

**Deferred to v2:** fan/pump control (driver write path), injected in-game overlay.

## Risks

- Spec §8 RAM target (< 120 MB) is tight — WebView2 alone is typically 60–100 MB. Measure on
  Windows as soon as P0 runs there.
- Hardware acceptance criteria require real machines from the §9 test matrix; CI can only prove
  compilation and unit tests.
- Anti-cheat: FACEIT refuses to run with PawnIO loaded; Vanguard untested. Low-level sensors
  must be switchable off, and PawnIO must be removable from the installer.
- PawnIO has a single upstream maintainer; module ABI or signing changes need a Vyro update.
- SignPath / Certum OSS require a public repo with an OSI license and a verifiable build.

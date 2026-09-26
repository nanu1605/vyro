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
| 3 | Gaming module (P10) in v1? | Decide at milestone M5 | open |
| 4 | Fan control (v2 write path) | Not built without explicit go-ahead | default: no |
| 5 | Ping target host | `1.1.1.1`, user-configurable | default |
| 6 | History tiers / cap | raw ≤ 30 min · 10 s avg ≤ 24 h · 1 min avg ≤ 7 d · ≤ 200 MB | default |
| 7 | Frontend framework | **Svelte 5** + strict TS + Vite | decided |
| 8 | Installer | **WiX (MSI)**; bundles `PawnIO_setup.exe` and runs `-install -silent` when PawnIO is missing or older | default |
| 9 | Extra Super-I/O coverage | Nuvoton NCT6687D first (MSI AM5), then NCT67xx + ITE IT87xx | default |
| 10 | Low-level hardware access | **PawnIO** (signed, free, HVCI-compatible) replaces our own KMDF driver | decided |
| 11 | Code signing | **SignPath Foundation** or **Certum Open Source** (no EV certificate) | decided |
| 12 | Source availability | **Open source**; license MIT OR Apache-2.0 unless changed before the repo goes public | default |

Other: the speedometer prototype referenced in spec §3 was not available, so it is rebuilt from
the §4.3 animation spec in Phase 0.

## Milestones

| Milestone | Phases | Output | Driver | Signing |
|---|---|---|:-:|---|
| **M1** Foundation | P0 | Pipeline + animated gauges on simulated data | – | – |
| **M2** No-driver app | P1 → P5 | Inventory, usage, GPU, displays, storage, memory, network, self-monitoring | – | – |
| **M3** Early release | P11 | Signed app + MSI (+ PawnIO prerequisite) + updater (runs elevated) | PawnIO | SignPath / Certum OSS |
| **M4** Low-level sensors | P6 | CPU temps/power/voltage, Super-I/O fans/temps/voltages, SPD + DIMM temps, RAM timings | PawnIO | – |
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
- Signing: SignPath Foundation or Certum Open Source (requires the repo public under an OSI
  license) for vyro.exe and the MSI.
- WiX MSI; bundles the unmodified signed `PawnIO_setup.exe`, run as an elevated custom action
  with `-install -silent` when the uninstall key `PawnIO\DisplayVersion` is missing or older.
- Tauri updater (signed manifest); elevated autostart via a Task Scheduler "highest privileges"
  task; perf pass against spec §8; verify on a clean Win11 24H2+ with Memory Integrity on.
- LICENSE, THIRD_PARTY_NOTICES (PawnIO driver GPL-2.0 with IOCTL exception, PawnIO modules
  LGPL-2.1, with source links), CONTRIBUTING.

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
temperature and core VID match; the Zen 5 VID uses the SVI3 formula; package and core power
match at the 88 W PPT under Cinebench). 6c reads the NCT6687 read-only (no monitor-enable or
fan writes), field-checked on the MSI B650M GAMING PLUS WIFI: fans, board temperatures and
voltages match HWMonitor. Other boards show +12V/+5V plus raw channels until checked. 6d
built, awaiting field check: DDR5 module temperatures from the SPD5 hub sensor (read-only: hub
registers only, no page or NVM access, skipped while another tool holds the SMBus mux on
another port) and DDR5 timings from the Zen 4/5 memory controller over SMN, shown in the Memory
card. DDR4 TSOD and DDR4 timings wait for a test machine; DIMM PMIC rails need writes and stay
out.

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
- Anti-cheat: FACEIT refuses to run with PawnIO loaded; Vanguard untested. Low-level sensors
  must be switchable off, and PawnIO must be removable from the installer.
- PawnIO has a single upstream maintainer; module ABI or signing changes need a Vyro update.
- SignPath / Certum OSS require a public repo with an OSI license and a verifiable build.

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
| 16 | Devices battery sources | BLE GATT 0x180F + PnP battery devnode property + XInput; from 1.3 also Logitech HID++ (receivers and direct), PlayStation controllers' input reports and the HID Battery Strength value; other makers' dongles by request, once `vyro-inventory` names them | decided |
| 17 | Open on Dashboard | Default on, with an "Open on: Dashboard / Last tab" setting | decided |
| 18 | Fan / pump control | **Yes, the V2 flagship**, safety-gated (V2_PLAN "Hardware-control safety"); read-only by default; per-board allow-list | decided |
| 19 | Remote / web companion | In V2 (P26): served by the desktop app (not the service) over plain HTTP on the LAN, off by default, QR + code pairing, hashed session tokens that expire, sign-out at once. Its "LAN only" is superseded by 34; the companion stays as it is | default |
| 20 | Versions | 0.2 / 0.3 at milestone merges; **1.0** at M13, with M9–M12 in it (the owner, 2026-10-16: no 0.4 or 0.5) | decided |
| 21 | Themes | Several themes + accent colour, accessibility preserved | decided |
| 22 | Performance Score method | Formula v1 ([`docs/performance-score.md`](docs/performance-score.md)): nine observed inputs, fixed weights and breakpoints, versioned, inputs shown | decided (signed off 2026-10-15) |
| 23 | Benchmark positioning | Self-relative (own engine), variance reported | decided |
| 24 | Fan-control launch boards | **MSI B650M GAMING PLUS WIFI (MS-7E24, NCT6687D) only**; each further board after its own validation | decided |
| 25 | Tagline | "Know your machine." unless changed | open |
| 26 | Navigation (new IA) | Adopted incrementally; V1 tabs keep working each release | decided |
| 27 | Fans with no speed reading (My PC) | Turn slowly and say "Speed not reported"; a real 0 RPM stays still (the owner, 2026-10-16) | decided |
| 28 | Installer size budget | **< 60 MB** from 1.1 (was < 20 MB, spec §8): "we have to make good software using good tools" (the owner, 2026-10-16). The window's start bundle budget goes from 125 to 200 kB gzipped | decided |
| 29 | Navigation | A sidebar with grouped views replaces the tab strip (supersedes 26); the views and their ids stay | decided |
| 30 | Widgets and Show desktop | "Behind windows" widgets stay in view through Show desktop (Win+D), as Rainmeter's "On desktop" skins do: a probe window detects it and the widgets go on top while it lasts; Vyro's windows are never put inside Explorer's (the owner, 2026-10-17) | decided |
| 31 | Moving widgets and clicks | Widgets snap while dragged, in Windows' own move loop (`WM_MOVING`): to the work area's edges, beside another widget 8 px apart, or in line with it; Shift places freely. "Clicks pass through" works only while a widget is locked, so unlocking it on the Widgets page is always the way back (in the plan the owner approved, 2026-10-17) | decided |
| 32 | Accent of any colour | A colour wheel (1.3): fills (gauges, graphs, the 3-D view, the logo, the share card's ring) show the colour as picked; text in the accent takes a shade of the same hue and chroma, lightened or darkened just to 4.5:1 on every surface of the theme. The GPU colour is the main one turned round the hue circle, away from warning hues (the owner, 2026-10-17) | decided |
| 33 | Connect and Disconnect | Paired Bluetooth devices only (the owner's "all Bluetooth", 2026-10-17): audio through its audio connection as Settings does; any other device's link dropped by the Vyro service, and Connect asks Windows to reach it. Wired and receiver devices get no actions | decided |
| 34 | Reach from anywhere | A phone app installed from the browser (no app stores) reaches the PC through the Vyro relay: a Cloudflare Worker, one Durable Object per PC, in the owner's free account, built into releases for everyone, with any other relay allowed in Settings (the owner, 2026-10-17). Readings sealed end to end (HKDF-SHA256 to an auth key and an AES-256-GCM key; a secret per phone that replaces the pairing link's); the relay keeps only hashes and sees metadata. Off by default; the PC opens no port. Supersedes 19's "LAN only" | decided |
| 36 | Control from the phone | A phone paired from anywhere may change the PC only if the owner allows it, on the PC, when pairing or later (1.5; the owner's request, 2026-10-18): a fixed list of requests (`relay::api`), each checked on the PC against that phone's permission; protocol 2 gives each connection a session and counts its messages, so nothing can be replayed; what phones changed is kept and shown in Settings. Turning fan control on, confirming a pump and editing GPU settings stay at the PC. The LAN page stays read-only. Amends 34's "read-only" | decided |
| 35 | 1.3 ships in 1.4.0 | 1.3.0 is not released on its own: its changes (decisions 32 and 33, device names, wireless batteries) ship in 1.4.0, once field check 1.3 passes on the 1.4 test build (the owner, 2026-10-18). One update for installed copies instead of two; the 1.3 branch is neither merged nor released | decided |
| 37 | GPU tuning | Vyro may change an NVIDIA card's core and memory clock offsets and its power limit (1.6; the owner's request, 2026-10-18, on an NVIDIA desktop card and a laptop GPU): only through the driver's own interface (NVML, loaded from System32), only in the service, off until the owner turns it on and confirms, only on RTX 20 to RTX 50 cards and only what the driver gives a range for. Presets per family scaled to those ranges, the owner's own, one per profile; a stability test and "Find my best"; stock on every exit, after heat, a crash or a driver reset, and when another program keeps changing the values; only tested settings at start; from a phone only Stock and tested presets. AMD and Intel later. Supersedes "never writes GPU settings" (anti-cheat notes) and the plans' "overclocking out of scope" | decided |
| 38 | The screen on the phone | A phone may see and use the PC's screen only if the owner allows it, on the PC, apart from control (1.7; the owner's request, 2026-10-18, "direct, slow fallback"): one phone at a time, a banner and a tray item with Stop while it lasts, a notification at the start, a record of each session, an end after 10 minutes unused. Direct over WebRTC (str0m in the app, the phone's own browser), its fingerprints inside the sealed offer and answer so the relay cannot step in; the picture as H.264 from Windows' own encoder, input as Windows input in the signed-in session. Where no direct path exists, the slow view: one JPEG at a time through the relay, at most 2 a second, 20 minutes a day; the relay caps what a PC sends its phones at 1 GB a day. Windows' secure screens cannot be seen or used, and Ctrl+Alt+Del cannot be sent | decided |
| 39 | Voice from the phone | The phone's voice reaches the PC two ways (1.7; the owner's report, 2026-10-21: a WhatsApp Web voice note sent through the phone's screen recorded nothing, as it records the PC's microphone and the phone's never reached the PC; the owner's choice, "clips and live mic"). Windows adds a microphone only through a driver Microsoft has signed (an EV certificate and a partner account Vyro does not have), so Vyro ships none. A **clip** recorded on the phone is kept on the PC (AAC through Windows' own encoder, or WAV without one; named by Vyro, never the phone; 5 minutes at most; deleted after 7 days) and put on the clipboard as a file, only when the phone sends one, so Paste in a chat attaches it. The **live microphone** plays into a virtual cable the owner installs (VB-CABLE, VoiceMeeter or Virtual Audio Cable, found by name, never bundled); while it plays, the cable's recording side is Windows' default microphone (an option, on by default) through `IPolicyConfig`, an interface Windows does not document but has kept since Windows 7 (EarTrumpet and SoundSwitch use it). The defaults it replaced are kept in a file first and put back when the microphone stops, the session ends or Vyro closes, or at the next start after a crash; a role the owner changed meanwhile is left alone. Both need the Screen permission and a session on; the live microphone the direct connection | decided |

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
| **M9** Intelligence → 1.0 | P16 – P19 | Intelligence engine, Performance Score, compound alerts, hardware changes | – | – |
| **M10** Benchmark & diagnostics → 1.0 | P20, P21 | Benchmark, one-click diagnostics, reports | – | – |
| **M11** Visualization & UX → 1.0 | P22 – P25 | Devices + battery, 3-D / My PC / thermal map, dashboard builder, themes | – | – |
| **M12** Remote → 1.0 | P26 | LAN-only companion, off by default | – | – |
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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
      quiet at normal desktop use; a CPU stress test gives headroom but
      no "hotter"; Silent or a lower fan curve under load is explained by the profile or fan
      change; a background job during a game names the program; badges, "Show in History"
      and the tab dot work; footprint within budget.
      The test build is the top of the stack (`v2-p27-polish`), which holds P16 to P27; one
      pass of `docs/field-check.md` covered them all.

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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
      a CPU-only stress test leaves an "all of" CPU-and-GPU rule quiet
      and a game with a heavy CPU load fires it; "rises 15 °C within 30 s" fires on starting a
      stress test; with fan control on, a fan action raises only the driven fans (Cooling log
      and readings) and they return 30 s after the rule clears, and with control off nothing
      changes; a profile action switches the profile (tray check mark) and back, and picking
      a profile meanwhile keeps it; two CPU rules firing within a minute notify once (the second
      is logged as a repeat); the diagnostic record names the busy program.
      The defaults above stay.

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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
      after a day of use the score appears with all inputs that apply;
      the thermal headroom matches the History of a gaming evening; a heavy stress test with a
      lowered fan curve shows in Throttling or Sustained clocks; the stability counts match the
      System log in Event Viewer (Kernel-Power 41, WHEA-Logger); the daily point appears in the
      trend the next day.
      With P16, P18 and P19 also field-checked, M9 is complete.

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
- [x] Field check (MS-7E24, RTX 5060 Ti, 2026-10-16, test build `vyro-windows-bab8a30`),
      all passed:
      - A Quick run completes in about 3 minutes, then a second scores within noise of the first.
      - The CPU multi-thread peak temperature and power match HWMonitor.
      - The GPU areas name the RTX 5060 Ti (not the iGPU), and Graphics holds it at full load.
      - Stop, closing the window and quitting each end all load within a second (Task Manager)
        and leave no `vyro-bench-*.tmp` in `%TEMP%`.
      - A Full run's Sustained loops read steady.
      - A run after a GPU driver update lists the change.
      The defaults above stay.

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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
      - A run completes in about a second, and its values match History, Storage and Event Viewer.
      - Unplugging a case fan for a minute turns Fans and pump to Attention after the Health
        event.
      - Each of the four exports opens (the PDF in Edge) and shows the on-screen values.
      - An export without serials contains none; with them it contains the drives' and the
        board's.
      - Running without the service turns Service and Sensors to Attention.
      The defaults above stay. With P20, M10 is complete; with M9, 0.4 could ship from here
      (the owner's call).

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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
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
- [x] Field check (MS-7E24, 2026-10-16, test build `vyro-windows-bab8a30`), all passed:
      - The PC view shows your cooler type, RAM sticks, GPU, drives and fans, each where
        expected. Labels match the Sensors tab.
      - The GPU looks like your card, or can be corrected and stays corrected after a restart.
      - Fans spin faster under load. The thermal map turns amber or red near the Health limits
        (a stress test or a benchmark).
      - Task Manager: Vyro's CPU while the tab spins its fans; the WebView's GPU process memory
        before, while and after the tab (it drops back after leaving it).
      - The 3-D toggle off gives the plan. The defaults above are what you want.
      With P25, P22 and P24 also field-checked, M11 is complete.

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
- [x] Field check (MS-7E24 and a phone on the same Wi-Fi, 2026-10-16, test build
      `vyro-windows-bab8a30`), all passed:
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
      M12 is complete; with M11, 0.5 could ship from here (the owner's call).

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
  (decision 20; set on `v2-prep-1.0.0`, 2026-10-16).

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
- [x] Field check (MS-7E24; ideally also a clean Windows PC or virtual machine; 2026-10-16,
      test build `vyro-windows-bab8a30`), all passed on MS-7E24; the clean machine was not done:
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
      The clean-machine install and uninstall are covered by CI's Windows runner on every run,
      without a person watching or a real board. Still to write down from the owner's notes:
      the games in `docs/anti-cheat.md` and the 1.0 footprint figures in the README's
      Performance table. M13 is complete, and with it V2.
- [x] **1.0.0 released** (2026-10-16): M9–M13 in one release (decision 20). `v2` merged into
      `main` (6cc4f8c) after CI passed on both; Release run 38899109384 tagged `v1.0.0` and
      published the 11.3 MB MSI, `latest.json` and checksums here and to
      `nanu1605/vyro-releases`, where copies of 0.3.0 find the update. The footprint figures
      and the anti-cheat table are still to be written down from the owner's notes.

## After 1.0

The owner's notes on 1.0.0 (2026-10-16), nine items: a larger installer is fine; a new look
with animation; use the empty space beside the content; the accent does not change; the tab
strip's sideways scroll; a livelier 3-D PC whose fans all turn; a laptop model on laptops; a
smooth overlay graph; desktop widgets. The owner chose the "Pro dashboard" look (sidebar,
layered cards, soft accent glow, live side rail, calm motion), the three fixes first as 1.0.1
and the rest as 1.1.0, and that fans with no reading turn slowly and say so. Built on the
branch `claude/blissful-knuth-begnu5`, merged into `main` at each release.

### 1.0.1 — accent, overlay graph, fans
- [x] Built (2026-10-16):
      - **Accent.** The theme set the accent correctly, but little showed it: the selected tab
        was grey, and My PC rebuilt its 3-D scene on a theme change only, so its lights kept
        the old accent. Now the selected tab (tint and underline), the logo, every checkbox,
        radio and slider (`accent-color`), links, text selection and a faint line along each
        card's top follow it, and My PC takes its colours from the settings and rebuilds on an
        accent change. The other charts are SVG on the theme's variables and already followed.
        New contrast test: text on the selected tab's tint, every theme and accent.
      - **Overlay graph.** Readings arrive every 0.5–2 s with 15 s of 100 ms points; drawn as
        they came, the line jumped. `src/overlay-graph.ts` lines each update up with the last
        (by the points they share, else by the time between them), plays the points back about
        one update late so they slide in at an even pace, and draws a monotone curve that never
        overshoots a reading. The scale eases to a new step. At most 30 frames a second, only
        while the line moves; with the profile's animation off (Benchmark, Silent) it is drawn
        when readings arrive, as before. This replaces P14's "no animation between readings".
      - **Fans.** A fan turned only with an RPM reading, so without PawnIO (or on a GPU that
        reports none) the model stood still. Fans with no reading now turn slowly
        (0.35 turns a second) and their details say "Speed not reported"; a real 0 RPM stays
        still and says "Stopped (0 RPM)" (decision 27).
- [x] **1.0.1 released** (2026-10-16): CI passed on 262c711 (Linux, Windows, accessibility);
      `main` fast-forwarded to it and Release run 38911862292 tagged `v1.0.1` and published the
      11.3 MB MSI, `latest.json` and checksums here and to `nanu1605/vyro-releases`, whose
      `latest.json` now offers 1.0.1 to installed copies (checksums verified).

### 1.1.0 — the new look, a livelier My PC, a laptop model, desktop widgets
Built in order P28 → P31 on the same branch. Budgets (decision 28): installer < 60 MB; the
window's start bundle < 200 kB gzipped (the companion's stays < 32 kB); the views and the 3-D
scene stay loaded on first use. Footprint as 1.0, plus what P29 and P30 measure for themselves.

#### P28 — the frame and the design system
- [x] Built (2026-10-16):
      - **Navigation.** A sidebar (`components/shell/Sidebar.svelte`) with the views in four
        groups (Overview, Performance, This PC, Vyro), Lucide icons (`@lucide/svelte`, ISC, one
        file per icon) and a highlight that slides to the chosen view; it folds to a rail of
        icons (remembered) and folds by itself below 1100 px. Still the tabs pattern, vertical,
        with the same `tab-<id>` ids, so links, "last tab" and the accessibility sweep keep
        working (decision 29). The view list lives in `src/nav.ts`.
      - **Top bar.** The view's name and what it is for, the fan control, remote and update chips
        (they were in the header and on the Dashboard), the page-state line along its foot, and
        a search button.
      - **Command palette** (Ctrl+K, loaded on first use): views, settings and Health sections,
        theme and accent, every sensor (opens it in History) and "Return fans to BIOS".
      - **The whole width.** The 72rem cap is gone; on a window 1500 px or wider the Dashboard
        has a live rail (vitals with their last minute, the running game, network and ping, the
        top five processes, Health), which folds away (remembered). Card grids that had a fixed
        minimum shrink with the window (`minmax(min(100%, …), 1fr)`).
      - **Settings** has a section list beside the cards (it follows the scroll), Appearance
        first, and on/off settings as switches (`input.switch` with `role="switch"`).
      - **Look and motion.** Layered cards (an accent edge on top, a soft drop whose colour each
        theme sets, a lift on reading tiles and widgets), segmented controls, a common button
        feel, a view that rises in when chosen, widgets that come in one after another, dialogs
        and toasts that spring in, shimmering loading lines. Durations are tokens that the
        profile's animation level shortens ("calmer") or turns off, as does the system's
        reduced-motion setting; nothing loops while the Dashboard is still.
      - **Overlay colour** "App accent" (`overlay::Accent::App`): the app accent's shade for
        dark backgrounds, since the plate is always dark.
      - Accessibility sweep: also the palette (empty and with results) and the folded sidebar;
        386 checks clean. Start bundle 132.8 kB of 200; the companion 31.9 of 32 (its shared
        Svelte runtime is why the toasts use a CSS animation rather than a Svelte transition).

#### P29 — a 3-D PC that looks real, and a laptop
- [x] Built (2026-10-16):
      - **Laptop or tower.** SMBIOS type 3 (chassis type) → `BoardId.formFactor`, with a
        system battery as the fallback when the firmware says nothing; Settings → My PC → Model
        (Automatic, Desktop, Laptop) for firmware that says wrong. Old files and services read
        as unknown; the P19 fingerprint does not include it, so no "hardware changed".
      - **The laptop** (`pcmodel.buildLaptop`): the lid open at 110° with Vyro on its screen
        (CPU and GPU temperature, use, memory and a minute of CPU use, redrawn each reading), a
        keyboard deck you can see through ("See inside", remembered), the board with the CPU and
        GPU chips, copper heat pipes to two blower fans and their fins at the back vents,
        memory, drives and the battery. The thermal map, labels, inspector and 2-D plan (from
        above) work as for the tower; there is no separate GPU view.
      - **The tower** (`scene3d.ts`): lit by a room reflected in metal and glass (PMREM),
        ACES tone mapping and a soft bloom on the lights; a full case (panels, a front light
        strip, top vents, feet, PCIe brackets, a PSU shroud with a smoked window, tempered glass);
        a circuit-board texture with slots, I/O cover and power plugs; sleeved cables; a tower
        cooler with copper heat pipes and a lit cap, or an AIO with its hoses; curved, pitched
        fan blades in framed fans with light rings that blur into a disc at speed; graphics
        cards with lit cuts, heat pipes and gold contacts. Two front fans and a rear one are
        always there (decision 27). Everything procedural: no image or model files.
      - **Motion.** RGB drifts around the accent; air flows from the intakes across the board to
        the exhausts ("Airflow", speed from the fans, remembered); "Explode" lifts the parts
        apart; the camera flies in on first view and turns slowly after 8 s untouched; hover
        lights a part and names it; double-click flies to it ("Reset view" back). Warm parts
        glow in their heat colour.
      - **Cost.** 60 frames a second with the profile's full animation, 30 with "calmer" (no
        bloom, drift, fly-in or turning), none with animation off (drawn when a reading or the
        view changes). Only on My PC, only while it shows; the WebGL context is freed on leaving.
        Fin stacks and fan blades are single meshes. The lazy `scene3d` chunk is 162.8 kB
        gzipped (was 141.7).

#### P30 — desktop widgets
- [x] Built (2026-10-16):
      - **What they are.** Up to 8 small windows on the desktop, each showing live readings: a
        gauge (one reading on a dial), a reading (one, large), a graph (one with its last
        minute), a stack (up to six with bars) or a clock (time, date and two readings). Any
        reading the Dashboard has. Each has a size (50–200 %), a background strength, a style
        (the app's card, glass, or solid), a layer ("Behind windows", the default, or "On top")
        and a lock.
      - **The Widgets page** (sidebar, under Vyro; loaded on first use): a gallery of the five
        kinds with live previews over a bit of wallpaper, "Add", then each widget's readings,
        size, background, style, layer and lock, with its preview beside them; "Show on the
        desktop" and "Lock all" / "Unlock all to arrange". In the browser preview the widgets
        live on the page only.
      - **On the desktop** (`src-tauri/src/widgets.rs`): `widgets.json` (sanitized, atomic, as
        `dashboard.json`); one window per widget, `widget-<id>` loading `index.html#widget=<id>`,
        transparent, frameless, no taskbar button, shown without taking focus, behind or above
        other windows by its layer, sharing the main window's WebView2 process (same browser
        arguments). Unlocked, a widget is dragged by its body; its place is saved once it stops
        moving, and the page's older copy never overrides it. A new widget goes down the right
        edge of the first monitor; a saved place off every connected monitor comes back there.
        Right-click: Lock or Unlock, Edit… (opens the Widgets page), Remove. The windows follow
        the theme, accent and animation level, and come back when Vyro starts in the tray.
      - **Data.** Each snapshot also goes to the widget windows (`widget-sensors`) while any
        widget shows, and they keep the profile's polling rate with the main window in the tray
        (`Engine::set_widgets`, as the overlay does). Tray: "Desktop widgets" shows or hides them
        all. Their own capability (`capabilities/widgets.json`) allows only listening to events
        and dragging their own window.
      - **Known limit (lifted in 1.1.1).** Windows' Show desktop (Win+D) could hide widgets set
        behind windows until a window was switched to; "On top" kept a widget visible.
      - Accessibility sweep: the Widgets page in every theme at both widths, each kind's window
        and the widget menu; 411 checks clean. The page's chunk is 4.5 kB gzipped; the widget
        window shares the start bundle (139.1 kB of 200). Each widget is one more WebView2 view in
        the same browser process: its memory goes into `docs/footprint.md` at the field check.

#### P31 — polish and 1.1.0
- [x] Built (2026-10-16):
      - Version 1.1.0 (`scripts/set-version.py`).
      - Accessibility sweep at 1920 px as well as 1440 and 390, every view in all six themes,
        with the no-sideways-scroll check at each width: 543 checks clean.
      - README: the status, the new frame (sidebar, Ctrl+K, live rail, motion), the accent's
        reach, My PC's tower and laptop, a Desktop widgets section; "header" is now the top bar.
      - `docs/footprint.md`: the 1.1 rows and what to measure; `docs/field-check-1.1.md`: one
        run sheet for P28–P30 (about two hours), with the defaults to confirm.
- [x] Field check (`docs/field-check-1.1.md`), then the owner's go for the release: passed
      (owner, 2026-10-17, test build `vyro-windows-d48c153`) after the two rounds of fixes
      below; the defaults in the run sheet stand as built.
      - **First pass (owner, 2026-10-16, test build `vyro-windows-123a938`): four problems,
        fixed.**
        1. Widgets added on the Widgets page never appeared.
        2. Hardware and Sensors kept waiting for data.
        3. "Quit Vyro" in the tray did nothing.

        One cause for all three: `set_widgets` was a synchronous command, so it ran on the main
        thread inside WebView2's message callback. Its window work was run at once there
        (`run_on_main_thread` does that on the main thread), and building a WebView2 window
        inside that callback hangs the main thread on Windows. No answer came to any later
        command, and the tray could not quit. Fix: the widget commands are async, and the
        window work is always queued to the event loop from a thread of its own, as the
        overlay's is.

        4. Scrolling was laggy. Causes:
           - a fixed page background (every scrolled frame repainted the whole window; it
             doubled the repaints in a trace);
           - blurs behind the sticky top bar and sidebar;
           - card hover effects that animated borders and shadows as cards slid under the
             pointer;
           - live bars that animated their width, a layout every frame.

           Fix: the background scrolls with the page, there are no backdrop blurs (the bar is
           98 % opaque), hover lifts are transform-only, and the bars scale instead of resizing.
           In the browser preview's trace, repaints while scrolling fell from 77 to 35 on
           Settings and from 161 to 97 on the Dashboard; the rest is the readouts easing once a
           second.
      - **CI now runs the installed app** (`src-tauri/src/smoke.rs`, armed by `VYRO_SMOKE`):
        through the real IPC, the pages' data commands must answer, a widget saved as the
        Widgets page saves it must open and load, and quitting as the tray does must end the
        process. Any hang fails the run.
      - **Second report (owner, 2026-10-16, test build `vyro-windows-227e896`):** after a small
        window was made full screen, the sidebar's highlight sat a line below the chosen view.
        It was measured once, as the sidebar began to unfold. At that moment the column was
        still narrow and "THIS PC" wrapped onto two lines, so every view below it was measured
        12 px low. Nothing measured it again. Fix: group labels never wrap, and the highlight
        is measured again whenever the list changes size. The accessibility sweep now checks
        it after the sidebar's Expand and after widening a narrow window (549 checks). It
        also waits for the views' and the sidebar's animations to end before each axe run.
        In one CI run axe caught a label mid-fade over the moving highlight, below 4.5:1 for a
        few frames in Midnight; every state at rest passes.
- [x] **1.1.0 released** (2026-10-17): CI passed on d48c153 (Linux, Windows with the smoke
      test, accessibility), the commit the owner checked. `main` fast-forwarded to it, and
      Release run 39000976543 tagged `v1.1.0` and published the 11.4 MB MSI, `latest.json`
      and checksums here and to `nanu1605/vyro-releases`. Its `latest.json` now offers 1.1.0
      to installed copies (checksums verified). The 1.0 and 1.1 footprint figures and the
      anti-cheat table are still to be written down from the owner's notes.

### 1.1.1 — widgets stay in view through Show desktop
The owner's pick after 1.1.0 (2026-10-17): remove P30's known limit, where Show desktop (Win+D)
hid widgets set behind windows (decision 30).
- [x] Built (2026-10-17):
      - **How** (`src-tauri/src/pin.rs`, the way Rainmeter keeps "On desktop" skins):
        - Show desktop lifts the window holding the desktop icons (`SHELLDLL_DefView`'s
          parent: Progman on Windows 11 24H2, a WorkerW before) above every window that is not
          always-on-top.
        - Vyro keeps a hidden, disabled tool window, the probe, just above the icons' window.
          When the icons' window rises above the probe with the desktop in front, Show desktop
          is on.
        - Widgets behind windows then go on top. They go back under windows when an app comes
          forward (Vyro's own main window counts as one), or when the desktop drops back (Win+D
          again).
        - Clicking the desktop doesn't lift it, so widgets don't jump over open windows.
          Through Start, the taskbar, task switching or a click on a widget, the state holds.
      - **When it runs:** a foreground-window hook, set only while a widget sits behind
        windows, starts a check at once and six more 100 ms apart (the shell lifts the desktop
        a little after). While the desktop shows, it checks every 100 ms. Nothing polls
        otherwise.
      - **Nothing is put inside Explorer's windows.** A cross-process parent shares input
        between the two, so a busy Vyro could freeze the desktop.
      - **Layers:** `widgets::stacking` turns the layer and the state into always-on-top or
        always-on-bottom. One flag is cleared before the other is set, because tao keeps an
        always-on-bottom window at the bottom of every move.
      - **CI's smoke test** presses Show desktop through Explorer
        (`IShellDispatch4::ToggleDesktop`) and checks that the widget stays visible and on top.
        Then it brings Vyro's main window forward, as a click on an app in the taskbar would,
        and checks that the widget goes back. Each failure says what the check saw (desktop
        lifted, what is in front). On a runner with no desktop to show, it reports "skipped" in
        the step summary instead of failing.
        - The first runs found two faults. The widget vanished as it went on top (see "Also
          fixed" below). It also stayed on top after Show desktop was pressed again: the check
          still saw the desktop lifted, and Vyro's main window, which could have ended it,
          counted as a widget. The probe now sits just above the icons' window, not at the
          bottom of the stack.
      - **Also fixed (from 1.1.0):** switching a widget between "Behind windows" and "On top"
        hid it.
        - Cause: widgets were shown with a bare `ShowWindow`, which tao never hears about. tao
          hides a window it believes hidden whenever its flags change, as a layer switch does.
        - Fix: they are shown through tao (`show()`). They are built unfocused, so tao shows
          them with `SW_SHOWNOACTIVATE`.
        - Found by the first CI run of the Show desktop test: the desktop was detected, and the
          widget vanished as it was put on top.
        - The smoke test now also fails if a new widget takes focus.
      - **Words:** the Widgets page and the README describe the new behaviour. Version 1.1.1.
      - **Not covered:** desktop "peek" (hovering the taskbar's corner) only makes windows
        see-through, and widgets stay where they are. With a wallpaper app that moves the icons
        into its own window, the check may see the desktop as lifted when it isn't. Then
        clicking the desktop can bring widgets over windows until an app is clicked. The field
        check covers the owner's setup.
      - **CI** (run 39008538780 on 8ed55e9): every job is green. On the Windows runner's own
        desktop, the smoke test reports "Show desktop: passed": the widget stayed over the shown
        desktop and went back when the main window came forward. It opened without taking
        focus. The installer is 11.41 MB.
- [x] **Folded into 1.2.0** at the owner's request (2026-10-17): no release or field check of
      its own. Its checks are section 1 of [`docs/field-check-1.2.md`](docs/field-check-1.2.md).

### 1.2.0 — widgets, next round
The owner's pick while 1.1.1 waited for its field check (2026-10-17): widgets that are easier
to live with (decision 31). Built on the same branch after 5536a64. 1.1.1 is folded in, so the
two ship together as one release (the owner's choice).
- [x] Built (2026-10-17):
      - **Snapping** (`src-tauri/src/snap.rs`). A widget is dragged by Windows' move loop, which
        asks the window with `WM_MOVING` where each step goes. A window subclass on each widget
        answers with `snap`:
        - Each axis is handled on its own. The nearest pull within 10 px (logical, at the
          window's DPI) wins: the work area's edges (flush), a neighbour's side 8 px away, or
          a neighbour's edge in line.
        - A neighbour counts only when it is near on the other axis, so a widget across the
          screen never pulls. Shift held: no snapping.
        - The other widgets' rectangles are read once as the drag starts. Outside a drag
          nothing runs, and the place saved after the move is the snapped one.
        - `snap` is pure and unit-tested on every platform.
      - **Click-through.** "Clicks pass through" on the Widgets page (`clickThrough` in
        `widgets.json`, off by default and in older files).
        - While the widget is locked, its window ignores the mouse (tao's
          `set_ignore_cursor_events`, the same call the game overlay uses).
        - Unlocking it on the page, or "Unlock all to arrange", gives the mouse back; the
          switch is greyed while it is unlocked.
      - **Three new kinds:**
        - **Network:** the default route's adapter, download and upload with one graph of
          both.
        - **Game FPS:** the game's name, FPS, 1% low and frame time with an FPS graph; "No game
          running" otherwise.
        - **Top processes:** the five busiest programs by CPU, memory or GPU, one row per name
          with its processes added up (`topProcesses`, tested). The snapshot already lists the
          busiest 25 by each measure every reading, so no new data is read.
        - `sanitize` keeps no readings for the first two and one known ranking for the last.
      - **The Widgets page:** the gallery has eight kinds; the network and game widgets say what
        they show instead of a readings picker; top processes has "Busiest by". The intro
        mentions snapping and Shift.
      - **`windows` features** that `pin.rs` and `snap.rs` use are declared in
        `src-tauri/Cargo.toml` rather than borrowed from tao's and wry's.
      - **CI's smoke test**, after Show desktop:
        - **snap:** plays a drag's messages to the widget window. A step 4 px from the work
          area's corner must come back flush; one 40 px away must come back as it was.
        - **click-through:** locks the widget with clicks passing through, and checks that its
          window takes `WS_EX_TRANSPARENT`; then unlocks it and checks that the style goes.
        - Both are printed with Show desktop in the log and the step summary.
      - **Accessibility sweep:** one widget of each new kind (8, the limit), one of them locked
        with clicks passing through: 555 checks clean. The start bundle is 140.7 kB of 200.
      - Version 1.2.0. `docs/field-check-1.1.1.md` now names its exact test build, since later
        runs carry this work.
- [x] **CI** (run 39012201775 on 019d0a7): every job is green. On the Windows runner the smoke
      test reports "Show desktop: passed; snap: passed; click-through: passed". The installer is
      11.41 MB.
- [x] **Field check 1** (the owner's PC, 2026-10-17, test build `vyro-windows-019d0a7…`) found
      six problems, all fixed:
      1. **Widgets added together overlapped.**
         - Cause: a new widget went down the right edge only 40 px below the last, while
           widgets are 104 to 200 px tall.
         - Fix: `widgets::place` now takes the other widgets' places and puts a new one in the
           first free spot, 8 px under the last. A full column continues to the left. Saved
           places are never moved.
      2. **Snapped widgets stuck, alone or side by side.**
         - Cause (confirmed on Windows by CI's mouse drag, run 39022016985): Windows' move loop
           builds each step from the last answered rectangle plus the mouse's movement.
           Snapping what it proposed carried each snap into the next step, so a slow drag
           stayed within reach and was pulled back every time.
         - Fix: each step is worked out from the pointer, where it grabbed the window
           (`snap::follow`), then snapped. A widget comes away as soon as the pointer is out
           of reach.
         - Tested by modelling the move loop: the old way stays stuck, the new way comes free.
      3. **The 3-D view was small and cut off the top and bottom of the case.**
         - Cause: a 4:3 box capped at 70 % of the window's height gave a short, wide canvas.
           The camera only stepped back for narrow views.
         - Fix: the view fills the window below the bar. Home is fitted to the model's box at
           its explode state for any shape (`camfit.ts`, tested at four aspects). The camera
           follows a resize while it is still at home. Explode and "Reset view" frame again.
      4. **The inside of the case looked white.**
         - Cause: not the glass, as first thought. The case's steel was glossy (roughness 0.42)
           and the PSU shroud's window had a clearcoat. Seen from above or square-on, they
           mirrored the lighting room's ceiling light, which is far brighter than white.
           Screenshots at the owner's angles showed the haze.
         - Fix:
           - The case is matte powder-coat. The shroud's window has no clearcoat.
           - The glass is a faint pane in a dark frame, and is gone under the thermal map.
           - Under the map, parts reflect less of the room, so their colours read.
      5. **Only the top fans turned.**
         - Cause: the board's case-fan headers read 0 RPM (empty, or the fans run from a hub)
           and took the front and rear places as stopped fans. The browser preview's own
           "System Fan 1" did the same.
         - Fix: a header takes a place only once it has turned this session (`spunFans`, a
           set that only grows). Otherwise the generic fans turn slowly with "Speed not
           reported".
      6. **Airflow should look like a wind tunnel's smoke lines.**
         - New `airflow.ts` builds a flow field from the parts:
           - jets in at the front fans;
           - pulls out through the rear fan and the radiator (a radiator shares one pull, so
             about two lines in five leave by the rear fan);
           - a tower cooler's fan pushing through its fins;
           - the card, the AIO block, the RAM and the PSU shroud as obstacles that the air
             slides round.
         - Streamlines are traced from rings over each intake (RK2, 3 mm steps). Each point
           carries how much of the CPU's and the GPU's heat the air has taken up.
         - The scene draws them as camera-facing ribbons in one draw call: faint smoke with
           streaks running at the fans' pace, coloured cool to warm by that exposure times the
           live heat.
         - They are built once per layout; a frame only moves a uniform, which is cheaper than
           the 260 dots before. They show in the PC view and under the thermal map, and fade
           out while exploded.
         - A laptop gets parallel lines through each blower.
         - Tested: lines start at the intakes, never enter a part, mostly leave by an exhaust,
           and only warm.
      - **CI's snap check** had played made-up messages to the window, which could not show
        finding 2. It now drags the widget with the mouse:
        1. Put it on top of every window.
        2. Press it with `SendInput`.
        3. Move 2 px at a time to 5 px from the work area's corner: it must snap flush.
        4. Move on 1 px at a time: it must follow the pointer out.
        5. Release: it must stay put.

        The result says how Windows built the first step after the snap: on the last one
        answered, or from the pointer.
- [x] **CI** (run 39021013011 on f987a61): every job is green. On the Windows runner the smoke
      test reports "Show desktop: passed; snap: passed; click-through: passed": the real mouse
      drag snapped flush and came away with the pointer. The installer is 11.43 MB. That run's
      note on the move loop compared the last step, where both ways give the same place; it now
      compares the first step after the snap.
- [x] **CI** (run 39022016985 on b4748f3): every job is green. The smoke test reports "snap:
      passed (Windows built each step on the last one answered)". That confirms finding 2's
      cause, and that the fix comes away from a snap with a real mouse drag. The app is the
      same as f987a61's.
- [x] **The owner's findings on b4748f3** (2026-10-17, their PC at about 2000 px wide, with two
      GPUs: an RTX 5060 Ti and the Ryzen's AMD graphics):
      1. **The clock widget's readings ran past its edge.**
         - Cause: the two readings sat on one line with nowhere to break. With two GPUs the
           labels gain "(GPU 1)", so the line was about 290 px in a 260 px widget.
         - Fix: one reading a row, the name cut with "…" when long and the value on the right.
           The window is 100 px tall plus 20 a reading, in `widgets::size` and `widgetSize`
           alike.
         - The other kinds got the same care:
           - a network rate from 1000 Mbps shows in Gbps;
           - a dial's long number gets smaller, and its label is cut to fit;
           - a large reading stays on one line.
      2. **The Logical processors bars filled only the left half of their card.**
         - Cause: `auto-fill` kept the empty tracks, so 16 bars took the first 16 of about 30.
         - Fix: `balancedColumns` (`packing.ts`) lays them across the card in even rows, at most
           7rem each and centred. 16 threads on a wide card are one row of 16. The CPU sensors'
           per-core power is split the same way: 8 cores read 4 and 4, not 7 and 1.
      3. **Other cards wasted space.**
         - Cause: each Dashboard row was as tall as its tallest card, and every card stretched
           to it. Two GPUs made the GPU card about 860 px, so CPU sensors (180 px of
           readings), Memory and Power were mostly blank.
         - Fix: the grid's rows are a quarter rem. `pack` (`packing.ts`) gives each widget as
           many rows as it is tall, never fewer than its h asks for, and places it in order
           where it sits highest (`skyline`, tested), so the next card moves up into the room a
           short one leaves.
           - Tiles and gauges keep one height: one-line labels, and the sub line's room kept
             even when empty. A row of them stays in line.
           - The editor packs the same way.
         - The GPU card leaves out what a GPU doesn't report and names it once ("Not reported:
           hotspot, memory temp, …"): the AMD graphics' card drops eight empty lines. Two GPUs
           sit side by side on a wide card.
         - On a wide card, the label and value lists run two or three pairs a row.
         - On the owner's layout the page is 320 px shorter, with no blank card.
      - The browser preview now has the processor's graphics as a second GPU, as the owner's
        PC does. The accessibility sweep checks, on the owner's layout at four widths:
        - text stays inside every widget, including the longest names two GPUs give;
        - no two Dashboard widgets overlap;
        - no card has blank space under its content;
        - the thread bars span their card.

        Run against the old code, these checks report every finding above.
- [x] **CI** (run 39034665063 on 0760929): every job is green, the accessibility sweep with its
      new checks included. On the Windows runner the smoke test reports "Show desktop: passed;
      snap: passed (Windows built each step on the last one answered); click-through: passed".
      The installer is 11.43 MB.
- [x] **The owner's finding on 0760929** (2026-10-17): in narrow Dashboard cards the lists' values
      broke a letter or two a line ("69.3 / °C", "runn / ing", "5. / 4 / 6 / %").
      - Cause: the list (`KV.svelte`) is a two-column grid. Its label column got the longest
        label first, at least 7rem, and the value, which may break anywhere, got what was left:
        about 38 px beside "Effective clock" and 15 px beside "Average CPU (< 1.5%)". A card 3
        columns of 12 wide is about 195 px in a window about 920–1000 px wide. The checks above
        ran where cards were wider, and measured space, not words.
      - Fix: on the Dashboard each pair is its own row. The value sits on the right where it fits
        beside its label, else whole on the next line. The browser decides each row, so nothing
        is measured and changing readings follow.
        - Wide cards set pairs side by side, as many as fit at 13rem or more. This replaces
          finding 3's two or three a row, which left values far from their labels.
        - Everywhere else the lists are unchanged: the Hardware page compares pixel for pixel.
        - Vyro itself's budget limits keep together when a label wraps ("(< 1.5%)").
      - Checks:
        - every card kind 3 columns wide, at 920 and 1100 px;
        - on that layout and the owner's: text stays inside every card, and no word is split
          across lines unless it is wider than its card.

        Run against the old code, these checks report 57 split words.
- [x] **CI** (run 39040225531 on d7bbf18): every job is green, the accessibility sweep with the
      narrow layout included. On the Windows runner the smoke test reports "Show desktop:
      passed; snap: passed (Windows built each step on the last one answered); click-through:
      passed". The installer is 11.42 MB.
- [x] Field check 2 ([`docs/field-check-1.2.md`](docs/field-check-1.2.md)), then the owner's go
      for the release: passed (owner, 2026-10-17, test build `vyro-windows-d7bbf18`).
- [x] **1.2.0 released** (2026-10-17), with 1.1.1 in it: CI passed on 3442c7b (Linux, Windows
      with the smoke test, accessibility), d7bbf18's code with records only. `main`
      fast-forwarded to it, and Release run 39060120405 tagged `v1.2.0` and published the
      11.4 MB MSI, `latest.json` and checksums here and to `nanu1605/vyro-releases`. Its
      `latest.json` now offers 1.2.0 to installed copies (checksums verified).

### 1.3.0 — devices by name, their batteries and connections; an accent of any colour
The owner's requests after 1.2.0 (2026-10-17), with their choices: two releases (this one, then
the phone app in 1.4), Connect and Disconnect for all Bluetooth devices, and an accent that
auto-adjusts for text. Their devices: Bluetooth audio, a Logitech receiver, a game controller,
another maker's dongle, and a USB condenser microphone.
- [x] Built (2026-10-17):
      - **Names** (the owner: "Settings shows the names, Vyro says Unknown device").
        - Cause, shown by a new Windows test on CI's runner before the fix: 45 of 58 present
          devices lost their name. The device query was asked for unlocalized properties, so
          descriptions came back as unresolved indirect strings (type 25), friendly names came
          back empty, and only plain strings were read. Only 3 containers had a name.
        - Fix: localized queries; an unresolved indirect string gives its fallback text. More
          sources: display names, the name a device reports on its bus (a USB product string)
          and the paired Bluetooth entries Settings lists (two queries filtered by protocol and
          pairing, so nothing searches the network).
        - Order: the device's own name, its paired entry, its bus name, then the most specific
          function name (a Bluetooth device's own node before its services); Windows' generic
          function names are skipped. With nothing at all, the kind ("Mouse"), never "Unknown
          device". The chosen name also decides the kind.
        - The test (`names_match_device_manager`) now holds on every present device: the name
          matches what Configuration Manager gives.
      - **Batteries** (decision 16):
        - Logitech HID++ 2.0 through Unifying, Bolt and Lightspeed receivers, or a device
          connected directly. Each device paired to a receiver gets its own row ("Through USB
          Receiver") with its own name, kind and battery: Unified Battery, Battery Status, or
          Battery Voltage on Solaar's curve. One that answered earlier and sleeps now is listed
          as "Asleep or off". Requests carry Vyro's software id; replies are read from both of
          the receiver's HID++ collections, since its errors come back as short reports.
        - PlayStation controllers: DualSense and DualShock 4 report their battery in their input
          report over USB; over Bluetooth only once something else (Steam) has switched them to
          the full report. Vyro never switches it: that breaks some games' input.
        - The standard HID Battery Strength value, from a feature report.
        - Windows' own reading is also taken from the container and the paired entry.
        - XInput slots match their controller by vendor and product (XInput 1.4's capabilities
          by number, as SDL reads them), so two different controllers each get their battery.
        - Batteries say whether they are charging; a low battery on its charger is no warning.
        - `vyro-inventory` lists every HID collection by vendor and product: the line that names
          a dongle whose battery is still missing.
      - **Connect and Disconnect** (decision 33): a button on each paired Bluetooth device.
        - Audio (headphones, earbuds, speakers, also while off): the Bluetooth audio driver's
          one-shot reconnect and disconnect, reached from the device's audio endpoint, as
          Settings does; unelevated, in the app.
        - Any other device: Connect asks Windows to reach it (an uncached service lookup through
          the WinRT Bluetooth API pages it); Disconnect drops its link at the radio
          (`IOCTL_BTH_DISCONNECT_DEVICE`), done by the service. IPC protocol 14:
          `BluetoothDisconnect { id }` with the list's hashed id; the service looks the device up
          again, refuses anything but a paired Bluetooth device and takes one action per device
          every 5 s. No address crosses the pipe.
        - The tab says what became of it from the listings that follow: connected,
          disconnected, came back by itself, or didn't connect ("turn it on, or press a button").
      - **Accent of any colour** (decision 32): a Custom choice with a colour wheel in Settings →
        Appearance (hue and saturation on the wheel, brightness on a slider, a hex field, the
        last six colours; the thumb is a keyboard slider). Dragging previews on the page;
        letting go saves.
        - Fills (gauges, sparklines, desktop widgets' dials, the 3-D view, the logo, the share
          card's ring) show the colour as picked (`--accent-cpu-fill`, `--accent-gpu-fill`).
        - Text in the accent takes a shade of the same hue and chroma, lightened or darkened
          (OKLCH) just to 4.5:1 on every surface of the theme; the picker shows that shade when
          it differs, and warns near amber and red.
        - The GPU colour is the main one turned round the hue circle, away from warning hues; a
          grey keeps the theme's own.
        - Saved as `#rrggbb`; an older version reads it as an unknown name and uses the theme's
          own accent. The overlay's "App" colour takes the custom accent's Midnight shade.
      - **Release workflow**: its installer limit said 20 MB; decision 28 and CI allow 60 MB.
      - Version 1.3.0.
- [x] Checks:
      - Rust: name order, indirect strings, HID++ requests and replies, Unified Battery, Battery
        Status and Voltage (the curve), DualSense and DualShock 4 reports, receiver rows,
        Bluetooth addresses, Connect and Disconnect per device, the action throttle, IPC 14 (its
        wire shape, and a round trip: a malformed id and a second action in a row refused), the
        custom accent in the settings.
      - Windows CI: every device name matches Configuration Manager's; the HID collections are
        described and a battery read ends in time; the installed app lists devices and the
        service refuses to disconnect a device that does not exist (smoke test).
      - Front end: OKLCH and HSV round trips; for 24 hues × 4 chromas × 5 lightnesses (and pure
        red and blue, navy, black, white, pale yellow, grey) on all six themes, the accent text
        reaches 4.5:1 on every surface, text on its tints 7:1, and fills keep the colour picked.
        The accessibility sweep adds navy on Midnight and pale yellow on Daylight on four views,
        with the colour wheel open.
      - Budget: the phone companion's start bundle 32 → 34 kB (it shows the PC's accent, now any
        colour, so it carries the colour maths).
- [x] **CI** (run 39070415024 on 294c5f1): every job green. On the Windows runner every device
      name matches Configuration Manager's, the HID collections are read in time, and the smoke
      test lists devices in 92 ms and has the service refuse a device that does not exist in
      16 ms; Show desktop, snap and click-through pass. The accessibility sweep: 647 checks. The
      installer is 11.48 MB.
- [x] **Field check 1.3** ([`docs/field-check-1.3.md`](docs/field-check-1.3.md)) passed: the
      owner, 2026-10-18, on the 1.4 test build `vyro-windows-d185506` (the PC not reported).
      1.3 is not released on its own: it shipped in 1.4.0 (2026-10-18, decision 35).

### 1.4.0 — the phone app, from anywhere
The owner's request after 1.2.0 (2026-10-17): "a Vyro app for the phone, Android and iOS, to
check the PC from anywhere, whatever network it is on". Their choices: an installable web app (no
app stores), through a Vyro relay on Cloudflare Workers in their free account, end to end
encrypted (decision 34); built on its own branch, `claude/vyro-1.4`, while 1.3 waits for its
field check; the relay built into releases for everyone, any other relay allowed.
- [x] Built (2026-10-17):
      - **Relay** (`relay/`, its own package and lockfile so the app's jobs never install
        Wrangler):
        - A Worker with one SQLite-backed Durable Object per PC, on the WebSocket Hibernation
          API with no timers: between messages it may sleep and costs nothing; `ping` is
          answered without waking it.
        - The PC connects to `/pc/<room>` (128 random bits), a phone to
          `/phone/<room>/<phone>`. Keys go in `Sec-WebSocket-Protocol`; only their SHA-256 hashes
          are kept. The first PC claims its room; a phone gets in only with a key the PC
          registered. A refused socket is accepted and closed at once with 4403, which a browser
          can read (it cannot read an HTTP refusal); through the hibernation API, since a plain
          accept and close left workerd logging "Network connection lost".
        - The PC's text messages are the only ones the relay reads: the phone list, answered
          with `listed`. Signing a phone out, or a key change, closes its sockets at once
          (4403, 4001). Data frames are opaque: 29 bytes to 64 KiB, `[phone][sealed]` from the
          PC, delivered only to that phone, and back to the PC with the phone named.
        - Limits: 8 phones, 2 sockets each; the PC 100 messages a second (bursts of 200), a
          phone 5 (bursts of 20); a breach closes the socket. A room is forgotten 90 days after
          its PC was last seen.
        - It also serves the phone app, with a strict content policy (`default-src 'none'`,
          sockets only to itself, no frames, no referrer).
      - **Sealing**: each phone's secret gives, through HKDF-SHA256, a key the relay checks and an
        AES-256-GCM key it never has. Frames are bound to the room, the phone and the direction.
        The PC (RustCrypto) and the phone (WebCrypto) are checked against the same vectors,
        which were checked once against OpenSSL.
      - **Pairing**: the link `https://<relay>/#p=1.<room>.<phone>.<secret>` (the part after `#`
        never reaches a server) lasts 10 minutes. The phone's first hello carries a secret of its
        own, which replaces the link's, so the link works once; the phone keeps both until the
        PC confirms. The PC shows a link only once the relay has the phone's key (the end-to-end
        test found a phone that scanned at once refused).
      - **PC** (`vyro_core::remote::relay`): one thread with tungstenite over rustls and the
        Windows certificate store (rustls, ring and the platform verifier were already in the
        app through the updater); keepalive every 30 s, dead after 75 s, reconnects 1 s to 60 s
        with jitter, 5 minutes after the relay refused the PC's key. Views go only to phones
        with the app open. `remote.json` version 2 keeps the room, the PC's key and the phones,
        the secrets sealed with DPAPI; 1.3 reads it and ignores the relay. Phones watching
        count with the local ones for the reading rate, the tray and the header chip.
      - **Phone app** (`phone.html`, `src/phone/`, `pnpm build:phone` into `relay/public`, never
        in the installer): pairing from the QR code (on iPhone, add to the Home Screen first,
        since it keeps its own storage, then scan in the app: the browser's QR reader or jsQR,
        loaded only then; or paste the link); several PCs, with a list that shows each one's
        state; states for a PC asleep, a phone signed out, an expired code, a relay out of
        reach, an older PC; a service worker keeps the app's files for opening offline; a
        manifest and icons drawn from the app icon (`scripts/phone-icons.mjs`). The readings
        are `src/remote/LiveView.svelte`, now shared with the local companion.
      - **Settings → Remote**: From anywhere (a confirmation that says what the relay sees and
        cannot, the state, Pair a phone with a QR code, Copy link and a countdown, Turn off) and
        On this network (the companion as before); one list of devices, each marked "Anywhere"
        or "This network"; Options: the relay's address, Start over on the relay.
      - **Deploy**: `relay.yml`, by hand, on the owner's go, with `CLOUDFLARE_API_TOKEN` and
        `CLOUDFLARE_ACCOUNT_ID`: Run workflow (GitHub offers it only for a file on the default
        branch, so `relay.yml` was put on `main` on 2026-10-18, ahead of 1.4) or a pushed
        `relay-*` tag; builds carry the variable `VYRO_RELAY_URL`.
      - **Records**: decision 34; [`docs/remote.md`](docs/remote.md) (how it works, what the
        relay sees and does not, the trust points, limits, a relay of one's own); README.
- Defaults: off; a pairing link lasts 10 minutes and works once; a phone stays paired while it is
  used at least once in the chosen days (30 by default; the same choice as the companion); 8
  phones.
- Budget: the phone app's start 40 kB gzipped (31.6 kB as built). The relay on the free plan:
  100,000 requests a day for everyone (WebSocket messages count 20 to 1), and one phone
  watching is about 180 an hour.
- Known limits: networks that block `*.workers.dev` or need a proxy; no alert notifications to
  phones yet; the phone app's code comes from the relay (docs/remote.md, "Trust points").
- [x] Checks:
      - Rust: the vectors; addresses and messages; pairing, its link's single use, sliding
        expiry, at most 8, saving and loading with sealed secrets, a 1.3 file; one connection's
        conversation with test phones (rekeying, info, watch, views, sign-out, frames that do
        not open); the client against a stand-in relay that drops it (it reconnects) and that
        confirms the list.
      - Relay, in workerd through Wrangler's test harness: headers; the first PC claims its
        room, another key is refused, a reconnect replaces the old socket; only registered
        phones get in and hear whether the PC is there; frames reach only their phone;
        signing out and key changes close sockets; text, tiny frames and floods close them.
      - Phone app (vitest): the vectors through WebCrypto; links and the stored PCs; the
        connection with a fake relay (pairing and rekeying, a lost answer, an expired code,
        asleep, signed out, an older PC, an unreachable relay, signing out).
      - End to end (`scripts/relay-e2e.mjs`, also in CI): the relay in workerd, the companion
        example as the PC, the phone app in Chromium: pairs and reads live readings, the same
        link refused on a second phone, no frame between phone and relay readable, the PC list,
        signed out from the PC, a PC gone shows as asleep, axe on every screen at 390 and 1440
        px: 22 checks.
      - The accessibility sweep walks the relay's confirmation, a phone's pairing and the
        options in Settings.
- [x] **CI** (run 39077275048 on 089ce4b): every job green. The relay job tests the relay in
      workerd and runs the end to end (22 checks) with the phone app's screens kept; the
      accessibility sweep: 664 checks; the installed app's smoke test passes (Show desktop,
      snap, click-through). The installer is 11.61 MB (1.3: 11.48 MB; rustls, ring and the
      platform verifier were already in it). The build has no relay address until the
      variable is set.
- [x] **The relay deployed** (Relay run 39099314796, 2026-10-18, from this branch at e94322c,
      for the field check): `https://vyro-relay.tanishqjain010.workers.dev`. The owner made the Cloudflare account and the two secrets;
      `relay.yml` went onto `main` (4c48e46, the same file) so that GitHub offers Run workflow.
- [x] The owner set the repository variable `VYRO_RELAY_URL` to that address (2026-10-18), so
      CI and release builds carry it; CI's Windows job checks `vyro.exe` holds it. (Its first
      run, 39099591703 on 362c8ac, did not see the variable yet and said "none": the check
      caught it.)
- [x] **The field-check build**: CI run 39100838311 on d185506, every job green; its Windows
      job found the relay's address in `vyro.exe`. `vyro-windows-d185506`, installer 11.61 MB,
      installed-app smoke test passed.
- [x] **Field check 1.4** ([`docs/field-check-1.4.md`](docs/field-check-1.4.md)) passed: the
      owner, 2026-10-18, test build `vyro-windows-d185506` (the PC and phones not reported).
- [x] **Field check 1.3** passed on `vyro-windows-d185506` (the owner, 2026-10-18; 1.3 ships in
      1.4.0, decision 35), and the owner's go for 1.4.0 (2026-10-18).
- [x] **1.4.0 released** (2026-10-18), with 1.3 in it: CI passed on 44dae6d (Linux, Windows with
      the smoke test, the relay and phone app, accessibility), d185506's code with records, the
      Release check of the relay address and a test-only change to the accessibility sweep.
      `main` fast-forwarded to it. Relay run 39103627878 deployed the relay from `main` (the same
      address), and Release run 39103740656 tagged `v1.4.0`, found the relay's address in
      `vyro.exe` and published the 11.6 MB MSI, `latest.json` and checksums here and to
      `nanu1605/vyro-releases`. Its `latest.json` now offers 1.4.0 to installed copies
      (checksums verified).
- [x] **The accessibility sweep on My PC** (test-only): on `main` at 4c48e46 it failed once, on
      My PC in Midnight, with Hardware's label muted and the sidebar's highlight still under it;
      a re-run passed. Its log now says what was still moving when the wait ran out, and showed
      why: on CI, My PC can go seconds without drawing a frame, every animation frozen part way
      (on 44dae6d, the highlight 117 ms into its 160 ms slide after 5 s). Each axe run also waits
      for the highlight to sit on the selected view, waits up to 15 s (56e0ac5, was 5 s), and
      counts a contrast finding only if a second look a second later sees it too. CI run
      39107001793 on 2a0e408 logged My PC still frozen after 15 s (the view's fade-in at 133 of
      its 160 ms), and the sweep passed. If it fails there again, a longer wait is unlikely to
      help: look at what My PC's 3-D view does on CI's software GPU, or sweep most themes with
      the flat 2-D plan and the 3-D view once.
- [x] **The relay client test on `main`** (2026-10-18): CI at the release record (7d74ba5, run
      39105491819) failed it, though it had passed on the same commit on the branch. The client
      sent a phone its first view before counting it as watching, so the test could read 0. Fixed
      in 2a0e408: the phone's effects are applied before the answer is sent (a 50 ms pause made
      the old order fail every time and the new one pass). CI green on `main` (run 39107861636).
      It ships in the next release; the count only sets the reading rate, so 1.4.0's users are
      not affected.

### 1.4.1 — Connect and Disconnect that work, low batteries in sight

The owner, after 1.4.0 (2026-10-18): Connect and Disconnect did nothing on headphones, earbuds
and a speaker, and a device that was disconnected still showed Connected. Asked for at the same
time: a low battery on the Dashboard, with a dot on Devices. Built on `claude/vyro-1.4.1` from
`main` (912d6f5, which has the relay client fix above). The rest of that request is planned as
1.5 (control from the phone: actions, processes, shut down and wake), 1.6 (GPU tuning, NVIDIA
first) and 1.7 (the PC's screen on the phone), each with its own field check.

- [x] **Connected, as Settings shows it.** A paired Bluetooth device's connection came only
      from its device container, and a container without the property counted as connected:
      a speaker that was off showed Connected, offered only Disconnect, and pressing it changed
      nothing to see. Now, in order: the paired entry's `System.Devices.Aep.IsConnected` (what
      Settings shows), else its audio (connected while one of its endpoints is active), else
      the container; nothing said is **Paired**, offering both Connect and Disconnect
      (`devices::bt_state`, `DeviceInfo.state`). A controller XInput has a slot for is
      connected. The hot-plug watch also follows the paired entries, so a device turned off or
      disconnected in Windows changes within seconds.
- [x] **Every audio profile.** Connect and Disconnect for an audio device stopped at the first
      profile that took the request; a headset's other one (stereo or hands-free) stayed
      connected. Now each of its profiles is asked once (`devices::audio_connection`).
- [x] **The answer comes from Windows.** After asking, the app reads the device's connection
      every 500 ms until it shows the change (10 s for Disconnect, 15 s for Connect) and
      answers with what it saw (`devices::settle`); the row's message comes from that, not from
      guesses at later reads. Each click has its own message and timers, and only the newest
      list read is shown. Every step is logged (profiles, refusals, the radio's answer, the
      check), without device addresses.
- [x] **Low battery**, by P22's colours: under 20 %, or XInput's "low", is low; under
      10 %, or "empty", critical; never while charging (`devices::low`, `DeviceInfo.low`). The
      app now reads the device list in the background (`src-tauri/src/device_watch.rs`): 20 s
      after launch, after each hot-plug change and every 5 minutes, and the hot-plug watch
      starts at launch instead of with the Devices tab. A strip on the Dashboard ("Low battery:
      MX Master 3S 17%", and N more) opens Devices, which has a dot (amber, red when critical)
      while any battery is low. The preview shows both.
- [x] **CI green** on 72a2571 (run 39154275785): Linux, Windows with the installed-app smoke
      test, the relay and phone app, accessibility. Test build `vyro-windows-72a2571`. Locally:
      fmt, clippy (Linux and windows-gnu), 666 core tests, 206 frontend tests, the builds, the
      budgets and the accessibility sweep (665 checks).
- [x] **Settings → Remote no longer reads in a loop** (found in 1.7's field check,
      2026-10-19; in every version since P26). Reading the remote status announced a change,
      and Settings → Remote reads the status again on each change: while it showed, the two kept
      each other going, and each later change (Pair, a phone joining or watching) started one
      more round. Every read asks Windows for the network adapters, which goes through its DNS
      Client service: the owner saw that service take 23 % of the CPU on opening Vyro (it opened
      on Settings), 50 % while pairing and 85 % with the phone paired, and all of it stop when
      the window went to the tray. A read now only reads; the installed-app smoke test calls it
      and fails if it announces a change (and if saving a widget is not heard, so silence means
      something).
- [x] **The Devices tab stayed on "Reading devices…"** (found by the owner on 1.7's build
      3095d4f, 2026-10-19, at the start of this check; the dot on Devices showed, so the
      background reader had read the list). The tab kept only its newest read, and with the
      background reader 1.4.1 started reading on every change: a change during a read threw the
      read away, so changes that kept coming kept the list from ever showing. Separately, the
      Bluetooth battery read (and one HID read) waits on the device with no limit, under a lock
      every later list waits for. Which of the two held the owner's tab is not known (no log
      was taken), so both are fixed: the tab reads one at a time and shows every read
      (`devices.ts` → `oneAtATime`, with a test of changes that keep coming); after a change
      only the background reader reads, at most every 5 s, and the tab shows its list
      (`devices-read`); and each battery read runs on its own thread, the list waiting 5 s for
      it and going on with the last answer (`devices/bounded.rs`, tested with a read that
      hangs). The log now says when a list takes over 2 s, when a battery read is late and
      when changes keep coming for a minute.
- [x] **…and it stayed there while a Bluetooth device was connected** (the owner, 2026-10-19:
      with any Bluetooth device connected the tab stuck; disconnecting it in Windows Settings
      let the list show the wired devices). Windows reports a connected Bluetooth device again
      and again (its container and its paired entry) with nothing changed, and each report
      counted as a change that read the whole list again: on 3095d4f each of those reads threw
      the one before away. The watch queries now keep each device's connection and wake the
      reader only when it changes, or when a device arrives or leaves (`devices/watch.rs`,
      tested). Every part of the list (containers, nodes, paired Bluetooth devices, sound
      devices, XInput and both battery reads) is now read on a thread of its own, all at once:
      the list waits up to 4 s for Windows' parts and 5 s for the battery reads, goes on with a
      late part's last answer, and does not wait again for a part already late
      (`devices/bounded.rs`, tested with reads side by side). The log names any part that takes
      over 2 s or does not answer, and, when changes keep coming, where they come from. The
      owner confirmed it on 1.7's build bbc8120 (2026-10-19): the list shows with Bluetooth
      connected.
- [x] Field check 1.4.1 ([`docs/field-check-1.4.1.md`](docs/field-check-1.4.1.md)): done
      2026-10-19 on 1.7's test build bbc8120 (the owner's choice), every step works.
- [x] **1.4.1 released** (2026-10-19), on the owner's go: CI passed on ea18ffc (Linux, Windows
      with the smoke test, the relay and phone app, accessibility), bbc8120's 1.4.1 code with the
      field check's records. `main` fast-forwarded to it, and Release run 39328531150 tagged
      `v1.4.1`, found the relay's address in `vyro.exe` and published the 11.6 MB MSI,
      `latest.json` and checksums here and to `nanu1605/vyro-releases`. Its `latest.json` now
      offers 1.4.1 to installed copies (checksums verified). The relay was not deployed: 1.4.1's
      phone app is 1.4.0's (only a test changed).

### 1.5.0 — control from the phone: what runs, power, wake

The owner, 2026-10-18: "User can check and run anything that vyro supports remotely", shut the
PC or laptop down from the phone and wake it if possible, and see what runs on it (decision 36).
The PC's screen on the phone is 1.7. Built on `claude/vyro-1.5`, from 1.4.1 (its device states
show on the phone).

- [x] **Protocol 2** (`relay/proto.rs`, `session.rs`): `hello` says the phone's protocol; the
      PC's `info` gives a protocol 2 phone a session, whether it may control the PC, and the
      requests it may make. Every later message carries the session and a count that only goes
      up; the PC drops anything else, so the relay cannot replay a request. `req {id, m, p}` →
      `res {id, ok | err}`, a long answer in `part`s of at most 48 KiB. Unknown messages are
      ignored. Requests run on threads of their own in the app; answers go back through the
      connection's queue. A phone on 1.4's app still sees readings; the 1.5 app still shows a
      1.4 PC's readings.
- [x] **What a phone may ask** (`relay::api`): reading (processes, devices, Health, Insights,
      score, alerts, fans, profiles, benchmark and diagnostics results, PC changes, updates,
      power state) for any paired phone; changing (profile, fans to BIOS, alert rules on/off,
      benchmark start/stop, diagnostics, Bluetooth Connect/Disconnect, end a process, updates,
      shut down, restart, sleep, hibernate, lock, sign out, cancel, wake another PC) only for a
      phone allowed to control the PC. Answered in `src-tauri/src/remote_api.rs` with the same
      work the window's commands do.
- [x] **Permission and record**: Settings → Remote asks, when pairing, whether the phone may
      only look (the default) or also change the PC, and the paired list changes it; the phone
      hears at once. What phones changed (the last 50) shows under "Recent from your phones",
      in `phone-actions.json`.
- [x] **End task** (`vyro_core::actions::process`), in the Processes view too: Windows' own
      processes, Explorer and Vyro are never ended; the process is named again just before, so a
      reused number is left alone; one that runs as administrator says so.
- [x] **Power** (`actions::power`): shut down and restart wait 30 s (or now), close apps with
      unsaved work only if asked, and are full shutdowns; sleep, hibernate (when Windows allows
      them), lock, sign out, cancel. The PC shows a notification and a banner with Cancel on
      every view while a shutdown waits.
- [x] **Wake** (`actions::wake`): with "Wake from phone" on, phones are told the PC's wired
      address, sealed (never the LAN page); another Vyro PC on the same network sends the wake
      packet when the phone asks (from More, or "Wake <PC>" on a PC that is asleep). Settings
      shows the readiness Windows can tell: the wired adapter, its "Wake on Magic Packet", Fast
      Startup, and that the BIOS setting cannot be read.
- [x] **The phone app**: tabs Live, Processes, Devices, Health and More (power, wake, profile,
      fans, benchmark, diagnostics, updates); actions show only to a phone allowed to control
      the PC. Start bundle 33.5 kB gzipped (budget 40 kB); each tab is its own chunk.
- [x] **Tests**: protocol 2 (sessions, counts, replays, permission, unknown requests, parts,
      old phones), stored permission (1.4 files read as "only look"), the request table, the
      protected processes, power requests, wake packets and addresses; the phone's link
      (stamps, requests, parts, refusals, a 1.4 PC); the end-to-end test drives the new tabs,
      a phone that may only look, the owner allowing control, Lock and End task.
- [x] **CI green** on a0f48c6 (run 39156961292): Linux, Windows with the installed-app smoke
      test, the relay and phone app (end-to-end test included), accessibility. Test build
      `vyro-windows-a0f48c6`. Locally: fmt, clippy (Linux and windows-gnu), 681 core tests,
      209 frontend tests, 17 relay tests, the builds, the budgets, the phone app's end-to-end
      test (44 checks) and the accessibility sweep (666 checks).
- [x] **The phone waits its turn instead of being cut off** (found by 1.7's end-to-end test on
      CI, 2026-10-19). The relay closes the connection of a phone that sends more than 20
      messages at once, or more than 5 a second after that. Opening More asks the PC 8 things at
      once, and again when the owner changes what the phone may do, so tabs opened in quick
      succession could pass that: the phone lost its connection, what it had asked failed, and it
      reconnected a second later. The phone now sends a little under the relay's limit (16 at
      once, then 4 a second); later messages wait. A test replays its sends through the relay's
      own rule, and the end-to-end test switches tabs quickly and fails if the relay ever cuts
      the phone off.
- [x] **Long answers always fit the relay's frame** (found testing the relay before its
      release, 2026-10-19). An answer over 48 KiB goes in parts, each a JSON string, where every
      quote and backslash takes two bytes: an answer full of Windows paths or short strings
      (a diagnostics report, a device list) could grow a part past the relay's 64 KiB frame, and
      the relay then dropped the PC, again on every try. Parts are now cut by their written
      size. A test sends answers full of paths, quotes and control characters and checks every
      frame fits. The same testing found nothing else to fix in protocol 2 against the relay:
      the 1.7 phone app works with a 1.4.0, 1.5 and 1.6 PC, and a phone holding the 1.4 app
      gets the new one the first time it opens after the relay is deployed.
- [x] **A room outlives its PC by the full 90 days** (the same testing). The relay counted a
      room's 90 days from when its PC last connected, not from when it last left: a PC that
      stayed connected for more than 90 days could find its room forgotten, and every phone
      signed out, soon after it went away. The relay now notes the time whenever its PC leaves
      or is still there at a check. A test runs the relay with rooms that live 3 s and checks
      that a PC connected past the first check keeps its room a whole life after leaving. The
      relay's tests also gained the frame size limit either way, the PC's rate, and a PC that
      leaves and comes back.
- [x] **My PC: the case's fans show from outside** (1.5 field check, 2026-10-21: the case was
      solid where its fans are, so they showed only through the glass). The front is now a
      frame round perforated steel, the top is perforated over the radiator's place, and the
      back has a round grille at the rear fan; from a little way off the metal reads as
      see-through, as a mesh case does. A test checks that every case and radiator fan the
      model can place sits behind one of these openings.
- [x] **CPU use as this PC's Task Manager shows it; readings go out as soon as they are
      read** (1.5 field check, 2026-10-21: Vyro's numbers differed from Task Manager's, and the
      gauges lagged it). Windows 11 24H2's update of May 2025 moved Task Manager to the
      standard `% Processor Time` on every page; Vyro read `% Processor Utility`, what Task
      Manager showed before, which counts a processor boosting above its base clock as busier,
      so on a laptop Vyro read well above Task Manager. Vyro now reads the counter this
      build's Task Manager shows: processor time from build 26100, utility before it (a test
      covers the choice; the log names the counter). Each snapshot went out only after every
      provider had read, and the CPU was read first, so the sensor chips, the SMBus, the
      drives, the network and the GPU's libraries all came between the CPU's reading and the
      gauge: memory, CPU and processes are now read last, just before it goes out. A provider
      whose read takes over 250 ms is logged (at most every 5 minutes), to find what else
      holds a reading up. Memory, GPU, disk and network already use Task Manager's own sources
      and sums.
- [x] **What's new** (the owner, 2026-10-21): the first time Vyro starts after an update, the
      window shows what the new version brings, once ("What's new in Vyro 1.5", "Got it");
      Settings → Updates and the palette (Ctrl+K → What's new) open it again. Started in the
      tray, a notification says so. The notes are `src/whatsnew.ts`, one entry per version,
      newest first, written for the owner, those for the phone marked; a test fails when the
      version being built has none, so no release goes out without its notes. The version last
      shown is kept in `version.json` beside the settings, not in them (the window saves its
      settings whole and would write over it). Vyro before 1.5 kept no version, so an update
      from it is told from a new install by its saved settings (settings, dashboard, widgets,
      remote, alert rules) or a WebView or log folder made before this start; a new install
      shows nothing and keeps its version. An update over several versions shows each one's
      notes, the newest open. Tests cover the decision (Rust) and which notes show (TS).
- [x] **CI green** on 286753a (run 39653188894), merged into 1.6 (16d0e44, run 39653229359) and
      1.7 (cbbaddb, run 39654835985), every job each. Test build `vyro-windows-cbbaddb`. Locally:
      fmt, clippy (Linux and windows-gnu), 689 core tests, 214 frontend tests, the builds, the
      bundle budget, the case seen from front, top and back in the preview.
- [ ] Field check 1.5 ([`docs/field-check-1.5.md`](docs/field-check-1.5.md)); 1.4.1 released
      first; the owner's go. Then the relay is deployed (`relay.yml`), which serves the new
      phone app.

### 1.6.0 — GPU tuning: presets, your own settings, a stability test

The owner, 2026-10-18: overclocking with presets "based on the GPU installed" and custom
settings, tested on an NVIDIA desktop card and a laptop GPU (decision 37). Built on
`claude/vyro-1.6`, from 1.5 (a phone may switch presets).

- [x] **Core** (`vyro_core::gpu_tune`): settings are a core offset, a memory offset (MHz, as the
      driver counts them) and a power limit (% of the card's default). The driver's ranges decide
      what may change: offsets through NVML's per-P-state calls (driver R555 and later), the
      power limit through its constraints; a setting without a range stays stock. Families:
      Turing, Ampere, Ada, Blackwell, desktop and laptop. Suggested presets per family (Quiet:
      75 % power; Efficient: 85 % and a small core offset; Performance: the highest power limit
      and modest offsets), scaled to the ranges; a suggestion that would come out as stock is
      left out. The owner's presets (16), one preset per profile, validated in Rust.
- [x] **Controller** in the service: writes the preset in force and reads it back every 5 s.
      Stock is the driver's own, so nothing is captured first. Guards: 90 °C edge or 105 °C
      hotspot on two readings in a row → stock until a preset is applied again; values that
      change are written again, three times in two minutes → stock and "another program";
      a write the driver does not keep → stock. A marker (`gpu-tuned.json`) is kept while the
      card runs tuned; at start one left behind means stock first, and a Kernel-Power 41,
      Display 4101, nvlddmkm 153 or a LiveKernelEvent 141/117 report since → "Apply at start"
      off and a notice (a crash during a test leaves it as it was). Stock before sleep, on
      service stop and shutdown; the preset again after waking. Only settings that passed the
      stability test are applied at start. NVML now loads only from System32 or NVIDIA's own
      folder, for the readings too. IPC 15.
- [x] **Stability test** (`gpu_tune::stress`, app side `src-tauri/src/gpu_tune.rs`): rounds of
      FP32 load, the integer hash the CPU checks, the scene, a fixed frame whose hash must match
      the test's first, and a pattern over about 70 % of free video memory (up to 4 GB) checked
      on the GPU, which also measures its bandwidth. Any mistake or a driver reset fails the
      settings. The service holds them on trial; the app renews the trial every 10 s, so it
      ends within 30 s if the app stops. 5 minutes; settings that pass are kept as tested.
- [x] **Find my best** (`gpu_tune::search`): at the highest power limit, the core offset rises
      15 MHz a step (45 s tests) until one fails or +300 MHz, keeps 15 MHz below the last pass;
      then the memory offset 100 MHz a step while its bandwidth does not drop more than 1 %,
      keeps 100 MHz below; then the 5-minute test, one step lower each time it fails (twice at
      most). Saved as "Best for this card", tested. Refused while a game or a benchmark runs or
      Health warns; benchmarks wait for it.
- [x] **Window**: GPU tuning view (Performance group): the card and driver, what runs now, the
      presets with Apply, Test and Copy, Custom sliders within the driver's ranges, Find my best
      with progress and Stop, "Apply at start", a preset per profile, the safety rules and the
      log; a confirmation the first time. A "GPU tuned" chip with Stock in the top bar, "Return
      GPU to stock" in the tray, and the tray's tooltip. The preview simulates an RTX 5060 Ti.
- [x] **Phone** (1.5's control permission): More shows what the card runs and, for a phone
      allowed to change the PC, Stock and the tested presets (`gpu.preset`, `gpu.stock`); custom
      values and tests stay at the PC.
- [x] **Tests**: presets per family and their scaling, validation, tested settings, the
      temperature guard, crash verdicts, drift, the search (climb, slower memory, a result that
      fails the long test, nothing passes), the stability test with a stand-in GPU, the
      controller (apply and read back, return to stock, profiles, heat, another program, a
      failed write, a crash at start, apply at start only when tested, another card's presets,
      trials, sleep), crash events from the logs, IPC 15, the tray tooltip, the window's logic.
      On Windows CI, WARP renders the fixed frame the same twice and keeps the memory pattern.
- [x] **What's new in 1.6** (1.5's What's new, merged in 2026-10-21): GPU tuning, the
      stability test and Find my best, back to stock, and presets from the phone.
- [x] **CI green** on eca77e0 (run 39161727115): Linux, Windows with the installed-app smoke
      test (WARP renders the fixed frame and keeps the memory pattern), the relay and phone app
      with the end-to-end test, accessibility. Test build `vyro-windows-eca77e0`. Locally: fmt,
      clippy (Linux and windows-gnu), 729 core tests, 214 frontend tests, the builds, the
      budgets (the view is its own 6.7 kB chunk) and the accessibility sweep (696 checks).
- [ ] Field check 1.6 ([`docs/field-check-1.6.md`](docs/field-check-1.6.md)) on the owner's
      NVIDIA desktop card and a laptop, including whether NVML accepts the offsets on GeForce
      under Windows (if not, NVAPI is the next step); 1.4.1 and 1.5 released first; the owner's
      go.

### 1.7.0 — the PC's screen on the phone

The owner, 2026-10-18: "see what is running on the PC from the phone, and use the system
from the phone", directly with a slow fallback (decision 38). Built on `claude/vyro-1.7`, from
1.6, after a spike that showed each part working first.

- [x] **Spike**: str0m 0.24.1 with its `rust-crypto` backend (no OpenSSL; its DTLS certificate
      comes from `aws-lc-rs`, a C library that builds on CI's MSVC runner with no extra tools),
      on one UDP socket at the PC's main address with the public one STUN reports. A loopback
      test plays a phone: it connects, receives H.264 and sends input back, also when its offer
      hides its address behind an mDNS name, as browsers do. On Windows CI: Windows' own H.264
      encoder (constrained baseline, low latency, a keyframe when asked, SPS and PPS with every
      keyframe), Desktop Duplication, input as `SendInput`, JPEG through WIC.
- [x] **Permission**: Screen, per phone, apart from control and off until the owner allows it in
      Settings → Remote's paired list; the phone's `info` says so at once. `screen.monitors`,
      `screen.start`, `screen.stop`, `screen.slow` and `screen.input` need it (`relay::api`).
      Taking it back, signing the phone out, or turning the relay off ends its session.
- [x] **The PC's session** (`src-tauri/src/screen.rs`): one phone at a time. The phone's offer
      in, the answer out; the monitor scaled to 1280×720 at most, 30 frames a second, 2.5 Mbit/s,
      and again once a second when still; notes to the phone over the data channel (its size,
      "secure screen", why it ended). Input lands on the monitor shown. A banner with "Stop
      sharing" on every view, "Stop sharing the screen with …" in the tray, a notification at
      the start, the start and end in Settings → Remote's record. It ends after 10 minutes with
      no input and the phone not showing it (the phone says it is there every 30 s while it
      does); "Lock this PC when a phone's session ends" is an option. Nothing captures without a
      session. Media Foundation is looked up in System32 when a session starts, so Vyro still
      starts on a Windows "N" edition without the Media Feature Pack (only the direct view is
      missing there, and the phone uses the slow view).
- [x] **The slow view**: if the phone has no direct path in 15 s, or plays no H.264, it asks for
      one picture at a time through the relay: JPEG at most 960×600, at most 2 a second, only
      when the screen changed; input in batches. 20 minutes a day per PC, counted on the PC
      (`screen.json`). The relay counts what each PC sends its phones in a day (1 GB, far above a
      day of readings) and drops the rest, telling the phones once.
- [x] **Phone** (Screen tab, its own chunk): the PC's screen, tap to click, hold for a right
      click, drag, two fingers to scroll, pinch to zoom here, a pointer dot; a key bar with Ctrl,
      Alt, Shift and Win held until the next key (so held Ctrl and "c" is Ctrl+C), a box to type
      into, a choice of monitor; connecting, live, slow view, secure screen and ended states.
      Hidden for a PC whose Vyro has no screen; says how to allow it for a phone that may not.
- [x] **Tests**: STUN, input messages and their checks, positions across monitors, the frame
      helpers, the loopback session (also with a hidden address), the Screen permission (kept,
      off by default, separate from control, refused without it), the idle rule, the slow
      view's daily minutes, the notes; on Windows the encoder, capture and JPEG; the phone's
      gestures, typing and batching, and its "allowance used up"; the relay's daily allowance
      in its rules and in the running relay. End to end: the Screen tab asks for the permission,
      the slow view shows the example PC's picture and a tap reaches it as a click, Stop ends
      it; with Google Chrome (CI's runner), a direct session connects on the runner and a tap
      arrives as input. Accessibility: the Screen tab's states, and the banner. CI's
      installed-app test caught a hang at start (the screen status took one lock twice); a test
      now fails within seconds if it hangs.
- [x] **What's new in 1.7** (1.5's What's new, merged in 2026-10-21): the PC's screen on the
      phone and the slow view; the rest of 1.7's notes come with the owner's requests of
      2026-10-21 below.
- [x] **CI green** on e0e76b5 (run 39191304470): Linux, Windows with the installed-app smoke
      test and the screen tests on the runner's desktop (it captured 1024×768, encoded 26
      pictures with 2 keyframes, and made a 3.9 kB JPEG), the relay and phone app with the end
      to end test, accessibility. Test build `vyro-windows-e0e76b5`; its archive grew from
      29.2 MB (1.6) to 36.1 MB, as `vyro.exe` and the installer each carry the WebRTC code, and
      the installer stays inside the 60 MB budget. Locally: fmt, clippy (Linux and windows-gnu),
      747 core tests, 48 app tests, 228 frontend tests, 20 relay tests, the builds, the budgets
      (the Screen tab is its own 6 kB chunk; the phone app's start stays at 33.8 kB), the end to
      end test (56 checks) and the accessibility sweep (703 checks).
- [x] **The slow view stays on** (2026-10-19). After 1.4.1's fix was merged here, the end to end
      test failed twice on CI: just as the slow view started, the phone lost its connection and
      the session ended. The test now says, when that step fails, what the phone showed, how its
      connection closed and what it had sent before, and that found it here: the relay had cut
      the phone off for sending too fast (close 1008, "Too many messages"), as the tabs opened
      before it had asked for a lot at once. 1.5's fix, merged here, has the phone wait its turn.
      It covers the slow view as well: its pictures (2 a second) and a drag's input (up to 3 a
      second) are more than the relay takes (5 a second), so a long drag would have been cut off
      after about a minute.
- [x] **Testing the relay before its release** (2026-10-19). Its tests, a deploy dry run (the
      Worker 14 KiB, the phone app 428 KiB), edge cases against it run locally, the 1.7 phone
      app against a 1.4.0, 1.5, 1.6 and 1.7 PC (each shows what that PC offers; with 1.4.0, live
      readings, view only), and a phone holding the 1.4 app opening it after the relay is
      deployed (the new app the first time, the old files deleted, the new ones opening
      offline). Found and fixed: on 1.5, long answers that grew past the relay's frame and a
      room forgotten too soon (both merged here); here, a phone that connected after the day's
      allowance ran out was never told, and its requests waited for an answer that could not
      come: the relay now tells each phone once a day, the first time something for it is held
      back. Left for the field check: run locally, the relay's closes reach a browser about 2 s
      late (the local server's, not the relay's code); on the real relay, signing a phone out
      on the PC should show on the phone at once.
- [x] **The relay deployed from this branch** (603270c, 2026-10-19, at the owner's go; Actions
      run 39227795520), so phones get the 1.7 app for the field checks. It works with a 1.4.0
      PC (view only) and with 1.5 and 1.6, so the 1.5 and 1.6 field checks need no deploy of
      their own. Deployed again at 7207f8f (2026-10-19, at the owner's go; Actions run
      39243700043), so the phone app has the screen fixes below: "Reconnecting…", asking for a
      whole picture when it stops, and the "Receiving…" line.
- [x] **Field result: the CPU while pairing** (2026-10-19). On the test build of 603270c the
      owner saw Vyro and Windows' DNS Client stay low while pairing and with a phone paired
      (1.4.1's fix to Settings → Remote, merged here); an earlier 1.7 build had shown 60 %.
- [x] **The screen shows on the phone** (field check, 2026-10-19: on Android the PC's screen
      showed for a second, blurred, then went black; on the iPhone it stayed black). No test had
      ever played the PC's video: the example PC sent none, and the loopback test sent stand-in
      bytes. Played for real (str0m and an H.264 encoder into Google Chrome), the causes showed:
      - Windows' encoder held about four pictures before giving one out (CI printed 26 out for
        30 in): its low-latency setting was made after its formats, where it does not count.
        A still screen was fed one picture a second, so the keyframe the phone waited for came
        out four seconds late, and every change after it lagged as much. The setting now comes
        first (CI: none held back), and the picture goes on at full rate for a second after any
        change or keyframe, so any encoder gives out what it holds and refines a still picture,
        then once a second.
      - Right after connecting, the first pictures out were ones made before the phone was
        there, which build on pictures it never got: it could decode nothing (an iPhone stops
        there). Nothing is sent now until a keyframe, after connecting and after any picture is
        lost; a full queue no longer drops one silently but asks for a keyframe.
      - Every picture out of one call took that moment's time, so two could share one. Each
        now keeps its own picture's time.
      - Text was blurred: the monitor was sent at 1280×720, picking one pixel in three. A 1080p
        monitor now goes as it is (5 Mbit/s); larger ones are scaled by averaging the pixels.
      - A moment's loss of the connection ended the session at once; it now has 10 s to come
        back, and the phone says "Reconnecting…". A phone whose picture stops for 3 s asks for
        a whole one, and it shows what arrives ("Receiving 1920×1080, 30 pictures a second").
      Tests: the producer is no longer Windows-only, so tests drive it with a test screen and an
      encoder that holds four pictures back, as Windows' does: the phone's first picture is a
      keyframe (it was a P picture), and a still screen's keyframe arrives within a second (it
      took 3.3 s). The example PC now sends real H.264 through the same producer (OpenH264, a
      tests-only stand-in for Windows' encoder), and the end-to-end test checks the phone app
      decodes it, that the picture is not blank, and that a still screen keeps arriving. On
      CI's Windows runner, `scripts/screen-check.mjs` plays the app's own capture and Windows'
      encoder in the runner's Chrome: the first picture (a keyframe) 0.4 s after connecting,
      the screen as it was (not blank), a still screen once a second, a keyframe when asked.
- [x] **Still black on both phones**: cause found, HDR (below); fixed in test build c87dd2d and
      confirmed on the owner's PC, the screen shows on both phones (with the relay deployed from
      60eda87, run 39265388166) (field check, 2026-10-19, test build 7207f8f): Android and
      iPhone both showed "Receiving 1920×1080, 30 pictures a second" over a black picture, from
      a 2560×1440 main monitor (a second one at 3840×2160). The phones decode, so the video
      arrives; what it holds, or how the page draws it, is the question. Ruled out so far:
      - the scaling and Windows' encoder: CI's Windows runner now sends a moving 2560×1440 test
        screen through Windows' encoder, scaled to 1920×1080 as the app does, into Chrome: the
        picture decodes, not blank (`scripts/screen-check.mjs --test`); the same on Linux with
        OpenH264;
      - the H.264 itself: CI prints each picture's headers. Windows' encoder gives each picture
        out whole (a delimiter, SPS, PPS, two SEI, then four slices of 17 macroblock rows):
        constrained baseline, level 4.0, one reference picture, POC type 2, cropped to 1080
        lines. Nothing a phone's hardware decoder refuses, and the phones did decode it;
      - capture at 1080p: the runner's desktop is now set to the largest size it offers
        (1920×1080, `scripts/ci-display.ps1`) for the checks; it is captured and plays in Chrome
        (brightness 120, varying 63).
      Not testable on CI: a real GPU's capture (the runner has none) and phones' hardware
      decoding and drawing. So the next test build says where the picture goes black:
      - every 2 s the PC tells the phone what it sent: its screen's size and the size sent, new
        and sent pictures a second, Mbit/s, keyframes, **how bright the picture it took is**,
        and how many whole pictures the phone asked for; the app's log keeps the first few and
        one every half minute, and what Windows hands over for the screen (size, pixel format,
        rotation, video or system memory). A pixel format other than 8-bit BGRA ended the
        session with a reason rather than sending noise (which is how the cause was found);
      - the phone reads how bright the picture it decodes is, and under "Receiving…" (tap it)
        shows both; when the picture is black it says whether the PC's own picture was black
        (the trouble is on the PC) or the PC sent a picture that comes out black on the phone.
      And one likely cause is handled already: Desktop Duplication hands over pictures of
      nothing but zeros on some PCs (reported with NVIDIA on Windows 11), which the conversion
      turns into exactly this black (not the green of broken video). A picture that is all
      zeros on a grid across it, and stays so for a second, is now taken again through GDI, as
      Chrome falls back to it; if GDI sees the screen, the rest of the session goes through GDI and the log and the phone's
      details say "through GDI". CI's runner captures its desktop through GDI too.
      **Found** (field check on aacf437, the same day): the session ended at once with "the
      screen's pixel format (10) cannot be read". Format 10 is 16-bit float: the main monitor
      runs with HDR on, and Desktop Duplication hands over the desktop as Windows composes it,
      in linear scRGB (1.0 is 80 nits, SDR white at the display's SDR white level). Read as
      8-bit, as every build before did, those bytes went out as the black both phones showed.
      Now a compute shader on the duplication's device converts each picture to 8-bit sRGB
      (`screen/hdr.rs`): SDR white, from DisplayConfig's SDR white level (200 nits when Windows
      does not say), becomes 255, and brighter highlights are clipped; the PC's line reads
      "through Desktop Duplication (HDR)". A picture in any other format, or one the shader
      cannot convert, moves the session to GDI rather than ending it. CI converts a known HDR
      picture through the shader on WARP and checks each colour, the byte order and the
      clipping; a real HDR desktop is the field check's.
- [x] **The delay grew on mobile data** (field check, 2026-10-19, c87dd2d; on Wi-Fi a delay of
      milliseconds and 10 % CPU at most): the video went out at a fixed 5 Mbit/s whatever the
      connection carried, so a slower link queued it up and the picture fell behind. Now it
      follows the connection: the phone's browser reports how the video arrives
      (transport-wide congestion control), str0m estimates what the connection carries (Google
      Congestion Control) and paces the packets, and the producer encodes at 85 % of it
      (`screen/producer.rs` → `Adapt`): within a step the bit rate follows every half second
      (Windows' encoder takes a new rate without a keyframe), and below 2 Mbit/s fewer, smaller
      pictures that stay sharp (720p, then 720p at 15 a second below 1 Mbit/s, then 540p at 10
      below 500 kbit/s, never under 300 kbit/s); a smaller step at once, a larger one after 5 s.
      The phone's details add "of the N the connection carries". A test sends the screen
      through an 800 kbit/s link holding half a second of packets: packets wait about 270 ms
      at first and about 20 ms after 12 s, and the pictures keep coming; with the estimate off
      they wait about 490 ms and two pictures arrive in 16 s; on CI's Windows runner it passes
      too. On the same runner Windows' encoder, fed noise, sends less than half as much after
      its rate drops from 2.5 to 0.5 Mbit/s mid-stream, with no keyframe (CI run 39272496314,
      3095d4f). The relay serves the phone app with that line since the owner's go (deployed
      from 2e42a83, run 39298073589). The owner, 2026-10-19, on 3095d4f with that relay, on
      both phones over the direct path: "mobile data works now, lag stays low".
- [x] **The owner's requests of 2026-10-21**, for 1.7 (What's new went into 1.5): "a full UI and
      UX redesign to the mobile app … some animations … user friendly … 'Powered by Jainav
      Infotech Pvt Ltd' at the bottom … no horizontal scrolling", "no option to make the screen
      fullscreen", "when on mobile data … select HD or SD", and "not able to send audio notes from
      my phone … on WhatsApp web … nothing was recorded".
- [x] **The phone app redesigned**: an app bar with the PC's name (it opens the list of PCs and
      their states) and the connection; five tabs at the foot within the thumb's reach (Live,
      Processes, Screen, Health, More; Devices moved into More, which shows a dot for a low
      battery); confirmations in sheets that rise from below; toasts for results; cards that come
      in one after another, views that slide in the direction of travel, a tab marker that
      slides, buttons that give a little when pressed, a shimmer while a list loads; none of it
      with reduced motion. "Powered by Jainav Infotech Pvt Ltd" closes every screen, the
      pairing, home and offline ones too, and the Wi-Fi companion page. Nothing scrolls
      sideways: the end-to-end test checks every tab, sheet and state at 320, 360 and 430 px, in
      both themes. The phone app's start grew from 33.8 to 42.2 kB with the shell (every screen
      still loads when opened); its budget goes from 40 to 44 kB.
- [x] **Full screen**: the Screen tab's toolbar (Full, HD/SD, Keys, Paste, Clip, Mic, Stop).
      Full fills the phone with the PC's screen, turned sideways for a wide one where the
      browser allows it (Android); on iPhone the picture fills the app's window and the bars
      go. Over the picture the controls hide after 3 s; a corner button brings them back, as a
      tap on the picture is a click on the PC. Back or Esc leave it.
- [x] **HD or SD**: SD sends at most 960×540, 15 pictures a second and 1 Mbit/s (about 7 MB a
      minute at most, HD up to 35), the slow view 640×400 at a lower JPEG quality. On mobile
      data (Android says so; iPhone does not) the phone asks before it starts, and remembers
      the answer; a change to mobile data mid-session offers SD. The toolbar switches at once:
      the PC rebuilds its pace for the new ceiling (`{t: "quality"}` over the data channel).
      The phone's details say how much the session used.
- [x] **Voice clips** (decision 39): the Clip tool records on the phone (16 kHz mono, up to
      5 minutes direct, 1 minute through the relay) and sends it: over its own data channel, or
      through the relay a piece a request (`screen.clip`). The PC keeps it in `Phone clips` as
      AAC (Media Foundation's own encoder, WAV without one), puts it on the clipboard as a file,
      records it and says so; the phone offers Paste (Ctrl+V on the PC). CI on Windows encodes a
      tone, round-trips the clipboard, and pastes a clip into the runner's Chrome or Edge,
      which must receive it as a file of its name and size (`scripts/clip-check.mjs`).
- [x] **The phone as the PC's microphone** (decision 39): the Mic tool (direct connection
      only) sends 20 ms frames on a data channel that never resends; the PC plays them through a
      80 ms jitter buffer into the cable it finds, makes the cable's recording side the default
      microphone while it plays (Settings → Remote → Screen, on by default) and puts the
      defaults back after; it stops 1.5 s after the frames stop. Settings shows the cable found,
      or how to install VB-CABLE; the phone says what the PC hears it as, and without a cable
      offers a clip instead. The banner says when the phone's microphone is on.
- [x] **The relay's page policy**: it refused the microphone (`microphone=()`), so the phone
      could not record at all; it now allows the page itself, and the audio worklet is a file
      of the app's own (the policy runs no `blob:` or `data:` script). Phones get the new app
      once the relay is deployed from this branch, at the owner's go.
- Administrator prompts stay unseen in 1.7, by the owner's choice (field check, 2026-10-19:
  the phone shows its secure-screen line, as designed). Seeing and answering them would need
  the service to capture and type on the secure desktop as SYSTEM, behind its own permission;
  an idea for after 1.7, not planned.
- [ ] Field check 1.7 ([`docs/field-check-1.7.md`](docs/field-check-1.7.md)): Android Chrome
      and iPhone Safari, on Wi-Fi and mobile data; latency and the PC's CPU; the secure screen,
      an administrator window, two monitors, the slow view, the idle end; 1.4.1 to 1.6 released
      first; the owner's go. Done by the owner on 2026-10-19 (3095d4f, relay from 2e42a83), every
      check working except the slow view, not tried (both phones found a direct path; CI's
      end-to-end test covers it). Waits for 1.4.1 to 1.6 and the owner's go.

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

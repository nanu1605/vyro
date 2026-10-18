# Hardware Monitor — Project Plan & Build Specification (v2)

> **Purpose.** Complete, standalone build spec for a Windows hardware‑monitoring desktop app. Written to be handed to Claude Code (or any engineer) with no other context. Read top to bottom before starting Phase 0. Build strictly phase by phase; each phase lists **Acceptance Criteria** that must pass before moving on.
>
> **v2 change:** incorporates the full "additional information to show" list — network, gaming/FPS, system health, alerts, historical monitoring, sensor explorer, profiles, intelligent analysis, app self‑monitoring, and expanded per‑subsystem detail. New/uncertain items and one scope conflict (fan **control**) are called out in §2.

---

## 1. Product summary

A low‑footprint Windows desktop app that reports, in real time with a polished animated UI, the full state of the machine and its sensors:

- **Headline gauges** — CPU and GPU **utilization as animated speedometers**, each showing **current clock (MHz)** prominently.
- **CPU** — per‑core utilization/clock/temperature, effective clock, multiplier, BCLK, package/core/SoC power, package/core voltage, power‑limit / thermal‑throttling / current‑limit status, P‑core/E‑core identification, plus histories.
- **GPU** — usage and per‑engine breakdown (3D, video decode/encode, compute, copy, tensor/RT where supported), core/memory clocks, edge/hotspot/memory temps, VRAM used/total, power + power limit, voltage, fan %, PCIe link speed/width, throttle/power‑limit status.
- **Memory** — total/used/available/cached/committed, commit limit, page‑file, compression, page faults, channels, frequency, timings (CAS/tRCD/tRP/tRAS/command rate), per‑DIMM manufacturer/model/capacity/temperature.
- **Storage** — health %, controller/NAND temps, capacity used/free, live read/write speed + IOPS, totals read/written, power‑on hours, power‑cycle count, SMART attributes, NVMe % used / available spare / critical‑warning, per‑drive activity.
- **Motherboard** — chipset/VRM/VRM‑MOS/PCH/socket/PCIe/M.2 temperatures, per‑sensor and per‑fan‑header identification.
- **Fans/cooling** — per‑fan RPM/%/PWM, min/max RPM, header names, pump RPM/%, status, fan‑curve **visualization** (control is deferred — see §2).
- **Displays** — per monitor: manufacturer/model/serial, connection type, color depth/space, HDR status + HDR/SDR brightness, Adaptive‑Sync/G‑SYNC/FreeSync/VRR + range + current state, DPI/scaling, orientation, physical size, EDID, primary/secondary, and a **visual multi‑monitor arrangement**.
- **Network** — adapter, up/down speed, utilization, link speed, ping/latency, packet loss, Wi‑Fi signal/channel, history.
- **Gaming/performance** — FPS, 1%/0.1% lows, frame time, CPU/GPU frame time, present latency, per‑process CPU/RAM/GPU, and a gaming overlay (see §2 for feasibility).
- **Power** — estimated total system power and per‑component contributions, with history.
- **Windows/system** — edition/version/build/install date, uptime, last reboot/boot time, Secure Boot, TPM, virtualization, BitLocker, startup apps/impact, sleep/wake history, running processes.
- **Historical monitoring** — selectable ranges (5 min / 30 min / 1 h / 6 h / 24 h / 7 days) for all key metrics.
- **System health** — per‑subsystem status, warnings, anomalies, thermal/throttle events, detectable hardware errors.
- **Alerts** — user‑configurable thresholds → Windows / tray / optional sound notifications.
- **Sensor explorer** — every sensor the app's own engine discovers, grouped, with value/unit/min/max/avg/source/availability.
- **Profiles** — Desktop/Gaming/Work/Benchmark/Silent presets controlling displayed sensors, poll rate, graphs, overlay, alert rules, animation intensity.
- **Intelligent analysis** (optional) — baseline anomaly detection and plain‑language explanations of unusual readings.
- **App self‑monitoring** — the app's own CPU/GPU/RAM, poll rate, UI render rate, active‑sensor count, engine status (so users can verify it stays lightweight).

**Design goals:** very low CPU/GPU/RAM footprint; user‑friendly; smooth "fancy" animations; **direct sensor access** — no dependency on any third‑party monitoring app or its library (LibreHardwareMonitor/HWiNFO/Open Hardware Monitor).

**Scope:** Windows 10 (21H2+) and Windows 11, **x64 only**. ARM64 out of scope for v1.

---

## 2. Critical constraints, risks & feasibility notes — read first

**Foundational constraints (unchanged from v1):**

1. **Core sensor data requires a kernel driver.** CPU core temps, mobo temps, fan RPM, voltages, RAM (DIMM) temps and SPD timings are only reachable via `RDMSR` and raw I/O‑port / SMBus access, which are ring‑0. A small kernel driver must do these reads over IOCTL. No user‑mode Windows API exposes them.
2. **Distributing a driver on current Windows is a signing project.** WinRing0 is blocklisted (CVE‑2020‑14979) — do **not** use it or any fork. Since the **April 2026 kernel‑trust change**, Win11 24H2/25H2/26H1 load only Microsoft‑signed drivers; we need an **EV certificate + Partner Center + attestation signing** (Phase 11). Development uses **test‑signing** on dev machines only.
3. **Write our own hardened, read‑only driver** exposing only **whitelisted** MSR indices and bounded I/O‑port/SMBus ranges — never arbitrary read/write (that is WinRing0's CVE).
4. **Super‑I/O / mobo‑sensor / RAM‑temp coverage is chip‑ and board‑specific.** Build per‑chip from datasheets (initial targets: one Nuvoton NCT67xx + one ITE IT87xx). Unknown chips → report "unsupported," never guess.
5. **AMD CPU sensor access is less documented and family‑dependent** (SMN / Tctl/Tdie; SVI telemetry via SMU). Budget more time than Intel.
6. **Admin rights required** for driver IOCTLs and storage SMART. v1 runs elevated; Phase 8 splits privilege.
7. **Vendor GPU SDKs are permitted** (NVML/NVAPI, ADLX/ADL, IGCL/Level‑Zero) — official hardware APIs, not third‑party apps. **LibreHardwareMonitorLib and its driver are not** — never bundle/link/ship them. Public datasheets may be used.

**New feasibility notes from the additional‑features list — act on these:**

8. **Fan/cooling CONTROL is a scope conflict and a hardware‑safety risk.** "Configurable fan curves," "cooling‑profile support," and pump control require the driver to **write** to the EC/Super‑I/O — the exact write surface constraint 3 forbids, and mis‑programming can overheat or damage hardware. **v1 is read‑only:** implement fan/pump **monitoring** and fan‑curve **visualization** only. Treat control as a **deferred, opt‑in v2 module** (see §11 "Deferred"), gated behind explicit warnings and extensive per‑board validation. Confirm before building any write path.
9. **Gaming FPS/frame metrics need PresentMon/ETW; the in‑game overlay is high‑risk.** FPS, 1%/0.1% lows, frame time, CPU/GPU frame time and present latency come from **PresentMon (ETW present events)** — not a simple API. An always‑on‑top **companion** overlay window is straightforward; a true **in‑game injected** overlay (drawing inside the game's DX/Vulkan present) is complex and can trip **anti‑cheat** and get users banned. Plan the companion overlay in v1; treat injected in‑game overlay as optional/v2 with a clear warning. This whole area is isolated in Phase 10 and can be deferred.
10. **Some requested metrics are best‑effort / often unavailable.** Do not overpromise; show "unavailable" gracefully:
    - **GPU tensor / ray‑tracing utilization** — generally not exposed by NVML/ADLX; usually unavailable.
    - **GPU hotspot/memory temperature and voltage (NVIDIA)** — via NVAPI (not NVML), and not on all cards.
    - **BCLK and memory channel count** — derived/decoded; treat as approximate.
    - **Memory timings (CAS/tRCD/tRP/tRAS/command rate)** — require reading SPD over SMBus (driver, Phase 6); SMBIOS alone gives speed but not full timings.
    - **Startup impact** — Task Manager's metric is not a public API; approximate with a heuristic or omit the number.
    - **Packet loss / ping** — require **active ICMP probing** to a configurable host; packet loss is sampled, not exact.
    - **RAM / storage power contribution** — rarely measured directly; estimate (Intel DRAM RAPL exists on some platforms) or omit.
    - **VRM / PCH / socket / PCIe temps** — only present if the board wires them to the Super‑I/O; coverage varies by board. **M.2 temperature** is usually the NVMe drive's own sensor (from storage), not a board sensor.
    - **Board serial number** — privacy‑sensitive: display **locally only**, never transmit or persist off‑device.
11. **Historical ranges need a lightweight embedded time‑series store.** 7‑day history across many metrics must persist to disk (survive restarts) without bloating RAM. Use **SQLite** (`rusqlite`), downsampled by range (raw for 5 min, progressively aggregated for longer ranges), with a size cap and retention policy. Keep only rolling in‑memory buffers for the live graphs.

---

## 3. Technology stack (decided)

| Layer | Choice | Why |
|---|---|---|
| App shell | **Tauri 2.x** (Rust core + system WebView2 UI) | Few‑MB binary, low idle RAM; web frontend is the best canvas for the speedometers and graphs. |
| Backend / sensors | **Rust** (stable) via the `windows` crate | Safe, near‑zero overhead for IOCTL/WMI/PDH/ETW and vendor‑SDK FFI. |
| Kernel driver | **C, KMDF** (WDK) | Required for MSR + port I/O + SMBus; attestation‑signed for release. |
| Frontend | **TypeScript + Vite**, **Svelte** (or vanilla TS) — no React | Minimal runtime; reactivity without heavy framework. Gauges/graphs hand‑rolled on `<canvas>` — **no charting library**. |
| History store | **SQLite** via `rusqlite` | Durable 5 min–7 day ranges with downsampling and a size cap. |
| FPS / present metrics | **PresentMon** (ETW consumer) | Standard source for FPS/frame time/present latency. Phase 10. |
| Frame/process/event data | **ETW** consumer (Rust) + PDH + `NtQuerySystemInformation` | Per‑process usage, GPU engine util, sleep/wake events. |
| Notifications | Windows Toast + tray + optional sound | Alert delivery. |
| Elevated helper (Phase 8) | **Rust Windows service** | Privileged reads; UI runs unelevated over a named pipe. |
| Installer | **WiX (MSI)** or **NSIS** | Installs service + signed driver; Tauri updater. |

**Rejected:** WinUI 3 (deployment + runtime weight), Electron (RAM), C++/Qt (footprint, LGPL, error‑prone).

**Reference prototype:** the animated dual‑speedometer UI already exists (HTML/Canvas) and demonstrates the poll‑slow / interpolate‑fast pattern. Port it into `src/gauge.ts` in Phase 0.

---

## 4. Architecture

### 4.1 Sensor engine & event/command topology

Build a **unified sensor registry** in the Rust core: every metric is a `Sensor { id, group, name, unit, value, min, max, avg, source, available }`. Each provider (cpu/gpu/memory/storage/display/network/driver) registers its sensors; the poller updates them; min/max/avg are tracked centrally. This single registry powers the **Sensor Explorer**, **history**, **health**, and **alerts** — build it once, in Phase 0.

Do **not** cram everything into one payload. Split by cadence:

- **`sensors` event (≈1 Hz):** fast‑changing telemetry — CPU/GPU/mem/storage/network live values, temps, clocks, fans, voltages, power, top‑N per‑process. Kept lean for performance.
- **`fps` event (only while a game runs, Phase 10):** FPS/frame‑time/latency.
- **Commands (on demand / once):** `get_system_info`, `get_hardware_id`, `get_displays`, `get_sensor_tree` (explorer, with min/max/avg), `query_history(metric, range)`, `get_health`, alert CRUD, profile CRUD, `get_app_metrics` (self‑monitoring).
- **`alert` event:** fires when a rule trips (also raises a Windows/tray/sound notification).

### 4.2 Poll strategy (performance is a feature)

- Poll ≈1 s (usage counters 500–1000 ms). Static inventory read **once** at startup and cached — **no WMI in the poll loop**.
- Poll on a background thread; never block UI.
- Throttle when hidden/tray (e.g., 5 s) and pause the frontend rAF loop when the document is hidden.
- One serialized snapshot per tick; write metrics into the registry and the history store each tick (history writes batched/downsampled).

### 4.3 Animation strategy

Decouple data rate (1 Hz) from frame rate (60 fps): snapshots set **target** values; a rAF loop eases `cur += (target-cur)*(1-e^(-dt/0.22))`. Compositor does the work. Respect `prefers-reduced-motion`; pause rAF when hidden. **Animation intensity** is a per‑profile setting (Phase 9).

### 4.4 Data contracts

Keep the Rust structs and TS types in lockstep (single source of truth). Fields are `null`/omitted when unavailable; the UI renders gracefully around gaps.

**Fast telemetry — `SensorSnapshot` (`sensors` event):**

```jsonc
{
  "ts": 1758800000,
  "cpu": {
    "usageTotal": 23.4, "usagePerCore": [/* % */],
    "mhzCurrent": 4821, "mhzPerCore": [/* MHz */], "mhzEffective": 4780,
    "multiplier": 48.0, "bclkMhz": 100.2,           // bclk approximate
    "mhzBase": 4500, "mhzBoost": 5700,
    "tempPackage": 62.0, "tempPerCore": [/* °C */],
    "powerPkgW": 88.5, "powerCoreW": 71.0, "powerSocW": 12.0,   // RAPL/SMU (driver)
    "voltagePkg": 1.28, "voltageCore": 1.27,        // driver
    "throttleThermal": false, "throttlePowerLimit": false, "throttleCurrentLimit": false
  },
  "gpu": [
    {
      "name": "NVIDIA GeForce RTX 4070",
      "usage": 41.0,
      "engine": { "graphics3d": 41.0, "decode": 0.0, "encode": 3.0,
                  "compute": 0.0, "copy": 2.0, "tensor": null, "raytrace": null },
      "mhzCore": 2610, "mhzMem": 10501,
      "tempEdge": 58.0, "tempHotspot": 71.0, "tempMem": 64.0,   // hotspot/mem: NVAPI, may be null
      "vramUsedMB": 4820, "vramTotalMB": 12288,
      "powerW": 140.0, "powerLimitW": 200.0, "voltageV": 0.95, "fanPct": 38,
      "pcieGen": 4, "pcieWidth": 16,
      "throttleThermal": false, "throttlePowerLimit": false
    }
  ],
  "memory": {
    "totalMB": 32768, "usedMB": 12040, "availableMB": 20728, "cachedMB": 8100,
    "committedMB": 15000, "commitLimitMB": 40000,
    "pageFileUsedMB": 900, "compressedMB": 420, "pageFaultsPerSec": 1200,
    "channels": 2,                                  // approximate
    "dimms": [ { "slot": "DIMM_A2", "manufacturer": "Kingston", "model": "KF560C40-16",
                 "sizeMB": 16384, "speedMHz": 6000, "tempC": 41.0 } ]
    // timings (cas/trcd/trp/tras/commandRate) delivered via get_hardware_id (SPD, driver)
  },
  "storage": [
    { "model": "Samsung SSD 990 PRO 1TB", "type": "NVMe",
      "tempC": 44.0, "tempControllerC": 46.0, "tempNandC": 52.0,   // nand where available
      "healthPct": 99, "capacityMB": 1000204, "usedMB": 420000, "freeMB": 580000,
      "readMBs": 12.5, "writeMBs": 3.2, "readIops": 320, "writeIops": 90, "activityPct": 4,
      "totalReadGB": 18200, "totalWriteGB": 9100,
      "powerOnHours": 1180, "powerCycles": 420,
      "nvmePctUsed": 1, "nvmeAvailableSparePct": 100, "nvmeCriticalWarning": 0 }
  ],
  "board": {
    "tempChipset": 44.0, "tempVrm": 51.0, "tempVrmMos": 55.0, "tempPch": 48.0,
    "tempSocket": 40.0, "tempPcie": null, "tempM2": 52.0          // many null unless wired
  },
  "fans": [ { "name": "CPU_FAN", "rpm": 780, "pct": 35, "pwm": 90,
              "minRpm": 300, "maxRpm": 2000, "kind": "fan|pump", "status": "ok" } ],
  "voltages": [ { "name": "+12V", "value": 12.06 }, { "name": "Vcore", "value": 1.28 } ],
  "network": [ { "adapter": "Wi‑Fi", "type": "wifi",
                 "downMbps": 42.1, "upMbps": 3.4, "utilizationPct": 5, "linkMbps": 1200,
                 "pingMs": 12, "packetLossPct": 0.0, "wifiSignalPct": 78, "wifiChannel": 44 } ],
  "power": { "estTotalW": 245.0, "cpuW": 88.5, "gpuW": 140.0, "ramW": null, "storageW": null },
  "processes": [ { "pid": 1234, "name": "chrome.exe", "cpuPct": 6.2, "ramMB": 810, "gpuPct": 3.1 } ],
  "app": { "cpuPct": 0.6, "ramMB": 74, "gpuPct": 0.0,
           "pollHz": 1.0, "uiFps": 60, "activeSensors": 142, "engine": "running" }
}
```

**Aux structs (served by commands, not in the 1 Hz payload):**
- `SystemInfo` — Windows edition/version/build/install date, uptime, last reboot/boot time, Secure Boot, TPM, virtualization, BitLocker, startup apps (+impact heuristic), sleep/wake history, process overview.
- `HardwareId` — CPU: arch/socket/node/cache sizes/ISA/core topology + P/E core map; GPU: model/vendor/VRAM type/BIOS version/driver version/PCIe info; board: manufacturer/model/BIOS version+date/chipset/serial (local only). Memory: timings (CAS/tRCD/tRP/tRAS/command rate), channels.
- `DisplayInfo[]` — per monitor: manufacturer/model/serial, connection type, color depth/space, HDR status + HDR/SDR brightness, Adaptive‑Sync/G‑SYNC/FreeSync/VRR + range + current state, DPI/scaling, orientation, physical size, EDID, primary/secondary, layout rect.
- `SensorTree` — grouped sensors with value/unit/min/max/avg/source/availability.
- `HistorySeries` — `query_history(metric, range)` → downsampled points.
- `HealthReport`, `Alert`/`AlertRule`, `Profile`, `AppMetrics`.

Frontend wiring (replaces the prototype simulator; gauge/easing code unchanged):

```ts
import { listen } from '@tauri-apps/api/event';
listen<SensorSnapshot>('sensors', ({ payload }) => applyTargets(payload));
```

---

## 5. Data acquisition reference (grouped)

Raw acquisition only. Derived/UI features (histories, health, explorer, analysis, profiles, graphs, overlay) are computed from the registry + history store and are covered in the roadmap, not here.

### CPU
| Data | Source | Driver? | Admin? |
|---|---|:--:|:--:|
| Model, arch, socket, cache, ISA, topology | `CPUID` + SMBIOS (once) | No | No |
| P‑core/E‑core type (hybrid) | `CPUID` leaf 0x1A per core | No | No |
| Utilization total + per‑core | PDH `% Processor Utility` / `NtQuerySystemInformation` | No | No |
| Clock per‑core (interim) | `CallNtPowerInformation(ProcessorInformation)` | No | No |
| **Effective clock, multiplier, BCLK** | `APERF`/`MPERF` (0xE8/0xE7); ratio from `MSR_PERF_STATUS` (0x198); BCLK derived | **Yes** | Yes |
| **Core/package temp** | Intel MSR 0x19C/0x1B1 + TjMax 0x1A2; AMD SMN Tctl/Tdie | **Yes** | Yes |
| **Package/core/SoC power** | Intel RAPL 0x611/0x639/DRAM 0x619 (+unit 0x606); AMD core/pkg energy MSRs / SMU | **Yes** | Yes |
| **Package/core voltage** | Intel VID (perf status); AMD SVI telemetry via SMU | **Yes** | Yes |
| **Throttle / power‑limit / current‑limit status** | IA32_(PACKAGE_)THERM_STATUS log bits; `MSR_CORE_PERF_LIMIT_REASONS` (0x64F) | **Yes** | Yes |

### GPU (vendor SDK: NVML/NVAPI, ADLX/ADL, IGCL/Level‑Zero)
| Data | Source | Notes |
|---|---|---|
| Usage; per‑engine (3D/decode/encode/compute/copy) | PDH `\GPU Engine(*)` per engine type; or SDK | copy/compute via engine type strings |
| Tensor / ray‑trace utilization | vendor SDK | **usually unavailable** — show N/A |
| Core/mem clock, VRAM used/total, power, power limit, fan % | NVML / ADLX / IGCL | core path, reliable |
| Edge temp | NVML | reliable |
| Hotspot / memory temp, voltage | **NVAPI** (NVIDIA), ADLX (AMD) | may be null on some cards |
| PCIe gen/width | NVML `...PcieLinkGeneration/Width` | |
| Throttle / power‑limit status | NVML `...ClocksThrottleReasons` | |
| Model/vendor/VRAM type/GPU BIOS/driver version | SDK + DXGI | |

### Memory
| Data | Source | Driver? | Admin? |
|---|---|:--:|:--:|
| Total/used/available/cached/committed/commit limit | `GlobalMemoryStatusEx` + `GetPerformanceInfo` + `\Memory\*` counters | No | No |
| Page‑file usage, compression, page faults | `\Memory\*` / `\Paging File\*` counters | No | No |
| Frequency, per‑DIMM mfg/model/capacity/slot | SMBIOS `Win32_PhysicalMemory` (once) | No | No |
| Channels | decode/heuristic | No | No |
| **Timings (CAS/tRCD/tRP/tRAS/command rate)** | SPD over SMBus | **Yes** | Yes |
| **Per‑DIMM temperature** | DDR4 TSOD 0x18–0x1F / DDR5 SPD5 hub 0x50–0x57 over SMBus | **Yes** | Yes |

### Storage (admin, no driver)
| Data | Source |
|---|---|
| Live read/write speed, IOPS, activity | PDH `\PhysicalDisk(*)\*` |
| Capacity total/used/free | volume APIs |
| Temp (composite/controller/NAND), health %, power‑on hours, power cycles, totals R/W, SMART attrs, NVMe %used/spare/critical‑warning | NVMe Health Log (0x02) via `DeviceIoControl`; SATA SMART via `IOCTL_ATA_PASS_THROUGH` |

### Motherboard / Fans (driver, Phase 6)
| Data | Source | Notes |
|---|---|---|
| Chipset/VRM/VRM‑MOS/PCH/socket/PCIe/M.2 temps | Super‑I/O (LPC) | only where board wires them; M.2 often = NVMe sensor |
| Per‑sensor / per‑fan‑header identity | Super‑I/O register map + board table | board‑specific |
| Fan RPM/%/PWM, min/max, pump RPM/%, status | Super‑I/O | read‑only |

### Displays (no driver, no admin)
| Data | Source |
|---|---|
| Enumeration, resolution, refresh, orientation, position, DPI/scaling, primary/secondary | `QueryDisplayConfig` + per‑monitor DPI APIs; DXGI adapter↔output |
| Connection type, color depth/space | DisplayConfig target device name / output technology; DXGI output desc |
| HDR status + HDR/SDR brightness | `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO`, SDR white level |
| Adaptive‑Sync / G‑SYNC / FreeSync / VRR + range + current state | DXGI/DisplayConfig (adaptive‑sync) + vendor SDK (G‑SYNC via NVAPI, FreeSync via ADL) |
| Manufacturer/model/serial/mfg date/native res/physical size/modes/EDID | EDID (SetupAPI/registry or `root\wmi`), parsed |

### Network (no driver; some admin)
| Data | Source |
|---|---|
| Adapter list, link speed | `GetAdaptersAddresses` (IP Helper) |
| Up/down speed, utilization | PDH `\Network Interface(*)` / `\Network Adapter(*)` |
| Wi‑Fi signal / channel | WLAN API (`WlanQueryInterface`) |
| Ping/latency, packet loss | active ICMP (`IcmpSendEcho`) to configurable host (sampled) |

### Windows / System
| Data | Source | Admin? |
|---|---|:--:|
| Edition/version/build/install date | registry `CurrentVersion` | No |
| Uptime / last reboot / boot time | `GetTickCount64`, perf | No |
| Secure Boot | UEFI var / firmware API | No |
| TPM | TBS / `Win32_Tpm` | No |
| Virtualization enabled | CPUID + firmware | No |
| BitLocker | `Win32_EncryptableVolume` | **Yes** |
| Startup apps (+impact heuristic), running processes | Run keys + Task Scheduler + `NtQuerySystemInformation` | No |
| Sleep/wake history | Event Log (Kernel‑Power) | No |

### Gaming (Phase 10)
| Data | Source | Notes |
|---|---|---|
| FPS, 1%/0.1% low, frame time, CPU/GPU frame time, present latency | **PresentMon (ETW)** | not a simple API |
| Per‑process CPU/RAM/GPU | perf counters / ETW / `NtQuerySystemInformation` | |
| Overlay | companion always‑on‑top window (v1); injected in‑game overlay optional/v2 | **anti‑cheat risk** |

---

## 6. Repository layout

```
hwmon/                          # rename to chosen codename
├─ PROJECT_PLAN.md              # this file
├─ README.md
├─ .github/workflows/ci.yml
├─ src/                         # frontend (Vite + TS/Svelte)
│  ├─ index.html, main.ts, types.ts, styles/theme.css
│  ├─ gauge.ts, sparkline.ts, chart.ts           # hand-rolled canvas
│  └─ components/               # Gauges, System, Displays, Sensors, Storage,
│                               #   Network, Health, Alerts, Explorer, Profiles, Overlay
├─ src-tauri/                   # Rust core
│  ├─ Cargo.toml, tauri.conf.json, build.rs
│  └─ src/
│     ├─ main.rs
│     ├─ registry.rs            # unified Sensor registry (id/group/unit/min/max/avg/source)
│     ├─ snapshot.rs            # SensorSnapshot + aux structs (serde)
│     ├─ poller.rs              # 1 Hz loop → registry + history + emit "sensors"
│     ├─ history/mod.rs         # SQLite store + downsampling + retention
│     ├─ health/mod.rs, alerts/mod.rs, analysis/mod.rs, profiles/mod.rs
│     ├─ sysinfo/, hwid/        # static inventory + hardware identification
│     ├─ usage/                 # PDH counters (cpu/gpu/disk/net) + per-process
│     ├─ gpu/{mod,nvml,nvapi,adlx,igcl}.rs
│     ├─ storage/, display/{mod,edid}.rs, network/
│     ├─ fps/                   # PresentMon/ETW (Phase 10)
│     └─ driver/{mod,ioctl}.rs
├─ driver/                      # KMDF driver (C) — Phase 6
│  ├─ hwmon.sln, src/{driver,msr,portio,smbus}.c, inc/ioctl.h, hwmon.inf
├─ service/                     # elevated Windows service — Phase 8
└─ installer/                   # WiX/NSIS + signing scripts — Phase 11
```

Keep IOCTL codes in `driver/inc/ioctl.h`, mirrored in `src-tauri/src/driver/ioctl.rs`.

---

## 7. Phased roadmap

> Phases 1–5 produce a genuinely useful app that needs **no driver**. Phase 6 adds driver‑gated sensors. 7 adds history/health/alerts/explorer. 8 hardens privilege. 9 polishes + profiles + analysis. 10 is the isolated gaming/overlay module (deferrable). 11 ships.

### Phase 0 — Scaffold, sensor engine, animation, history skeleton
- [ ] Tauri 2 project (Rust + Vite/Svelte); WebView2 confirmed.
- [ ] **Unified Sensor registry** (`registry.rs`) with min/max/avg tracking; provider trait.
- [ ] `SensorSnapshot` + aux structs (Rust) mirrored in `types.ts`; event/command topology (§4.1).
- [ ] Poller emitting `sensors` (simulated) at 1 Hz through the registry.
- [ ] Port prototype speedometer + sparkline; 60 fps easing; `prefers-reduced-motion`; pause‑when‑hidden.
- [ ] **History store** (SQLite) skeleton with write path + `query_history` stub + retention config.
- [ ] Dark instrument theme; responsive (stacks ~720px).

**Acceptance:** app launches; gauges animate on simulated data via the real pipeline; registry + history write path exercised; footprint minimal.

### Phase 1 — Static inventory, hardware ID, system info (no driver)
- [ ] `sysinfo/` + `hwid/`: CPU (model/arch/socket/cache/ISA/topology/P‑E map), RAM (per‑DIMM mfg/model/capacity/speed/slot, channels approx), board (mfg/model/BIOS+date/chipset/serial‑local‑only), GPU ID (model/vendor/VRAM type/BIOS/driver/PCIe), OS (edition/version/build/install date), uptime/last reboot/boot time, Secure Boot, TPM, virtualization, BitLocker (admin), startup apps (+impact heuristic), sleep/wake history, process overview.
- [ ] **System** and **Hardware** panels.

**Acceptance:** fields match `msinfo32`/Task Manager/`systeminfo` on the dev machine; no WMI in the poll loop; serial shown locally only.

### Phase 2 — Live usage, clocks, GPU, per‑process (no driver)
- [ ] `usage/`: CPU total+per‑core (PDH); GPU usage + per‑engine (3D/decode/encode/compute/copy) via GPU Engine counters; per‑process CPU/RAM/GPU (top‑N).
- [ ] `gpu/`: detect vendor; NVML→ADLX→IGCL: clocks, temps (edge; hotspot/mem via NVAPI where present), VRAM, power+limit, voltage, fan %, PCIe gen/width, throttle reasons, model/VRAM type/BIOS/driver.
- [ ] CPU MHz interim via `CallNtPowerInformation`.
- [ ] Real data into gauges + a **Processes** view.

**Acceptance:** utilization tracks Task Manager; GPU clock/temp/VRAM/power match `nvidia-smi`/vendor tool; tensor/RT show N/A cleanly; per‑engine values populate.

### Phase 3 — Displays (no driver)
- [ ] Enumeration + EDID parse; connection type; color depth/space; HDR + HDR/SDR brightness; VRR/G‑SYNC/FreeSync + range + current state (vendor SDK); DPI/scaling; orientation; physical size; primary/secondary.
- [ ] **Displays** panel with visual multi‑monitor arrangement + hot‑plug refresh.

**Acceptance:** all fields correct vs. Windows Display Settings + monitor spec on single and multi‑monitor; VRR/HDR states correct where hardware supports them.

### Phase 4 — Storage & memory runtime (admin, no driver)
- [ ] `storage/`: NVMe Health Log + SATA SMART — temps (composite/controller/NAND), health, capacity used/free, live R/W speed + IOPS + activity, totals R/W, power‑on hours, power cycles, SMART attrs, NVMe %used/spare/critical‑warning.
- [ ] Memory runtime: used/available/cached/committed/commit limit/page‑file/compression/page faults (perf counters).
- [ ] App runs elevated (manifest); unelevated launch degrades gracefully.
- [ ] **Storage** panel + memory runtime tiles.

**Acceptance:** drive temps within ~2 °C of reference; NVMe + SATA (if present) report full attribute set; memory tiles match Task Manager; unelevated degrade path works.

### Phase 5 — Network, power (partial), app self‑monitoring (no driver)
- [ ] `network/`: adapters, up/down, utilization, link speed, Wi‑Fi signal/channel, ping + packet loss (active ICMP, configurable host). **Network** panel + history.
- [ ] App self‑metrics (`get_app_metrics`): own CPU/GPU/RAM, poll Hz, UI FPS, active sensors, engine status. **Self‑monitoring** widget.
- [ ] Power panel: GPU contribution now; total/CPU contribution completed after Phase 6 (wire the panel, mark CPU as pending).

**Acceptance:** network rates match Task Manager; Wi‑Fi metrics correct on a Wi‑Fi machine; ping/loss update against the configured host; self‑monitoring reflects real footprint.

### Phase 6 — Kernel driver: CPU sensors, Super‑I/O, RAM SPD/temps
Development uses **test‑signing**.
- [ ] **6a** KMDF skeleton; whitelisted read‑only IOCTLs (§5 driver surface); test‑signed; shared `ioctl.h`.
- [ ] **6b** Rust driver client: install/start/stop, IOCTL wrappers, graceful absence handling.
- [ ] **6c** CPU: temps (Intel MSRs / AMD SMN), effective MHz + multiplier + BCLK (APERF/MPERF), package/core/SoC power (RAPL/SMU), package/core voltage, throttle/power‑limit/current‑limit status. Replace interim MHz.
- [ ] **6d** Super‑I/O: detect chip; register maps for initial targets (≥1 Nuvoton NCT67xx + ≥1 ITE IT87xx); fan RPM/%/PWM/min/max, pump RPM/%, voltages (scaled), mobo temps (chipset/VRM/VRM‑MOS/PCH/socket/M.2 where wired), per‑sensor/header identity.
- [ ] **6e** RAM: SPD timings (CAS/tRCD/tRP/tRAS/command rate) + per‑DIMM temps (DDR4 TSOD / DDR5 SPD5) over SMBus.
- [ ] Complete the Power panel (CPU + GPU + estimates).

**Acceptance:** on a supported Intel board and a supported AMD board, CPU temps/clocks/power/voltage/throttle‑status, fan RPMs, voltages, mobo temps, and RAM temps/timings match a trusted reference within tolerance; driver **rejects** any non‑whitelisted MSR/port/SMBus request (test proves it); unknown Super‑I/O degrades to "unsupported."

### Phase 7 — History, System Health, Alerts, Sensor Explorer
- [ ] History: persist all key metrics with downsampling + retention; ranges 5 min/30 min/1 h/6 h/24 h/7 days; graphs for CPU/GPU temp+util+clock, RAM, VRAM, storage temp, fan RPM, power, voltage.
- [ ] **System Health**: per‑subsystem (CPU/GPU/memory/storage/thermals/fans/voltages) status, warnings, sensor anomalies, thermal‑limit + throttle events, detectable hardware errors.
- [ ] **Alerts**: configurable rules (temp/fan/storage‑health/throttle/power‑limit/pump‑fail thresholds) → Windows + tray + optional sound notifications.
- [ ] **Sensor Explorer**: full grouped sensor tree with value/unit/min/max/avg/source/availability.

**Acceptance:** history ranges render from the store and survive restart within the size cap; alerts fire and notify on simulated + real threshold breaches; explorer lists every registered sensor with live min/max/avg; health reflects induced stress (e.g., a load test).

### Phase 8 — Privilege separation
- [ ] Elevated **Windows service** performs privileged reads (driver IOCTLs + SMART); non‑elevated UI over an authenticated, ACL‑restricted **named pipe** (versioned protocol); service lifecycle wired to the installer.

**Acceptance:** UI runs unelevated with the full sensor set via the service; pipe rejects unauthorized clients; service restart recovers cleanly.

### Phase 9 — Performance hardening, Profiles, Intelligent Analysis, polish
- [ ] Meet §8 budget; throttle/pause when hidden; release build + binary trim.
- [ ] **Profiles** (Desktop/Gaming/Work/Benchmark/Silent) controlling displayed sensors, poll rate, graphs, overlay visibility, alert rules, animation intensity.
- [ ] **Intelligent Analysis** (optional): per‑user baselines; flag unusual temps; correlate temp with load/clock/power; identify likely throttling and fan/cooling anomalies; explain major power changes — in plain language, from the history store.
- [ ] Settings (°C/°F, per‑sensor show/hide, autostart, start‑minimized, tray), empty/error states, accessibility.

**Acceptance:** idle footprint meets §8 on the reference machine (verified via the self‑monitoring widget); profiles switch behavior; analysis produces sensible explanations on real data; settings persist.

### Phase 10 — Gaming/performance capture & overlay (isolated; deferrable)
- [ ] FPS/1%/0.1% low/frame time/CPU+GPU frame time/present latency via **PresentMon (ETW)**; `fps` event only while a game runs.
- [ ] **Companion overlay** (always‑on‑top borderless window) showing chosen metrics.
- [ ] *(Optional/v2)* injected in‑game overlay — **only with an explicit anti‑cheat warning**; default off.

**Acceptance:** FPS/frame times match a reference overlay in a real game; companion overlay stays on top without stealing focus; injected overlay (if built) is opt‑in and clearly warned.

### Phase 11 — Signing, packaging, distribution
- [ ] EV code‑signing certificate; Microsoft Partner Center (Hardware) account.
- [ ] **Attestation‑sign** the driver (CAB submit → signed driver); EV‑sign app + installer.
- [ ] MSI/NSIS installer (installs service + signed driver elevated; autostart); Tauri auto‑updater.

**Acceptance:** on a fresh Windows 11 24H2+ machine with default security (blocklist + Memory Integrity on), the installer runs, the driver **loads**, and the full sensor set works with no test‑signing or manual exceptions.

---

## 8. Performance budget (reference: 6–8 core desktop, window visible, 1 Hz poll)

| Metric | Target |
|---|---|
| CPU (app total, steady) | < 1.5% average |
| RAM (working set) | < 120 MB (raised from v1 for the history store + extra providers) |
| GPU | ~0% idle; negligible during animation |
| History store on disk | capped (e.g., ≤ 200 MB) with retention/downsampling |
| Poll → render latency | < 1 frame after snapshot |
| Installer size | < 60 MB (excluding vendor SDK redistributables; raised from 20 MB for 1.1, ROADMAP decision 28) |
| Startup to first data | < 2 s |

The **app self‑monitoring** widget is the built‑in verification instrument for these numbers; record them in `README.md`.

---

## 9. Hardware test matrix

- **CPU:** an **Intel** (12th gen+ for P/E‑core ID) and an **AMD Ryzen** (Zen 3/4/5; note family for temp code).
- **GPU:** **NVIDIA** (NVML/NVAPI) and, if available, **AMD** (ADLX) and **Intel Arc** (IGCL); also iGPU‑only.
- **Super‑I/O:** a **Nuvoton NCT67xx** and an **ITE IT87xx** board (record chip IDs).
- **RAM:** **DDR4** (TSOD) and **DDR5** (SPD5) kits; and a kit without a thermal sensor (degrade gracefully).
- **Storage:** one **NVMe** and one **SATA** drive.
- **Displays:** single; dual/triple mixed‑refresh; a laptop panel (brightness); an external over DDC/CI; an **HDR** monitor; a **VRR/G‑SYNC/FreeSync** monitor.
- **Network:** **Ethernet** and **Wi‑Fi**.
- **Gaming:** a real game for FPS/frame‑time validation (Phase 10).
- **OS:** Windows 11 24H2+ (release‑signing target) and Windows 10 21H2.

Cross‑check every sensor against a trusted reference; never display an unverifiable value — prefer "unsupported/unavailable."

---

## 10. Coding & quality standards

- Rust: `clippy` clean, `rustfmt`; no `unwrap()` in the poll path — each sensor fails individually (→ `null`, logged once), never crashing the snapshot.
- Driver: treat all IOCTL input as hostile; validate ranges; **no arbitrary read/write, no write paths beyond register/bank selection**; tests assert non‑whitelisted requests are rejected.
- Frontend: strict TS; render correctly with any field missing; visible focus; honor `prefers-reduced-motion` and per‑profile animation intensity.
- Keep Rust structs and `types.ts` in lockstep (§4.4 is the source of truth).
- Rotating log file + a diagnostics view (detected chips/SDKs/available sensors) for field debugging.
- Privacy: board serial and identifiers are **local display only**; no telemetry/exfiltration.

---

## 11. Scope boundaries

**In v1:** everything in §1 except the two deferred items below; gaming capture + companion overlay (Phase 10) may be deferred to v2 without blocking release.

**Deferred to v2 (explicit, opt‑in, with warnings):**
- **Fan / cooling CONTROL** — configurable fan curves, cooling profiles, pump control. Requires driver **write** access to EC/Super‑I/O; hardware‑safety and stability risk. Build only after per‑board validation and explicit user consent. v1 ships fan/pump **monitoring** + curve **visualization** only.
- **Injected in‑game overlay** — anti‑cheat risk; default off, warned.

**Out of scope entirely (v1 & v2 unless revisited):** overclocking/voltage tuning (revisited for NVIDIA clock offsets and power limits: decision 37, 1.6; voltages stay out), ARM64 Windows, remote/network monitoring of other machines, cloud sync.

---

## 12. Open decisions to confirm

1. **Product name / codename** (repo currently `hwmon`).
2. **First release cut:** ship Phases 1–5 (no driver → no EV cert/Partner Center yet) as an early release, or hold until Phase 6 + 11 so the first release already includes temps/fans/voltages? Decides when the EV/Partner‑Center cost is incurred.
3. **Gaming module (Phase 10):** in v1, or defer to v2? (Highest‑effort area; PresentMon + overlay.)
4. **Fan control:** build the deferred v2 control module at all, given the hardware‑safety/liability exposure? If yes, on which validated boards only?
5. **Ping/latency target host** for the network panel (user‑configurable default, e.g., 1.1.1.1 / 8.8.8.8).
6. **History retention/size cap** and downsampling tiers (proposed: raw ≤ 30 min, 10 s avg ≤ 24 h, 1 min avg ≤ 7 days, ≤ 200 MB).
7. **Frontend framework:** Svelte vs. vanilla TS (pick one).
8. **Installer:** WiX (MSI) vs. NSIS.
9. **Initial Super‑I/O coverage** beyond the required Nuvoton + ITE.

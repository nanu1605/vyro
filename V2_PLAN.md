# Vyro — V2 Plan & Decision Log (consolidated)

Planning doc for **Vyro V2**. It consolidates two inputs — the "Vyro V2 — Product Roadmap & Feature
Specification" (the Observe → Understand → Control → Optimize vision) and the earlier three ideas
(3-D GPU model, devices + battery, open-on-dashboard) — into one ordered, executable plan that
follows the conventions of [`PROJECT_PLAN.md`](PROJECT_PLAN.md) (the build spec) and
[`ROADMAP.md`](ROADMAP.md) (the execution tracker).

V1 (roadmap P0–P11, incl. low-level PawnIO sensors, service split, gaming capture + overlay) is
achieved. V2 phases continue the P-numbering at **P12** and roll up into new milestones **M7–M13**
(V1 used M1–M6). The V2 direction: **stop being the app that says the CPU is hot, and become the
app that says why, whether it matters, what changed, and — with explicit permission — what it can
safely do about it.**

Cross-cutting conventions from `ROADMAP.md` still hold unchanged (provider = parse + win, `ts-rs`
bindings are the source of truth, failing sensor → `null`, static inventory once, `SimProvider`).

---

## What changed vs. the first V2 draft

- **Fan/pump control is now the flagship**, not an optional gated add-on. Treated as approved (you
  placed it as P0), and built to the strict safety envelope in the roadmap §4.5/§26 (capability
  detection, per-board allow-list, failsafe, watchdog, restore-on-crash, control-event logging,
  read-only default). Decision 18 flips to **yes, safety-gated**.
- **Remote/web companion is pulled into V2** (was V3). Kept LAN-only, opt-in, authenticated, and
  sequenced **last** so its network attack surface never blocks the rest. (Since 1.4, ROADMAP
  decision 34: a phone app also reaches the PC from anywhere through a relay, encrypted end to
  end and off by default; the PC still opens no port to the internet.)
- **Most of the earlier "suggested backlog" is adopted** (benchmark, export/report, auto game
  profiles, themes, per-process net) — they appear in the roadmap, so they're in.
- **The three confirmed ideas are retained and mapped in:** devices+battery → P22; the stylized 3-D
  GPU model → P23 (merged with "My PC visualization" + Thermal Map); open-on-dashboard → P25.
- This scope is **2–3 releases**. See *Suggested release sequencing* — ship it in increments, merge
  each milestone to `main` as it lands, don't hold one giant V2 merge.

---

## Branching & release strategy

`main` stays the shippable v1 line; V2 is built on a long-lived branch and merged in **per
milestone**.

```
git switch main && git pull
git switch -c v2 && git push -u origin v2
```

- `main` = released line (v0.1.x bug/security/signing only), tagged, mirrored to `RELEASES_REPO`.
  Cherry-pick those fixes into `v2` so it never drifts behind.
- Each phase is a feature branch off `v2` (`v2/fan-control`, `v2/game-auto`, …) → PR into `v2` once
  its Acceptance Criteria pass and CI is green.
- **Merge `v2` → `main` at each milestone boundary** (real `--no-ff` merge, then tag), not once at
  the end — keeps `main` shippable and each release testable. Keep `ts-rs` drift, installer-size,
  and axe-core checks green on `v2` throughout.

## Suggested release sequencing

| Release | Milestones | Theme |
|---|---|---|
| **v0.2 "Control"** | M7 | Fan/pump control (flagship) + the safety foundation |
| **v0.3 "Gaming"** | M8 | Auto game profiles, Overlay 2.0, session replay |
| **v0.4 "Intelligence"** | M9, M10 | Diagnostics, cause chains, Performance Score, benchmark, one-click diagnostics |
| **v0.5 "Command Center"** | M11, M12 | Devices, 3-D/My-PC/thermal-map, dashboard builder, themes, remote |
| **v1.0** | M13 | Full V2 polish + control-safety validation → 1.0 |

---

## V2 decision log (continues `ROADMAP.md`; numbers from 14)

| # | Decision | Answer | Status |
|---|---|---|---|
| 14 | GPU visual | Own stylized parameterized **WebGL** model; no third-party photos; offline | decided |
| 15 | 3-D renderer | **Three.js** bundled locally; render on-demand only | decided |
| 16 | Devices battery sources | BLE GATT 0x180F + PnP battery devnode property + XInput; vendor HID++ later | decided |
| 17 | Open-on-Dashboard | Default on, with "Open on: Dashboard / Last tab" setting | decided |
| 18 | **Fan/pump control** | **Yes — V2 flagship**, safety-gated (roadmap §4.5/§26); read-only default; per-board allow-list | decided |
| 19 | Remote / web companion | **In V2**, LAN-only, opt-in, authenticated, sequenced last (M12); "LAN-only" superseded in 1.4 by ROADMAP decision 34 (a relay, end to end encrypted) | decided |
| 20 | Version at milestone merges | v0.2/0.3/0.4/0.5 per milestone; **v1.0** at M13 | decided |
| 21 | Themes | Multi-theme + accent (Midnight/Cyber/Glass/Minimal/AMOLED/…); accessibility-preserving | decided |
| 22 | **Performance Score** methodology | Observed-condition, **versioned fixed formula** (so history stays comparable); shows inputs; not hardware-prestige | open — needs formula sign-off |
| 23 | Benchmark positioning | **Self-relative** (own engine; not cross-comparable to 3DMark); report stability/variance, not just a number | decided |
| 24 | Fan-control launch boards | Start **NCT6687D** (field-proven read path); expand per validated board | open — pick launch set |
| 25 | Tagline | Keep **"Know your machine."**; roadmap alternates are options, not adopted unilaterally | open — your call |
| 26 | Navigation restructure (§28) | Adopt the new IA **incrementally**, keeping V1 tabs working each release | decided |

---

## Cross-cutting engineering constraints

**Footprint stays the gate (roadmap §25).** V1 sits at ~120 MB avg / 132 MB peak (P9a). Every V2
feature declares a measured CPU/GPU/RAM/disk/poll budget *before* implementation and is re-measured
in the "Vyro itself" card. Specific watch-items: the WebGL model (P23), the dashboard builder (P24),
Overlay 2.0 graphs (P14), and the remote server (P26) each threaten the budget — all must render
on-demand, suspend when hidden, and stay off the 1 Hz tick.

**Hardware-control safety (roadmap §4.5/§26) is non-negotiable and gates M7.**
- Read-only remains the default; a writable controller must be **positively detected**, never
  assumed. Unknown/EC-owned controllers are **blocked from control** and stay read-only.
- No unrestricted register writes — only known fan/pump control registers on allow-listed chips,
  through PawnIO, with **read-back verification** after every write.
- Hard **safe min/max bounds** + a temp-mapped **minimum floor** (thermal safety always beats
  performance). **Emergency thermal override** + **failsafe curve**.
- **Watchdog** on the control loop; **restore BIOS/EC control** on app exit, service crash, and
  control-provider failure. Full **control-event log**.
- ⚠ *Engineering note:* on some boards fans are owned by the EC, not the Super-I/O — those can't be
  safely driven and must be refused. AIO pump headers are easy to mislabel; require explicit
  header identification + user confirmation before any write.

**Intelligence must not overclaim (roadmap §26).** The UI always distinguishes **measured** values
from **inferred** explanations; findings carry **confidence** (high/med/low) and show the supporting
evidence; weak correlations are never presented as proven cause. The correlation engine reports a
relationship + confidence, not a verdict.

**Scores must be defined to be meaningful.** Performance Score and benchmark numbers use a **fixed,
versioned methodology** stored with each result, so historical comparison stays valid; the benchmark
reports **stability/variance** (Windows background load, thermals, and power plan make single runs
noisy) and is positioned as self-relative, not cross-tool-comparable.

**Data model additions (roadmap §24)** — extend the SQLite store, keeping growth bounded/retained:
`game_sessions`, `game_events`, `benchmark_runs`, `benchmark_metrics`, `diagnostic_runs`,
`diagnostic_findings`, `hardware_changes`, `fan_profiles`, `fan_control_events`, `dashboard_layouts`.

---

## Phase tracker (P12–P27)

### M7 — Control Foundation  → v0.2 "Control"  *(flagship; safety-critical)*

**P12 — Fan & pump control.** Break into sub-phases; do not ship any writable board unvalidated.
- [ ] 12a Writable-capability detection + safe write boundaries via PawnIO (NCT6687D first): identify
      controllable headers, manual-mode enable, read-back verify, bounds. Unknown chips → read-only.
- [ ] 12b Curve engine + **visual curve editor** (drag points, add/remove, labels, live preview,
      reset, save-per-profile, copy between headers, duplicate) following any temperature source
      (CPU/CCD/GPU/hotspot/VRAM/VRM/board/storage/any sensor); hysteresis, ramp up/down delays.
- [ ] 12c Pump control (RPM/%/curve, fixed/quiet/performance), header identification + confirm.
- [ ] 12d Failsafe: emergency thermal override, failsafe curve, control-loop **watchdog**,
      restore-on-exit / service-crash / provider-failure; zero-RPM/fan-stop only where supported.
- [ ] 12e Per-profile curves + control-event logging (`fan_profiles`, `fan_control_events`); a clear
      always-visible indicator when hardware control is active.

**Acceptance:** control works on every allow-listed writable controller and is safely blocked on
unsupported hardware; killing the app/service or a provider fault restores BIOS/EC cooling;
emergency override engages under a forced thermal event; every write is read-back-verified and
logged; read-only remains the default until explicitly enabled.

### M8 — Gaming Automation  → v0.3 "Gaming"

**P13 — Automatic game profiles.** Reuse the P10 foreground-fullscreen-game signal: detect game →
match to a per-game profile → switch profile, start session, enable overlay, apply poll rate + apply
cooling profile → restore previous profile on exit. Triggers: process detected/closed, fullscreen
detected, CPU/GPU-load threshold, manual. Per-game store (refresh Hz, cooling, overlay layout, poll).
**Acceptance:** launching/closing a game reliably switches and restores profile+cooling+overlay; a
game with no profile uses a sensible default; manual override always wins.

**P14 — Gaming Overlay 2.0.** Layout modes Minimal / Performance / Full / Graph; customization
(position, scale, opacity, visible metrics, graph on/off, update rate, theme/accent, compact).
Injected overlay stays **research-only** (out of scope as a blocker). **Acceptance:** all four modes
render over borderless/windowed games without stealing focus, within budget; exclusive-fullscreen
still reported as "overlay unavailable" (unchanged from P10b).

**P15 — Game Session Replay.** Turn logged sessions into an explorable timeline (FPS/GPU/CPU/temp/
power tracks) with event markers (FPS drop, thermal warning, throttle, power-limit, alt-tab, start/
exit, profile change, overlay) and a session summary; click a marker → surrounding sensor context.
**Acceptance:** a completed session renders its timeline + markers from the store; marker context
matches history at that timestamp.

### M9 — Intelligence & Diagnostics  → v0.4 "Intelligence"

**P16 — Vyro Intelligence engine.** Layered (roadmap §23): derived metrics (thermal headroom, load-
normalized temp, fan response, power efficiency, stability) → baselines (recent / same load / same
game / same profile / previous benchmark) → event detection → **explanation** with **cause chains**
and a **correlation engine**; bottleneck detection (CPU/GPU); every finding tagged measured-vs-
inferred + confidence + evidence. **Acceptance:** on induced conditions (load, thermal, background
interference) it produces correct findings with evidence; no weak correlation shown as cause;
measured vs inferred always visually distinct.

**P17 — Vyro Performance Score.** Observed-condition score (thermals, sustained clocks, throttling,
cooling, storage health, memory pressure, network quality, stability, historical deviation) with a
**versioned formula** stored per result and a visible input breakdown. **Acceptance:** score is
reproducible on a stable machine, moves sensibly under degraded conditions, and shows its inputs;
formula version recorded so past scores stay comparable.

**P18 — Smart compound alerts.** Multi-condition rules (AND/OR, rate-of-change, FOR-duration,
baseline-relative) with actions: notify/toast/sound/tray, **switch profile**, **raise fan curve
(only if control explicitly enabled)**, record diagnostic event; noise control via hysteresis / hold
/ cooldown / duplicate-suppression / baseline thresholds. **Acceptance:** compound rules fire only
when all conditions hold; profile/fan actions gated on control being enabled; noise controls
verified.

**P19 — Hardware change detection.** Diff inventory across runs (GPU/RAM/monitor/storage/BIOS
changes) → `hardware_changes` + a **PC change history** the intelligence engine can cite. **Acceptance:**
swapping a monitor / RAM / driver / BIOS is detected and logged with before→after.

### M10 — Benchmark & Diagnostics  → v0.4 "Intelligence"

**P20 — Benchmark Mode.** Own engine across CPU/GPU/memory/storage/gaming/sustained-thermal/power-
efficiency/stability; result = scores + peak temp/power + throttle count + **stability %**; history
with full config (drivers/BIOS/scores/thermals) for run-to-run comparison. Self-relative, variance
reported. **Acceptance:** a run scores each area, records peaks/throttling/stability, and compares to
a prior run on the same machine; stops cleanly and never leaves load running.

**P21 — One-click diagnostics + reports.** "Run diagnostics" → the check list (thermal behavior,
clock stability, power limiting, throttling, fan/pump, RAM pressure, storage health/temp, network
latency/loss, hardware errors, driver info, sensor availability, service status) → pass/attention
findings backed by telemetry; export **HTML / JSON / Markdown / PDF** (serials redacted by default).
**Acceptance:** diagnostics complete off the UI thread, every "attention" item cites its telemetry,
and all four export formats open and match on-screen values.

### M11 — Visualization & UX  → v0.5 "Command Center"

**P22 — Devices & peripherals + wireless battery.** New `devices/` provider (SetupAPI/Config
Manager + WinRT Bluetooth/Enumeration + Core Audio). Enumerate wired+wireless peripherals with
connection state; battery via BLE GATT 0x180F, the PnP battery devnode property, and XInput (vendor
HID++ later). `DeviceWatcher` for add/remove; battery polled ~60 s; addresses local-only. No driver/
admin. **Acceptance:** lists wired+wireless devices with correct state; hot add/remove reflects in
seconds; battery shows where reported and "—" otherwise.

**P23 — 3-D GPU / "My PC" visualization + Thermal Map.** Stylized parameterized **Three.js** GPU
model driven by a bundled device-ID spec table (fans/length/slots/connectors/accent) + generic
fallback + user model-confirm; live fan-spin from RPM, temp tint. Extend to a **My PC** view (CPU/
GPU/board/RAM/storage/cooling/monitors with live temp labels + click-to-inspect) and a **Thermal
Map** (heat intensity from configured thresholds, modes CPU/GPU/board/storage/full). On-demand
render only, ≤30 fps / render-once idle, never in tray, honours animation-intensity, 2-D fallback.
⚠ *No exact-board render offline* (licensing + no offline model library) — stylized-own-model only.
**Acceptance:** model reflects the detected card on ≥2 GPUs; thermal map colors track real temps vs
thresholds; GPU/RAM cost negligible when hidden and within budget when shown; toggling off stops all
WebGL work.

**P24 — Custom dashboard builder.** Widget palette (gauges, temp, clock, power, fan, pump, RAM,
VRAM, FPS, frametime, network, storage, sparklines, history charts, health, events); drag/resize/
delete/duplicate/lock/reset; saved layouts (`dashboard_layouts`) with presets (Gaming/Work/
Streaming/Benchmark/Monitoring/Minimal). **Acceptance:** a user-built layout persists across restart,
widgets show live data, and layout edits stay within the footprint budget.

**P25 — Themes, dynamic states, open-on-dashboard, shareable cards.** Multi-theme + accent (contrast/
accessibility preserved, warning states consistent); dynamic UI states (idle/gaming/warning/critical)
within the animation budget; **open-on-Dashboard default** + "Open on" setting; **shareable PC cards**
(16:9 / 1:1 / story / minimal / transparent, "Powered by Vyro", specs + score, serials off).
**Acceptance:** themes/accent persist and pass axe-core in every theme; app lands on Dashboard unless
"Last tab" chosen; exported cards render at each size with no serials.

### M12 — Remote  → v0.5 "Command Center"  *(largest new attack surface — do last)*

**P26 — Remote Vyro + mobile/web companion.** Read-only LAN dashboard served by `VyroService`
(CPU/GPU/RAM/storage/fans/temps/power/FPS/network/health/alerts) with a mobile-friendly view + remote
alerts. **Security is the feature:** LAN-only, **opt-in, off by default**, token auth + pairing (QR),
session expiration, no public-internet exposure, explicit firewall prompt, one-click revoke/disable.
⚠ *Engineering note:* HTTPS on a LAN name is awkward (self-signed → browser warnings); prefer
loopback+LAN bind with token auth over plain HTTP + a clear "LAN only" indicator, or a pairing token
in the URL — decide at P26. **Acceptance:** disabled by default; when on, only authenticated LAN
clients connect, sessions expire, revoke works immediately, and nothing is exposed to the public
internet.

### M13 — V2 Polish  → v1.0

**P27 — Polish & validation.** Performance/footprint audit across all V2 features; accessibility
pass; crash recovery; **V1→V2 settings/history migration** testing; clean-machine testing;
**control-safety validation on every supported writable controller**; anti-cheat compatibility
documented; docs. **Acceptance:** the roadmap §30 gate is met — control tested on every writable
board, unsupported hardware blocked, failures restore safe cooling, migration preserves V1 data,
resource budget held, V1 features still work.

---

## Target information architecture (roadmap §28) — adopt incrementally

Move toward Home → Performance → Cooling → Gaming → Diagnostics → Benchmark → History → Hardware →
Settings, but **without breaking V1 tabs mid-release**: add each new section as its milestone lands
(Cooling with M7, Gaming expansion with M8, Diagnostics/Benchmark with M9–M10, etc.), and only
retire/rename old tabs once their replacement is field-checked. Home answers, in seconds: is my PC
healthy, what's limiting it, is anything abnormal, current performance, and what I can do about it —
with one-click Diagnose / Benchmark / Cooling actions, Sensor Explorer still one level down.

---

## Out of scope / non-blockers (roadmap §31)

Never V2 launch blockers, and mostly not V2 at all: automatic overclocking (1.6 added GPU
tuning the owner applies, with a tested "Find my best": decision 37), automatic voltage
modification, BIOS flashing, unrestricted register writes, mandatory cloud account, cloud telemetry
by default, public-internet exposure, mandatory AI/cloud API usage, Electron-class runtime,
excessive background animation, and **injected in-game overlay** (research item; companion overlay
remains the shipping path).

---

## Open decisions to confirm

1. **Performance Score formula (22):** sign off on the exact inputs + weighting (I'll draft a v1
   formula for review) so it's fixed and history stays comparable.
2. **Fan-control launch boards (24):** ship NCT6687D first (field-proven), then which boards next?
   Each needs its own validation pass before its control path ships.
3. **Tagline (25):** keep "Know your machine." or adopt one of the roadmap's V2 alternates?
4. **Remote timing (19):** confirm remote in v0.5, or push to a later point release if v0.5 is
   already heavy.
5. **Dashboard-builder depth (24):** full drag/resize/duplicate grid, or start with reorderable
   fixed-size widgets and grow it?
6. Anything here you'd rather **cut or defer to V3** to keep V2 focused (roadmap §31's own warning).

# Vyro Diagnostics: checks v1

Status: **defaults chosen at build (2026-10-15)**; the owner can change the checks and limits at
the P21 field check. A change to a check or a limit after release becomes checks v2, and every
stored report keeps its version.

## What it is for

**"Run diagnostics" answers one question in about a second: is anything about this PC worth a
look, and what shows it?** It adds no load (the Benchmark does that). It reads what Vyro already
records:
- the week of history and the minute digests
- the Health log and the Windows event log
- the drives' SMART or NVMe health
- the readings now

The result is a report with one entry per check:
- **Pass:** nothing worth a look.
- **Attention:** something worth a look.
- **Info:** worth knowing, never a fault.
- **Unavailable:** there is no data for the check, and the entry says why.

**Every Attention entry cites its telemetry.** That means the measured values behind it, where
they come from (history, Health log, SMART, event log, live reading), and the sensor, so the
History view can open it. Explanations stay apart from measurements, as in P16.

## The checks

| # | Check | Reads | Attention when |
|---|---|---|---|
| 1 | Thermals | CPU and GPU median under load over 7 days, against their limits (as in Performance Score formula v1); Health temperature events over 7 days | headroom under 10 °C, or a Health temperature warning or problem in 7 days |
| 2 | Clock stability | clock sag over full-load stretches of 5 minutes or more (7 days) | sag of 5 % or more |
| 3 | Power limits | share of loaded time the CPU (PPT) or GPU (power target) held its power limit | never: Info. This is how CPUs and GPUs normally hold their budget |
| 4 | Throttling | share of loaded time with the CPU's thermal or current flag or the GPU's thermal flag | 2 % or more |
| 5 | Fans and pump | fan and pump speeds now; Health fan events over 7 days; CPU temperature for the load and fan response against usual (P16); fan control's safety override | a fan or pump stopped (now or in 7 days), the CPU 5 °C or more hotter than usual for the load, the fan at 70 % or less of its usual, or a safety override active. The owner's own choice (Silent, a fan curve) does not count |
| 6 | Memory | RAM in use, 95th percentile over 7 days | 90 % or more |
| 7 | Storage | each drive's SMART verdict, SSD life left, temperature now; free space on the system drive | a drive at caution or bad, under 10 % life, at 70 °C or more, or under 10 % free on the system drive |
| 8 | Network | ping round trip (p95) and lost replies over 7 days | p95 of 150 ms or more, or 3 % lost or more |
| 9 | Hardware errors | Windows System log over 30 days: unexpected shutdowns (Kernel-Power 41), WHEA hardware errors | any unexpected shutdown, any fatal hardware error, or 10 or more corrected ones |
| 10 | Changes from usual | P16's warnings now, with their evidence | any warning |
| 11 | Drivers | each GPU's driver version and date; the Windows build | never: Info |
| 12 | Sensors | PawnIO, and which readings arrive: CPU temperature and power, board fans and voltages, RAM temperatures, drive health | no CPU temperature (most checks then lack data) |
| 13 | Service | how the app reaches the engine: the Vyro service, or the app's own engine without the privileged sensors; versions | the app runs without the service, or the two versions differ |

**When a check passes.** A check passes when it has data and nothing reaches its limit. It is
**Unavailable** when the data is missing, for example with too little history, no long full-load
stretch, or ping off. Each check's citations are shown either way.

## The report

**What it holds:**
- the time and the checks version
- the Vyro version and how the app reached the engine
- a summary of the PC: CPU, GPUs, board, BIOS, RAM, Windows
- every check with its status, summary, citations and, for Attention, advice

**Where it is kept.** Reports are kept in the history store: `diagnostic_runs` holds the report as
JSON and `diagnostic_findings` holds one row per check. The newest 50 reports are kept.

## Exports

**Formats.**
- **HTML:** one self-contained page that prints well.
- **JSON:** the report as stored.
- **Markdown.**
- **PDF:** written by Vyro itself, with no browser or printer involved.

**Same values as on screen.** All four are made from the same report, with the same formatting,
in the temperature unit chosen in Settings.

**Serial numbers.** These are never stored with a report. They are left out of exports by
default, and an export includes them only when "Include serial numbers" is ticked.

**Where exports go.** Exports go to `Documents\Vyro Reports`, named
`vyro-diagnostics-<date>-<time>.<ext>`, and Vyro shows the file in Explorer.

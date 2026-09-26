# PawnIO modules

Signed [PawnIO](https://github.com/namazso/PawnIO) modules, unmodified, from the
[PawnIO.Modules 0.2.11 release](https://github.com/namazso/PawnIO.Modules/releases/tag/0.2.11)
(as vendored by LibreHardwareMonitor at commit `931616f`). Source code:
<https://github.com/namazso/PawnIO.Modules>.

License: GNU LGPL 2.1 — see [`COPYING`](COPYING). Vyro loads them into the separately installed
PawnIO driver at run time; `SHA256SUMS` pins the exact files (checked by a unit test).

| File | Used for |
|---|---|
| `AMDFamily17.bin` | AMD Zen 1–5: whitelisted MSR reads (RAPL, P-state, APERF/MPERF), SMN reads (temperatures, memory controller) |
| `LpcIO.bin` | Super-I/O chip detection and hardware-monitor register access (fans, voltages, board temperatures) |
| `SmbusPIIX4.bin` | AMD FCH SMBus (SPD, DIMM temperature sensors) |
| `RyzenSMU.bin` | AMD SMU power-management table (SoC rails) |

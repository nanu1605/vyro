# Field check 2.4

What only real hardware can show for 2.4:
- the second ITE chip of Gigabyte boards;
- the first step of fan control on a Gigabyte board;
- Tiger Lake's memory controller, dumped so its timings can be decoded.

The tests check the reading and the writing with register values made up from the documentation
(Linux `it87`'s and LibreHardwareMonitor's facts). None of it has run on such hardware yet, so a
difference may need a fix, and `clokrix-live.json` holds what that fix needs. Each section is for
a PC that has the part: skip the ones you have no PC for. It can be done with the 2.3 check
([`docs/field-check-2.3.md`](field-check-2.3.md)) on the same build.

Install the test build `clokrix-windows-<commit>` of the latest green CI run on `claude/vyro-2.0`
over 2.3.0. The same artifact has `clokrix-live.exe` and `clokrix-fans.exe`. Have
[HWiNFO](https://www.hwinfo.com/) in sensors-only mode open beside Clokrix to compare, and
[CPU-Z](https://www.cpuid.com/softwares/cpu-z.html) on the laptop.

About 10 minutes a PC, and 10 more for the Gigabyte fans.

## 1. A Gigabyte board with a second chip (if one is at hand)

HWiNFO shows it as a second ITE section ("ITE IT8792E" or "ITE IT87952E") beside the first.

1. Sensors: the first chip's channels as in 2.3, then the second chip's: System Fan 5 / Pump,
   System Fan 6 / Pump and System Fan 4, and the External 2 and System 2 temperatures. On the
   boards LibreHardwareMonitor names (X570 and B550 AORUS, Z390 AORUS PRO, Z690 and Z790 AORUS,
   X870 and X870E AORUS) also its PCIe temperature and a few voltages (DDR VTT, Chipset Core and
   so on); elsewhere its voltages are not shown. A channel of it with no name says "(second
   chip)" after the chip's input name.
2. Against HWiNFO's section for that chip: fans within a few RPM, voltages within 0.02 V (its
   step is taken as 11 mV, as Gigabyte's software reads it; Linux uses 10.9), temperatures within
   1 °C. Note any name that is on the wrong channel.
3. The first chip's readings are as in 2.3.
4. `clokrix-live.json` (run as administrator): `sioRaw.second` holds the second chip's
   registers. `%ProgramData%\Clokrix\service.log` has a line `sio: second chip ITE …`, or
   `sio: second chip not read: …` with the reason.

## 2. A Gigabyte board: fan control, first step

This step writes to the fans, one header for 10 seconds at a time, with you watching. Nothing in
the app drives this board's fans yet (decision 24): what you send names its headers, and then it
goes on the list of boards Clokrix may drive. Only the first chip's fans can be tried: the second
chip's (System Fan 4 to 6 on most boards) are driven by the board's own controller.

1. Quit Gigabyte Control Center (or SIV / EasyTune) and FanControl if they run.
2. Right-click `clokrix-fans.exe` → **Run as administrator**, with nothing after it. It writes
   nothing: it prints the fan main control and fan control registers, then each PWM output by
   number (the `pwm` column, 0 to 5, or 0 to 4) with its control register ("BIOS (0x80)" and so
   on: the board's curve), its duty and the RPM of the fan input with the same number, and saves
   `clokrix-fans.json`.
3. For each output that has a fan on it, open a command prompt as administrator in that folder and
   run `clokrix-fans.exe --try N 40`, N from the `pwm` column. For 10 seconds that output runs at
   40 %, then the board's own settings come back. Note which fan slowed down or sped up (CPU_FAN,
   SYS_FAN1, CPU_OPT and so on, as printed on the board). After each, rename
   `clokrix-fans-try.json` to `clokrix-fans-try-N.json`.
4. If it says the chip's PWM setup looks broken, stop there and send `clokrix-fans.json`.
5. If a fan does not come back, or the window was closed during a test: run
   `clokrix-fans.exe --restore`. A restart also gives the BIOS its settings back.
6. In the app, Cooling still says the board is not validated, and nothing it does changes a fan.

## 3. A Tiger Lake laptop (11th gen Core with a G or H after the number, such as i7-1165G7)

Clokrix does not read this controller's timings yet: the source its fields would come from was out
of reach when 2.4 was built. This step gives what a decoder needs.

1. Memory card → Timings: "not read on this CPU", as before.
2. Right-click `clokrix-live.exe` → **Run as administrator**. In `clokrix-live.json`, `ramRaw.imc`
   has `"layout": "none: raw dump for a decoder"` and a little over 500 registers.
3. In CPU-Z's Memory tab: the type (DDR4 or LPDDR4X), the DRAM frequency, CL, tRCD, tRP, tRAS
   and tRC. A screenshot is enough.

## Report

"Everything is working", or what differs, with each PC's `clokrix-live.json` and board or laptop
model; from the Gigabyte PC `clokrix-fans.json`, the `clokrix-fans-try-N.json` files and which
header each output moved; from the laptop the CPU-Z screenshot.

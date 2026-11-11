# Field check 2.3

What only real hardware can show for 2.3:
- the ITE chip of Gigabyte boards;
- the memory timings on AM4 and on Intel's 10th gen desktop, 11th gen and Core Ultra 200S;
- the first step of fan control on an ASUS board;
- NVAPI's clock offsets on an NVIDIA card whose NVML refuses them.

The tests check the decoding with register values made up from the documentation (ITE's and
Linux's, ZenStates-Core's, memtest86+'s, NVIDIA's SDK). None of it has run on such hardware yet,
so a difference may need a fix, and `clokrix-live.json` holds what that fix needs. Each section
is for a PC that has the part: skip the ones you have no PC for.

Install the test build `clokrix-windows-<commit>` of the latest green CI run on `claude/vyro-2.0`
(or 2.3.0 once it is out) over 2.2.0. The same artifact has `clokrix-live.exe` and
`clokrix-fans.exe`. Have [HWiNFO](https://www.hwinfo.com/) in sensors-only mode open beside
Clokrix to compare, and [ZenTimings](https://zentimings.protonrom.com/) on an AMD PC.

About 15 minutes a PC, and 10 more for the ASUS fans.

## 0. On every PC: what it is, and its raw registers

1. Note the CPU, the board (Hardware → Motherboard) and the memory kit.
2. Right-click `clokrix-live.exe` → **Run as administrator** (Clokrix may stay open: they share
   the buses' locks). It saves `clokrix-live.json` beside itself: keep it. `sioRaw` holds the
   board chip's registers and `ramRaw.imc` the memory controller's, with the `layout` it was
   read as.

## 1. Your PC (MSI B650M, Ryzen 9700X, the NVIDIA card)

Nothing it had should change:
1. Sensors: the fans, voltages and board temperatures as in 2.2.
2. Memory card → Timings as in 2.2 (40-40-40-77 @ 5600 MT/s, tRFC / tRFCsb).
3. Cooling: with control on, one header set to a fixed speed changes that fan, and Return to
   BIOS hands it back.
4. GPU tuning: `%ProgramData%\Clokrix\service.log` has a line `gpu tuning: … offsets through
   NVML` (or `NVAPI`): note which. Turn tuning on and apply Efficient: the GPU card's clocks
   follow, and Stock puts them back. If NVML refused, the log also says `NVML refused the …
   offset (…); NVAPI sets it`. This answers 1.6's open question (does NVML take offsets on
   GeForce) and is that check's first half.

## 2. The ASUS board (the Intel PC): fan control, first step

This step writes to the fans, one header for 10 seconds at a time, with you watching. Nothing in
the app drives this board's fans yet (decision 24): what you send names its headers, and then it
goes on the list of boards Clokrix may drive.

1. Quit Armoury Crate, AI Suite and FanControl if they run (two programs driving one chip fight).
2. Right-click `clokrix-fans.exe` → **Run as administrator**, with nothing after it. It writes
   nothing: it lists each PWM output by number (the `pwm` column, 0 to 6) with its mode, duty and
   the RPM of the fan input with the same number, and saves `clokrix-fans.json`.
3. For each output that has a fan on it, open a command prompt as administrator in that folder and
   run `clokrix-fans.exe --try N 40`, N from the `pwm` column. For 10 seconds that
   output runs at 40 %, then the board's own settings come back. Note which fan slowed down or
   sped up (CPU_FAN, CHA_FAN1, AIO_PUMP and so on, as printed on the board). After each, rename
   `clokrix-fans-try.json` to `clokrix-fans-try-N.json`.
4. If a fan does not come back, or the window was closed during a test: run
   `clokrix-fans.exe --restore`. A restart also gives the BIOS its settings back.
5. In the app, Cooling still says the board is not validated, and nothing it does changes a fan.

## 3. A Gigabyte board (if one is at hand)

1. Sensors (and the Dashboard's board tiles): System, Chipset, CPU, PCIe x16 and VRM MOS
   temperatures where the board has them; CPU Fan and System Fan 1 to 3, the others as "Fan #n";
   Vcore, +3.3V, +12V, +5V, +3.3V Standby, CMOS Battery, AVCC3, and the rest by the chip's input
   names (VIN4 and so on). On X570 and B550 boards, Chipset is the sixth input and the second is
   by its name (TMPIN2); X570's fourth fan is the Chipset Fan.
2. Against HWiNFO's section for the chip ("ITE IT8689E" and so on): fans within a few RPM, +12V,
   +5V and +3.3V within 0.1 V, the temperatures within 1 °C. Note any channel named wrongly.
3. Not read: the second ITE chip many Gigabyte boards have (HWiNFO lists an "IT8792E" or
   "IT87952E" on its own), with a few more fans and temperatures.
4. Cooling: read-only; nothing Clokrix does changes a fan.

## 4. Memory timings (on a PC with one of these)

1. **AM4** (Ryzen 1000 to 5000, DDR4): Memory card → Timings, against ZenTimings: CL-tRCD-tRP-
   tRAS @ MT/s (DDR4-3466 and 3733 as sold, rounded down), GDM and 1T or 2T, and tRC, tRCDWR,
   tRRD S / L, tFAW, tWTR S / L, tWR / tRTP, tCWL, tRFC and tREFI.
2. **Intel 10th gen desktop, 11th gen, Core Ultra 200S**: CL-tRCD-tRP-tRAS @ MT/s against
   HWiNFO's memory section or CPU-Z's Memory tab (the data rate is twice the DRAM frequency
   they show). Only those four show: these controllers give no others to Clokrix.

## Report

"Everything is working", or what differs, with each PC's `clokrix-live.json` and board model, and
from the ASUS PC `clokrix-fans.json`, the `clokrix-fans-try-N.json` files and which header each
output moved.

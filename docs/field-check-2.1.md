# Field check 2.1

What only the owner's Intel PC can show: its CPU's sensors through PawnIO's Intel module, the
ASUS board's Nuvoton chip, and the DDR4 modules. The tests check the decoding, but only with
register values made up from the documentation (Intel's, Linux's drivers, memtest86+'s), and
before this check none of it had run on an Intel CPU, an ASUS board or DDR4: on a PC not checked
yet, a field difference may need a fix, and `vyro-live.json` holds what that fix needs.

Install the test build `vyro-windows-<commit>` of the latest green CI run on `claude/vyro-2.0`
(or 2.1.0 once it is out) over 2.0.4. The same artifact has `vyro-live.exe`. Have
[HWiNFO](https://www.hwinfo.com/) in sensors-only mode open beside Clokrix to compare.

About 20 minutes.

## 0. What the PC is, and its raw registers

1. Note the CPU, the board (Hardware → Motherboard) and the memory kit.
2. Right-click `vyro-live.exe` → **Run as administrator** (Clokrix may stay open: they share the
   buses' locks). It prints a few samples and saves `vyro-live.json` beside itself: keep it. It
   holds the raw registers (`intelRaw`, `sioRaw`, `ramRaw`) to set a decoding right against
   HWiNFO.

## 1. The CPU

1. Dashboard → **CPU sensors**: Package, Hottest core, Package power, Cores power, Core VID and
   Effective clock all show a value; none says "not read on this CPU" or "needs PawnIO".
2. Against HWiNFO's CPU section:
   - Package within 1 °C of "CPU Package";
   - the cores within 1 °C of HWiNFO's per-core temperatures (Sensors view: "P-core #0" and so
     on, then "E-core #n", numbered from 0 across the CPU as Windows numbers cores);
   - Package power within a watt or two of "CPU Package Power", Cores power of "IA Cores Power";
   - Core VID near HWiNFO's "VID" / "Core VID".
3. Under a load (Cinebench, a game): the power climbs to the CPU's limit, and the temperatures
   follow HWiNFO's. If the CPU reaches 100 °C, Health says "CPU is thermal throttling".
4. Power card: the system total shows (CPU and GPU), not "Partial".
5. With the CPU's own graphics in use: Sensors lists "CPU Graphics Power".

## 2. The ASUS board

1. Sensors (and the Dashboard's board tiles): Motherboard and CPU Socket temperatures, Chipset if
   the board reports it, CPU Fan and Chassis Fan 1, the other fans as "Fan #n", and the voltages:
   Vcore, +12V, +5V, +3.3V, AVSB, +3.3V Standby, CMOS Battery, VTT, the rest by the chip's input
   names (VIN0 and so on).
2. Against HWiNFO's section for the board (it names the chip, e.g. "Nuvoton NCT6798D"): fans within
   a few RPM, +12V, +5V and +3.3V within 0.05 V, Motherboard within 1 °C. Note which "Fan #n"
   is which header and which input or AUXTIN reads what HWiNFO calls VRM, PCH, DRAM and so on:
   that names them for this board.
3. Cooling: the fans are shown read-only ("fan control is not validated on this motherboard");
   nothing Clokrix does changes a fan.
4. In `vyro-live.json`, `sioRaw.ioLocked` true means the firmware set the lock on the monitor's
   ports. If `vendor` then reads other than `0x5ca3`, nothing can be read until it is cleared:
   Clokrix's service clears that one bit when it starts, as Linux and LibreHardwareMonitor do, and
   makes no other write to the chip (`vyro-live` itself never clears it).

## 3. DDR4

1. Memory card → **Timings**: CL-tRCD-tRP-tRAS @ MT/s as HWiNFO's memory section or CPU-Z's
   Memory tab gives them (the data rate is twice the DRAM frequency they show). Only those four
   show: Intel's controller registers Clokrix reads give no others.
2. **Module temperatures**: only on modules with a thermal sensor, which many DDR4 kits lack. If
   HWiNFO lists "DIMM[n]" temperatures, the Memory card lists the same slots, within 0.5 °C. If
   it lists none, Clokrix shows none either.

## Report

"Everything is working", or what differs, with `vyro-live.json` and the board's model.

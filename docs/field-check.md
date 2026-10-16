# Field check: V2 (P16 – P27) in one pass

Status: **passed (2026-10-16)** on the reference PC (MS-7E24, Ryzen 7 9700X, RTX 5060 Ti)
with test build `vyro-windows-bab8a30`: sittings 0 to 9; sitting 10 (a clean machine) was not
done. Phases P16 to P27 merge into `v2` in the order below.

Every phase branch is stacked on the one before it, so **one test build holds all eleven**:
- the `vyro-windows-<commit>` artifact of the latest CI run on `v2-p27-polish` (Actions →
  the run → Artifacts);
- unzip it and install the MSI over 0.3.0.

This sheet puts their checks in an order that suits a person at the PC: grouped into sittings,
with the overlaps removed (one stress test serves four phases). Each check names its phase.
A phase can merge into `v2` once all its checks pass. The ROADMAP's per-phase lists stay the
record, and results go there (see the end).

**Merge order:** P16 → P18 → P17 → P20 → P21 → P25 → P22 → P24 → P23 → P26 → P27.

## What to have at hand

- Fan control on, as you use it, with your usual curves.
- HWMonitor, Task Manager and Event Viewer.
- A CPU-only stress test: Cinebench, Prime95 or OCCT's CPU test.
- A game with a heavy CPU load, plus the games you own that use an anti-cheat.
- A phone on the same Wi-Fi that can also switch to mobile data. A second device helps.
- A USB device to plug in, a Bluetooth device, and an Xbox controller if you have one.
- An administrator prompt, for sittings 3 and 7.
- Optional: a clean PC or a virtual machine that never had Vyro (sitting 10).

## 0. Before installing (5 min)

Note what 0.3.0 shows now, to compare after the upgrade:
- the number of alert rules;
- the fan settings and the profile in force;
- the oldest day in History;
- the last game session in Gaming.

## 1. Install over 0.3.0 (10 min)

- [ ] **P27** Settings, alert rules, fan settings and profile, History (7 days back), the
      Health and alert logs, and game sessions with their replays are all still there.
- [ ] **P25** Vyro opens on the Dashboard.
- [ ] **P24** The Dashboard looks as before on Default, with the tiles now four to a row.

## 2. Looks, layouts, devices, My PC (1 h, at the desk, no load)

**Themes (Settings → Appearance):**
- [ ] **P25** Each theme looks right, and the title bar follows it.
- [ ] **P25** Daylight opens without a dark flash.
- [ ] **P25** The dials change colour with the theme at once.
- [ ] **P25** From the tray, Vyro opens on the Dashboard; with "Last tab", where you left it.
- [ ] **P25** Glass costs no more GPU than Midnight at idle (Task Manager).

**Share card (Hardware → Share card):**
- [ ] **P25** A saved card opens in Photos at its size.
- [ ] **P25** Show in folder finds it, and Copy pastes into Discord or Paint.

**Dashboard builder:**
- [ ] **P24** Build a layout:
  - add, duplicate and remove widgets;
  - move one by dragging and by keyboard;
  - resize one by its corner and by keys;
  - change a widget's reading.

  Press Done and restart Vyro: the layout is still there.
- [ ] **P24** Lock a layout: Edit is off until it is unlocked. Reset puts a preset back.
- [ ] **P24** With 40 widgets on a layout, Vyro's memory and CPU in Task Manager stay near
      what they were.

**Devices:**
- [ ] **P22** Every keyboard, mouse, headset and controller in use is listed once, with the
      right kind and connection. Monitors, hubs and the PC's own parts are not listed.
- [ ] **P22** A USB device plugged in and out appears and goes within a couple of seconds.
- [ ] **P22** A Bluetooth device turned off and on goes to "Not connected", then "Connected".
- [ ] **P22** Battery matches Windows Settings › Bluetooth & devices. An Xbox controller
      shows a level, and a 2.4 GHz receiver shows "—".
- [ ] **P22** Sound lists the outputs and inputs and marks the defaults. An empty front jack
      says "nothing plugged in".
- [ ] **P22** With the tab closed, Vyro's CPU in Task Manager is unchanged, and the tab opens
      at once.

**My PC:**
- [ ] **P23** The PC view shows your cooler type, RAM sticks, GPU, drives and fans, each
      where expected, and the labels match the Sensors tab.
- [ ] **P23** The GPU looks like your card, or can be corrected and stays corrected after a
      restart.
- [ ] **P23** Turning 3-D off gives the plan.

**Keyboard and screen reader:**
- [ ] **P27** Use Narrator on two tabs. Tab through Settings and Cooling.

## 3. Diagnostics and damaged files (30 min)

**Health → Diagnostics:**
- [ ] **P21** A run completes in about a second, and its values match History, Storage and
      Event Viewer.
- [ ] **P21** Each of the four exports opens (the PDF in Edge) and shows the on-screen values.
- [ ] **P21** An export without serials contains none; with them it contains the drives' and
      the board's.
- [ ] **P21** Unplug a case fan for a minute: Fans and pump turns to Attention after the
      Health event.
- [ ] **P25** Meanwhile the stopped fan is a Critical Health problem: the rail under the
      header pulses four times, then holds.
- [ ] **P21** Stop the service (`sc stop VyroService`) and run again: Service and Sensors turn
      to Attention. Then `sc start VyroService`.

**Damaged files:** quit Vyro first.
- [ ] **P27** Put `{` in `%APPDATA%\app.vyro.monitor\settings.json` and start Vyro. It starts
      on the defaults, and `settings.bad.json` is beside the file.
- [ ] **P27** Put `"temperatureUnit": "k"` in a good file: only the unit resets.

## 4. Under load (1 h; fan control on, HWMonitor open)

First make three alert rules (Health tab):
1. **"CPU and GPU busy":** all of CPU usage above 50 % and GPU usage above 50 %, held 10 s,
   with a notification.
2. **"CPU heating fast":** CPU temperature rises by 15 °C within 30 s. Give it a notification,
   fans +20 and a diagnostic record.
3. **"CPU warm":** CPU temperature above your idle temperature + 20 °C. Give it a notification
   and the Work profile. A profile has its own fan settings, so it is kept off rule 2: a fan
   change then has only one cause.

Then run a CPU-only stress test for 10 minutes:
- [ ] **P18** "CPU heating fast" fires when the test starts.
- [ ] **P18** "CPU and GPU busy" stays quiet.
- [ ] **P18** "CPU warm" fires within a minute of "CPU heating fast" and is logged as a
      repeat, with one notification for both.
- [ ] **P18** The diagnostic record names the stress test as the busy program.
- [ ] **P18** The fans Vyro drives rise; the Cooling log and readings show it, and headers
      left to the BIOS do not change. They go back 30 s after the rule clears.
- [ ] **P18** The profile switches to Work (tray check mark) and back. Picking another
      profile meanwhile keeps your pick.
- [ ] **P18** With fan control off, the same rule changes no fan.
- [ ] **P16** Insights shows headroom, but no "hotter than usual".
- [ ] **P23** In My PC, the fans spin faster, and the thermal map turns amber or red near the
      Health limits.

Lower a fan curve and run the stress test again:
- [ ] **P16** The Silent profile, or the lower curve, is shown as the explanation.
- [ ] **P17** It shows in Throttling or Sustained clocks (on the next day's score).

Benchmark:
- [ ] **P20** A Quick run completes in about 3 minutes, and a second scores within noise of
      the first.
- [ ] **P20** The CPU multi-thread peak temperature and power match HWMonitor.
- [ ] **P20** The GPU areas name the RTX 5060 Ti (not the iGPU), and Graphics holds it at
      full load.
- [ ] **P20** Stop, closing the window and quitting each end all load within a second (Task
      Manager), and leave no `vyro-bench-*.tmp` in `%TEMP%`.
- [ ] **P20** A Full run's Sustained loops read steady.

## 5. A gaming evening (overlay on, fan control on)

- [ ] **P18** A game with a heavy CPU load fires "CPU and GPU busy".
- [ ] **P16** Start a background job during the game, such as a large copy or an update. Its
      program is named in Insights.
- [ ] **P24** Switching between the Dashboard presets while the game runs works, and Gaming
      shows the frame times.
- [ ] **P27** Play each anti-cheat game you own (Easy Anti-Cheat, BattlEye, Vanguard, FACEIT)
      with Vyro running, the overlay on and fan control on. Fill in the table in
      [`anti-cheat.md`](anti-cheat.md).
- [ ] **P17** The next day: the score's thermal headroom matches the History of this evening.

## 6. Phone companion (30 min; phone on the same Wi-Fi)

- [ ] **P26** Settings → Remote → Turn on. Windows asks about the firewall: allow Private.
      The card then says the firewall allows Vyro, and the header chip shows.
- [ ] **P26** Scan the QR code with the phone and pair. The tiles update every second, in
      Vyro's theme and unit, and the chip says "1 watching".
- [ ] **P26** Type the code by hand on a second device. Five wrong codes spend it.
- [ ] **P26** Sign the phone out in Vyro: its page goes back to pairing within seconds.
- [ ] **P26** Close the window to the tray: the phone keeps updating every second.
- [ ] **P26** Turn the companion off: the phone shows "Can't reach Vyro". Turn it back on:
      the phone reconnects without pairing again.
- [ ] **P26** On mobile data instead of the Wi-Fi, the address does not answer.
- [ ] **P26** Note Vyro's memory and CPU in Task Manager with the companion off, on, and with
      a phone watching.

## 7. Fans after a crash (20 min; administrator prompt; fan control on)

This is a hardware write. Stay at the PC until the BIOS curve is back.
- [ ] **P27** `taskkill /F /IM vyro-service.exe`: the BIOS curve comes back after the restart,
      as in P12.
- [ ] **P27** The damaged case:
  1. Pause the automatic restart with `sc failure VyroService reset= 0 actions= ""`.
  2. Kill the service again.
  3. Replace `%ProgramData%\Vyro\fan-restore.json` with `{`.
  4. Run `sc start VyroService`.

  Expected:
  - the fans Vyro drove go to full speed;
  - Cooling says to restart the PC;
  - `fan-restore.bad.json` exists.

  Restart the PC: the BIOS curve is back. Put the restart actions back with
  `sc failure VyroService reset= 86400 actions= restart/5000/restart/5000/restart/60000`
  (reinstalling also does it).
- [ ] **P27** Every MS-7E24 row of [`control-safety.md`](control-safety.md) still holds on
      this build. The P18 boost row is covered by sitting 4.

## 8. Footprint for 1.0 (45 min, after a restart)

- [ ] **P27** Follow "Field measurement for 1.0" in [`footprint.md`](footprint.md): idle,
      hidden, every tab, on demand, gaming. The results go into the README's Performance
      table.

The same numbers also answer:
- [ ] **P16** Footprint within budget (+2 MB of 0.3.0).
- [ ] **P23** Vyro's CPU while My PC spins its fans; the WebView's GPU process memory before,
      during and after the tab (it drops back after leaving it).

## 9. Over the next days

- [ ] **P16** Insights stays quiet at normal desktop use.
- [ ] **P16** The badges, "Show in History" and the tab dot work.
- [ ] **P17** After a day, the score appears with all the inputs that apply.
- [ ] **P17** The stability counts match the System log in Event Viewer (Kernel-Power 41,
      WHEA-Logger).
- [ ] **P17** The daily point appears in the trend the next day.
- [ ] **P20** A run after a GPU driver update lists the change (whenever one comes).
- [ ] **P27** One pass through every V1 feature:
  - sensors, history, Health;
  - alerts and a toast;
  - processes, storage SMART, network and ping;
  - displays, system;
  - gaming capture and the overlay;
  - the tray and updates.

## 10. A clean PC or virtual machine (30 min, optional but wanted for 1.0)

On a PC that never had Vyro:
- [ ] **P27** Install, run, and accept PawnIO. The app shows readings within 2 s.
- [ ] **P27** Uninstall: the service and program files go, and `%ProgramData%\Vyro` stays.

CI's Windows runner already does this on every run, but not with a person watching or a real
board.

## Defaults to confirm

Each phase lists what was chosen without you under "Defaults chosen at build" in the
ROADMAP. Say yes or what to change:

| Phase | Most worth a look |
|---|---|
| P18 | Actions end 30 s after a rule clears; fans only raised, 5–50 points; one notification per part a minute |
| P20 | Quick about 3 min and Full about 13 min; a 1 GiB storage file; the stops |
| P21 | Passive checks only; reports in `Documents\Vyro Reports`; serials only when asked |
| P25 | The six themes and five accents; open on the Dashboard; card sizes |
| P22 | What is listed and left out; battery sources; no low-battery alert yet |
| P24 | A flow grid rather than free placement; a lock per layout; 12 layouts × 40 widgets |
| P23 | Stylized models; 30 fps cap; the thermal colours |
| P26 | Port 8976; 30-day sessions; plain HTTP; served by the app, not the service |
| P27 | The size budgets (125 kB and 32 kB); full speed when the saved fan state is unreadable; a damaged history kept aside, not deleted |

P16 and P17 were your own choices on 2026-10-15.

## Recording results and merging

- **A phase passes.** Its ROADMAP entry gets
  "Field check (MS-7E24, `<date>`, test build `vyro-windows-<commit>`)", and the phase merges
  into `v2` in the order above.
- **A check fails.** Note it in that phase's ROADMAP entry. The fix goes on that phase's
  branch and is carried up the stack, as the P20 GPU-name fix was. Only the phases from there
  on need that check again.
- **Releases.**
  - Merged up to P21: 0.4 can ship.
  - Up to P26: 0.5 can ship.
  - P27: 1.0.

  The owner chose straight to 1.0 (2026-10-16): no 0.4 or 0.5.

  A release built from a point lower in the stack is a different build from the one checked
  here, so give its MSI a short smoke pass: install over the last release, open every tab,
  and turn fan control on and off.

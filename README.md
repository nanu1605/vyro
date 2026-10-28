# Vyro

A low-footprint Windows hardware monitor: animated CPU/GPU speedometers, per-subsystem
sensors, history, health and alerts, with direct sensor access (no third-party monitoring
app or library).

- **Build spec:** [`PROJECT_PLAN.md`](PROJECT_PLAN.md) — what to build and why.
- **Roadmap:** [`ROADMAP.md`](ROADMAP.md) — milestones, phase tracker, decision log.

**Status:** 2.0. Live CPU, GPU, board, fan, voltage, RAM, storage and network sensors on
Windows (low-level ones through PawnIO, read by the Vyro Sensor Service), history, System
Health, alerts, game frame rates (PresentMon), tray and an MSI installer. New in 2.0:
[notifications on your phone](#notifications-20) with the app closed (alerts, low batteries,
game summaries and the PC going quiet), and [GPU tuning](#gpu-tuning-16) for AMD Radeon and
Intel Arc cards too. New in 1.7: the
[phone app](#phone-app-from-anywhere-14) shows the PC's screen and uses it, for a phone you
allow, straight to the PC where it can and a picture at a time through the relay where it
cannot, in full screen and in HD or SD; the phone's voice reaches the PC as a clip to paste or as
its microphone; and the app is redesigned. New in 1.6:
[GPU tuning](#gpu-tuning-16) for NVIDIA RTX 20 to RTX 50 cards, with presets made for the card,
your own settings, a stability test and "Find my best". New in 1.5: the
[phone app](#phone-app-from-anywhere-14) shows what runs on the PC, its devices and Health, and,
if you allow it on the PC, changes it: end a program, shut down, restart, sleep or lock, wake
another PC, switch profiles, run a benchmark or diagnostics. New in 1.4.1:
Bluetooth Connect and Disconnect that work for headphones and speakers, a device's connection
as Windows Settings shows it, and a low battery on the Dashboard. New in 1.4: a
[phone app](#phone-app-from-anywhere-14) for Android and iPhone shows the PC's readings from any
network, through the Vyro relay, encrypted end to end. Also new in 1.4: every device named as
Windows Settings names it, the batteries of Logitech, PlayStation and Xbox wireless devices,
Connect and Disconnect for Bluetooth devices, and an accent of any colour. 1.2:
[desktop widgets](#desktop-widgets-11-12) stay in view through Show desktop, snap to the
screen's edges and to each other, can let clicks through, and come in three new kinds (Network,
Game FPS, Top processes); My PC shows its airflow as smoke lines; and Dashboard cards pack
without blank space. 1.1 added a new look (a sidebar, a command palette and the whole width of
the window), a livelier 3-D My PC with a laptop model, desktop widgets and a smoother overlay
graph. 1.0 added
Insights with a Performance Score, alerts with several conditions and actions, a benchmark,
one-click diagnostics and reports, themes, a dashboard builder, a My PC view, a Devices tab and
a phone companion (the sections marked (1.0) below). 0.3 added automatic game profiles, a
configurable in-game overlay and a replay of each game session (see [Gaming](#gaming)). 0.2
added fan and pump control, so far only on the MSI B650M GAMING PLUS WIFI (MS-7E24). See
[`ROADMAP.md`](ROADMAP.md) for what is verified on which hardware.
Contributions: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Install

Releases are published by the Release workflow (see [CONTRIBUTING](CONTRIBUTING.md#releasing)):
install `Vyro_<version>_x64_en-US.msi` from the latest release. Installed copies look for a new
release daily and update themselves from Settings → Updates (one administrator prompt; app and
service are both replaced). The first time Vyro starts after an update it shows what's new in
that version, once; Settings → Updates opens it again. Every CI run also uploads a test build,
`vyro-windows-<commit>`, which never updates itself.

The MSI installs Vyro for all users with a Start-menu entry, installs the signed
[PawnIO](https://github.com/namazso/PawnIO) driver silently if it is missing (it reads CPU
temperature and power, fans, voltages and RAM sensors; it stays installed when Vyro is removed),
and installs the **Vyro Sensor Service**. The service runs the sensor engine with the rights
PawnIO and SMART need, keeps history while the app is closed, and serves the app over a local
named pipe; the app itself runs without administrator rights. Intel's
[PresentMon](https://github.com/GameTechDev/PresentMon) console app is installed beside the
service, which runs it to measure the game in front (nothing is injected into games). Without the service the app still
starts, reads what it can, and says what is missing. "Start with Windows" (Settings) opens it
in the tray at sign-in. Builds are not code-signed yet, so SmartScreen will warn; from 2.0 a
release signs itself once SignPath Foundation has taken the project on
([`docs/signing.md`](docs/signing.md)).

## Fan and pump control

The **Cooling** tab sets each fan header to the BIOS curve, a fixed speed or Vyro's own curve
on any temperature sensor, per profile. It is off until you turn it on and confirm. Control runs
in the service, so it keeps going with the app closed. Safeguards:

- Fans never go below 20 % and pumps never below 60 %. A pump header is driven only after you
  confirm a pump is on it, and a pump that stops reporting a speed ends control.
- CPU at 95 °C or GPU at 90 °C (both adjustable), or a source sensor without a reading, sends
  every controlled fan to 100 %.
- The BIOS settings are saved before the first change. They are put back on Return to BIOS
  (Cooling, the top bar's chip or the tray), when the service stops, before sleep, on uninstall, when
  a write doesn't read back, when another program takes over the fans, and at the next start
  after a crash. If the control loop stalls for 10 s, the BIOS takes over until it recovers.

Only boards that have been checked on real hardware can be controlled: today the MSI B650M
GAMING PLUS WIFI (MS-7E24, Nuvoton NCT6687D). Every other board shows its fans read-only.
[`docs/control-safety.md`](docs/control-safety.md) lists every safety rule with the code that
enforces it, its tests and its result on that board.

## GPU tuning (1.6)

The **GPU tuning** view changes a card's core and memory clocks and its power limit: NVIDIA RTX
20 to RTX 50 cards, and from 2.0 AMD Radeon RX 6000 to RX 9000 and Intel Arc A- and B-series
cards, desktop and laptop. It is off until you turn it on and confirm, and it runs in the
service through the maker's own driver interface, as the maker's tools do: NVML for NVIDIA,
ADLX for AMD (as AMD Software), the Intel Graphics Control Library for Intel. Intel's driver
offers no memory clock, and an Intel card's warranty waiver is set when you turn tuning on, as
Intel asks of every tuning tool. The AMD and Intel parts have not yet run on a real card:
[`docs/field-check-2.0.md`](docs/field-check-2.0.md).

- **Presets** made for the card's family and scaled to what its driver allows: Quiet (lower
  power limit), Efficient (nearly the same speed for less power) and Performance (the highest
  power limit and modest offsets). Your own settings with sliders kept within the driver's
  ranges, saved as presets. A preset per profile, so Gaming can run Performance and Silent can
  run Quiet. A laptop's driver often fixes the power limit: only what it allows is offered.
- **Test** runs a 5-minute stability test: full load with every result checked, a frame that
  must come out the same every time, and a pattern over most of the free video memory. Settings
  that pass are marked tested; only tested settings are applied when Vyro starts.
- **Find my best** raises the core clock, then the memory clock, step by step while the test
  passes and memory stays fast, keeps a margin, tests the result for 5 minutes and saves it as
  "Best for this card". About 20 to 40 minutes.
- **Safeguards.** Stock again at 90 °C (105 °C hotspot), before sleep, when the service stops
  and on uninstall. After a crash or a display driver reset while tuned, Vyro starts the card at
  stock and turns "Apply at start" off. If another program keeps changing the clocks, Vyro stops
  tuning. "Return GPU to stock" is in the top bar and the tray. From the phone, only Stock and
  tested presets can be applied. [`docs/control-safety.md`](docs/control-safety.md) lists each
  rule with its code and tests.

Higher clocks can crash games or the PC, and may affect the card's warranty. Graphics built
into a processor are not tuned.

## Gaming

- **Automatic game profiles.** A fullscreen or borderless game switches Vyro to the Gaming
  profile, or to the settings saved for that game (profile or "Don't switch", whose fan
  settings, overlay, update rate 0.5–5 s), and everything goes back when it closes. A saved
  game is also recognised in a window; an opt-in trigger also counts an app using at least 60 %
  of the GPU for 10 s. A profile you pick during a game wins until the game closes; the Gaming
  tab offers "Keep Desktop" and "Not a game". Settings → Gaming turns it off.
- **Overlay.** Minimal, Performance, Full and Graph modes; corner and offset, size, background
  opacity, your own choice of readings, refresh every 0.5 / 1 / 2 s, colour and a compact
  layout (Settings → Overlay, with a live preview). It is click-through, never takes focus,
  and cannot show over exclusive fullscreen.
- **Session replay.** The Gaming tab keeps every reading of the last 100 sessions, inside the
  200 MB history cap: frame rate, frame time, use, temperatures and power on one time axis,
  with markers for frame-rate drops, stutters, thermal and power limits, switching away and
  back, profile changes and the overlay shown or hidden. Any moment also shows a minute either
  side of the PC's sensor history, while that history is kept (7 days).

0.3 moves the history database (`%ProgramData%\Vyro\history.sqlite`) to a new format that 0.2
cannot read: after going back to 0.2 its history stays off until that file is deleted. 0.2's
session log (`games.json`) is imported once, as summaries without a replay.

1.0 adds tables to the history database (schema v3), which 0.3 cannot read: after going back to
0.3 its history stays off until that file is deleted. If any alert rule uses 1.0's conditions
or actions, 0.3 moves `alerts.json` aside (to `alerts.json.bad`) and uses its default rules.

## Insights, Performance Score and alerts (1.0)

- **Insights.** A view after Health that compares the last half hour with this PC's own past
  week at the same load, and says what changed:
  - running hotter than usual, or a fan slower than usual;
  - slowed by heat, or close to a temperature limit;
  - a game slower than its earlier sessions, or what set its frame rate;
  - a program busy in the background during a game.

  Each finding shows its evidence ("Show in History"), whether it was measured or inferred,
  and how sure Vyro is. A cause is called likely only when it can produce the effect, came
  just before it began, and is the only candidate. Hardware and driver changes, profile
  switches, fan settings and busy programs are the candidates. Anything else that merely
  happened around the same time is listed apart. A change you made yourself, such as the
  Silent profile, is shown as the explanation, not as a warning. The check runs every 10
  minutes and puts a dot on the tab when something is worth a look; it never notifies. To
  spot background load, the names of the five busiest programs of each minute are kept with
  the history for 7 days (names only).
- **Performance Score.** At the top of Insights. One number from:
  - thermal headroom and sustained clocks;
  - throttling and cooling;
  - drive health and free space;
  - memory use and the network;
  - stability: unexpected shutdowns and hardware (WHEA) errors in the System log.

  Each input shows its sub-score, its weight and the values behind it. The score appears
  after about a day of history, is recorded daily, and shows a year's trend. The formula is
  fixed and versioned: [`docs/performance-score.md`](docs/performance-score.md).
- **Alerts.** A rule (Health tab) can combine up to five conditions, all of them or any one:
  - above or below a limit;
  - rising or falling by an amount within a set time;
  - above or below the sensor's usual reading. For a CPU or GPU temperature, "usual" means
    usual for the load at that moment.

  A hold time, a cooldown and a clearing margin keep a reading that hovers at the limit from
  flapping. Besides a notification (with or without sound, or none), a rule can:
  - put its name in the tray tooltip;
  - switch the profile;
  - raise the fans Vyro drives by 5–50 points;
  - save a diagnostic record: the readings and the five busiest program names.

  The profile, tray and fan actions end 30 s after the conditions clear, and a profile you
  pick meanwhile wins. Fans are raised only while fan control is on, never on a header left
  to the BIOS, and the safety full-speed rules still come first. Two alerts about the same
  part within a minute notify once; a hardware problem always notifies.

## Benchmark and diagnostics (1.0)

- **Benchmark.** A tab that measures this PC against itself. It covers:
  - CPU single- and multi-thread;
  - memory and storage;
  - GPU compute and graphics (Direct3D 11 on the high-performance GPU).

  Quick takes about 3 minutes. Full, about 13 minutes, adds ten one-minute loops of CPU and
  GPU load together to show whether performance holds. Each area runs a warm-up and three
  measured passes, and reports the median and the spread, with temperatures, power and
  throttling. The run is compared with your last one, and anything that changed in between is
  listed: setup, drivers, hardware. Every result is checked against known checksums and data
  read back, so an unstable PC shows up as errors, not as a fast score.

  A run stops on Stop, on closing the window, at Health's problem limits, when a game starts,
  or when readings go missing; all load ends within a second. The storage test uses a 1 GiB
  temporary file, deleted afterwards. Scores compare this PC with itself, not with other tools
  or PCs: [`docs/benchmark.md`](docs/benchmark.md).
- **Diagnostics.** "Run diagnostics" in Health checks 13 areas in about a second, without
  loading the PC: thermals, clock stability, power limits, throttling, fans and pump, memory,
  storage, network, hardware errors, changes from usual, drivers, sensors and the service.
  - Each area is Pass, Attention, Info or Unavailable.
  - Every Attention cites the readings behind it, with a link into History.
  - A report exports as HTML, JSON, Markdown or PDF to `Documents\Vyro Reports`.
  - Serial numbers are left out unless you tick "Include serial numbers".

  More in [`docs/diagnostics.md`](docs/diagnostics.md).

## Looks, layouts and your PC

- **The frame (1.1).** A sidebar groups the views (Overview, Performance, This PC, Vyro) and
  folds to a rail of icons, by itself on a narrow window. **Ctrl+K** opens a search for any
  view, setting, sensor or action. On a window 1500 px or wider the Dashboard has a live rail
  beside it: vitals with their last minute, the running game, network, the top processes and
  Health (it can be folded away). Cards lift a little under the mouse, views rise in, and the
  profile's animation level ("calmer" or off) and Windows' reduced-motion setting turn that
  down.
- **Themes.** Settings → Appearance offers Midnight, AMOLED, Cyber, Glass, Minimal and
  Daylight, each with its own accent or one of five more; the accent colours the sidebar,
  buttons, switches and the 3-D model. **Custom** (1.4) takes any colour from a colour wheel
  (or typed as `#rrggbb`): gauges, graphs and the 3-D model show it as picked, and text in the
  accent takes a lighter or darker shade of it where the colour alone would be hard to read.
  OK, warning and problem colours keep their meaning in every theme, and every theme and
  accent meets WCAG AA contrast. A thin line along the top bar shows the
  PC's state: idle, gaming, warning or critical. Critical pulses four times, then holds; it
  does not pulse with reduced motion. Vyro opens on the Dashboard, or on the last view if you
  choose.
- **Dashboard builder.** "Edit layout" on the Dashboard adds, moves, resizes, duplicates and
  removes widgets, with the mouse or the keyboard. Widgets:
  - gauges, and any of about 35 readings;
  - a minute's graph, or an hour of history;
  - the game's frame times;
  - Health, events and the familiar cards.

  Up to 12 layouts can be saved, with a picker. Seven presets ship: Default, Gaming, Work,
  Streaming, Benchmark, Monitoring and Minimal. A layout can be locked, and a preset reset.
  Each widget is as tall as it needs (at least the size you gave it), and the next one moves up
  into the room a short card leaves, so no card stretches into blank space (1.2).
- **My PC.** Your PC in 3-D, with three views:
  - your PC: a tower with glass, lit fans, your cooler, RAM sticks, GPU, drives and cables;
    or, on a laptop, the laptop with its lid open, heat pipes and blower fans (1.1);
  - your GPU (on a tower);
  - a thermal map, coloured by how close each part is to its Health limits.

  Fans spin at their reported speed; a fan with no reading turns slowly and says "Speed not
  reported" (a board fan header that has never turned counts as no reading: it is empty, or its
  fans run from a hub). The view fills the window and frames the whole PC at any size. Hover a
  part to name it, click it for its details, double-click to fly to it. "Explode" pulls the
  parts apart. "Airflow" draws the air through the case as a wind tunnel's smoke lines: in at
  the front fans, round the graphics card and the RAM, out through the rear fan and the
  radiator, warming from blue to orange as it passes the CPU's cooler and the GPU (1.2). On a
  laptop "See inside" makes the keyboard see-through. Vyro tells a laptop from a desktop by
  the firmware's chassis type (Settings → My PC → Model corrects it). If the GPU does not look
  like your card, you can correct it. The models are Vyro's own, with no brand logos. The 3-D
  view uses WebGL only while the view is shown, at 60 frames a second (30 with "calmer"
  animation, still with it off), and a flat 2-D plan is there instead if you prefer.
- **Devices.** A tab after Displays lists keyboards, mice, controllers, headsets and other
  USB and Bluetooth devices, each once, by the name Windows Settings shows (1.4), with how it
  connects. It shows the battery, and whether it is charging, where the device reports it:
  Bluetooth and Bluetooth LE, Xbox controllers, PlayStation controllers on a cable, and
  Logitech devices, each device on a Logitech receiver in its own row (1.4). Other makers'
  2.4 GHz receivers keep it to their own software; `vyro-inventory` names the dongle so its
  protocol can be added. A paired Bluetooth device is Connected or Not connected as Windows
  Settings shows it (1.4.1); **Paired** means Windows doesn't say. It has **Connect** and
  **Disconnect** (1.4): audio devices as Windows Settings does, every audio profile of them
  (1.4.1); other devices through the Vyro service, and they may connect again by themselves as
  soon as they are used. After either, the row says what Windows then showed. A battery under
  20 % (red under 10 %) that is not charging shows as a strip on the Dashboard and a dot on
  Devices (1.4.1); Vyro reads the devices in the background for it every 5 minutes and when
  one comes or goes. It also lists the sound outputs and inputs, with the defaults. The list
  updates within a second when a device arrives, leaves, connects or disconnects. Device names
  stay on this PC: they are not put in reports, cards or the history.
- **Share card.** In Hardware: a picture of your PC's specs and Performance Score, in 16:9,
  square or story size. It is saved to `Pictures\Vyro` or copied to the clipboard, and never
  shows the computer's name or serial numbers.

## Desktop widgets (1.1, 1.2)

The Widgets page (in the sidebar, under Vyro) puts live readings on your desktop, up to eight
widgets:

- **Gauge:** one reading on a dial.
- **Reading:** one reading, large.
- **Graph:** one reading and its last minute.
- **Stack:** up to six readings, one under the other, with bars.
- **Clock:** the time and date, with up to two readings, one a row.
- **Network** (1.2): download and upload, and their last minute.
- **Game FPS** (1.2): the running game's frame rate, 1% low and frame time, and the last
  minute; "No game running" otherwise.
- **Top processes** (1.2): the five busiest programs by CPU, memory or GPU, a program's
  processes added up ("chrome.exe ×12").

Any reading the Dashboard has can go on the first five. Each widget has a size (50–200 %), a
background strength, a style (the app's card, see-through glass, or solid) and a place: behind
your windows (the default) or on top of them. Right-click a widget to lock it, edit it or
remove it. "Desktop widgets" in the tray icon's menu shows or hides them all, and they come back
when Vyro starts.

- **Moving.** Drag an unlocked widget; Vyro remembers where it is. It snaps to the edges of the
  screen and to other widgets, beside them with a small gap or in line with them, and comes
  away as soon as you drag on; hold Shift to place it freely (1.2). A new widget goes where no
  other widget is, down the right edge of the screen.
- **Clicks pass through** (1.2): a locked widget can ignore the mouse, so clicks reach the
  window or the desktop under it. Unlocking it on the Widgets page makes it catch the mouse
  again.

Widgets follow your theme and accent, and update at your profile's rate while the window is
in the tray. Widgets behind your windows stay in view when you use Show desktop (Win+D) and go
back under your windows when you switch to one (1.1.1); "On top" keeps a widget over
everything.

## Phone app, from anywhere (1.4)

**Off by default.** Settings → Remote → **From anywhere** lets your phone show this PC's
readings on any network: at home, on mobile data, at work. It is an app you install from the
browser, on Android and iPhone, with no app store. A phone only looks unless you let it change
the PC (1.5).

- **How it connects.** The PC keeps one connection to the Vyro relay, a small service on
  Cloudflare, and the phone reaches it there: no port is opened on the PC, and no router is set
  up. The PC sends readings only while a phone has the app open.
- **Encrypted end to end.** Readings are sealed on the PC with a key only that phone has, and
  opened on the phone. The relay cannot read them; it sees when the PC and a phone connect, from
  which addresses, and how much they send. [`docs/remote.md`](docs/remote.md) has the details,
  what to trust, and how to run a relay of your own.
- **Pairing.** "Pair a phone" shows a QR code; scan it with the phone's camera.
  - On Android or another computer, the app opens and pairs; install it from the browser's
    menu, or its Install button.
  - On iPhone, add it to the Home Screen first (Share → Add to Home Screen), open it there and
    scan again: a Home Screen app keeps its own storage, apart from Safari's.
  - The code lasts 10 minutes and works once. Up to 8 phones; each stays paired while it is used
    at least once in the chosen time (30 days by default). Settings signs any phone out at once.
- **The app** shows the same readings as the companion, from several PCs if you pair them, and
  says when a PC is asleep or off. It opens without a network too, to show where things stand.
  From 1.5 its tabs also show what runs on the PC (Processes), its Devices with their batteries,
  and Health with alerts; More has the PC's profile, fans, benchmark, diagnostics and updates.
- **Changing the PC from the phone (1.5).** When pairing, Settings asks whether the phone may
  only look or also change the PC; the paired list changes it any time. A phone allowed to may
  end a program (never Windows' own or Vyro), shut down or restart (after 30 seconds unless
  "now", with a banner and Cancel on the PC), sleep, hibernate, lock or sign out, switch the
  profile, give the fans back to the BIOS, turn alerts on or off, run a benchmark or
  diagnostics, connect or disconnect a Bluetooth device, apply Stock or a tested GPU preset
  (1.6), and update Vyro. Turning fan control on, confirming a pump and GPU tuning's own values
  and tests stay at the PC. Settings lists what phones changed. After a restart,
  the phone reaches the PC again once someone signs in.
- **The PC's screen on the phone (1.7).** A phone you mark "Sees the screen" in Settings →
  Remote (apart from changing the PC) can watch the PC's screen in its Screen tab and use it: tap
  to click, hold for a right click, drag, two fingers to scroll, pinch to zoom, a key bar and a
  box to type into, and a choice of monitor. It connects straight to the PC where it can (fast,
  encrypted end to end, the relay only introduces them), or (2.0.1) through a TURN server
  where the relay's owner turned one on for the PC, still end to end and without a time limit
  ([remote.md](docs/remote.md#a-turn-server-for-the-screen-201)); where it cannot, a slow view
  sends a picture at a time through the relay, 20 minutes a day. The picture follows the
  connection: on a slow one (mobile data) it is sent smaller and less often so it stays
  current. One phone at a time; the PC shows a banner and a tray item with Stop while it
  lasts, and a session left alone ends after 10 minutes. Windows' sign-in and administrator
  prompts cannot be seen or used, and Ctrl+Alt+Del cannot be sent. Optionally the PC locks when a phone's session ends.
  **Full** fills the phone with the PC's screen (sideways for a wide one on Android); **HD** or
  **SD** chooses the quality, and on mobile data the phone asks first (SD uses about 7 MB a
  minute at most, HD up to 35).
- **Your voice from the phone (1.7).** A WhatsApp Web voice note, or any app on the PC, records
  the PC's microphone, not the phone's. Two ways around it, from the Screen tab:
  - **Clip**: record on the phone and send it; it lands on the PC's clipboard as an audio file
    (kept in `Phone clips` for a week), so **Paste** (or Ctrl+V on the PC) attaches it to a chat.
    Nothing to install.
  - **Mic**: the phone becomes the PC's microphone while it is on, so WhatsApp's own voice-note
    button, calls and recorders hear the phone. It needs a virtual audio cable on the PC: install
    the free [VB-CABLE](https://vb-audio.com/Cable/) (run its setup as administrator, then
    restart). Vyro finds it, makes its "CABLE Output" Windows' default microphone while the
    phone's is on (Settings → Remote can turn that off) and puts your microphone back after.
    Direct connections only; Settings → Remote says whether a cable was found.
- **The app's look (1.7).** Tabs at the foot of the screen, sheets that rise from below, a little
  motion (none with the phone's reduced motion), and nothing wider than the phone.
- **Waking the PC (1.5).** A phone cannot send the wake signal itself (web pages cannot send it),
  so another Vyro PC on the same network sends it: turn on **Wake from phone** in Settings on
  the PC to be woken, and keep another PC paired with the phone. Settings says what Windows
  shows about the wired adapter and Fast Startup; the BIOS needs "Wake on LAN" on. A router with
  a Wake-on-LAN page, or a smart plug with the BIOS set to power on after power loss, also work.
- **Limits.** Networks that block `workers.dev` or allow the web only through a proxy cannot
  reach the relay. The relay runs on Cloudflare's free plan, shared by everyone: past its
  daily limit it pauses until midnight UTC. Alerts are not sent to phones yet.

### Notifications (2.0)

**On your phone, with the app closed.** On the phone, More → **Notifications** turns them on
for that PC and picks what they are about: alerts (as Windows shows them), a device's battery
running low, a game's summary when it ends, and the PC **stopping answering**: its connection
ended without it shutting down, restarting or going to sleep (a crash, a power cut, a lost
network) and it was not back within a few minutes; you hear again when it is back. "Send a test"
shows one at once. On iPhone they come only to Vyro on the Home Screen (iOS 16.4 or later).

- **Sealed for the phone.** The phone's browser gives the PC, sealed like everything else, the
  address its push service (Google's, Apple's, Mozilla's or Microsoft's) takes notifications at
  and two keys. The PC seals each notification for those keys (the Web Push standard, RFC 8291)
  and the relay posts it there, signed with a key of its own: neither the relay nor the push
  service can read it. The relay posts only to those push services, for phones the PC has, a
  few at once and at most 300 a day per PC.
- **"Stopped answering"** is a notification the PC seals ahead of time and leaves with the
  relay, which sends it only if the PC goes quiet without saying it is leaving. Vyro says so as
  it quits and as Windows shuts down, restarts, signs out or goes to sleep.
- Settings → Remote says which phones get notifications; each phone turns them off for itself,
  and signing a phone out ends them. [`docs/remote.md`](docs/remote.md#notifications-20) has
  the details.

## Phone companion (1.0)

**Off by default.** Settings → Remote → **On this network** turns on a read-only page that a
phone or another computer on your local network can open. It shows CPU, GPU, memory, drives, fans,
temperatures, power, FPS, the network, Health and active alerts, refreshed every second.
Nothing on it changes the PC.

- **Pairing.** "Pair a device" shows a QR code and an 8-character code.
  - The code is valid for 5 minutes and for one device; five wrong tries spend it.
  - A paired device stays signed in for 30 days (1, 7 or 90 can be chosen), up to 8 devices.
  - Settings lists the devices and signs any of them out at once.
- **Local network only.**
  - Only private addresses and this PC can connect; anything else is dropped. The port is
    8976 and can be changed.
  - Windows asks about the firewall the first time: allow Private networks only.
  - It is plain HTTP, not encrypted: HTTPS would need a self-signed certificate, which puts a
    warning on every phone. Use it on a network you trust.
- **Never sent:** processes, serial numbers, IP or MAC addresses, user names, device names
  (as the Devices tab shows them), and Health notes. The page does show the PC's own name, as
  the network already knows it.
- **While it is on**, a "Remote" chip in the top bar shows how many devices are watching, and
  the tray tooltip says so.
- **It runs in the app, not the service.** It answers only while Vyro runs (the tray counts),
  and nothing running as LocalSystem listens on the network. Turning it off closes the port
  and every connection at once.

## Reliability

- **Upgrades keep your data.** Settings, alert rules, fan settings, the history and game
  sessions from every release since 0.1 carry over. Tests open files written by each
  release's own code (`crates/vyro-core/tests/migration.rs`).
- **A crash or power cut never leaves half a file.** Every state file is written to a temporary
  file, flushed to disk and then renamed.
- **A damaged file is set aside, not overwritten.** It is kept as `<name>.bad.<ext>` (for
  example `settings.bad.json` or `history.bad.sqlite`) and the defaults are used. In
  `settings.json` a bad value resets only that one setting.
- **Fan control after a crash.** The saved BIOS settings go back before anything else. If that
  write fails, Vyro keeps trying with them. If they cannot be read, the fans Vyro may have
  driven go to full speed until a restart.

## Documentation

| | |
|---|---|
| [`docs/control-safety.md`](docs/control-safety.md) | Fan and pump control: every safety rule, its code, tests and field result |
| [`docs/anti-cheat.md`](docs/anti-cheat.md) | What an anti-cheat can see of Vyro, what Vyro never does, and what to turn off |
| [`docs/footprint.md`](docs/footprint.md) | What each feature costs and when; how 1.0 and 1.1 are measured |
| [`docs/performance-score.md`](docs/performance-score.md) | The Performance Score formula |
| [`docs/benchmark.md`](docs/benchmark.md) | The benchmark's method |
| [`docs/diagnostics.md`](docs/diagnostics.md) | The diagnostics checks |
| [`docs/field-check.md`](docs/field-check.md) | The owner's field check of 1.0 (P16–P27) on the reference PC, in one pass |
| [`docs/field-check-1.1.md`](docs/field-check-1.1.md) | The field check of 1.1 (P28–P30): the new look, My PC and the laptop, desktop widgets |
| [`docs/field-check-1.2.md`](docs/field-check-1.2.md) | The field check of 1.2: widgets through Show desktop, snapping, clicks passing through, three new kinds; My PC's view, glass, fans and airflow; text inside every widget; the Dashboard's packing |
| [`docs/field-check-1.3.md`](docs/field-check-1.3.md) | The field check of 1.3: device names, wireless batteries, Bluetooth Connect and Disconnect, the colour wheel |
| [`docs/remote.md`](docs/remote.md) | The phone app and the relay (1.4): how it connects and pairs, the encryption, what the relay sees, limits, running your own relay |
| [`docs/field-check-1.4.md`](docs/field-check-1.4.md) | The field check of 1.4: the phone app from anywhere, on Android and iPhone |
| [`docs/field-check-1.4.1.md`](docs/field-check-1.4.1.md) | The field check of 1.4.1: Bluetooth Connect and Disconnect, connected or not, low batteries |
| [`docs/field-check-1.5.md`](docs/field-check-1.5.md) | The field check of 1.5: the phone's new tabs, letting a phone change the PC, End task, power, waking a PC |
| [`docs/field-check-2.0.md`](docs/field-check-2.0.md) | The field check of 2.0: notifications on Android and iPhone, the PC going quiet, AMD and Intel tuning |
| [`docs/signing.md`](docs/signing.md) | Code signing (2.0): what the release does once SignPath is set up, and what only the owner can do |

`vyro-remote.exe` in the same artifact checks the service from the outside (state, connection,
sensors; `--capture Game.exe` measures a running game for 15 s). The service logs to `%ProgramData%\Vyro\service.log`.

## License

Vyro is licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Third-party components and their licenses, including PawnIO and PresentMon, are listed in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md), with the full texts in
[`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) (regenerate both with
`python3 scripts/third_party.py`; CI checks they are current).

## Layout

| Path | What |
|---|---|
| `crates/vyro-core/` | Sensor engine, platform-neutral: registry, provider trait, poller, SQLite history, simulator, data contracts, service IPC |
| `crates/vyro-service/` | The Windows service that runs the engine with the rights PawnIO and SMART need |
| `src-tauri/` | Tauri 2 shell: connects to the service (or starts the engine), emits the `sensors` event, serves commands |
| `src/` | Svelte 5 + TypeScript UI; canvas gauges and sparklines, no chart library |
| `src/bindings/` | TypeScript types **generated** from the Rust structs by ts-rs — never edit by hand |

## Development

Prerequisites: Rust stable (≥ 1.88), Node 22 + pnpm 10. On Windows, the WebView2 runtime
(preinstalled on Windows 11) and the MSVC build tools. On Linux, the
[Tauri system dependencies](https://v2.tauri.app/start/prerequisites/) (webkit2gtk 4.1).

```sh
pnpm install
pnpm tauri dev          # desktop app with hot reload
pnpm dev                # UI only, in a browser, fed by an in-page simulator
```

The app uses the installed sensor service when it runs and otherwise starts its own engine
(`VYRO_LOCAL=1` forces that). To develop against a service you build yourself:

```sh
cargo run -p vyro-service -- --console          # Windows: as administrator; Linux: simulated sensors
VYRO_DEV_PIPE=1 pnpm tauri dev                  # Windows: trust the console service's pipe
VYRO_SOCKET=/run/user/$UID/vyro-engine.sock pnpm tauri dev   # Linux
```

Checks (what CI runs):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p vyro-core   # also regenerates src/bindings — commit any changes
cargo test -p vyro-service
pnpm check && pnpm test && pnpm build
python3 scripts/bundle_budget.py --check   # what each page loads at start, gzipped
pnpm a11y                 # axe-core on every tab, theme and width (needs a Playwright Chromium)
```

On Windows, CI also installs the MSI on its runner, checks the service with `vyro-probe`, and
uninstalls it again.

The core crate builds and tests on any OS without WebView dependencies
(`cargo test` at the root targets it by default). Windows-only code can be type-checked from
Linux with `rustup target add x86_64-pc-windows-gnu` plus mingw-w64:
`cargo clippy --workspace --target x86_64-pc-windows-gnu`.

**No toolchain?** Every CI run uploads a `vyro-windows-<sha>` artifact (Actions → run →
Artifacts) with the MSI, `vyro-inventory.exe` (double-click; saves `vyro-inventory.json` beside
it), `vyro-live.exe` (samples the sensors in-process, as administrator), `vyro-remote.exe`
(checks the installed service), `vyro-probe.exe` (asks the installed service for readings and
history and reports its footprint) and the bare `vyro.exe` / `vyro-service.exe`.

To see exactly what Vyro reads on a machine (e.g. to compare with `msinfo32`):

```sh
cargo run -p vyro-core --example inventory              # board serial redacted
cargo run -p vyro-core --example inventory -- --show-serial
```

## Architecture in brief

The **Vyro Sensor Service** (`crates/vyro-service`, LocalSystem) runs the engine; the app
(`src-tauri`, unelevated) receives its snapshots and calls it over `\\.\pipe\vyro-engine`
(`vyro_core::ipc`, one commands and one snapshot connection per app). Without the service the
app runs the same engine in-process. Providers register sensors in a unified registry and fill a `SensorSnapshot` each tick. A
background poller runs at 1 Hz (5 s while the window is hidden), isolates failing providers,
records history to SQLite (raw ≤ 30 min, 10 s ≤ 24 h, 1 min ≤ 7 days, ≤ 200 MB; the same
file keeps the game replays) and emits the snapshot. The UI eases gauges toward each new value on a single `requestAnimationFrame` loop
that sleeps when values settle or the window is hidden. The service never listens on the
network: the optional phone companion is served by the app, as the signed-in user, the
connection to the relay for the phone app is the app's too, and benchmarks run in the app, so
quitting Vyro always ends them.

## Performance

Measured against the spec §8 budget with the built-in "Vyro itself" card (Dashboard),
which counts the sensor service, the app and its WebView2 processes. Reference PC: Ryzen 7
9700X, RTX 5060 Ti, Windows 11, 144 Hz display; Desktop profile (1 s polling).

| Metric | Target | 0.1.4 | 0.2.0 | 0.3.0 | 1.0.0 | 1.1.0 | 1.2.0 | 1.4.0 | 1.4.1 |
|---|---|---|---|---|---|---|---|---|---|
| CPU (average, window visible) | < 1.5 % | 0.68 % | 0.46 % | not re-measured | within target, not written down | within target, not written down | not measured | not reported | as 1.4.0, not written down |
| RAM (private working set, window visible) | < 120 MB | 120 MB | 125 MB | not re-measured | within budget, not written down | within budget, not written down | not measured | not reported | as 1.4.0, not written down |
| RAM peak | — | 132 MB after visiting every tab | 125–150 MB with History open | not re-measured | not written down | not written down | not measured | not reported | not measured |
| RAM (closed to the tray) | — | ≈ 40 MB (service 33 MB, app 7 MB, WebView2 suspended) | not re-measured | not re-measured | not written down | not written down | not measured | not reported | not measured |
| Startup to first data | < 2 s | 0.76 s | 1.02 s | not re-measured | within target, not written down | within target, not written down | not measured | not reported | not measured |
| Installer | < 60 MB (< 20 MB before 1.1) | 9.5 MB | 9.7 MB | 9.9 MB | 11.3 MB | 11.4 MB | 11.4 MB | 11.6 MB | 11.6 MB |

In 0.2.0, of the visible figure, WebView2 takes 85 MB (five processes), the service 33 MB and
the app 7 MB. Animation runs at most 60 fps and only while values move. For 0.3.0 the field
checks of automatic game profiles and the overlay re-measured the footprint within their
budgets (within 2 MB of 0.2.0; the overlay window only while it is shown), but the figures
were not written down. 1.0.0 was measured at its field check (2026-10-16) by the procedure in
[`docs/footprint.md`](docs/footprint.md), which also lists what each V2 feature costs and when.
It was within the budgets there; the figures go into the table when they are written down.
1.1.0 was checked the same way at its field check (2026-10-17), with the same caveat.
1.2.0's field check (2026-10-17) compared only the CPU with 1.1.0: in the tray with widgets
showing, and on My PC with Airflow on. It was no higher; the figures were not written down.
1.4.0's field check (2026-10-18) did not report the footprint, so it has no result yet.
1.4.1's field check (2026-10-19) found Vyro's idle CPU and memory as with 1.4.0, with the
device list now read in the background; the figures were not written down.

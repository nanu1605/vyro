# Clokrix and anti-cheat software

Status: **P27 (2026-10-16)**. Whether a given game's anti-cheat accepts Clokrix is a field result, not a
promise: anti-cheat vendors change their rules without notice. This page says what Clokrix does that
an anti-cheat can see, what it never does, and what has been reported. If a game refuses to start
or warns about Clokrix, the "If a game complains" section lists what to turn off, from least to
most.

## What an anti-cheat can see

| What | Why Clokrix does it | When |
|---|---|---|
| **The PawnIO kernel driver** (signed, bundled version 2.2.0). The service loads its modules to read CPU, board and memory sensors, and to drive fans on the allow-listed board. The modules are `AMDFamily17`, `LpcIO`, `SmbusPiix4`, `RyzenSmu` and `Nvidia`. | Windows allows these readings only from a driver. PawnIO replaced the old WinRing0 driver, which Windows now blocks as vulnerable (decision 10). | While the Clokrix service runs, which is from boot by default |
| **The Clokrix service** (`clokrix-service.exe`, LocalSystem). | It runs the engine: sensors, history, alerts and fan control. | From boot |
| **PresentMon** (`PresentMon.exe` 2.6.0, Intel, signed, MIT). It records frame timing through Event Tracing for Windows (ETW), the same events Windows' own tools read. | Frame rate and frame times of the game in front. | Only while a game is in front and frame capture is on (Settings) |
| **The overlay window.** It is an ordinary desktop window, always on top and click-through, that never takes focus. | FPS and readings over the game. | Only while a captured game is in front and the overlay is on |
| **Reads of other processes' names, CPU and memory use** (the Processes tab and the footprint). | Task-Manager-style listing. | While the app is open |
| **GPU tuning through the maker's driver interface** (1.6; AMD and Intel from 2.0): the service sets the card's clock offsets and power limit with the driver's own calls, as the maker's tools do: NVML for NVIDIA, ADLX for AMD (as AMD Software), the Intel Graphics Control Library for Intel Arc. Nothing touches the game. | The owner's presets. | Only after the owner turns GPU tuning on and applies a preset other than Stock |
| **Screen capture and input** (1.7): the app copies the screen with Windows' Desktop Duplication, as screen-sharing tools do, and turns a phone's taps and typing into Windows input (`SendInput`, marked by Windows as injected). | A phone the owner allowed sees and uses the screen. | Only during a screen session, which the PC shows with a banner; never otherwise |
| **Screen capture for ambient lighting** (2.6): the app copies the screen with Desktop Duplication, as ambient-lighting tools do, makes each picture small on the GPU and keeps only one colour from it. The colour goes to OpenRGB on this PC, which drives the lights. | The owner's RGB devices in the screen's colour. | Only while the owner has ambient lighting on (the Lighting view) and a device picked, or the view is open |
| **OpenRGB, set up by the owner from the Lighting view** (2.6.1): OpenRGB 1.1's own installer, downloaded when the owner presses Set up OpenRGB, puts OpenRGB on the PC as its own program and service ("OpenRGB", LocalSystem), which reaches RGB memory and boards through PawnIO, as OpenRGB does wherever it is installed. Clokrix only sends it colours on this PC. | Ambient lighting without installing OpenRGB by hand. | OpenRGB's service runs from Windows' start once it is set up, whether lighting is on or not, until the owner removes it in Windows' Apps (uninstalling Clokrix leaves it) |
| **A virtual Xbox 360 controller** (2.6): ViGEmBus (Nefarius Software Solutions, signed, BSD-3), the driver controller tools such as DS4Windows use. It comes with the installer and is installed only when the owner sets it up (one administrator prompt). While a phone steers, Windows and games see an ordinary Xbox 360 controller. | The phone wheel: a phone the owner allowed steers driving games. | The driver, once set up, stays installed (other apps share it). The controller is plugged in only while a phone steers, and unplugged when it stops |

## What Clokrix never does

- It never injects code or a DLL into a game, and never hooks its graphics API. The overlay is a separate window.
- It never reads or writes another process's memory.
- It never reads keyboard or mouse input. It sends input only during a screen session a phone the owner allowed asked for (1.7), and only what that phone sends; a game may ignore or refuse injected input, as it may with any remote desktop. Its virtual controller (2.6) exists only while a phone the owner allowed steers, and moves only as that phone does.
- It never writes CPU settings or any voltage. GPU settings (a card's clock offsets and power limit) are written only once the owner turns GPU tuning on and applies a preset (1.6, decision 37), and only through the card maker's own driver interface: NVML for NVIDIA, ADLX for AMD, the Intel Graphics Control Library for Intel (2.0). The other hardware writes are fan and pump speeds, on the allow-listed board, once the owner turns fan control on.

## What has been reported

- **FACEIT blocked PawnIO** in September 2025. The reason was the certificate it was signed with, which had also signed cheat software ([PawnIO.Setup#1](https://github.com/namazso/PawnIO.Setup/issues/1)). PawnIO's author reported it fixed in PawnIO 2.1.0 (February 2026) with a new signature. Clokrix bundles 2.2.0, and its installer upgrades older 1.x installs. FACEIT has not been seen confirming the fix.
- **Riot Vanguard:** Valorant ran on the reference PC with PawnIO 2.2 installed (the owner's field test, 27 September 2026, in the Clokrix 0.1 days; recorded in the ROADMAP's P6 section). The overlay, frame capture and fan control came later and are part of the P27 check below.
- **No other anti-cheat** (Easy Anti-Cheat, BattlEye, EA Javelin) was found reported as blocking PawnIO up to October 2026 ([FanControl discussion 3537](https://github.com/Rem0o/FanControl.Releases/discussions/3537) asks and has no reports). Absence of reports is not a guarantee.
- **ETW frame capture** (PresentMon) is what tools such as RTSS and Intel PresentMon use alongside protected games. No vendor statement about it was found.

## If a game complains

From least to most:
1. **Turn off the overlay** (Settings → Overlay), then frame capture (Settings → Gaming). PresentMon and the overlay then never start. With GPU tuning on, **Return GPU to stock** (the tray) or turn it off. Stop any screen session (the banner's Stop sharing) and the phone wheel (the banner's Stop).
2. **Stop the service while playing:** `sc stop ClokrixService` from an administrator prompt, or Services → Clokrix Sensor Service → Stop. PawnIO's modules are no longer used. The app keeps showing what it can read without the driver (usage, GPU, drives, network), and fan control hands the fans back to the BIOS before the service stops.
3. **Uninstall PawnIO** (Apps → PawnIO) if the anti-cheat refuses to run while the driver is merely installed. Clokrix then shows its "needs PawnIO" banner, and works without the low-level sensors and fan control.

**The virtual controller.** Some online games' anti-cheat refuses virtual controllers, or ignores them. ViGEmBus is no longer developed by its author (it is retired, though still signed and working). If a game refuses it, steer that game with a real controller or wheel. To remove the driver, uninstall "Nefarius Virtual Gamepad Emulation Bus Driver" from Apps; other controller tools that use it stop working too.

## Field check (P27)

The owner plays at least one game protected by each anti-cheat available to them, with Clokrix running, the overlay on and fan control on. The result for each goes in the table below.

| Game | Anti-cheat | Clokrix version | Result |
|---|---|---|---|
| Valorant | Riot Vanguard | 0.1 (PawnIO 2.2), 2026-09 | Runs. Before the overlay, frame capture and fan control: check again |
| — | — | — | to be filled at the field check |

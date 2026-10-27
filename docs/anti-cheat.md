# Vyro and anti-cheat software

Status: **P27 (2026-10-16)**. Whether a given game's anti-cheat accepts Vyro is a field result, not a
promise: anti-cheat vendors change their rules without notice. This page says what Vyro does that
an anti-cheat can see, what it never does, and what has been reported. If a game refuses to start
or warns about Vyro, the "If a game complains" section lists what to turn off, from least to most.

## What an anti-cheat can see

| What | Why Vyro does it | When |
|---|---|---|
| **The PawnIO kernel driver** (signed, bundled version 2.2.0). The service loads its modules to read CPU, board and memory sensors, and to drive fans on the allow-listed board. The modules are `AMDFamily17`, `LpcIO`, `SmbusPiix4`, `RyzenSmu` and `Nvidia`. | Windows allows these readings only from a driver. PawnIO replaced the old WinRing0 driver, which Windows now blocks as vulnerable (decision 10). | While the Vyro service runs, which is from boot by default |
| **The Vyro service** (`vyro-service.exe`, LocalSystem). | It runs the engine: sensors, history, alerts and fan control. | From boot |
| **PresentMon** (`PresentMon.exe` 2.6.0, Intel, signed, MIT). It records frame timing through Event Tracing for Windows (ETW), the same events Windows' own tools read. | Frame rate and frame times of the game in front. | Only while a game is in front and frame capture is on (Settings) |
| **The overlay window.** It is an ordinary desktop window, always on top and click-through, that never takes focus. | FPS and readings over the game. | Only while a captured game is in front and the overlay is on |
| **Reads of other processes' names, CPU and memory use** (the Processes tab and the footprint). | Task-Manager-style listing. | While the app is open |
| **GPU tuning through the maker's driver interface** (1.6; AMD and Intel from 2.0): the service sets the card's clock offsets and power limit with the driver's own calls, as the maker's tools do: NVML for NVIDIA, ADLX for AMD (as AMD Software), the Intel Graphics Control Library for Intel Arc. Nothing touches the game. | The owner's presets. | Only after the owner turns GPU tuning on and applies a preset other than Stock |
| **Screen capture and input** (1.7): the app copies the screen with Windows' Desktop Duplication, as screen-sharing tools do, and turns a phone's taps and typing into Windows input (`SendInput`, marked by Windows as injected). | A phone the owner allowed sees and uses the screen. | Only during a screen session, which the PC shows with a banner; never otherwise |

## What Vyro never does

- It never injects code or a DLL into a game, and never hooks its graphics API. The overlay is a separate window.
- It never reads or writes another process's memory.
- It never reads keyboard or mouse input. It sends input only during a screen session a phone the owner allowed asked for (1.7), and only what that phone sends; a game may ignore or refuse injected input, as it may with any remote desktop.
- It never writes CPU settings or any voltage. GPU settings (a card's clock offsets and power limit) are written only once the owner turns GPU tuning on and applies a preset (1.6, decision 37), and only through the card maker's own driver interface: NVML for NVIDIA, ADLX for AMD, the Intel Graphics Control Library for Intel (2.0). The other hardware writes are fan and pump speeds, on the allow-listed board, once the owner turns fan control on.

## What has been reported

- **FACEIT blocked PawnIO** in September 2025. The reason was the certificate it was signed with, which had also signed cheat software ([PawnIO.Setup#1](https://github.com/namazso/PawnIO.Setup/issues/1)). PawnIO's author reported it fixed in PawnIO 2.1.0 (February 2026) with a new signature. Vyro bundles 2.2.0, and its installer upgrades older 1.x installs. FACEIT has not been seen confirming the fix.
- **Riot Vanguard:** Valorant ran on the reference PC with PawnIO 2.2 installed (the owner's field test, 27 September 2026, in the Vyro 0.1 days; recorded in the ROADMAP's P6 section). The overlay, frame capture and fan control came later and are part of the P27 check below.
- **No other anti-cheat** (Easy Anti-Cheat, BattlEye, EA Javelin) was found reported as blocking PawnIO up to October 2026 ([FanControl discussion 3537](https://github.com/Rem0o/FanControl.Releases/discussions/3537) asks and has no reports). Absence of reports is not a guarantee.
- **ETW frame capture** (PresentMon) is what tools such as RTSS and Intel PresentMon use alongside protected games. No vendor statement about it was found.

## If a game complains

From least to most:
1. **Turn off the overlay** (Settings → Overlay), then frame capture (Settings → Gaming). PresentMon and the overlay then never start. With GPU tuning on, **Return GPU to stock** (the tray) or turn it off. Stop any screen session (the banner's Stop sharing).
2. **Stop the service while playing:** `sc stop VyroService` from an administrator prompt, or Services → Vyro Sensor Service → Stop. PawnIO's modules are no longer used. The app keeps showing what it can read without the driver (usage, GPU, drives, network), and fan control hands the fans back to the BIOS before the service stops.
3. **Uninstall PawnIO** (Apps → PawnIO) if the anti-cheat refuses to run while the driver is merely installed. Vyro then shows its "needs PawnIO" banner, and works without the low-level sensors and fan control.

## Field check (P27)

The owner plays at least one game protected by each anti-cheat available to them, with Vyro running, the overlay on and fan control on. The result for each goes in the table below.

| Game | Anti-cheat | Vyro version | Result |
|---|---|---|---|
| Valorant | Riot Vanguard | 0.1 (PawnIO 2.2), 2026-09 | Runs. Before the overlay, frame capture and fan control: check again |
| — | — | — | to be filled at the field check |

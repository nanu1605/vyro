# Field check: 1.5 (control from the phone)

Status: **to do.** Test build: the `vyro-windows-<commit>` artifact of the latest green CI run
on `claude/vyro-1.5`, version 1.5.0. Install the MSI over 1.4.1. About 40 minutes.

1.5 answers the owner's request to check and run anything Vyro does from the phone, shut the PC
down from it, wake it, and see what runs on it (ROADMAP → 1.5.0, decision 36,
[`remote.md`](remote.md)). The PC's screen on the phone comes in 1.7.

**The phone app** comes from the relay. For this check, deploy the relay from this branch first
(Actions → **Relay** → Run workflow, `ref`: `claude/vyro-1.5`), then open the app on the phone and
pull to refresh (or close and reopen it) so it loads the new version. A phone on the old app
still shows readings.

Have at hand: the PC with Vyro and Reach from anywhere on, a phone already paired (from 1.4),
and, for waking, a second PC with Vyro on the same wired network, if you have one.

## 1. The new tabs

- Open the PC in the phone app: tabs **Live, Processes, Devices, Health, More**.
- **Processes:** the busiest programs, every 3 seconds; sort by CPU, Memory or GPU.
- **Devices:** the PC's devices with batteries; a low one shows at the top.
- **Health:** the same as the PC's Health, the Performance Score, recent alerts.
- **More:** profile, fans, benchmark, diagnostics, updates.
- A phone paired before 1.5 may only look: the top says "view only", and no End, Power or other
  change is offered.

## 2. Letting the phone change the PC

- On the PC: Settings → Remote → Paired devices → the phone's **Only looks** button → it becomes
  **May change this PC**. Within a few seconds the phone says "can change this PC", and More shows
  **Power**.
- Pair another phone (or the same one again after signing it out) with **Look and change this PC**
  chosen above Pair a phone: it can change the PC at once. With **Only look**, it cannot.

## 3. Doing things from the phone

- **End task:** start Notepad on the PC; on the phone, Processes → Notepad → End… → End. It
  closes; the PC shows a notification; Settings → Remote → Recent from your phones lists it.
  Try to end `explorer.exe` or `vyro.exe`: refused.
- **Profile:** More → Profile → Gaming: the PC's tray shows Gaming.
- **Benchmark:** More → Quick: the PC runs it; the phone shows its progress; Stop works.
- **Diagnostics:** More → Run diagnostics: the phone says how many things to look at.
- **Bluetooth:** Devices → Disconnect then Connect the headphones (as in field check 1.4.1).
- **Lock:** More → Lock… → Lock: the PC locks. Sign in again.
- **Restart:** More → Restart… → Restart (in 30 seconds): the PC shows a banner with Cancel and a
  notification. Cancel it on the PC: it does not restart. Ask again and let it restart: the
  phone shows "asleep or off" until you sign in, then the readings come back.
- **Shut down:** More → Shut down… → untick "In 30 seconds" → Shut down: the PC shuts down.

## 4. Waking it (needs a second PC with Vyro on the same wired network)

- On the PC to wake: Settings → Remote → **Wake from phone** on. Check what it says about the
  adapter and Fast Startup, and turn on "Wake on LAN" in the BIOS if needed.
- Pair the phone with the second PC too, allowed to change it.
- Shut the first PC down (from the phone, as above). Its screen in the app says it is asleep or
  off, with **Wake <PC>**. Tap it: "<second PC> sent <PC> the wake signal", and the PC starts.
- Without a second PC: note what Settings says under Wake from phone.

## 5. Nothing else changed

- The LAN page (Settings → Remote → On this network) still only shows readings.
- Task Manager: Vyro's idle CPU and memory as with 1.4.1.

## Results

| Step | Result | Notes |
| --- | --- | --- |
| 1. The new tabs |  |  |
| 2. Letting the phone change the PC |  |  |
| 3. Doing things from the phone |  |  |
| 4. Waking it |  |  |
| 5. Nothing else changed |  |  |

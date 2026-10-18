# Field check: 1.3 (device names, wireless batteries, Bluetooth Connect and Disconnect, the colour wheel)

Status: **not run yet.** Test build: **`vyro-windows-d185506`** (CI run 39100838311 on
`claude/vyro-1.4`, version 1.4.0): 1.3 is not released on its own but ships in 1.4.0 (ROADMAP,
decision 35), and that build carries all of it. (`vyro-windows-294c5f1`, the 1.3 branch's build,
is no longer the one to check.)

1.3 answers the owner's requests after 1.2.0 (ROADMAP → 1.3.0 has how each part works):
- devices named as Windows Settings names them, not "Unknown device";
- the battery of wireless devices: Logitech (through its receiver or directly), PlayStation and
  Xbox controllers, and Bluetooth as before;
- Connect and Disconnect for paired Bluetooth devices;
- an accent of any colour, from a colour wheel.

**Build.** `vyro-windows-d185506` (Actions → CI run 39100838311 → Artifacts), version 1.4.0. If it
is installed already (field check 1.4 used it), there is nothing to install. It takes about 30
minutes.

## Before you start

- Have your devices on: the Bluetooth headphones or earbuds, the Logitech mouse or keyboard
  (and its receiver plugged in), the controller, the other dongle, the USB microphone.
- Open Windows Settings → Bluetooth & devices → Devices beside Vyro's Devices tab.

## 1. Names

- Every device in Vyro's Devices tab has the name Settings shows. No row says "Unknown device".
- A paired Bluetooth device that is off still shows its name, with "Not connected".
- The USB condenser microphone shows by its own name (as Settings shows it), as a microphone.
- **If a name is wrong or missing:** run `vyro-inventory.exe` from the same artifact and send
  the "devices" part of `vyro-inventory.json` (next to it) for that device.

## 2. Batteries

- **Logitech on its receiver:** the receiver has its own row, and each device on it has a row of
  its own ("Through USB Receiver") with its name and battery. Compare with Logitech G Hub or
  Options+: within a few percent. While it charges, it says "charging". Switch the device off:
  within a minute its row says "Asleep or off".
- **Logitech directly** (on its cable, or over Bluetooth): its battery shows on its own row.
- **Controller:**
  - Xbox, wireless or Bluetooth: a level (Full, Medium, Low).
  - PlayStation on a USB cable: a percentage and "charging". Over Bluetooth it shows only
    while Steam (or another app) is running; that is expected.
- **Bluetooth headphones or earbuds:** as Settings shows.
- **The other dongle:** "—" for now. Run `vyro-inventory.exe` and send the lines under "hid"
  with its maker's vendor id (Razer 1532, SteelSeries 1038, Corsair 1b1c, HyperX 0951 or
  03f0): its protocol goes into the next build.

## 3. Connect and Disconnect

- **Headphones or earbuds:** Disconnect. Within a few seconds the row says "Disconnected" and
  the sound moves to the PC's own output. Connect: they connect, as from Settings.
- **A Bluetooth mouse, keyboard or controller:** Disconnect. The row says "Disconnected", or
  "It came back" if the device connects again by itself when moved: both are expected. With
  it switched off, Connect says "Didn't connect: turn it on, or press a button on it"; switch
  it on and press Connect: it connects.
- Wired devices and devices on a receiver have no button.
- Pressing a button twice quickly does nothing the second time.

## 4. The colour wheel

- Settings → Appearance → Accent → **Custom**. A colour wheel opens.
- Drag on the wheel: the whole window follows as you drag; let go and it stays after a restart.
- Pick a dark navy on Midnight: gauges and graphs show the navy; text in the accent (links,
  the gauge's MHz) is a lighter blue you can read. The picker says which shade text uses.
- Switch to Daylight and pick a pale yellow: text in the accent is a darker olive.
- Pick a red or an orange: the picker notes it is close to the warning colours.
- The keyboard works too: Tab to the wheel's thumb, the arrow keys move it.
- Type `#00897b` in the colour field and press Enter: it applies.
- The overlay set to "App" colour, desktop widgets and the phone companion follow the accent.

## 5. Footprint

- With the Devices tab open, Task Manager's CPU for Vyro is no higher than with 1.2 when idle;
  battery reads happen at most once a minute.

## Recording results

- **Passes:** ROADMAP → 1.3.0 and 1.4.0 get "Field check 1.3 (`<PC>`, `<date>`, test build
  `vyro-windows-<commit>`)".
- **A check fails:** note what you saw, with your Windows version (from `winver`), and for a
  device its make and model. The fix goes on `claude/vyro-1.4`, and the checks it affects are
  run again.
- **Release:** with 1.4.0 (field check 1.4's sheet), after the owner's go.

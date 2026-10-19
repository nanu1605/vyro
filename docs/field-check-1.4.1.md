# Field check: 1.4.1 (Connect and Disconnect, low batteries)

Status: **to do.** Test build: **`vyro-windows-72a2571`** (CI run 39154275785, every job
green), version 1.4.1, or the artifact of any later green run on `claude/vyro-1.4.1`. Install the
MSI over 1.4.0. About 20 minutes.

1.4.1 answers the owner's report after 1.4.0: Connect and Disconnect did nothing on headphones,
earbuds and a speaker, and a device that was disconnected still showed Connected. It also adds
the low-battery notice the owner asked for (ROADMAP → 1.4.1).

Have at hand: Bluetooth headphones or earbuds, a Bluetooth speaker, and, if you have one, a
Bluetooth mouse or keyboard, and a wireless device whose battery is under 20 %.

## 1. Connected or not, as Windows says

- Open Devices. Each paired Bluetooth device shows **Connected** or **Not connected**, matching
  Windows Settings → Bluetooth & devices.
- Turn the speaker off: within a few seconds its row says **Not connected** and offers
  **Connect**. Turn it on: **Connected**, and **Disconnect**.
- Disconnect the headphones in Windows Settings: Vyro follows within a few seconds.
- A device that shows **Paired** is one Windows says nothing about: it offers both Connect and
  Disconnect. Note which device, if any.

## 2. Connect and Disconnect

For the headphones (or earbuds) and the speaker:

- **Disconnect**: the row says "Disconnecting…", then **Disconnected** within about 10 seconds,
  and Windows Settings agrees. No sound plays through it any more.
- **Connect** (the device on and near): "Connecting…", then **Connected** within about 15
  seconds, and sound plays through it again.
- With a headset that has a microphone: after Disconnect, neither "Headphones" nor "Headset"
  for it is listed as playing in Sound.
- A mouse or keyboard: **Disconnect** says "Disconnected", or "It came back" when it connects
  again by itself as soon as it is used (expected).

If a step fails: note the message on the row, then send the app's log from
`%LOCALAPPDATA%\app.vyro.monitor\logs` (the lines starting `devices:` say what was tried).

## 3. Low battery

- With a wireless device under 20 % (not charging): within about half a minute of starting Vyro,
  or at once when Devices is opened, the Dashboard shows **Low battery: <device> <n>%** and
  Devices has an amber dot (red under 10 %). The strip opens Devices.
- Put it on its charger, or let it pass 20 %: the strip and the dot go.

## 4. Nothing else changed

- Task Manager: Vyro's CPU and memory when idle are as with 1.4.0 (the device list is now read
  in the background every 5 minutes).
- The phone app (Settings → Remote → From anywhere) still shows readings.
- Settings → Remote stays quiet: with it open, pair a phone and open the phone app. In Task
  Manager, neither Vyro nor "Service Host: Network Service" (DNS Client) stays above a few
  percent (1.7's field check saw 23 % on opening, 85 % with a phone paired, fixed here).

## Results

| Step | Result | Notes |
| --- | --- | --- |
| 1. Connected or not |  |  |
| 2. Connect and Disconnect |  |  |
| 3. Low battery |  |  |
| 4. Nothing else changed |  |  |

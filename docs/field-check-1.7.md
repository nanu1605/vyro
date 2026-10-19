# Field check: 1.7 (the PC's screen on the phone)

Status: **to do.** Test build: **`vyro-windows-e0e76b5`** (CI run 39191304470, every job green),
version 1.7.0, or the artifact of any later green run on `claude/vyro-1.7`. Install the MSI over
1.6, and open the phone app from the relay this branch
deploys (or the branch's own relay; see `docs/remote.md` → Running your own relay). About 60
minutes, with an Android phone (Chrome) and an iPhone (Safari, the Home Screen app).

1.7 answers the owner's request to "see what is running on the PC from the phone, and use the
system from the phone" (ROADMAP → 1.7.0, decision 38): directly where it can, and a slow view
through the relay where it cannot.

## 1. The permission

- Settings → Remote → Paired devices: each phone paired from anywhere shows **No screen**. On the
  phone, the **Screen** tab says it may not see the screen and how to allow it.
- Tap **No screen** for the Android phone: it reads **Sees the screen**. The phone's Screen tab
  offers "Show *PC*'s screen" within a few seconds, without reopening the app.
- A phone that may only look (not change the PC) may still be given the screen: check both
  buttons work on their own.

## 2. Direct, on Wi-Fi (the same network as the PC)

- Show the screen. Within a few seconds the phone shows the PC's desktop and "Tap to click…",
  and below the picture "Receiving W×H, N pictures a second" (note both: on a still screen about
  one a second is right). Leave the PC alone for a minute: the picture stays, sharp, never
  black. The PC shows the **Your screen is shared** banner on every view, a notification, and
  the tray menu has **Stop sharing the screen with *phone***.
- Move a window around on the PC: the phone follows. Note the delay (a stopwatch on the PC's
  screen, filmed beside the phone, or by eye) and the PC's CPU in Task Manager for Vyro.
- **Tap** an icon: it is clicked where the pointer dot shows. **Hold** on the desktop: the
  right-click menu opens. **Drag** a window by its title bar. **Two fingers** up and down in a
  browser page: it scrolls. **Pinch**: the picture zooms on the phone only; two fingers pan it;
  **Fit the screen** goes back.
- Type into Notepad with the box: text arrives, also with accents and an emoji. **Ctrl** then
  type **a**: everything is selected; the Ctrl button lets go after. **Alt**, then **Tab**:
  windows switch. **Win**: the Start menu opens.
- With two monitors: the screen buttons list both; choosing the other one shows it, and taps
  land on it.
- **Stop** on the phone: the banner goes, the record in Settings → Remote shows the session's
  start and end with its length.

## 3. Direct, on mobile data (Wi-Fi off on the phone)

- Show the screen again. Note whether it connects directly (the hint "Tap to click") or falls
  back to the slow view after about 15 seconds ("No direct path…"). Either is fine; write down
  which, and the network (operator, home router).

## 4. The slow view

- If 3 did not show it: on the phone, turn on a VPN or a network that blocks UDP, or test from
  another network, until "No direct path" shows.
- The picture refreshes about twice a second when something changes; taps and typing still work,
  a little slower. Settings → Remote → Screen shows the minutes left today going down.
- Drag around the PC's screen for a minute or more: the session goes on, and the top of the phone
  keeps saying **Live**.

## 5. What it cannot do

- Lock the PC (Win+L) while sharing: the phone says the PC is showing a secure screen; sign in
  at the PC: the picture comes back by itself.
- Open an administrator Command Prompt on the PC: the phone shows it, but taps and typing do not
  reach it (Windows keeps input from a non-administrator program out of it).
- Start something that asks for administrator rights (UAC): the phone shows the secure screen
  until it is answered at the PC.

## 6. Ending a session

- Stop sharing from the PC's banner, then once more from the tray: the phone says the session
  ended.
- Settings → Remote: tick **Lock this PC when a phone's session ends**; start a session and stop
  it on the phone: the PC locks. Stopping at the PC does not lock it.
- Leave the phone with the Screen tab open, then lock the phone for 10 minutes: the session ends
  by itself ("nobody used it for 10 minutes" in the record).
- Turn the phone's permission off while it shows the screen: the session ends at once.
- With a session on, start one from the iPhone (also allowed): it is told the screen is already
  shared with the Android phone.

## 7. iPhone

- Repeat 2 (briefly) on the iPhone from the Home Screen app: the picture plays (Safari), taps,
  hold, typing.

## Results

| Check | Android | iPhone | Notes |
|---|---|---|---|
| Permission | | | |
| Direct on Wi-Fi: picture, delay, CPU | | | |
| Taps, hold, drag, scroll, zoom | | | |
| Typing, Ctrl+A, Alt+Tab, Win | | | |
| Two monitors | | | |
| Mobile data: direct or slow | | | |
| Slow view | | | |
| Secure screen, administrator window | | | |
| Stop, lock on end, idle end, permission off | | | |

# Field check: 1.7 (the PC's screen on the phone)

Status: **1–7 done, 2026-10-19 (see Results), except the slow view (not tried); 8–12 (the
owner's requests of 2026-10-21) to do.** Test build now: **`vyro-windows-68d0d6c`** (CI run
39952173968, every job green, version 1.7.0; the phone's microphone ends at once when turned off
and rides out a stalled link), or the artifact of any later green run on `claude/vyro-1.7`
(before it, `vyro-windows-3095d4f` for 1–7). Install the MSI over 1.6, and open the phone app
from the relay this branch deploys (or the branch's own relay; see `docs/remote.md` → Running
your own relay). The relay was last deployed from 3d80a68 (the same phone app; run 39946326727,
2026-10-23, at the owner's go); reload the phone app once to get it. About 60 minutes, with an
Android phone (Chrome) and an iPhone (Safari, the Home Screen app).

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
  black.
- Tap "Receiving…": it opens two lines. **Here** is how bright the picture this phone decodes
  is; **the PC's** line is what the PC sent (its screen's size, how it took it: "Desktop
  Duplication", "Desktop Duplication (HDR)" on a monitor with HDR on, or "GDI" when Desktop
  Duplication gave blank pictures or ones it cannot convert; the size sent, pictures a
  second, Mbit/s, how bright the picture it took is, whole pictures asked for). If the picture
  is black, a line under it says whether the PC's own picture was black or the PC sent one that
  comes out black on the phone. Screenshot both lines on each phone, picture or not. The PC shows the **Your screen is shared** banner on every view, a notification, and
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
- **HDR** (Settings → System → Display → Use HDR, on the monitor shown): the picture shows with
  its usual colours, neither black, dark nor washed out, and the PC's line says "Desktop
  Duplication (HDR)". Move the "SDR content brightness" slider, then show the screen again: white
  windows stay white. Turn HDR off and on while the screen is shown: the picture comes back each
  time. The app's log has "screen: … is HDR; its pictures are converted with SDR white at N
  nits".
- **Stop** on the phone: the banner goes, the record in Settings → Remote shows the session's
  start and end with its length.

## 3. Direct, on mobile data (Wi-Fi off on the phone)

- Show the screen again. Note whether it connects directly (the hint "Tap to click") or falls
  back to the slow view after about 15 seconds ("No direct path…"). Either is fine; write down
  which, and the network (operator, home router).
- Directly: move a window around for a minute. The delay may be a little longer than on Wi-Fi,
  but it stays the same rather than growing. "Receiving…" shows what the connection carries
  ("… Mbit/s of the N the connection carries"); on a slow one the picture is sent smaller and
  less often (720p, then 15 or 10 a second) and comes back to full size within seconds once
  the connection allows it.

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

## 8. The phone app, redesigned

- On each phone, in the dark theme and then the light one (the PC's Settings → Appearance):
  every tab (Live, Processes, Screen, Health, More and each of its tiles), the list of PCs (tap
  the PC's name), a confirmation (More → Power → Shut down, then Cancel), pairing and home.
  Nothing scrolls sideways, nothing is cut off at the right, and **Powered by Jainav Infotech
  Pvt Ltd** is at the foot of each screen.
- Moving between tabs slides; cards come in one after another; sheets rise and close with Back
  or a tap above them. With the phone's reduced motion on (Android: Remove animations; iPhone:
  Reduce Motion), none of it moves.
- The Wi-Fi companion page (Settings → Remote → On this network) shows the same line at its
  foot.

## 9. Full screen

- Android: show the screen, tap **Full**. The PC's screen fills the phone, turned sideways for a
  wide monitor; the controls disappear after 3 s and the small button in the corner brings them
  back. Taps, drags and typing still land where they should. Back leaves full screen.
- iPhone, from the Home Screen app: **Full** fills the app's window with the screen (turn the
  phone yourself); **Exit** brings the tabs back.

## 10. HD or SD

- On Android with Wi-Fi off: **Show** asks "You’re on mobile data", SD or HD. Choose SD: the
  details under the picture say 960×540 at most, and how much data the session used. Show it
  again: it does not ask again (it remembered).
- In a session on Wi-Fi in HD, turn Wi-Fi off: the phone offers "Switch to SD"; switching, or
  **HD/SD** in the toolbar, changes the picture within a second or two.
- iPhone (it does not say it is on mobile data): the HD/SD choice shows on the start card and in
  the toolbar.

## 11. Voice clips

- In a session, tap **Clip**, allow the microphone, record 5–10 seconds, **Send to *PC***. The
  phone says it is on the PC's clipboard; the PC shows a notification, and Settings → Remote's
  record lists the clip.
- In WhatsApp Web on the PC, open a chat and tap **Paste on *PC*** (or press Ctrl+V on the PC):
  the clip attaches as an audio file; send it and play it on another phone. Note whether it
  arrives as a voice message or as an audio file, and whether it plays.
- `%LOCALAPPDATA%\app.vyro.monitor\Phone clips` holds it as `Clip <date time>.m4a`.
- In the slow view (if 4 showed it), a clip of up to a minute goes too, a little slower.

## 12. The phone as the PC's microphone (VB-CABLE)

- Before installing anything: the Screen tab's **Mic** says the PC needs VB-CABLE and offers a
  clip instead; Settings → Remote → Screen says none is installed.
- Install VB-CABLE (vb-audio.com/Cable: run `VBCABLE_Setup_x64.exe` as administrator), restart
  the PC. Settings → Remote → Screen now names "CABLE Output (VB-Audio Virtual Cable)".
- Note the default microphone (Windows Settings → System → Sound → Input). In a direct session,
  tap **Mic**: the phone says the PC hears it as CABLE Output, its default microphone now; the
  PC's banner says the phone's microphone is on; Windows' Input shows CABLE Output, and its
  level moves when you speak into the phone.
- In WhatsApp Web, hold the voice-note button and speak into the phone: the note has your voice.
  The delay is short (note it), and it does not break up on Wi-Fi.
- On mobile data, while recording a note, walk somewhere the signal drops for a few seconds:
  the note has silence there, not the room around the PC, and Windows' Input stays CABLE Output.
- Tap **Mic** again: at once (within a second) Windows' default microphone is the one you noted.
  The same after stopping the session on the PC with the Mic on.
- Turn off "Make the cable Windows' default microphone…" in Settings → Remote: **Mic** then leaves
  the default alone and the phone says to choose CABLE Output in the app.

## Results

The owner, 2026-10-19, on `vyro-windows-c87dd2d` with the relay deployed from 60eda87 (run
39265388166). Builds before it showed a black picture: the main monitor runs with HDR on (ROADMAP
→ 1.7, "Still black on both phones"). The later checks (mobile data again, the permission, the
secure screen and administrator window, ending a session) on `vyro-windows-3095d4f` with the
relay deployed from 2e42a83 (run 39298073589).

| Check | Android | iPhone | Notes |
|---|---|---|---|
| Permission | works | works | "permission and admin window both work" |
| Direct on Wi-Fi: picture, delay, CPU | works | works | a delay of milliseconds; Vyro at 10 % CPU at most |
| Taps, hold, drag, scroll, zoom | works | works | "everything working" |
| Typing, Ctrl+A, Alt+Tab, Win | works | works | |
| Two monitors | works | works | 2560×1440 (HDR) and 3840×2160 |
| HDR on and off, SDR brightness | works | works | |
| Mobile data: direct or slow | works, direct | works, direct | on c87dd2d the delay grew; on `vyro-windows-3095d4f` with the relay from 2e42a83, "mobile data works now, lag stays low" |
| Slow view | not tried | not tried | both phones always found a direct path; CI's end-to-end test (`scripts/relay-e2e.mjs`) plays the slow view in Chromium through the relay |
| Secure screen, administrator window | works | works | an administrator prompt (UAC) can't be seen ("not able to see admin permission yes or no"); the phone shows the secure-screen line, as designed. The owner keeps it so for 1.7. The administrator window shows on the phone and keeps its input out: works |
| Stop, lock on end, idle end, permission off | works | works | "session ending work on both phones" |
| 8. Redesign: every screen, both themes, no sideways scroll, the footer, motion | not yet | not yet | |
| 9. Full screen | not yet | not yet | |
| 10. HD or SD on mobile data | not yet | not yet | |
| 11. A clip pasted into WhatsApp Web | not yet | not yet | |
| 12. The phone as the microphone (VB-CABLE), the default put back | not yet | not yet | |

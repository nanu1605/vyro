# Field check 2.0

What only real phones and real cards can show. The tests cover the rest: the PC's sealing against
RFC 8291's own example, the relay in workerd with a push service that checks its signature and
keeps what it was sent, the phone's subscription and service worker, and the end to end through
the relay with a push service played by the test, which opens every notification with Node's own
crypto (159 checks). The AMD and Intel interfaces' layouts are checked against the makers'
headers; no AMD or Intel card has run them.

Install the 2.0 MSI (or the artifact of the latest green run on `claude/vyro-2.0`) over 1.7, and
deploy the relay from the same commit first (Actions → Relay): notifications need the relay's
new `Keys` object. Reload the phone app once to get its new version.

About 45 minutes for notifications, with an Android phone (Chrome) and an iPhone (the Home Screen
app, iOS 16.4 or later); 20 minutes per card for tuning.

## Notifications

1. **Turn them on (Android).** On the phone, open the PC → More → **Notifications** → Turn on.
   Chrome asks to allow notifications: Allow. The section shows Send a test and four choices:
   Alerts, Low batteries and Stopped answering ticked, Game summaries not.
   On the PC, Settings → Remote's paired list says the phone "gets notifications".
2. **A test.** Send a test. Within a few seconds the phone shows "Notifications work" with the
   PC's name, even with Chrome in the background. Close Chrome (swipe it away) and send one from
   a second phone, or from the iPhone below, to see it arrive with Chrome closed.
3. **An alert.** On the PC, Alerts: add a rule that fires at once (CPU load above 1 %, held 0 s),
   then close the app on the phone. The phone shows the alert with the PC's name in its text.
   Tapping it opens the app on that PC. Turn the rule off again.
4. **Choices.** Untick Alerts on the phone and fire the rule again: nothing comes. Tick it again.
5. **A low battery** (with a wireless mouse, headset or controller under 20 %): the phone says
   which device and how much is left, once (not again while it stays low).
6. **A game summary.** Tick Game summaries, play a game for more than five minutes with frame
   capture on (Settings → Gaming), and quit it: within half a minute the phone shows its name, how
   long, its frame rates and temperatures. A game under five minutes sends none.
7. **Stopped answering.** With Stopped answering ticked:
   - Shut the PC down from Start: no notification. Start it again: none.
   - Put it to sleep: none. Wake it: none.
   - Pull its network cable (or turn Wi-Fi off) for under a minute, then plug it back: none.
   - Pull it again and wait: the phone shows "*PC* stopped answering" four to six minutes later.
     Plug it back: within a minute or so, "*PC* is back", after about as many minutes as it was
     unplugged.
   - End `vyro.exe` in Task Manager's Details tab: "stopped answering" about two minutes later.
     Start Vyro: "is back".
   - Optional, if you can: switch the PC off at the wall while it runs. As with the cable.
8. **iPhone.** In Safari, More → Notifications says to add Vyro to the Home Screen. Do so, open
   it there, pair it, and repeat steps 1–3. iOS asks to allow notifications on the tap.
9. **Off.** Turn off on one phone: tests and alerts stop for that phone only; Settings → Remote no
   longer lists it as getting them. Sign the other out from the PC: its notifications stop too.

## AMD tuning (an RX 6000, 7000 or 9000 card)

1. **It is offered.** GPU tuning shows the card, "RDNA 2/3/4", "AMD Software" and its version.
   The Custom sliders show a core range around 0 (down to −500), memory (on desktop cards) and
   a power limit (often 90–115 %). Note the ranges.
2. **Turn on, apply Performance**, then open AMD Software → Performance → Tuning: its manual
   tuning shows the higher max frequency, VRAM max frequency and power limit Vyro set.
3. **Return to stock:** AMD Software shows its defaults again (Vyro uses AMD's own reset).
4. **Test this setting** on Performance: passes or fails as on an NVIDIA card; the log says so.
5. **Find my best:** its steps say "Core +95 MHz" and so on, memory in 25 MHz steps.
6. **The guards:** reboot with Apply at start on and a tested preset: it is applied again.
   Change the max frequency in AMD Software while Vyro's preset is on: Vyro writes it back, and
   stops tuning if that keeps happening.

## Intel tuning (an Arc A- or B-series card)

1. **It is offered.** "Alchemist (Arc A)" or "Battlemage (Arc B)" and the driver's version; a
   core range from 0 and a power limit (an A770: about 50–120 % of 190 W). No memory slider.
2. **Turn on:** the dialog says turning tuning on accepts Intel's warranty waiver.
3. **Apply Performance**, then open Intel Graphics Software → Performance: the frequency offset
   and power limit match.
4. **Return to stock:** Intel's own values again.
5. **Test this setting** and **Find my best** (core in 10 MHz steps) as above.

## Notes

- Record each card's model, driver and ranges in ROADMAP's 2.0 section, and whether ADLX or the
  Intel library ran in the service (the GPU tuning view says "not available" with a reason when
  it did not).
- A card that misbehaves: Return to stock, turn tuning off, and send `%ProgramData%\Vyro\service.log`.

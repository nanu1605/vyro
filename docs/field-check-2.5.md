# Field check 2.5

What only a real PC (and a real Razorpay account) can show for 2.5:
- Settings' sections in their own window, and its animation;
- the shorter lists in Settings → Remote and Health;
- Clokrix Pro bought in Razorpay's Test mode, and its key on one PC;
- the glass, and the window see-through on Windows 11.

The tests cover the rest:
- the checkout in workerd, with Razorpay played by a stand-in;
- the app's side of the activation, against one signed as the checkout signs;
- the page in the browser preview, as Free, in the trial and at its end.

**Before you start**, do the set-up in [`docs/pro.md`](pro.md) → Setting it up, with Razorpay in
**Test mode**. Then install the test build `clokrix-windows-<commit>` of the latest green CI run
on `claude/vyro-2.0`: once both variables are set, it carries the checkout as a release will. For
§3 you need a second PC, or a virtual machine with Windows.

About 40 minutes.

## 1. Settings' sections

1. Settings, then click each name in the list on the left (Appearance, Gaming, General and the
   rest). Each section opens in a window over the page, growing out of its name. Escape, its ×,
   or a click on the dimmed page beside it closes it, shrinking back into the name.
2. Change something in a window (a theme, a switch): it takes effect as before. Close the window:
   the card in the page shows the change.
3. Ctrl K → "units": Settings opens with Units in its window. The update chip, when there is one,
   opens Updates the same way.
4. Settings → Profile → Animation: Reduced. A section now fades in and out, without growing.
   Back to Full.

## 2. The shorter lists

1. **Settings → Remote**, with a phone that has changed something: under "Recent from your
   phones", the latest 3. "Show all N actions" shows the rest, and "Show fewer" hides them.
2. **Health → Recent events:** the latest 5, then "Show all N events".
   - A chip for each part of the PC with events (CPU, GPU, Memory and so on), with a count.
   - A chip shows that part's events only, and All brings back every part.

## 3. Clokrix Pro (Razorpay's Test mode)

1. **The trial.** The top bar says "Pro trial · 7 days left". Its chip opens Settings → Clokrix
   Pro, which says when the trial ends.
2. **Buy.** Buy Pro opens the checkout in the browser at $100. Pay with one of Razorpay's test
   cards ([Razorpay's docs](https://razorpay.com/docs/payments/payments/test-card-details/)).
   - The page shows a key starting `CLKX-`; keep it.
   - Within a few seconds Clokrix says Pro, the chip goes, and Settings → Clokrix Pro shows the
     key's last part.
3. **Offline.** Restart Clokrix with the network cable out (or Wi-Fi off): it is still Pro.
4. **A second PC.** Enter the key there (Settings → Clokrix Pro → Already have a key?).
   - It is refused as in use on another PC, and offers Move Pro here.
   - Move Pro here: the second PC is Pro.
   - On the first PC, `%AppData%\app.clokrix.monitor\edition.json` holds `checked`. Set it a
     week back (subtract 604800), then restart Clokrix: within a minute it says Pro moved, and
     the PC is Free.
5. **Back.** On the first PC, enter the key: refused, as moved less than 30 days ago. On the
   second PC, Settings → Clokrix Pro → Free this PC. Now the first PC takes the key.
6. **Free.** To see the trial's end without waiting 7 days, use a third PC, or a virtual
   machine. On it, quit Clokrix, set `trial_started` in `edition.json` 8 days back (subtract
   691200), and start it.
   - A dialog says the trial has ended; Not now closes it, and it does not come back.
   - The Dashboard works. Every other view shows what Pro adds, with a lock in the sidebar.
   - Settings → Remote, Gaming, Overlay and Profile say "Part of Clokrix Pro".
   - If fan control or GPU tuning was on, the fans are back on the BIOS and the GPU at stock.
     Desktop widgets and the overlay are gone, and the tray's Profile, Game overlay and Desktop
     widgets are greyed out.
7. **A refund.** In Razorpay (Test mode), refund the test payment. Set `checked` back a week on
   the PC with the key, as in step 4, and restart Clokrix: it says the payment was refunded, and
   the PC is Free.

## 4. Glass

On Windows 11 (Settings → System → About shows version 22H2 or later), with a colourful
wallpaper.

1. **The window.** The desktop shows through Clokrix's window, frosted, behind the cards. Drag
   the window and resize it: both stay smooth. Click another window: Clokrix's turns solid, as
   Windows' own apps do, and comes back when clicked.
2. **Reading it.** Change the wallpaper to a plain white one, then a black one. Every text
   reads well on both, grey text included.
3. **The glass.** Cards are see-through over a colour wash, lit along their top edge. Ctrl K,
   the Settings window and What's new frost what is under them. Scrolling a long view feels as
   before, and typing in Ctrl K is as quick as before.
4. **Each theme.** Settings → Appearance: each theme in turn. AMOLED stays pure black, and its
   window is not see-through.
5. **Off.** Settings → Appearance → Glass effects off: everything is solid, as in 2.4, and the
   window is opaque. Back on.
6. **Windows' switch.** Windows Settings → Personalisation → Colours → Transparency effects
   off, then click Clokrix: the glass goes, and Glass effects says it is off in Windows. Back
   on.
7. **Elsewhere.** A desktop widget and the game overlay (in a game) are lit along their edge.
   The phone app (after the relay is deployed) has a still wash behind the cards and frosted
   bars.
8. **Windows 10**, if you have a PC with it: the glass is inside the window only; the window is
   opaque, as before.

## Report

"Everything is working", or what differs. For §3, send `%AppData%\app.clokrix.monitor\
edition.json` from each PC, with its `key` line removed. For §4, a screenshot of anything that
looks wrong, with the wallpaper behind it.

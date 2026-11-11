# Field check 2.2

What only a real PC that had Vyro can show: the update from Vyro to Clokrix through the app's own
updater, with everything Vyro kept still there, and the new name and logo where Windows and the
phone show them. CI checks the same upgrade on a fresh runner (Vyro 2.1.0's MSI, then 2.2's over
it), but not a PC used for weeks, its phones, or what Windows draws.

Before it: the relay deployed at its new address and `CLOKRIX_RELAY_URL` set
([`docs/remote.md`](remote.md#the-relays-new-address-22)), and 2.2.0 released.

About 20 minutes.

## 0. Before the update, on Vyro 2.1.0

1. Note what you would miss: the theme and accent, the Dashboard's layout, a view's card order,
   the desktop widgets and where they sit, your alert rules, a fan curve (MS-7E24), a game
   profile, and whether "Start with Windows" is on (Settings → General).
2. Make a share card (Hardware → **Make a share card** → **Save PNG**) and a diagnostics report
   (Health → Diagnostics → **Export**), so that `Pictures\Vyro` and `Documents\Vyro Reports` have
   something in them.

## 1. The update

1. Vyro: Settings → Updates → **Install 2.2.0**. It closes, the installer runs, and it opens
   again as Clokrix.
2. The window's title, the tray icon and the taskbar icon are Clokrix's: the X, drawn larger
   than in the big icon. The Start menu has **Clokrix**, and Vyro is gone from it.
3. Settings → Apps: one **Clokrix**, published by Clokrix; no Vyro.
4. What's new shows 2.2.0: "Vyro is now Clokrix" and "Pair your phones again".

## 2. Everything kept

1. Everything noted in 0.1 is as it was.
2. History reaches back before the update (History → 7 days).
3. Share cards and reports are in `Pictures\Clokrix` and `Documents\Clokrix Reports`, with the
   ones made in 0.2; `Pictures\Vyro` and `Documents\Vyro Reports` are gone.
4. Restart Windows. If "Start with Windows" was on, Clokrix starts in the tray; Task Manager →
   Startup apps lists Clokrix, not Vyro. Services: **Clokrix Sensor Service** running, no Vyro
   Sensor Service. The CPU's temperature and the board's sensors read as before.

## 3. The logo

1. The sidebar shows the logo with its pixel scatter and "hardware monitor"; folded, the X alone.
   In Daylight its strokes are near-black, in every other theme white; magenta and cyan stay
   whatever the accent.
2. Health → Alerts → **Send test notification**: the notification is from Clokrix, with its icon.
3. A share card shows the logo above the title.

## 4. Your phones

1. Settings → Remote → From anywhere → **Pair a device**; scan the code on the phone. The new
   app opens at the `clokrix-relay` address and pairs.
2. Add it to the home screen: its icon is Clokrix's. Turn notifications on (Android; on iPhone,
   from the home-screen app), then **Send a test**: it arrives.
3. The screen view works as before. Remove the old Vyro app from the home screen.
4. On the same network, a phone that used the companion page (`http://<PC>:8976`) is still signed
   in. If Windows asks whether Clokrix may accept connections, allow it on private networks.

## 5. The installer (optional, any spare PC)

1. Run `Clokrix_2.2.0_x64_en-US.msi` by hand: its first page shows the logo on a dark band on the
   left, the other pages the small icon at the top right.

Tell me what differs from the above, with a screenshot where it is about how something looks.

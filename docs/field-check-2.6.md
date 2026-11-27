# Field check 2.6

What only a real PC, real RGB devices, a real phone and real games can show for 2.6:
- ambient lighting with your own devices, a game and a film;
- the phone wheel in Euro Truck Simulator 2, American Truck Simulator and SnowRunner, and its
  delay on your Wi-Fi;
- your own PC on the owner's dashboard.

The tests cover the rest:
- the colour picker, OpenRGB's protocol against a stand-in server, and the GPU's small picture
  on Windows' software renderer;
- the wheel's packets, letting go and the dead zone; the phone's steering arithmetic; the
  phone, the relay and a PC played by the companion example end to end, the wheel steering
  and letting go;
- the dashboard in workerd with Razorpay played by a stand-in, and in Chromium with virtual
  passkeys.

**Before you start:**
- Install the test build `clokrix-windows-<commit>` of the latest green CI run on
  `claude/vyro-2.0`.
- The website and the relay deployed from it ("deploy the licence", "deploy the relay"): the
  phone's Wheel tab comes from the relay.
- For §1: nothing to install first; Clokrix sets OpenRGB up (2.6.1). If OpenRGB is already on
  the PC, remove it in Windows' Apps to try that, or skip §1's step 1. Close the lighting in
  iCUE, Synapse, G HUB or Armoury Crate.
- For §2: a phone paired from anywhere, on the same Wi-Fi as the PC; Euro Truck Simulator 2,
  American Truck Simulator or SnowRunner (any you have).

About an hour.

## 1. Ambient lighting

1. **Set up OpenRGB** (2.6.1). **Lighting** in the sidebar; turn on "Light my devices with the
   screen's colour": "OpenRGB isn't on this PC" with **Set up OpenRGB**. Press it: Downloading
   (about 23 MB), then one Windows prompt for an administrator and a small progress bar, then
   "Starting OpenRGB…" for up to two minutes while it finds your devices. Note how long each
   took. In Services (`services.msc`), "OpenRGB" is Running, Automatic. Restart the PC: with
   Clokrix open and OpenRGB not opened by hand, the devices come back on their own.
2. **Devices.** OpenRGB's devices are listed; those that cannot follow the screen say why. Pick
   your keyboard, mouse and any other device. RGB memory and boards are listed too (OpenRGB's
   service reaches them through PawnIO).
3. **A still picture.** Open a mostly dark picture with one bright red sign (a search for
   "neon sign" will do), full screen: the devices go red within a moment. A grey document or the
   desktop with no colour: white (or Clokrix blue or off, as "When the screen has no colour"
   says).
4. **A film** (Speed: Smooth or Auto): the devices follow the scenes without flicker or
   flashing, even in a film's fast cuts or explosions.
5. **A game** (Speed: Auto): the devices follow quickly. Write down:
   - Clokrix's CPU use in Task Manager (Details, `clokrix.exe`), lighting on and then off;
   - the game's frame rate and its present mode (Clokrix's overlay, or PresentMon), lighting on
     and then off. They should be the same.
6. **HDR**, if your monitor has it: Windows' HDR on, the same picture and film: the colours are
   the same as without HDR, not washed out.
7. **A phone screen session** while lighting is on (the phone's Screen tab): the screen shows
   on the phone as before, and the devices keep following the screen. Stop it: lighting goes on.
8. **Off.** Turn lighting off: every device goes back to the effect it had before. Turn it on
   again and quit Clokrix from the tray: back to their effects again.
9. **Memory devices** (RGB RAM sticks), if you have them: their colour changes a little more
   slowly; the Dashboard's readings stay smooth.

## 2. The phone wheel

1. **Set up.** Settings → Remote → Wheel → **Set up the virtual controller**. One Windows
   prompt for an administrator; then "The virtual controller (ViGEmBus) is set up". Note
   whether Memory Integrity is on (Windows Security → Device security → Core isolation): it
   should install either way. If it asks for a restart, restart and look again.
2. **Allow the phone.** Settings → Remote → Phones: "No wheel" next to your phone turns into
   "Steers games". The phone's Wheel tab now offers **Start steering**.
3. **Start.** Hold the phone like a steering wheel, its screen towards you, and tap Start
   steering. The phone goes full screen and sideways. The PC shows "… is steering games" with
   Stop, and a notification.
4. **Windows sees a controller.** Press Win+R, type `joy.cpl`: "Controller (XBOX 360 For
   Windows)" is listed. Its Properties: turning the phone moves the cross left and right; the
   right edge (throttle) and left edge (brake) move the Z axis; the buttons light up.
5. **The delay.** The phone's top line says "Round trip … ms". Write it down, on the same
   Wi-Fi as the PC; then with the PC on a cable and the phone on Wi-Fi, if you can.
6. **Euro Truck Simulator 2** (and American Truck Simulator): Options → Controls → choose the
   controller. Steering on the left stick, throttle on the right trigger, brake on the left
   trigger. In the game's controller settings, set the stick's dead zone to its least, or set
   Clokrix's "Make up for a game's dead zone" to match. Drive a few minutes:
   - Does the truck steer smoothly, with no jumps? Does a small turn of the phone steer a little?
   - "Full lock at" on the phone (before Start): try 60°, 90° and 120°; which feels right?
   - The pedals: press each harder by sliding your thumb up.
7. **SnowRunner:** the same, with its own controller settings. If the game reads the triggers as
   one, set Settings → Remote → Wheel → The pedals to "On the right stick".
8. **Letting go.** Driving with the throttle held, turn off the phone's Wi-Fi: within half a
   second the truck stops accelerating and the wheel goes back to the middle. Wi-Fi back on: the
   phone says the session ended; start again.
9. **Rumble** (Android): in a game that rumbles a controller, the phone vibrates.
10. **Stop.** Stop on the PC's banner: the phone says it ended. Start again, then Stop on the
    phone: the PC's banner goes.
11. **Not allowed.** Settings → Remote → Phones: "Steers games" back to "No wheel" while
    steering: the session ends at once, and the phone's Wheel tab says it may not steer.

## 3. The owner's dashboard

1. Sign in at the website's `/dashboard` with your passkey. Every key is listed; the tiles add
   up.
2. **Your PC on it.** On the PC with Clokrix 2.6 and a Pro key, Settings → Clokrix Pro shows Pro.
   On the dashboard, that key's PC shows the PC's name, processor, graphics card, Windows and
   Clokrix 2.6.0 (within a day of the update; at once for a key entered now).
3. **A name.** Buy a test key in Razorpay's Test mode: the checkout asks for a name before Pay.
   The dashboard shows that name, its email and phone.
4. **Cancel and restore** a test key on a second PC (or a virtual machine): Cancel, then on that
   PC set `checked` back a week (as in the 2.5 field check, §3 step 4) and restart Clokrix: it
   says "This key was cancelled by its seller" and the PC is Free. Restore it, do the same: Pro
   again.
5. **On the phone:** the dashboard on your phone's browser: the keys are cards, nothing runs
   past the side.

## Report

"Everything is working", or what differs. For §1, how long Set up OpenRGB took, the CPU use and
frame rates written down; for §2, the round trips, which "Full lock at" felt right in each game,
and anything a game refused; for §3, a screenshot of anything that looks wrong (blur the buyers'
details first).

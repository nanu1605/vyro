# Field check: 1.6 (GPU tuning)

Status: **to do.** Test build: the `vyro-windows-<commit>` artifact of the latest green CI run on
`claude/vyro-1.6`, version 1.6.0. Install the MSI over 1.5. About 90 minutes on the desktop
(most of it Find my best, which can run unattended) and 30 on the laptop.

1.6 answers the owner's request for GPU overclocking with presets made for the installed card and
custom settings (ROADMAP → 1.6.0, decision 37). Run it on the NVIDIA desktop card and on the
laptop. Close MSI Afterburner, EVGA Precision and other tuning tools first, and save your work:
a step that fails can crash a game, the driver or the PC, which is part of the check.

## 1. What the driver allows (the first thing to know)

- GPU tuning (Performance group). Before turning it on, note the card, its family and driver.
- Turn it on: the confirmation lists the risks; Turn on. Note what **Now** shows and whether
  the page says the driver does not allow changing something.
- **Custom**: note the three ranges (core, memory, power) the sliders allow.
- On the laptop, expect offsets but often no power limit.
- If **core and memory offsets are missing on the desktop card**, NVML does not offer them on
  GeForce under Windows with this driver: write down the driver version. Vyro then needs NVIDIA's
  other interface (NVAPI) for offsets, the next step after this check. The power limit and
  the rest of this sheet can still be checked.

## 2. Presets

- Apply **Quiet**: the top bar shows "GPU tuned" with Stock; the tray's tooltip says "GPU
  tuned"; Now shows the lower power limit. In a game or a benchmark the card draws less power.
- Apply **Performance**: Now shows its offsets and the highest power limit. Run the Benchmark's
  GPU part: the score is a little higher than at Stock.
- **Stock** in the top bar: the card is at stock, and the page says to apply a preset to tune
  again. "Return GPU to stock" in the tray does the same.
- Settings of a profile: Options → Gaming → Performance. Switch the profile to Gaming (tray):
  Performance goes on; back to Desktop: what was chosen above.

## 3. Your own settings and the stability test

- Custom: core +60, memory +200, name "Mine" → Save and apply. It runs.
- **Test** on Mine: about 5 minutes, with progress; leave the PC alone. Passed → Mine shows
  "tested" with the date.
- Test something too high (core +400 or the most the slider allows): it fails within a few
  rounds with a reason, or the screen goes black for a moment and the driver resets. Either
  way the card comes back at its preset, and the log says so.

## 4. Find my best

- Find my best (20 to 40 minutes). It may reset the driver a few times; that is expected. At the
  end: "Best for this card" with its values, marked tested. Apply it and play a game for a while.
- Note the result and how long it took.

## 5. Safeguards

- **Apply at start** on, with a tested preset chosen. Restart the PC: after sign-in the preset
  is on. With an untested preset chosen: the card stays stock and the page says why.
- **Crash guard**: with a preset applied and Apply at start on, pull the power (or hold the
  power button). After starting again: the card is at stock, the page says what happened, and
  Apply at start is off.
- **Another program**: with a preset applied, open MSI Afterburner and change the core clock
  three times within two minutes: Vyro writes its values back twice, then stops tuning and says
  another program keeps changing them.
- **Sleep**: with a preset applied, sleep and wake the PC: the preset is on again afterwards.
- **Heat** (optional, if the card can reach it): Vyro returns it to stock at 90 °C or 105 °C
  hotspot.

## 6. From the phone

- With a phone allowed to change the PC (1.5): More → GPU tuning shows what runs, with Stock and
  the tested presets only. Apply one, then Stock. Settings → Remote → Recent from your phones
  lists both.

## 7. Nothing else changed

- With GPU tuning off, no NVIDIA setting changes (Now shows stock values throughout).
- Task Manager: Vyro's idle CPU and memory as with 1.5.

## Results

| Step | Desktop | Laptop | Notes |
| --- | --- | --- | --- |
| 1. What the driver allows |  |  |  |
| 2. Presets |  |  |  |
| 3. Your own settings and the test |  |  |  |
| 4. Find my best |  |  |  |
| 5. Safeguards |  |  |  |
| 6. From the phone |  |  |  |
| 7. Nothing else changed |  |  |  |

# Vyro Performance Score: formula v1 (draft for sign-off)

Status: **draft, awaiting the owner's sign-off** (decision 22). Nothing is built until it is
signed off; after that this formula is fixed, and any change to an input, a breakpoint or a weight
becomes formula v2. Every stored score keeps its version, so older scores stay comparable.

## What it measures

**How well this PC is running compared with how it should run. It does not measure how fast the
hardware is.** A budget PC in good shape scores high. A top-end PC that throttles, runs hot for its
load, has a failing drive or keeps crashing scores low.

- **Observed:** it uses only what Vyro already records while the PC is used normally. It runs no
  benchmark and adds no load (benchmarks are P20).
- **Self-relative:** clocks and temperatures are judged against the PC's own limits and its own
  history, never against other PCs or a hardware database.
- **Shown in full:** every input appears with its value, its sub-score, its weight and whether it was
  measured or inferred, and a missing input says why it is missing.

## The score

`score = Σ (weight × sub-score) / Σ weight`, summed over the inputs that have data, and rounded to
a whole number from 0 to 100. Each sub-score is also 0–100, read from fixed breakpoints with
straight lines between them; beyond the end breakpoints the value stays flat.

| Band | Score |
|---|---|
| Excellent | 90–100 |
| Good | 75–89 |
| Fair | 50–74 |
| Needs attention | 0–49 |

**Coverage.** When an input has no data, its weight is shared out among the others, and the result
says so ("based on 7 of 9 inputs, 85 % of the weight"). Below 60 % of the weight there is no score:
the view shows "Not enough data yet" and lists what is missing. A new install gets its first score
after about a day of history. Cooling and the change from usual join after 3 days, once their
baselines exist (as in P16).

**Window.** The window is the last 7 days, the same span as the 1-minute history. Stability looks
back 30 days in the Windows event log. In this document, a "loaded minute" is one in which the CPU
or the GPU averaged at least 50 % load.

## Inputs, weights and breakpoints

| # | Input | Weight | Basis |
|---|---|---:|---|
| 1 | Thermals | 15 | measured |
| 2 | Sustained clocks | 10 | measured |
| 3 | Throttling | 15 | measured |
| 4 | Cooling | 10 | measured, against a baseline |
| 5 | Storage health | 10 | measured |
| 6 | Memory pressure | 10 | measured |
| 7 | Network quality | 5 | measured |
| 8 | Stability | 15 | measured (Windows event log) |
| 9 | Change from usual | 10 | inferred (P16 findings) |
| | **Total** | **100** | |

### 1. Thermals (15)
**Input.** The headroom: the device's limit minus its median temperature in loaded minutes. The
median is used rather than the peak, so a brief boost spike costs nothing.
- **CPU limit.** This is the CPU's design maximum: 95 °C for AMD Ryzen 7000/9000 (which run up to
  it by design), 100 °C for Intel Core, and 95 °C when Vyro cannot tell.
- **GPU limit.** This is the edge temperature at which Health warns, 85 °C.

| Headroom | ≥ 25 °C | 15 °C | 10 °C | 5 °C | 0 °C | < 0 °C |
|---|---|---|---|---|---|---|
| Sub-score | 100 | 85 | 70 | 50 | 20 | 0 |

The sub-score is the worse of the CPU's and the GPU's. The input is missing without at least 60
loaded minutes in the window.

### 2. Sustained clocks (10)
**Input.** How much the clock sags as a long stretch of full load goes on (heat soak).
- **Stretch.** A stretch is at least 5 minutes in a row at 85 % load or more: the CPU, or the GPU
  for its core clock.
- **Sag.** Sag = 1 − (the clock in the stretch's last 2 minutes ÷ its clock in minutes 2–3). Minute 1
  is skipped because it is partial.
- **Combining stretches.** Each stretch counts in proportion to its length.

| Sag | ≤ 2 % | 5 % | 10 % | ≥ 20 % |
|---|---|---|---|---|
| Sub-score | 100 | 75 | 40 | 0 |

The sub-score is the worse of the CPU's and the GPU's. The input is missing without such a stretch
in the window: a PC that never runs at full load is not penalised.

### 3. Throttling (15)
**Input.** The share of loaded time during which the hardware itself reported limiting for heat or
current. These are the CPU's thermal and current flags and the GPU's thermal flag, already counted
second by second in the P16 minute digest.
- **Power limiting** (PPT, a GPU's power target) is how CPUs and GPUs normally hold their power
  budget, so it does not count.
- **No flags reported.** On hardware that reports no flags at all the input is missing; it is not
  guessed. P16 still infers throttling from clocks for its findings.

| Share of loaded time | 0 % | 0.5 % | 2 % | 5 % | ≥ 15 % |
|---|---|---|---|---|---|
| Sub-score | 100 | 90 | 70 | 40 | 0 |

### 4. Cooling (10)
**Inputs.** These come from P16's baselines and need 3 days of history:
- **Temperature for the load.** How much hotter the CPU runs now than its usual for the same load,
  measured over the last 30 minutes and leaving out the last hour of the week.
- **Fan response.** The CPU fan's speed against its usual speed at the same CPU temperature.

| Hotter than usual for the load | ≤ +2 °C | +5 °C | +10 °C | ≥ +15 °C |
|---|---|---|---|---|
| Sub-score | 100 | 80 | 40 | 0 |

| Fan speed vs usual | ≥ 90 % | 70 % | ≤ 50 % |
|---|---|---|---|
| Sub-score | 100 | 60 | 20 |

**Combining.** The sub-score is the worse of the two.

**When the profile explains it.** If P16 traces a slower fan or a hotter CPU to the owner's own
choice (the Silent profile, or a fan curve they set), that part counts as 100: it is how the owner
wants the PC to run, not a fault.

### 5. Storage health (10)
**Input.** The internal drives, read from SMART or the NVMe health log.

| Drive health (worst drive) | good | caution | bad |
|---|---|---|---|
| Sub-score | 100 | 50 | 0 |

| Life left (worst SSD) | ≥ 30 % | 10 % | 0 % |
|---|---|---|---|
| Sub-score | 100 | 70 | 30 |

| Free space on the system drive | ≥ 15 % | 10 % | 5 % | ≤ 2 % |
|---|---|---|---|---|
| Sub-score | 100 | 85 | 60 | 30 |

**Combining.** The sub-score is the lowest of the three. When SMART is not readable, only free space
counts.

### 6. Memory pressure (10)
**Input.** The 95th percentile of the share of RAM in use, minute by minute over the window.

| RAM in use (p95) | ≤ 75 % | 85 % | 92 % | ≥ 98 % |
|---|---|---|---|---|
| Sub-score | 100 | 80 | 50 | 10 |

### 7. Network quality (5)
**Input.** The ping to the configured target (default 1.1.1.1).

| Round trip (p95 of minute averages) | ≤ 40 ms | 80 ms | 150 ms | ≥ 300 ms |
|---|---|---|---|---|
| Sub-score | 100 | 80 | 50 | 10 |

| Lost replies | 0 % | 1 % | 3 % | ≥ 10 % |
|---|---|---|---|---|
| Sub-score | 100 | 80 | 50 | 0 |

**Combining.** The sub-score is the worse of the two.
- **Missing.** The input is missing while ping is off or the PC is offline.
- **Weight.** It weighs only 5 because it measures the connection more than the PC.
- **New history metric.** P17 adds lost replies to the history; today only the round trip is kept.

### 8. Stability (15)
**Inputs.** These come from the Windows System event log over the last 30 days:
- **Unexpected shutdowns.** Kernel-Power 41: the PC stopped without shutting down, from a crash, a
  hang or a power cut. Vyro already reads this for System's power history.
- **Hardware errors.** WHEA-Logger events: fatal ones (18), and corrected ones (17, 19 and 47). These
  are machine-check and memory errors that the CPU or the chipset reported. This is new: the same
  event log query, one more provider.

| Unexpected shutdowns (30 days) | 0 | 1 | 2 | ≥ 3 |
|---|---|---|---|---|
| Sub-score | 100 | 70 | 45 | 20 |

| Hardware errors (30 days) | none | 1–9 corrected | 10–99 corrected | ≥ 100 corrected, or any fatal |
|---|---|---|---|---|
| Sub-score | 100 | 80 | 50 | 20 (0 if any fatal) |

**Combining.** The sub-score is the worse of the two.

### 9. Change from usual (10)
**Input.** The P16 findings worth a look now. These are warnings only: an explanation of the owner's
own choice does not count.

Start at 100 and subtract 25 for each high-confidence warning, 15 for each medium one and 10 for each
low one, never going below 0. The basis is inferred, because the findings are.

## When it is worked out and kept
- **Recording.** The score is worked out once a day by the intelligence pass and also when the
  score view opens (the view's own result is not stored). That is one stored result per day, kept
  for a year: about 1 KB per day, under 400 KB in total.
- **What a stored result holds.** Each result holds:
  - the formula version, the time and the window
  - the score, its band and the coverage
  - for every input: the value, unit, sub-score, weight, basis, and the reason when missing
- **Reproducibility.** The same inputs always give the same score. Tests cover every breakpoint, the
  coverage rule and each "missing" path. A synthetic week of healthy data has to score ≥ 90, and the
  same week with throttling, a failing drive or crashes has to fall into the expected band.

## Worked example (reference PC, illustrative)
This example uses a Ryzen 7 9700X with an RTX 5060 Ti, healthy NVMe drives, 32 GB of RAM and two
evenings of gaming.

| Input | Value | Sub-score | Weight |
|---|---|---:|---:|
| Thermals | CPU 78 °C median under load (17 °C headroom); GPU 62 °C (23 °C) | 88 | 15 |
| Sustained clocks | CPU sag 1.4 %, GPU 0.8 % | 100 | 10 |
| Throttling | 0 % (CPU and GPU report flags) | 100 | 15 |
| Cooling | +1 °C for the load; fan at 98 % of usual | 100 | 10 |
| Storage health | both drives good, 96 % life left, C: 41 % free | 100 | 10 |
| Memory pressure | p95 61 % in use | 100 | 10 |
| Network quality | p95 18 ms, 0.1 % lost | 98 | 5 |
| Stability | no unexpected shutdowns, no hardware errors | 100 | 15 |
| Change from usual | no warnings | 100 | 10 |
| **Score** | | **98, Excellent** | 9 of 9 |

## Open points for the sign-off
1. **Weights.** Thermals 15, Sustained clocks 10, Throttling 15, Cooling 10, Storage 10, Memory 10,
   Network 5, Stability 15, Change from usual 10.
2. **Network.** Keep it at weight 5, or leave it out (it is the connection, not the PC).
3. **Stability.** Add the WHEA hardware errors (recommended), or use unexpected shutdowns only.
4. **Where it shows.** For example at the top of Insights, with its inputs and a one-year trend.

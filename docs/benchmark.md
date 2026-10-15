# Vyro Benchmark: method v1

Status: **defaults chosen at build (2026-10-15)**. Decision 23 settles the direction: Vyro's own
workloads, results compared only with this PC's earlier runs, and the variance always reported. The
owner can change the areas, durations and limits below at the P20 field check. After the first
release that carries them, any change to a workload, a duration or a metric becomes method v2.
Every stored run keeps its method version, and runs are only compared within one version.

## What it is for

**A benchmark answers one question: is this PC still as fast as it was?** It runs fixed workloads,
watches the PC while they run, and compares the result with this PC's earlier runs. Typical uses
are before and after a driver, BIOS, cooling or RAM change, after cleaning the dust out, or when
the PC feels slow.

- **Not cross-tool.** The numbers are not comparable with 3DMark, Cinebench or another PC. Points
  are Vyro's own units.
- **Variance is part of the result.** Windows background work, temperatures and power plans make
  single runs noisy. So every area runs 3 measured passes, and a result shows the median and the
  spread. A comparison counts a difference only beyond that noise.
- **Watched.** Every area records peak temperatures and power, throttled seconds and the clocks,
  from the same sensors as the rest of Vyro.

## Modes

| Mode | Areas | Time |
|---|---|---|
| **Quick** | CPU single-thread, CPU multi-thread, memory, storage, GPU compute, graphics | about 3 minutes |
| **Full** | Quick, plus Sustained (10 minutes of CPU and GPU load together) | about 13 minutes |

While a run is going on, Vyro switches to the **Benchmark** profile: readings twice a second and
quiet alerts, plus that profile's own fan settings if the owner gave it any. It switches back when
the run ends. As with games, a profile the owner picks during the run wins.

## Before a run

**Vyro refuses to start:**
- while a game runs;
- while a temperature is already at Health's warning level;
- while another run is going on.

**Vyro starts but notes it in the result:**
- **Busy PC.** Background CPU use was above 10 % while the run settled for its first 3 seconds,
  before any load. The result is marked *busy*, because its numbers are likely low.
- **No CPU temperature.** The CPU temperature is not read (no PawnIO). The run then cannot stop
  on CPU heat, though the CPU's own protection still applies.
- **Storage skipped.** The storage area is skipped when the drive with the temporary folder has
  less than 2 GiB free.

## Areas and metrics

**Measuring.** Each area runs a warm-up pass, which is not counted, then 3 measured passes.
- **Result:** the median of the 3 passes.
- **Spread:** (highest − lowest) ÷ median.
- **Load:** all CPU work runs at normal priority with Windows' power throttling (EcoQoS) turned off
  for the run, so a minimised Vyro is not slowed down.

| # | Area | Metric | Unit | Better |
|---|---|---|---|---|
| 1 | CPU single-thread | Points | rounds/s | higher |
| 2 | CPU multi-thread | Points | rounds/s | higher |
| | | Scaling (multi ÷ single) | × | shown only |
| 3 | Memory | Read | GB/s | higher |
| | | Copy | GB/s | higher |
| | | Latency | ns | lower |
| 4 | Storage | Sequential read | MB/s | higher |
| | | Sequential write | MB/s | higher |
| | | Random 4K read (QD1) | IOPS | higher |
| 5 | GPU compute | FP32 throughput | GFLOPS | higher |
| | | Memory bandwidth | GB/s | higher |
| 6 | Graphics | Frame rate | fps | higher |
| | | 1 % low | fps | higher |
| 7 | Sustained (Full) | Stability | % | higher |
| | | Clock sag, CPU and GPU | % | lower |
| 8 | Efficiency | CPU points per watt | pts/W | higher |
| | | Graphics frames per joule | fps/W | higher |
| 9 | Stability | Calculation errors, hardware errors, GPU resets | count | lower |

### 1–2. CPU
**Round.** One round runs four fixed kernels, each on its own deterministic input:
- **Hash:** 64-bit multiply-and-shift mixing over 16 KiB (integer ALU, L1).
- **Match:** greedy LZ-style match search over 64 KiB with a hash table (branches, L2).
- **Matrix:** a 32 × 32 double-precision matrix product (floating point).
- **Sort:** sorting 8 192 32-bit integers (branches, memory).

**Points.** Points are rounds completed per second: one thread for single-thread, one thread per
logical processor for multi-thread.

**Check.** Each round yields a checksum, which must equal the reference worked out before the run.
A mismatch counts as a calculation error (area 9), the way Prime95 finds an unstable PC.

**Passes.** A warm-up of 2 s, then passes of 4 s.

### 3. Memory
**Buffers.** Two 256 MiB buffers, freed after the area.
- **Read.** Every logical processor sums its share of a buffer. The sum is known in advance, so a
  wrong one counts as an error.
- **Copy.** Every logical processor copies its share from one buffer to the other. Bytes read plus
  bytes written count.
- **Latency.** One thread follows a chain of 64 Mi indices over 256 MiB in a random cycle (Sattolo's
  algorithm, fixed seed), so neither the caches nor the prefetchers help. The result is
  nanoseconds per step.

**Passes.** Each pass lasts at least 1 s.

### 4. Storage
**The file.** A 1 GiB temporary file on the drive that holds the Windows temporary folder (normally
C:), written with incompressible data. The file is read and written uncached (no buffering, write
through), so Windows' cache does not count.
- **Sequential.** 1 MiB blocks, from 4 threads, each on its own quarter of the file.
- **Random 4K read.** 4 KiB at random aligned offsets, one at a time (queue depth 1), for 4 s. This
  is the access most felt as "snappy".

**Passes.** A pass stops after 8 s even if the file is not done, so a slow hard drive still
finishes. The rate is the bytes done ÷ the time.

**Writes and cleanup.**
- About 3 GiB are written per run, a negligible fraction of an SSD's rated endurance.
- The file is deleted when the area ends, and also when the run stops.
- A file left over after a crash is deleted at the next run.

### 5–6. GPU
**Which GPU.** The high-performance GPU, as Windows ranks them (the discrete card on a PC with an
iGPU as well), through Direct3D 11. The result names the GPU and its driver. Without a hardware
Direct3D 11 GPU, both areas are missing.

**GPU compute:**
- **FP32 throughput.** A compute shader runs long chains of fused multiply-adds. The figure counts
  2 operations per multiply-add.
- **Memory bandwidth.** A compute shader copies a 128 MiB buffer to another in video memory. Bytes
  read plus bytes written count.
- **Timing.** Work goes out in batches, two in flight, and is timed from the first batch submitted
  to the last one finished.
- **Check.** A third shader works out integer hashes, and the CPU checks a sample of them. A wrong
  value counts as a calculation error.

**Graphics.** An offscreen scene at 2560 × 1440, made of many lit objects drawn as 400 instanced draw
calls a frame, with a pixel shader heavy enough to keep the GPU busy. Two frames may be in flight,
as in games.
- **Frame rate:** frames ÷ seconds.
- **1 % low:** 1000 ÷ the 99th-percentile frame time in ms.
- **Passes:** 8 s each.

**Batches.** All GPU work is submitted in batches that take at most about 50 ms, so a stop takes
effect quickly and Windows never resets the driver for a too-long batch.

### 7. Sustained (Full only)
**The load.** The CPU kernels on every logical processor but one, and the graphics scene on the
GPU, together for 10 loops of 1 minute. This is the heaviest heat soak, as in a long gaming session.
- **Stability:** the worst loop ÷ the best loop, for the CPU (points) and for the GPU (fps). The
  lower of the two counts. 97 % or more reads as steady.
- **Clock sag:** the average clock in the last loop compared with the first, for the CPU and for
  the GPU.

### 8. Efficiency
- **CPU:** multi-thread points ÷ the average CPU package power during those passes.
- **Graphics:** frame rate ÷ the average GPU board power during those passes.

Each is missing when its power is not read.

### 9. Stability
**What counts.** The run's error counts:
- calculation errors from every checked workload;
- WHEA hardware errors logged during the run;
- GPU driver resets (Direct3D "device removed").

**Result.** Any of these marks the result as *errors found*. Its message lists what usually causes
them: an unstable overclock or undervolt, memory, or heat.

## What every area records
**Recorded for each area:**
- the peak CPU temperature and package power
- the peak GPU temperature and board power
- the seconds throttled: the CPU's thermal or current flag, or the GPU's thermal flag (as in
  Performance Score formula v1)
- the seconds the fans ran at full speed for safety
- the average CPU and GPU clock

**The setup** recorded with the run:
- the CPU, cores and threads
- the GPU and its driver
- the RAM size and speed
- the board and BIOS version
- the Windows build and the power plan
- the Vyro profile and fan profile in force
- the storage drive tested
- the background CPU use before the start

## Stopping
**Vyro stops a run, ending all load, when:**
- the owner presses Stop, closes the Vyro window (also when it hides to the tray) or quits;
- a temperature reaches Health's problem limit (CPU 99 °C, GPU 92 °C) on 2 readings in a row;
- Health reports a new problem with the CPU, GPU, fans or board thermals;
- no sensor reading arrives for 10 s, because then the run cannot watch the temperatures;
- the run takes twice its planned time.

**How fast.** Every worker checks for a stop at least every 50 ms, so all load ends within a second.
The runner waits for every worker thread and for the GPU's last batch, then deletes the temporary
file.

**What is kept.** A stopped run is kept, with the reason and the areas it finished, and it is never
used as a comparison baseline.

## Comparing runs
**Which run.** A result is compared with the newest earlier *completed* run of the same method on
this PC that was not marked busy. The Sustained metrics come only from the last Full run. The owner can also pick any two runs.

**What counts as a change.** Each metric shows its change in percent. It counts as faster or slower
only beyond the noise, which is the larger of 3 % and either run's spread. Otherwise it reads
"within noise".

**What changed in between.** Setup differences are listed with the comparison, such as "GPU driver
576.02 → 577.00" or "power plan Balanced → High performance". Hardware changes that P19 logged
between the two runs are listed too.

## Storage
**Where.** The `benchmark_runs` table holds one row per run (the whole result as JSON) and the
`benchmark_metrics` table one row per metric (the value and the spread), for the run list and its
trends.

**How much.** The newest 100 runs are kept, under 1 MB.

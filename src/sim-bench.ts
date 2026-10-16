// Browser preview of Benchmark mode: a few earlier runs on the reference PC (Ryzen 7 9700X,
// RTX 5060 Ti) and a live run that walks through the areas in seconds rather than minutes.

import type { AreaId, AreaResult, BenchMode, BenchProgress, BenchRun, BenchSetup, Better, Metric, Preflight, RunSummary, SustainLoop } from './types';

const D = 86_400;
const SETTLE = 3;
const PLANNED: Record<AreaId, number> = {
  cpuSingle: 14, cpuMulti: 14, memory: 18, storage: 40, gpuCompute: 16, graphics: 26, sustained: 600, efficiency: 0, stability: 0,
};
const PLAN: Record<BenchMode, AreaId[]> = {
  quick: ['cpuSingle', 'cpuMulti', 'memory', 'storage', 'gpuCompute', 'graphics'],
  full: ['cpuSingle', 'cpuMulti', 'memory', 'storage', 'gpuCompute', 'graphics', 'sustained'],
};
const planned = (mode: BenchMode) => SETTLE + PLAN[mode].reduce((s, a) => s + PLANNED[a], 0);

/** Deterministic jitter in ±`amp` from a seed. */
function jitter(seed: number, amp: number): number {
  const x = Math.sin(seed * 12.9898) * 43758.5453;
  return (x - Math.floor(x) - 0.5) * 2 * amp;
}

function measured(key: string, label: string, unit: string, better: Better, v: number, seed: number, amp = 0.01): Metric {
  const passes = [0, 1, 2].map((i) => v * (1 + jitter(seed + i * 7.1, amp)));
  const sorted = [...passes].sort((a, b) => a - b);
  const median = sorted[1]!;
  return { key, label, unit, value: median, spread: (sorted[2]! - sorted[0]!) / median, passes, better };
}

const derived = (key: string, label: string, unit: string, better: Better, value: number): Metric => ({
  key, label, unit, value, spread: null, passes: [], better,
});

const SETUP: BenchSetup = {
  vyro: '0.4.0',
  cpu: 'AMD Ryzen 7 9700X 8-Core Processor',
  cores: 8,
  threads: 16,
  gpu: 'NVIDIA GeForce RTX 5060 Ti',
  gpuDriver: '576.02',
  ramGb: 32,
  ramMts: 6000,
  board: 'Micro-Star International Co., Ltd. B650M GAMING PLUS WIFI (MS-7E24)',
  bios: '1.A0',
  windows: 'Windows 11 Pro 24H2 26100.4652',
  powerPlan: 'Balanced',
  profile: 'Benchmark',
  fanProfile: 'Benchmark',
  drive: 'C: · Samsung SSD 990 PRO 2TB',
  busyPct: 2.1,
};

interface Shape {
  /** Multipliers on the reference figures. */
  cpu?: number;
  gpu?: number;
  driver?: string;
  busy?: boolean;
}

function area(a: AreaId, seed: number, s: Shape): AreaResult {
  const cpu = s.cpu ?? 1;
  const gpu = s.gpu ?? 1;
  const base = { area: a, watch: null, missing: null, detail: null, loops: [] as SustainLoop[] };
  const watch = (w: Partial<NonNullable<AreaResult['watch']>>) => ({
    secs: PLANNED[a], cpuTempMax: null, cpuPowerMax: null, cpuPowerAvg: null, cpuMhzAvg: null, gpuTempMax: null,
    gpuPowerMax: null, gpuPowerAvg: null, gpuMhzAvg: null, throttledSecs: 0, fullSpeedSecs: 0, ...w,
  });
  switch (a) {
    case 'cpuSingle':
      return { ...base, detail: '1 thread', metrics: [measured('cpu.single', 'Points', 'pts', 'higher', 3950 * cpu, seed)],
        watch: watch({ cpuTempMax: 61, cpuPowerMax: 38, cpuPowerAvg: 34, cpuMhzAvg: 5520 }) };
    case 'cpuMulti': {
      const m = measured('cpu.multi', 'Points', 'pts', 'higher', 41_200 * cpu, seed + 1);
      return { ...base, detail: '16 threads', metrics: [m, derived('cpu.scaling', 'Scaling (multi ÷ single)', '×', 'shown', m.value / (3950 * cpu))],
        watch: watch({ cpuTempMax: 81, cpuPowerMax: 92, cpuPowerAvg: 88.6, cpuMhzAvg: 5180 }) };
    }
    case 'memory':
      return { ...base, metrics: [
        measured('mem.read', 'Read', 'GB/s', 'higher', 62.4, seed + 2),
        measured('mem.copy', 'Copy', 'GB/s', 'higher', 71.8, seed + 3),
        measured('mem.latency', 'Latency', 'ns', 'lower', 78.5, seed + 4, 0.006),
      ], watch: watch({ cpuTempMax: 66, cpuPowerMax: 61, cpuPowerAvg: 55 }) };
    case 'storage':
      return { ...base, detail: 'C:\\Users\\owner\\AppData\\Local\\Temp', metrics: [
        measured('disk.read', 'Sequential read', 'MB/s', 'higher', 6850, seed + 5, 0.02),
        measured('disk.write', 'Sequential write', 'MB/s', 'higher', 5120, seed + 6, 0.03),
        measured('disk.random', 'Random 4K read', 'IOPS', 'higher', 14_800, seed + 7, 0.02),
      ], watch: watch({ cpuTempMax: 58, cpuPowerMax: 41 }) };
    case 'gpuCompute':
      return { ...base, detail: SETUP.gpu, metrics: [
        measured('gpu.compute', 'FP32 throughput', 'GFLOPS', 'higher', 21_400 * gpu, seed + 8),
        measured('gpu.bandwidth', 'Memory bandwidth', 'GB/s', 'higher', 395 * gpu, seed + 9),
      ], watch: watch({ gpuTempMax: 63, gpuPowerMax: 178, gpuPowerAvg: 171, gpuMhzAvg: 2790 }) };
    case 'graphics':
      return { ...base, detail: `${SETUP.gpu} · 2560×1440 offscreen`, metrics: [
        measured('gfx.fps', 'Frame rate', 'fps', 'higher', 214.6 * gpu, seed + 10),
        measured('gfx.low', '1 % low', 'fps', 'higher', 188.2 * gpu, seed + 11, 0.015),
      ], watch: watch({ gpuTempMax: 66, gpuPowerMax: 186, gpuPowerAvg: 182, gpuMhzAvg: 2805, cpuTempMax: 63 }) };
    case 'sustained': {
      const loops: SustainLoop[] = Array.from({ length: 10 }, (_, i) => ({
        cpu: 38_300 * cpu * (1 - i * 0.0012 + jitter(seed + i, 0.003)),
        gpu: 201 * gpu * (1 - i * 0.0009 + jitter(seed + 30 + i, 0.003)),
        cpuMhz: 5150 - i * 6,
        gpuMhz: 2790 - i * 2,
        cpuTempMax: 79 + Math.min(i, 5),
        gpuTempMax: 64 + Math.min(i, 4),
      }));
      const ratio = (v: number[]) => (Math.min(...v) / Math.max(...v)) * 100;
      const c = ratio(loops.map((l) => l.cpu!));
      const g = ratio(loops.map((l) => l.gpu!));
      return { ...base, detail: '15 CPU threads + the graphics scene', loops, metrics: [
        derived('sustain.stability', 'Stability', '%', 'higher', Math.min(c, g)),
        derived('sustain.cpu', 'CPU loops, worst ÷ best', '%', 'shown', c),
        derived('sustain.gpu', 'GPU loops, worst ÷ best', '%', 'shown', g),
        derived('sustain.cpuSag', 'CPU clock sag', '%', 'lower', (1 - (5150 - 54) / 5150) * 100),
        derived('sustain.gpuSag', 'GPU clock sag', '%', 'lower', (1 - (2790 - 18) / 2790) * 100),
      ], watch: watch({ cpuTempMax: 84, cpuPowerMax: 90, cpuPowerAvg: 86, gpuTempMax: 68, gpuPowerMax: 184, gpuPowerAvg: 179 }) };
    }
    default:
      return { ...base, metrics: [] };
  }
}

function derivedAreas(areas: AreaResult[]): AreaResult[] {
  const multi = areas.find((a) => a.area === 'cpuMulti');
  const gfx = areas.find((a) => a.area === 'graphics');
  const eff: Metric[] = [];
  const pts = multi?.metrics.find((m) => m.key === 'cpu.multi');
  if (pts && multi?.watch?.cpuPowerAvg) eff.push(derived('eff.cpu', 'CPU points per watt', 'pts/W', 'higher', pts.value / multi.watch.cpuPowerAvg));
  const fps = gfx?.metrics.find((m) => m.key === 'gfx.fps');
  if (fps && gfx?.watch?.gpuPowerAvg) eff.push(derived('eff.gfx', 'Graphics frames per joule', 'fps/W', 'higher', fps.value / gfx.watch.gpuPowerAvg));
  return [
    { area: 'efficiency', metrics: eff, watch: null, missing: eff.length ? null : 'Needs the CPU package power or the GPU board power.', detail: null, loops: [] },
    {
      area: 'stability',
      metrics: [
        derived('stab.calc', 'Calculation errors', '', 'shown', 0),
        derived('stab.storage', 'Storage read-back errors', '', 'shown', 0),
        derived('stab.hardware', 'Hardware errors (WHEA)', '', 'shown', 0),
        derived('stab.resets', 'GPU driver resets', '', 'shown', 0),
      ],
      watch: null,
      missing: null,
      detail: 'No errors.',
      loops: [],
    },
  ];
}

function makeRun(id: number, ts: number, mode: BenchMode, s: Shape = {}, upTo?: number, reason?: string): BenchRun {
  const plan = PLAN[mode].slice(0, upTo ?? PLAN[mode].length);
  const areas = plan.map((a, i) => area(a, id * 31 + i, s));
  const stopped = upTo != null;
  return {
    id,
    ts,
    method: 1,
    mode,
    status: stopped ? 'stopped' : 'completed',
    reason: stopped ? reason ?? 'Stopped by you.' : null,
    secs: stopped ? plan.reduce((t, a) => t + PLANNED[a], SETTLE) : planned(mode) * 1.04,
    busy: !!s.busy,
    notes: s.busy ? ['The PC was busy before the start (23 % CPU): the results are likely low.'] : [],
    setup: { ...SETUP, gpuDriver: s.driver ?? SETUP.gpuDriver, busyPct: s.busy ? 23 : 2.1 },
    areas: [...areas, ...derivedAreas(areas)],
    errors: { calc: 0, storage: 0, hardware: 0, gpuResets: 0 },
  };
}

function initial(now: number): BenchRun[] {
  return [
    makeRun(1, now - 10 * D, 'full'),
    makeRun(2, now - 6 * D, 'quick', { busy: true, cpu: 0.9 }),
    makeRun(3, now - 3 * D, 'quick', {}, 3),
    makeRun(4, now - 1 * D - 3600, 'full', { cpu: 1.004 }),
    makeRun(5, now - 2 * 3600, 'quick', { gpu: 1.052, driver: '577.00' }),
  ];
}

let runs: BenchRun[] | null = null;
const all = () => (runs ??= initial(Math.floor(Date.now() / 1000)));

export function simRuns(): RunSummary[] {
  return [...all()]
    .sort((a, b) => b.id - a.id)
    .map((r) => ({
      id: r.id, ts: r.ts, method: r.method, mode: r.mode, status: r.status, busy: r.busy,
      errors: r.errors.calc + r.errors.storage + (r.errors.hardware ?? 0) + r.errors.gpuResets,
      metrics: r.areas.flatMap((a) => a.metrics).map((m) => ({ key: m.key, value: m.value, spread: m.spread })),
    }));
}

export function simRun(id: number): BenchRun | null {
  return all().find((r) => r.id === id) ?? null;
}

export function simDelete(id: number): boolean {
  const before = all().length;
  runs = all().filter((r) => r.id !== id);
  return runs.length < before;
}

// ---------------------------------------------------------------------------------------------
// A live run, compressed: each area takes an eighth of its planned time, Sustained 12 s.

type Live = { mode: BenchMode; start: number; timer: ReturnType<typeof setInterval>; progress: BenchProgress };
let live: Live | null = null;
const progressCbs = new Set<(p: BenchProgress) => void>();
const doneCbs = new Set<(r: BenchRun) => void>();

const simSecs = (a: AreaId) => (a === 'sustained' ? 12 : PLANNED[a] / 8);

export function simPreflight(): Preflight {
  return {
    blockers: live ? ['A benchmark is already running.'] : [],
    notes: [],
    gpu: SETUP.gpu,
    drive: SETUP.drive,
    quickSecs: planned('quick'),
    fullSecs: planned('full'),
  };
}

export function simProgress(): BenchProgress | null {
  return live?.progress ?? null;
}

function stepFor(a: AreaId, frac: number): string {
  if (a === 'sustained') return `Loop ${Math.min(10, Math.floor(frac * 10) + 1)} of 10`;
  const what = { cpuSingle: 'Rounds', cpuMulti: 'Rounds', memory: 'Read', storage: 'Sequential write', gpuCompute: 'FP32', graphics: 'Scene' }[a as string] ?? '';
  return frac < 0.25 ? `${what}: warm-up` : `${what}: pass ${Math.min(3, Math.floor((frac - 0.25) / 0.25) + 1)} of 3`;
}

function finish(reason?: string): void {
  if (!live) return;
  clearInterval(live.timer);
  const done = live.progress.finished.length;
  const next = Math.max(0, ...all().map((r) => r.id)) + 1;
  const run = makeRun(next, Math.floor(live.start / 1000), live.mode, { gpu: 1.05, driver: '577.00' }, reason ? done : undefined, reason);
  live = null;
  all().push(run);
  for (const cb of doneCbs) cb(run);
}

export function simStart(mode: BenchMode): void {
  if (live) throw new Error('A benchmark is already running.');
  const plan = PLAN[mode];
  const total = 1 + plan.reduce((s, a) => s + simSecs(a), 0);
  const start = Date.now();
  const tick = () => {
    if (!live) return;
    const t = (Date.now() - start) / 1000;
    let acc = 1;
    let cur: AreaId | null = null;
    let frac = 0;
    const finished: AreaResult[] = [];
    for (const [i, a] of plan.entries()) {
      const d = simSecs(a);
      if (t >= acc + d) {
        finished.push(area(a, 999 + i, { gpu: 1.05 }));
      } else if (t >= acc) {
        cur = a;
        frac = (t - acc) / d;
        break;
      }
      acc += d;
    }
    const p = planned(mode);
    const doneSecs = t < 1 ? (t / 1) * SETTLE : SETTLE + finished.reduce((s, a) => s + PLANNED[a.area], 0) + (cur ? PLANNED[cur] * frac : 0);
    live.progress = {
      mode,
      area: cur,
      step: cur ? stepFor(cur, frac) : t < 1 ? 'Settling' : '',
      done: Math.min(1, doneSecs / p),
      elapsed: doneSecs,
      planned: p,
      finished,
    };
    for (const cb of progressCbs) cb(live.progress);
    if (t >= total) finish();
  };
  live = {
    mode,
    start,
    timer: setInterval(tick, 250),
    progress: { mode, area: null, step: 'Settling', done: 0, elapsed: 0, planned: planned(mode), finished: [] },
  };
}

export function simStop(reason = 'Stopped by you.'): void {
  finish(reason);
}

export function onSimProgress(cb: (p: BenchProgress) => void): () => void {
  progressCbs.add(cb);
  return () => progressCbs.delete(cb);
}

export function onSimDone(cb: (r: BenchRun) => void): () => void {
  doneCbs.add(cb);
  return () => doneCbs.delete(cb);
}

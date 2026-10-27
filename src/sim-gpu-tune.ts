// The browser preview's GPU tuning (1.6): an RTX 5060 Ti with ranges like a real driver's, the
// suggestions the service makes for Blackwell, and tests that take seconds instead of minutes.

import { STOCK, describe, same } from './gputune';
import type { GpuTestDone, GpuTestKind, GpuTestProgress, TuneConfig, TuneEvent, TuneEventKind, TunePresetView, TuneSettings, TuneStatus } from './types';

const LIMITS = {
  core: { min: -500, max: 1000 },
  mem: { min: -1000, max: 3000 },
  power: { min: 70, max: 112 },
  powerDefaultW: 180,
};
const SUGGESTED: TunePresetView[] = [
  { id: 'stock', name: 'Stock', settings: STOCK, kind: 'stock', note: "The card's own clocks and power limit.", testedAt: 0 },
  { id: 'quiet', name: 'Quiet', settings: { coreMhz: 0, memMhz: 0, powerPct: 75 }, kind: 'suggested', note: 'Power limit 75 %: cooler and quieter, a few percent slower.', testedAt: null },
  {
    id: 'efficient',
    name: 'Efficient',
    settings: { coreMhz: 75, memMhz: 0, powerPct: 85 },
    kind: 'suggested',
    note: 'Power limit 85 % and a small core offset: nearly the same speed for less power.',
    testedAt: null,
  },
  {
    id: 'performance',
    name: 'Performance',
    settings: { coreMhz: 150, memMhz: 800, powerPct: 112 },
    kind: 'suggested',
    note: 'The highest power limit and modest offsets most cards of this family run.',
    testedAt: null,
  },
];

let config: TuneConfig = {
  enabled: false,
  preset: 'stock',
  applyAtStart: false,
  presets: [],
  profiles: {},
  gpu: null,
  tested: [],
};
let current: TuneSettings = STOCK;
let applied: string | null = null;
let waiting: string | null = null;
const events: TuneEvent[] = [];
let test: { progress: GpuTestProgress; timer: ReturnType<typeof setInterval> } | null = null;
let onProgress: ((p: GpuTestProgress) => void) | null = null;
let onDone: ((d: GpuTestDone) => void) | null = null;

function event(kind: TuneEventKind, message: string): void {
  events.push({ ts: Math.floor(Date.now() / 1000), kind, message });
  if (events.length > 100) events.shift();
}

function testedAt(s: TuneSettings): number | null {
  if (same(s, STOCK)) return 0;
  const t = config.tested.filter((x) => same(x.settings, s));
  return t.length ? Math.max(...t.map((x) => x.at)) : null;
}

function presets(): TunePresetView[] {
  return [...SUGGESTED, ...config.presets.map((p) => ({ ...p, testedAt: null }))].map((p) => ({ ...p, testedAt: testedAt(p.settings) }));
}

function inForce(): TunePresetView | undefined {
  const id = config.profiles.desktop ?? config.preset;
  return presets().find((p) => p.id === id);
}

export function simGpuTune(): TuneStatus {
  return {
    capability: { state: 'writable', gpu: 'NVIDIA GeForce RTX 5060 Ti' },
    gpu: { vendor: 'nvidia', name: 'NVIDIA GeForce RTX 5060 Ti', uuid: 'GPU-preview', family: 'blackwell', laptop: false, driver: '581.57' },
    limits: LIMITS,
    config: structuredClone(config),
    presets: presets(),
    current,
    clocks: {
      coreMaxMhz: 3090 + current.coreMhz,
      memMaxMhz: 14_001 + current.memMhz,
      powerLimitW: (LIMITS.powerDefaultW * current.powerPct) / 100,
    },
    applied,
    profile: 'desktop',
    held: null,
    waiting,
    lastError: null,
    trial: test ? { settings: test.progress.settings, step: test.progress.step, done: test.progress.done } : null,
  };
}

export function simGpuBrief(): { tuned: boolean; held: boolean } {
  return { tuned: !same(current, STOCK), held: false };
}

export function simSetGpuTune(next: TuneConfig, apply: boolean): void {
  if (!config.enabled && next.enabled) event('on', 'GPU tuning turned on');
  if (config.enabled && !next.enabled) event('off', 'GPU tuning turned off');
  config = structuredClone({ ...next, gpu: 'GPU-preview' });
  if (!config.enabled) {
    current = STOCK;
    applied = null;
    waiting = null;
    return;
  }
  if (apply) {
    const p = inForce();
    current = p?.settings ?? STOCK;
    applied = p && !same(p.settings, STOCK) ? p.id : null;
    waiting = null;
    if (p) event('applied', same(p.settings, STOCK) ? `${p.name} applied` : `${p.name} applied (${describe(p.settings)})`);
  }
}

export function simRestoreGpu(): void {
  current = STOCK;
  applied = null;
  if (config.enabled) waiting = 'Returned to stock: apply a preset to tune the card again';
  event('stock', 'The card was returned to stock');
}

export function simGpuEvents(): TuneEvent[] {
  return [...events];
}

export function simGpuTestProgress(): GpuTestProgress | null {
  return test?.progress ?? null;
}

export function simOnGpuTest(progress: (p: GpuTestProgress) => void, done: (d: GpuTestDone) => void): () => void {
  onProgress = progress;
  onDone = done;
  return () => {
    onProgress = null;
    onDone = null;
  };
}

/** A test that passes in about 6 seconds ("Find my best": about 12, ending at a found best). */
export function simStartGpuTest(kind: GpuTestKind, settings: TuneSettings | null): void {
  if (!config.enabled) throw new Error('Turn GPU tuning on first.');
  if (test) throw new Error('A GPU test is already running.');
  const target = kind === 'test' ? (settings ?? STOCK) : (SUGGESTED[3]?.settings ?? STOCK);
  const before = current;
  const total = kind === 'test' ? 6 : 12;
  let t = 0;
  const progress: GpuTestProgress = { kind, step: kind === 'test' ? `Testing ${describe(target)}` : `Core +${target.coreMhz} MHz`, done: 0, settings: target };
  current = target;
  const timer = setInterval(() => {
    t += 0.5;
    progress.done = Math.min(1, t / total);
    if (kind === 'best') {
      const core = target.coreMhz + Math.min(120, Math.floor(t * 15));
      progress.settings = { ...target, coreMhz: core };
      progress.step = t < total * 0.5 ? `Core +${core} MHz` : `Memory +${target.memMhz + Math.floor((t - total * 0.5) * 100)} MHz`;
    }
    onProgress?.({ ...progress });
    if (t >= total) finish(true);
  }, 500);
  test = { progress, timer };
  function finish(passed: boolean): void {
    if (!test) return;
    clearInterval(test.timer);
    test = null;
    current = before;
    const at = Math.floor(Date.now() / 1000);
    if (kind === 'test') {
      if (passed) {
        config.tested = [...config.tested.filter((x) => !same(x.settings, target)), { settings: target, at }];
        event('passed', `Passed the stability test: ${describe(target)}`);
      }
      onDone?.({ kind, passed, settings: passed ? target : null, message: passed ? `Passed: ${describe(target)} is stable on this card.` : 'Stopped.' });
      return;
    }
    const best = { coreMhz: target.coreMhz + 90, memMhz: target.memMhz + 600, powerPct: target.powerPct };
    if (passed) {
      config.tested = [...config.tested, { settings: best, at }];
      config.presets = [
        { id: 'best', name: 'Best for this card', settings: best, kind: 'custom', note: 'Found and tested on this card by Find my best.' },
        ...config.presets.filter((p) => p.id !== 'best'),
      ];
      event('passed', `Passed the stability test: ${describe(best)}`);
    }
    onDone?.({
      kind,
      passed,
      settings: passed ? best : null,
      message: passed ? `Best for this card: ${describe(best)}. Saved as a preset.` : 'Stopped.',
    });
  }
  stopper = () => finish(false);
}

let stopper: (() => void) | null = null;

export function simStopGpuTest(): void {
  stopper?.();
  stopper = null;
}

import { describe, expect, it } from 'vitest';
import type { ChartDef } from './presets';
import { MAX_CONTEXT_METRICS, clock, contextCharts, markerInfo, points, sampleAt, span } from './replay';
import { simTimeline } from './sim-replay';
import type { GameSample, GameTimeline, Profile } from './types';

const sample = (t: number, fps: number | null, extra: Partial<GameSample> = {}): GameSample => ({
  t, fps, fpsMin: fps == null ? null : fps - 20, frameMs: null, frameMaxMs: null, cpuUsage: null, gpuUsage: null,
  cpuTemp: null, gpuTemp: null, cpuPower: null, gpuPower: null, ...extra,
});

const timeline = (samples: GameSample[]): GameTimeline => ({
  session: {
    id: 1, app: 'Game.exe', runtime: null, started: 1000, secs: 60, frames: 0, avgFps: null, low1Fps: null, low01Fps: null,
    maxFrameMs: null, bound: null, cpuTempMax: null, gpuTempMax: null, cpuUsageAvg: null, gpuUsageAvg: null,
    profile: null, ended: 1062, hasTimeline: true,
  },
  stepMs: 2000,
  samples,
  events: [],
});

describe('replay tracks', () => {
  const tl = timeline([sample(0, 140), sample(2000, null), sample(4000, 120, { gpuTemp: 64 })]);

  it('places samples on the wall clock and leaves gaps where a reading is missing', () => {
    const p = points(tl, (s) => s.fps, (s) => s.fpsMin);
    expect(p).toEqual([
      { ts: 1000, avg: 140, min: 120, max: 140 },
      { ts: 1004, avg: 120, min: 100, max: 120 },
    ]);
    expect(points(tl, (s) => s.gpuTemp)).toEqual([{ ts: 1004, avg: 64, min: 64, max: 64 }]);
  });

  it('spans the session and finds the nearest sample', () => {
    expect(span(tl)).toEqual([1000, 1062]);
    expect(sampleAt(tl, 1003.5)?.t).toBe(4000);
    expect(sampleAt(timeline([]), 1000)).toBeNull();
  });

  it('reads times into the session', () => {
    expect(clock(0)).toBe('0:00');
    expect(clock(754_400)).toBe('12:34');
    expect(clock(3_723_000)).toBe('1:02:03');
  });
});

describe('markers', () => {
  const name = (p: Profile) => p[0]!.toUpperCase() + p.slice(1);

  it('says what each one means and groups it', () => {
    expect(markerInfo({ kind: 'fpsDrop', fps: 52.4, usual: 141 }, name)).toEqual({
      group: 'frames', title: 'Frame rate drop', text: 'Fell to 52 FPS, from a usual 141 FPS.',
    });
    expect(markerInfo({ kind: 'stutter', ms: 61.7, usual: 7.06 }, name).text).toBe('A frame took 62 ms; frames usually took 7.1 ms.');
    expect(markerInfo({ kind: 'powerLimit', part: 'gpu' }, name)).toMatchObject({ group: 'limits', title: 'GPU power limit' });
    expect(markerInfo({ kind: 'throttle', part: 'gpu', reason: 'hardware slowdown' }, name).text).toBe('Clocks held back: hardware slowdown.');
    expect(markerInfo({ kind: 'profile', profile: 'silent', auto: false }, name)).toEqual({
      group: 'you', title: 'Silent profile', text: 'Picked by you.',
    });
    expect(markerInfo({ kind: 'left' }, name).group).toBe('you');
  });

  it('the browser preview has every kind of moment, inside the session', () => {
    const tl = simTimeline(3)!;
    expect(tl.samples.length).toBeLessThanOrEqual(1500);
    const kinds = new Set(tl.events.map((e) => e.marker.kind));
    for (const k of ['fpsDrop', 'stutter', 'powerLimit', 'thermal', 'left', 'back', 'profile', 'overlay']) expect(kinds).toContain(k);
    const [start, end] = span(tl);
    expect(tl.events.every((e) => start + e.t / 1000 <= end)).toBe(true);
    expect(simTimeline(1)).toBeNull();
  });
});

describe('context of a moment', () => {
  const chart = (key: string, n: number): ChartDef => ({
    key, title: key, unit: '%', zero: true, metrics: Array.from({ length: n }, (_, i) => ({ id: `${key}.${i}`, name: `${key} ${i}` })),
  });

  it('takes the context charts in order, at most four readings each and the engine limit in all', () => {
    const picked = contextCharts([chart('fans', 3), chart('network', 12), chart('usage', 2), chart('disks', 8), chart('clocks', 3), chart('power', 2), chart('temps', 2), chart('memory', 2)]);
    expect(picked.map((c) => c.key)).toEqual(['usage', 'clocks', 'power', 'temps', 'memory', 'disks', 'network']);
    expect(picked.find((c) => c.key === 'disks')!.metrics).toHaveLength(4);
    const total = picked.reduce((n, c) => n + c.metrics.length, 0);
    expect(total).toBeLessThanOrEqual(MAX_CONTEXT_METRICS);
    expect(picked.find((c) => c.key === 'network')!.metrics).toHaveLength(4);
    // Every chart full: the engine limit leaves the last one out.
    const full = contextCharts(['usage', 'clocks', 'power', 'temps', 'memory', 'disks', 'network'].map((k) => chart(k, 6)));
    expect(full.reduce((n, c) => n + c.metrics.length, 0)).toBe(MAX_CONTEXT_METRICS);
    expect(full.map((c) => c.key)).not.toContain('network');
  });
});

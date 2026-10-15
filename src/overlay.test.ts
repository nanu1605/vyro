import { describe, expect, it } from 'vitest';
import { DEFAULT_OVERLAY, formatRow, graphScale, overlayData, overlaySize, shownMetrics } from './overlay';
import type { GameSnapshot, OverlayRow, SensorSnapshot } from './types';

describe('readings shown', () => {
  it('uses the mode unless the owner picked some, in a fixed order', () => {
    expect(shownMetrics('minimal', [])).toEqual(['fps']);
    expect(shownMetrics('full', [])).toHaveLength(13);
    expect(shownMetrics('minimal', ['gpuTemp', 'fps'])).toEqual(['fps', 'gpuTemp']);
  });
});

describe('window size (mirrors the Rust layout)', () => {
  it('matches the Rust numbers and grows with what it shows', () => {
    // Same figures as vyro_core::gaming::overlay::size for these inputs.
    expect(overlaySize('minimal', ['fps'], false, false, 100)).toEqual([130, 46]);
    expect(overlaySize('performance', shownMetrics('performance', []), true, false, 100)).toEqual([268, 162]);
    expect(overlaySize('performance', shownMetrics('performance', []), false, true, 150)).toEqual([354, 144]);
    const [, perf] = overlaySize('performance', shownMetrics('performance', []), false, false, 100);
    const [, full] = overlaySize('full', shownMetrics('full', []), false, false, 100);
    expect(full).toBeGreaterThan(perf);
    const all = shownMetrics('full', []);
    expect(overlaySize('minimal', all.slice(0, 3), false, false, 100)).toEqual([352, 46]);
    expect(overlaySize('minimal', all, false, false, 100)).toEqual([760, 66]);
    expect(overlaySize('minimal', all, false, true, 100)).toEqual([760, 58]);
  });
});

describe('rows and formatting', () => {
  const snap = {
    cpu: { usageTotal: 41.2, tempPackage: 71.5, powerPkgW: 88 },
    gpu: [{ usage: 97, tempEdge: 64, powerW: 151.4, vramUsedMB: 6400 }],
  } as unknown as SensorSnapshot;
  const game = {
    app: 'Game.exe', state: 'capturing', fps: 141.6, low1Fps: 117, low01Fps: 90, frameMs: 7.06, latencyMs: null,
    bound: 'gpu', timeline: [{ avgMs: 7, maxMs: 9 }],
  } as unknown as GameSnapshot;

  it('fills the rows from the snapshot and hides the graph in Minimal', () => {
    const d = overlayData({ ...DEFAULT_OVERLAY, mode: 'full' }, snap, game);
    const by = Object.fromEntries(d.rows.map((r) => [r.metric, r]));
    expect(by.cpuPower!.value).toBe(88);
    expect(by.vram!.value).toBe(6400);
    expect(by.bound!.text).toBe('GPU');
    expect(d.timeline).toHaveLength(1);
    expect(overlayData({ ...DEFAULT_OVERLAY, mode: 'minimal' }, snap, game).timeline).toEqual([]);
    const paused = overlayData(DEFAULT_OVERLAY, snap, { ...game, state: 'idle' });
    expect(paused.paused).toBe(true);
    expect(paused.rows[0]!.value).toBeNull();
  });

  it('formats each kind of reading', () => {
    const row = (metric: OverlayRow['metric'], value: number | null, text: string | null = null): OverlayRow => ({ metric, label: '', value, text });
    const c = (v: number) => `${Math.round(v)} °C`;
    expect(formatRow(row('fps', 141.6), c)).toBe('142');
    expect(formatRow(row('frameTime', 7.06), c)).toBe('7.1 ms');
    expect(formatRow(row('cpuUsage', 41.2), c)).toBe('41%');
    expect(formatRow(row('gpuTemp', 64), c)).toBe('64 °C');
    expect(formatRow(row('gpuPower', 151.4), c)).toBe('151 W');
    expect(formatRow(row('vram', 6400), c)).toBe('6.3 GB');
    expect(formatRow(row('vram', 800), c)).toBe('800 MB');
    expect(formatRow(row('bound', null, 'Capped'), c)).toBe('Capped');
    expect(formatRow(row('latency', null), c)).toBe('—');
  });

  it('picks a round graph scale above twice the median frame', () => {
    expect(graphScale([{ avgMs: 7, maxMs: 9 }, null, { avgMs: 7.2, maxMs: 8 }])).toBe(16.7);
    expect(graphScale([{ avgMs: 16.7, maxMs: 20 }])).toBe(50);
    expect(graphScale([])).toBe(16.7);
  });
});

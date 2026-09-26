import { describe, expect, it } from 'vitest';
import { bandPath, linePath, nearest, niceScale, niceStep, runs, timeTicks } from './chart';
import type { HistoryPoint } from './types';

const pt = (ts: number, avg: number, min = avg, max = avg): HistoryPoint => ({ ts, avg, min, max });

describe('niceStep / niceScale', () => {
  it('picks 1-2-2.5-5 steps', () => {
    expect(niceStep(100, 4)).toBe(25);
    expect(niceStep(7, 4)).toBe(2);
    expect(niceStep(0.3, 4)).toBeCloseTo(0.1);
  });

  it('starts at zero for zero-based metrics', () => {
    const s = niceScale([12, 87], { zero: true });
    expect(s.lo).toBe(0);
    expect(s.hi).toBe(100);
    expect(s.ticks).toEqual([0, 25, 50, 75, 100]);
  });

  it('hugs the data for temperatures', () => {
    const s = niceScale([41.2, 58.9]);
    expect(s.lo).toBe(40);
    expect(s.hi).toBe(60);
  });

  it('gives a flat or empty series a visible range', () => {
    const flat = niceScale([1.2, 1.2]);
    expect(flat.hi).toBeGreaterThan(flat.lo);
    expect(niceScale([]).hi).toBeGreaterThan(niceScale([]).lo);
  });
});

describe('runs', () => {
  it('breaks where buckets are further apart than the gap', () => {
    const r = runs([pt(0, 1), pt(10, 1), pt(20, 1), pt(100, 1), pt(110, 1)], 30);
    expect(r.map((x) => x.length)).toEqual([3, 2]);
  });
});

describe('paths', () => {
  const x = (ts: number) => ts;
  const y = (v: number) => 100 - v;
  it('draws the average line', () => {
    expect(linePath([pt(0, 10), pt(5, 20)], x, y)).toBe('M0.0 90.0 L5.0 80.0');
  });
  it('closes the min–max band', () => {
    expect(bandPath([pt(0, 10, 5, 15), pt(5, 20, 10, 30)], x, y)).toBe('M0.0 85.0 L5.0 70.0 L5.0 90.0 L0.0 95.0 Z');
    expect(bandPath([pt(0, 10)], x, y)).toBe('');
  });
});

describe('time axis', () => {
  it('uses round steps inside the range', () => {
    const t = timeTicks(0, 3600, 4);
    expect(t.length).toBeGreaterThanOrEqual(3);
    expect(t.every((v) => v >= 0 && v <= 3600)).toBe(true);
    expect(t[1]! - t[0]!).toBe(900);
  });
  it('finds the nearest point', () => {
    const p = [pt(0, 1), pt(10, 2), pt(20, 3)];
    expect(nearest(p, 12)).toBe(1);
    expect(nearest(p, 16)).toBe(2);
    expect(nearest([], 5)).toBe(-1);
  });
});

describe('stepSecs', () => {
  it('matches the engine buckets (HistoryRange::step_secs)', async () => {
    const { stepSecs } = await import('./sim-tree');
    expect(['5m', '30m', '1h', '6h', '24h', '7d'].map((r) => stepSecs(r as never))).toEqual([1, 3, 10, 40, 150, 1020]);
  });
});

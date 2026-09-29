import { describe, expect, it } from 'vitest';
import { add, evaluate, move, path, PRESETS, remove, toPoint, x, y, type Box } from './curve';

const box: Box = { width: 420, height: 220, left: 20, right: 0, top: 0, bottom: 20 };
const pts = (...p: Array<[number, number]>) => p.map(([tempC, pct]) => ({ tempC, pct }));

describe('fan curve', () => {
  it('interpolates like the engine', () => {
    const c = pts([40, 30], [60, 50], [80, 100]);
    expect(evaluate(c, 20)).toBe(30);
    expect(evaluate(c, 50)).toBe(40);
    expect(evaluate(c, 70)).toBe(75);
    expect(evaluate(c, 95)).toBe(100);
    expect(evaluate([], 50)).toBe(100);
  });

  it('maps between chart coordinates and points', () => {
    expect(x(box, 20)).toBe(20);
    expect(x(box, 100)).toBe(420);
    expect(y(box, 100)).toBe(0);
    expect(y(box, 0)).toBe(200);
    expect(toPoint(box, x(box, 63), y(box, 42))).toEqual({ tempC: 63, pct: 42 });
    expect(toPoint(box, -50, 999)).toEqual({ tempC: 20, pct: 0 });
    expect(path(box, pts([40, 30], [80, 100])).startsWith('M20.0,')).toBe(true);
  });

  it('keeps temperatures rising when a point is dragged', () => {
    const c = pts([40, 30], [60, 50], [80, 100]);
    expect(move(c, 1, { tempC: 95, pct: 120 })[1]).toEqual({ tempC: 79, pct: 100 });
    expect(move(c, 1, { tempC: 10, pct: -5 })[1]).toEqual({ tempC: 41, pct: 0 });
    expect(move(c, 0, { tempC: 5, pct: 20 })[0]).toEqual({ tempC: 20, pct: 20 });
  });

  it('adds on the curve and keeps 2–8 points', () => {
    const c = pts([40, 30], [80, 100]);
    expect(add(c, 60)).toEqual(pts([40, 30], [60, 65], [80, 100]));
    expect(add(c, 40.3)).toEqual(c);
    const full = pts([20, 0], [30, 10], [40, 20], [50, 30], [60, 40], [70, 50], [80, 60], [90, 70]);
    expect(add(full, 85)).toHaveLength(8);
    expect(remove(c, 0)).toEqual(c);
    expect(remove(add(c, 60), 1)).toEqual(c);
  });

  it('presets are valid curves', () => {
    for (const p of Object.values(PRESETS)) {
      expect(p.length).toBeGreaterThanOrEqual(2);
      for (let i = 1; i < p.length; i++) expect(p[i]!.tempC).toBeGreaterThan(p[i - 1]!.tempC);
      expect(p[p.length - 1]!.pct).toBe(100);
    }
  });
});

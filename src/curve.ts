// Pure helpers for the fan-curve editor (P12b): the same interpolation the engine uses
// (crates/vyro-core/src/fan/mod.rs `evaluate`), chart geometry, and point edits that keep a
// curve valid (2–8 points, temperatures rising, 0–100 %).

import type { CurvePoint, FanCurve } from './types';

export const T_MIN = 20;
export const T_MAX = 100;
export const MAX_POINTS = 8;
/** Smallest gap between neighbouring points' temperatures. */
export const MIN_GAP = 1;

export interface Box {
  width: number;
  height: number;
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export function evaluate(points: readonly CurvePoint[], temp: number): number {
  const first = points[0];
  const last = points[points.length - 1];
  if (!first || !last) return 100;
  if (temp <= first.tempC) return first.pct;
  if (temp >= last.tempC) return last.pct;
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1]!;
    const b = points[i]!;
    if (temp <= b.tempC) {
      const span = b.tempC - a.tempC;
      return span <= 0 ? b.pct : a.pct + ((b.pct - a.pct) * (temp - a.tempC)) / span;
    }
  }
  return last.pct;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function x(box: Box, temp: number): number {
  const w = box.width - box.left - box.right;
  return box.left + ((clamp(temp, T_MIN, T_MAX) - T_MIN) / (T_MAX - T_MIN)) * w;
}

export function y(box: Box, pct: number): number {
  const h = box.height - box.top - box.bottom;
  return box.top + (1 - clamp(pct, 0, 100) / 100) * h;
}

/** Chart coordinates → a point, rounded to whole °C and %. */
export function toPoint(box: Box, px: number, py: number): CurvePoint {
  const w = box.width - box.left - box.right;
  const h = box.height - box.top - box.bottom;
  return {
    tempC: Math.round(clamp(T_MIN + ((px - box.left) / w) * (T_MAX - T_MIN), T_MIN, T_MAX)),
    pct: Math.round(clamp((1 - (py - box.top) / h) * 100, 0, 100)),
  };
}

/** SVG path through the points, flat to the chart's edges. */
export function path(box: Box, points: readonly CurvePoint[]): string {
  if (!points.length) return '';
  const first = points[0]!;
  const last = points[points.length - 1]!;
  const pts = [{ tempC: T_MIN, pct: first.pct }, ...points, { tempC: T_MAX, pct: last.pct }];
  return pts.map((p, i) => `${i ? 'L' : 'M'}${x(box, p.tempC).toFixed(1)},${y(box, p.pct).toFixed(1)}`).join('');
}

/** Moves point `i`, kept between its neighbours so temperatures stay rising. */
export function move(points: readonly CurvePoint[], i: number, to: CurvePoint): CurvePoint[] {
  const lo = i > 0 ? points[i - 1]!.tempC + MIN_GAP : T_MIN;
  const hi = i < points.length - 1 ? points[i + 1]!.tempC - MIN_GAP : T_MAX;
  return points.map((p, j) => (j === i ? { tempC: clamp(to.tempC, lo, hi), pct: clamp(to.pct, 0, 100) } : p));
}

/** Adds a point at `temp` on the current curve (unchanged when full or too close to one). */
export function add(points: readonly CurvePoint[], temp: number): CurvePoint[] {
  const t = Math.round(clamp(temp, T_MIN, T_MAX));
  if (points.length >= MAX_POINTS || points.some((p) => Math.abs(p.tempC - t) < MIN_GAP)) return [...points];
  return [...points, { tempC: t, pct: Math.round(evaluate(points, t)) }].sort((a, b) => a.tempC - b.tempC);
}

/** Removes point `i`, keeping at least two. */
export function remove(points: readonly CurvePoint[], i: number): CurvePoint[] {
  return points.length <= 2 ? [...points] : points.filter((_, j) => j !== i);
}

const pts = (...p: Array<[number, number]>): CurvePoint[] => p.map(([tempC, pct]) => ({ tempC, pct }));

export const PRESETS = {
  silent: pts([45, 25], [65, 35], [80, 60], [90, 100]),
  balanced: pts([40, 30], [60, 45], [75, 70], [85, 100]),
  performance: pts([35, 40], [55, 60], [70, 85], [80, 100]),
} as const;
export type Preset = keyof typeof PRESETS;

export function defaultCurve(source = 'cpu.temp.package'): FanCurve {
  return {
    source,
    points: PRESETS.balanced.map((p) => ({ ...p })),
    hysteresisC: 3,
    rampUpPctS: 10,
    rampDownPctS: 5,
  };
}

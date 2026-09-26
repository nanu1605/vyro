// Pure helpers for the History charts: axis scales, gap-aware paths and time ticks.

import type { HistoryPoint } from './types';

export interface Scale {
  lo: number;
  hi: number;
  ticks: number[];
}

/** "Nice" step (1, 2, 2.5 or 5 × 10ⁿ) that splits `span` into about `count` intervals. */
export function niceStep(span: number, count: number): number {
  if (!(span > 0)) return 1;
  const raw = span / Math.max(1, count);
  const mag = 10 ** Math.floor(Math.log10(raw));
  const f = raw / mag;
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10;
  return nice * mag;
}

/**
 * Y scale covering `values`: from 0 when `zero` (usage, power, RPM), otherwise hugging the
 * data (temperatures, voltages). A flat series still gets a visible range.
 */
export function niceScale(values: number[], opts: { zero?: boolean; max?: number; count?: number } = {}): Scale {
  const count = opts.count ?? 4;
  const finite = values.filter(Number.isFinite);
  let lo = finite.length ? Math.min(...finite) : 0;
  let hi = finite.length ? Math.max(...finite) : 1;
  if (opts.zero) lo = Math.min(0, lo);
  if (opts.max != null) hi = Math.max(hi, opts.max);
  if (hi - lo < 1e-9) {
    const pad = Math.abs(hi) * 0.05 || 1;
    hi += pad;
    if (!opts.zero) lo -= pad;
  }
  const step = niceStep(hi - lo, count);
  const nlo = opts.zero && lo >= 0 ? 0 : Math.floor(lo / step) * step;
  const nhi = Math.ceil(hi / step) * step;
  const ticks: number[] = [];
  for (let t = nlo; t <= nhi + step * 1e-9; t += step) ticks.push(Number(t.toPrecision(12)));
  return { lo: nlo, hi: nhi, ticks };
}

/**
 * Splits points into runs wherever consecutive buckets are further apart than `maxGap`
 * seconds (the app was closed or the sensor had no reading), so lines break there.
 */
export function runs(points: readonly HistoryPoint[], maxGap: number): HistoryPoint[][] {
  const out: HistoryPoint[][] = [];
  let cur: HistoryPoint[] = [];
  for (const p of points) {
    const prev = cur[cur.length - 1];
    if (prev && p.ts - prev.ts > maxGap) {
      out.push(cur);
      cur = [];
    }
    cur.push(p);
  }
  if (cur.length) out.push(cur);
  return out;
}

/** SVG path data for the average line of each run ("M x y L x y …"). */
export function linePath(run: readonly HistoryPoint[], x: (ts: number) => number, y: (v: number) => number): string {
  return run.map((p, i) => `${i ? 'L' : 'M'}${x(p.ts).toFixed(1)} ${y(p.avg).toFixed(1)}`).join(' ');
}

/** Closed SVG path for a run's min–max band. */
export function bandPath(run: readonly HistoryPoint[], x: (ts: number) => number, y: (v: number) => number): string {
  if (run.length < 2) return '';
  const top = run.map((p, i) => `${i ? 'L' : 'M'}${x(p.ts).toFixed(1)} ${y(p.max).toFixed(1)}`);
  const bottom = [...run].reverse().map((p) => `L${x(p.ts).toFixed(1)} ${y(p.min).toFixed(1)}`);
  return `${top.join(' ')} ${bottom.join(' ')} Z`;
}

const TICK_STEPS = [60, 120, 300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200, 86400];

/** Round-time x-axis ticks (unix seconds) between `start` and `end`, about `count` of them. */
export function timeTicks(start: number, end: number, count: number): number[] {
  const span = end - start;
  if (!(span > 0)) return [];
  const want = span / Math.max(1, count);
  const step = TICK_STEPS.find((s) => s >= want) ?? 86400 * Math.ceil(want / 86400);
  // Align to local midnight-based boundaries so labels read 12:00, 12:30, …
  const offset = new Date(start * 1000).getTimezoneOffset() * 60;
  const first = Math.ceil((start - offset) / step) * step + offset;
  const out: number[] = [];
  for (let t = first; t <= end; t += step) out.push(t);
  return out;
}

/** Tick label: "14:05" within a day, "Mon 14:00" / "Mon" across days. */
export function timeLabel(ts: number, spanSecs: number): string {
  const d = new Date(ts * 1000);
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  if (spanSecs <= 86400) return hm;
  const day = d.toLocaleDateString('en-US', { weekday: 'short' });
  return d.getHours() === 0 && d.getMinutes() === 0 ? day : `${day} ${hm}`;
}

/** Index of the point nearest to `ts` (binary search; points sorted by ts). */
export function nearest(points: readonly HistoryPoint[], ts: number): number {
  if (!points.length) return -1;
  let lo = 0;
  let hi = points.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (points[mid]!.ts <= ts) lo = mid;
    else hi = mid;
  }
  return Math.abs(points[lo]!.ts - ts) <= Math.abs(points[hi]!.ts - ts) ? lo : hi;
}

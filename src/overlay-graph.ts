// The overlay's frame-time graph in motion (1.0.1). Readings reach the overlay every 0.5–2 s,
// each with the last 15 s in 100 ms points. Drawn as they come, the line jumps a second at a
// time; instead the graph plays the points back a little late, so new ones slide in from the
// right at an even pace. Pure, so it is unit-tested; `OverlayGraph.svelte` draws it.

import type { FramePoint } from './types';

/** One point per 100 ms (`vyro_core::gaming::stats::BUCKET_MS`). */
export const POINT_MS = 100;
/** Furthest the line runs behind the newest point, in points: the slowest update rate (2 s)
 * plus slack for a late tick. */
export const MAX_LAG = 24;

const same = (a: FramePoint | null | undefined, b: FramePoint | null | undefined): boolean =>
  a != null && b != null && a.avgMs === b.avgMs && a.maxMs === b.maxMs;

/**
 * How many points `next` moved on from `prev`: a shift at which the two agree wherever both
 * have frames, the one nearest `expected` (from the time between them) when several do, as in
 * a frame-capped game whose points repeat. `null` when none up to `max` fits (a new game, or
 * only gaps to go by).
 */
export function alignShift(
  prev: ReadonlyArray<FramePoint | null>,
  next: ReadonlyArray<FramePoint | null>,
  expected = 0,
  max = 40,
): number | null {
  let best: number | null = null;
  for (let k = 0; k <= Math.min(max, prev.length - 1); k++) {
    let hits = 0;
    let miss = false;
    for (let i = 0; i + k < prev.length && i < next.length; i++) {
      const a = prev[i + k];
      const b = next[i];
      if (a != null && b != null) {
        if (same(a, b)) hits++;
        else {
          miss = true;
          break;
        }
      }
    }
    // Two agreeing points at least, so a lone repeated value does not decide it.
    if (!miss && hits >= 2 && (best == null || Math.abs(k - expected) < Math.abs(best - expected))) best = k;
  }
  return best;
}

/**
 * The points as they arrive, numbered from the first one seen, and the moving end of the
 * window (`pos`, a fractional point number) that trails the newest point by about one update.
 */
export class GraphFeed {
  private buf = new Map<number, FramePoint | null>();
  private last: Array<FramePoint | null> | null = null;
  private lastAt = 0;
  /** Number of the newest point; -1 before the first update. */
  total = -1;
  /** Right edge of the window, in point numbers. */
  pos = -1;
  /** Time between updates, ms (measured). */
  interval = 1000;

  /** How far the window trails the newest point, in points. */
  get lag(): number {
    return Math.min(MAX_LAG, Math.max(3, this.interval / POINT_MS + 2));
  }

  /** A new timeline, oldest first, arrived at `now` (ms). */
  push(points: ReadonlyArray<FramePoint | null>, now: number): void {
    const n = points.length;
    if (!n) return;
    let shift: number | null = null;
    if (this.last) {
      const dt = now - this.lastAt;
      const expected = Math.max(0, Math.round(dt / POINT_MS));
      shift = alignShift(this.last, points, expected) ?? expected;
      if (dt > 0) this.interval = Math.min(3000, Math.max(POINT_MS, this.interval * 0.6 + dt * 0.4));
    }
    if (shift == null || shift >= n) {
      // First update, or the game started again: begin afresh.
      this.buf.clear();
      this.total = n - 1;
      this.pos = this.total - this.lag;
    } else {
      this.total += shift;
    }
    for (let i = 0; i < n; i++) this.buf.set(this.total - (n - 1) + i, points[i] ?? null);
    for (const k of this.buf.keys()) if (k < this.total - 2 * n) this.buf.delete(k);
    this.last = [...points];
    this.lastAt = now;
  }

  /** Moves the window on by `dt` ms. Returns true while it still moves. */
  advance(dt: number): boolean {
    if (this.total < 0) return false;
    const target = this.total - this.lag;
    const before = this.pos;
    this.pos += dt / POINT_MS;
    if (this.pos < target - 2 * this.lag) this.pos = target;
    // A slow pull towards the target keeps the trail even when ticks run early or late.
    else this.pos += (target - this.pos) * (1 - Math.exp(-dt / 1500));
    this.pos = Math.min(this.pos, this.total);
    return this.pos - before > 1e-6;
  }

  /** The points in a window `span` points wide ending at `pos`, each with its place across
   * it (0 at the left edge, 1 at the right; the first and last may lie just outside). */
  window(span: number): Array<[number, FramePoint | null]> {
    const start = this.pos - span;
    const out: Array<[number, FramePoint | null]> = [];
    for (let i = Math.floor(start); i <= Math.ceil(this.pos); i++) {
      if (!this.buf.has(i)) continue;
      out.push([(i - start) / span, this.buf.get(i) ?? null]);
    }
    return out;
  }
}

/**
 * A smooth curve through the points that never overshoots them (monotone cubic,
 * Fritsch–Carlson), so a smooth line shows no spike the readings did not have. One cubic
 * Bézier per gap: `[c1x, c1y, c2x, c2y, x, y]`, from the previous point to `(x, y)`.
 */
export function monotoneSegments(xs: readonly number[], ys: readonly number[]): Array<[number, number, number, number, number, number]> {
  const n = xs.length;
  if (n < 2) return [];
  const d: number[] = [];
  for (let k = 0; k < n - 1; k++) d.push((ys[k + 1]! - ys[k]!) / (xs[k + 1]! - xs[k]! || 1));
  const m: number[] = [d[0]!];
  for (let k = 1; k < n - 1; k++) m.push(d[k - 1]! * d[k]! <= 0 ? 0 : (d[k - 1]! + d[k]!) / 2);
  m.push(d[n - 2]!);
  for (let k = 0; k < n - 1; k++) {
    if (d[k] === 0) {
      m[k] = 0;
      m[k + 1] = 0;
      continue;
    }
    const a = m[k]! / d[k]!;
    const b = m[k + 1]! / d[k]!;
    const s = a * a + b * b;
    if (s > 9) {
      const t = 3 / Math.sqrt(s);
      m[k] = t * a * d[k]!;
      m[k + 1] = t * b * d[k]!;
    }
  }
  const out: Array<[number, number, number, number, number, number]> = [];
  for (let k = 0; k < n - 1; k++) {
    const h = xs[k + 1]! - xs[k]!;
    out.push([xs[k]! + h / 3, ys[k]! + (m[k]! * h) / 3, xs[k + 1]! - h / 3, ys[k + 1]! - (m[k + 1]! * h) / 3, xs[k + 1]!, ys[k + 1]!]);
  }
  return out;
}

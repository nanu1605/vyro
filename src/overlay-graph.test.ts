import { describe, expect, it } from 'vitest';
import { alignShift, GraphFeed, MAX_LAG, monotoneSegments } from './overlay-graph';
import type { FramePoint } from './types';

/** A game's 100 ms points: a slow wave with a stutter now and then, numbered from `from`. */
const run = (from: number, n = 150): Array<FramePoint | null> =>
  Array.from({ length: n }, (_, i) => {
    const k = from + i;
    if (k % 37 === 5) return null;
    const avg = 7 + Math.sin(k / 9) + (k % 53 === 0 ? 6 : 0);
    return { avgMs: Math.round(avg * 1000) / 1000, maxMs: Math.round((avg + 1 + (k % 7) / 10) * 1000) / 1000 };
  });

describe('overlay graph', () => {
  it('finds how far the timeline moved between updates', () => {
    expect(alignShift(run(0), run(10), 10)).toBe(10);
    expect(alignShift(run(0), run(4), 10)).toBe(4);
    expect(alignShift(run(0), run(0), 0)).toBe(0);
    // A different game: nothing lines up.
    const other = run(0).map((p) => (p ? { avgMs: p.avgMs + 0.5, maxMs: p.maxMs } : null));
    expect(alignShift(run(0), other, 10)).toBeNull();
    // Only gaps to go by.
    expect(alignShift(Array(150).fill(null), Array(150).fill(null), 10)).toBeNull();
  });

  it('picks the shift nearest the time between updates when points repeat', () => {
    // A frame-capped game: every point the same.
    const flat = Array.from({ length: 150 }, () => ({ avgMs: 16.667, maxMs: 17 }));
    expect(alignShift(flat, flat, 10)).toBe(10);
    expect(alignShift(flat, flat, 5)).toBe(5);
  });

  it('slides the window at an even pace and never past the newest point', () => {
    const feed = new GraphFeed();
    feed.push(run(0), 0);
    expect(feed.total).toBe(149);
    const steps: number[] = [];
    let t = 0;
    for (let tick = 1; tick <= 6; tick++) {
      // 33 ms frames for a second, then the next update.
      for (let f = 0; f < 30; f++) {
        const before = feed.pos;
        feed.advance(1000 / 30);
        t += 1000 / 30;
        steps.push(feed.pos - before);
        expect(feed.pos).toBeLessThanOrEqual(feed.total);
      }
      feed.push(run(tick * 10), t);
      expect(feed.total).toBe(149 + tick * 10);
    }
    // About a third of a point a frame, every frame: no jump when an update lands.
    for (const s of steps.slice(30)) {
      expect(s).toBeGreaterThan(0.2);
      expect(s).toBeLessThan(0.5);
    }
    // It trails the newest point by about one update.
    expect(feed.total - feed.pos).toBeGreaterThan(5);
    expect(feed.total - feed.pos).toBeLessThanOrEqual(MAX_LAG);
  });

  it('keeps the window filled and the points in order', () => {
    const feed = new GraphFeed();
    feed.push(run(0), 0);
    feed.advance(500);
    const span = 150 - MAX_LAG - 2;
    const w = feed.window(span);
    expect(w.length).toBeGreaterThanOrEqual(span);
    expect(w[0]![0]).toBeLessThanOrEqual(0);
    expect(w.at(-1)![0]).toBeGreaterThanOrEqual(1);
    for (let i = 1; i < w.length; i++) expect(w[i]![0]).toBeGreaterThan(w[i - 1]![0]);
  });

  it('starts afresh when a new game begins', () => {
    const feed = new GraphFeed();
    feed.push(run(0), 0);
    feed.push(run(0).map((p) => (p ? { avgMs: p.avgMs * 2, maxMs: p.maxMs * 2 } : null)), 60_000);
    expect(feed.total).toBe(149);
  });

  it('draws a smooth line that never overshoots the points', () => {
    const xs = [0, 1, 2, 3, 4, 5, 6];
    const ys = [10, 10, 30, 12, 12, 11, 40];
    const segs = monotoneSegments(xs, ys);
    expect(segs).toHaveLength(6);
    segs.forEach(([, c1y, , c2y, x, y], k) => {
      const lo = Math.min(ys[k]!, ys[k + 1]!);
      const hi = Math.max(ys[k]!, ys[k + 1]!);
      for (const c of [c1y, c2y]) {
        expect(c).toBeGreaterThanOrEqual(lo - 1e-9);
        expect(c).toBeLessThanOrEqual(hi + 1e-9);
      }
      expect([x, y]).toEqual([xs[k + 1], ys[k + 1]]);
    });
    // Flat stretches stay flat.
    expect(segs[3]![1]).toBe(12);
    expect(segs[3]![3]).toBe(12);
    expect(monotoneSegments([0], [1])).toEqual([]);
  });
});

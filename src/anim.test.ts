import { describe, expect, it } from 'vitest';
import { TAU, Tween, ease } from './anim';

describe('ease', () => {
  it('matches the exponential approach formula', () => {
    expect(ease(0, 100, 0)).toBe(0);
    expect(ease(0, 100, TAU)).toBeCloseTo(100 * (1 - Math.exp(-1)), 10);
  });

  it('is frame-rate independent', () => {
    let a = 0;
    for (let i = 0; i < 60; i++) a = ease(a, 100, 1 / 60);
    let b = 0;
    for (let i = 0; i < 30; i++) b = ease(b, 100, 1 / 30);
    expect(a).toBeCloseTo(b, 8);
  });
});

describe('Tween', () => {
  it('settles on the target and then reports idle', () => {
    const t = new Tween(0);
    t.set(50);
    let frames = 0;
    while (t.step(1 / 60)) frames++;
    expect(t.cur).toBe(50);
    // ~1 s of frames at τ = 0.22 s, well under two seconds.
    expect(frames).toBeGreaterThan(30);
    expect(frames).toBeLessThan(120);
  });

  it('settles in about a third of the frames at the reduced level', () => {
    const count = (tau: number) => {
      const t = new Tween(0);
      t.set(50);
      let frames = 0;
      while (t.step(1 / 60, false, tau)) frames++;
      return frames;
    };
    expect(count(0.08)).toBeLessThan(count(0.22) / 2);
  });

  it('snaps immediately when motion is reduced', () => {
    const t = new Tween(0);
    t.set(80);
    expect(t.step(1 / 60, true)).toBe(false);
    expect(t.cur).toBe(80);
  });
});

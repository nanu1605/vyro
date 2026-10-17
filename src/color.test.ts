import { describe, expect, it } from 'vitest';
import { contrast, fromHsv, fromLch, isHex, parseColor, readableOn, toHsv, toLch, turn } from './color';

const close = (a: string, b: string) =>
  parseColor(a)
    .slice(0, 3)
    .every((v, i) => Math.abs(v - parseColor(b)[i]!) <= 1);

describe('colour maths', () => {
  it('round-trips colours through OKLCH and HSV', () => {
    for (const c of ['#000000', '#ffffff', '#4cc9f0', '#1a237e', '#fff59d', '#ff0000', '#7ee787', '#808080']) {
      expect(close(fromLch(toLch(c)), c), `${c} → ${fromLch(toLch(c))}`).toBe(true);
      expect(close(fromHsv(toHsv(c)), c), c).toBe(true);
    }
    expect(toHsv('#ff0000')).toEqual({ h: 0, s: 1, v: 1 });
    expect(fromHsv({ h: 120, s: 1, v: 1 })).toBe('#00ff00');
  });

  it('brings a colour outside sRGB inside it by lowering its chroma', () => {
    const c = fromLch({ l: 0.7, c: 0.5, h: 200 });
    expect(isHex(c)).toBe(true);
    expect(toLch(c).c).toBeLessThan(0.5);
    expect(Math.abs(toLch(c).h - 200)).toBeLessThan(3);
  });

  it('moves only the lightness, and only as far as text needs', () => {
    const dark = ['#0a0d12', '#10161e'];
    // Readable already: unchanged.
    expect(readableOn('#4cc9f0', dark)).toBe('#4cc9f0');
    // Navy on a dark page: lightened to just past 4.5:1, same hue.
    const navy = readableOn('#1a237e', dark);
    for (const s of dark) expect(contrast(navy, s)).toBeGreaterThanOrEqual(4.5);
    expect(Math.min(...dark.map((s) => contrast(navy, s)))).toBeLessThan(4.8);
    expect(Math.abs(toLch(navy).h - toLch('#1a237e').h)).toBeLessThan(4);
    // Pale yellow on white: darkened.
    const yellow = readableOn('#fff59d', ['#ffffff', '#eef1f5']);
    expect(contrast(yellow, '#ffffff')).toBeGreaterThanOrEqual(4.5);
    expect(toLch(yellow).l).toBeLessThan(toLch('#fff59d').l);
  });

  it('turns a colour round the hue circle at the same lightness', () => {
    const t = turn('#4cc9f0', 90);
    expect(Math.abs(toLch(t).l - toLch('#4cc9f0').l)).toBeLessThan(0.02);
    expect(isHex('#4CC9F0')).toBe(false);
    expect(isHex('#4cc9f0')).toBe(true);
  });
});

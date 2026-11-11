import { describe, expect, it } from 'vitest';
import { CYAN, LAYER, MAGENTA, MARK, VIEW, WORD } from './brand';
import { PIXELS, SCATTER } from './brand-scatter';
import source from './components/Logo.svelte?raw';

// Logo.svelte writes the shapes out (they compile to plain markup); the share card draws them from
// brand.ts. The two must stay alike.
const logo = source.replace(/\s+/g, ' ');

describe('the logo', () => {
  it('draws the same shapes in Logo.svelte as brand.ts gives the share card', () => {
    expect(logo).toContain(`d="${MARK.d}"`);
    expect(logo).toContain(`d="${WORD.d}"`);
    expect(logo).toContain(`translate(${MARK.at.join(',')}) scale(${MARK.scale})`);
    expect(logo).toContain(`translate(${WORD.at.join(',')}) scale(${WORD.scale})`);
    expect(logo).toContain(`stroke-width="${MARK.width}"`);
    expect(logo).toContain(`stroke-width="${WORD.width}"`);
    const [mx, my] = MARK.shift;
    const [wx, wy] = WORD.shift;
    expect(logo).toContain(`stroke="${MAGENTA}" opacity="${LAYER}" transform="translate(${-mx},${-my})"`);
    expect(logo).toContain(`stroke="${CYAN}" opacity="${LAYER}" transform="translate(${mx},${my})"`);
    expect(logo).toContain(`transform="translate(${-wx},${-wy}) skewX(${WORD.skew})"`);
    expect(logo).toContain(`transform="translate(${wx},${wy}) skewX(${WORD.skew})"`);
    for (const v of Object.values(VIEW)) expect(logo).toContain(`'${v.join(' ')}'`);
  });

  it("keeps the artwork's pixels: 40 by the X, 132 by the word, all inside the logo", () => {
    expect(SCATTER.mark).toHaveLength(40);
    expect(SCATTER.word).toHaveLength(132);
    for (const [x, y, s, c, o] of [...SCATTER.mark, ...SCATTER.word]) {
      expect([3.5, 4.5, 5.5, 7]).toContain(s);
      expect([0, 1, 2]).toContain(c);
      expect(o).toBeGreaterThanOrEqual(55);
      expect(o).toBeLessThanOrEqual(95);
      expect(Math.abs(x)).toBeLessThan(420);
      expect(Math.abs(y)).toBeLessThan(120);
    }
    expect(PIXELS.mark.match(/<rect /g)).toHaveLength(40);
    expect(PIXELS.word).toContain('fill="var(--logo-ink, #fff)"');
  });
});

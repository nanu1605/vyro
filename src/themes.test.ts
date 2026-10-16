import { describe, expect, it } from 'vitest';
import { ACCENTS, THEMES, accentPair, contrast, over, parseColor, tokens, uiState } from './themes';
import type { SensorSnapshot } from './types';
import rust from '../src-tauri/src/look.rs?raw';

const AA = 4.5;
const ACCENT_IDS = ['theme', ...ACCENTS.map((a) => a.id)];

/** Every surface text sits on, made opaque (Glass cards over the bare page and both washes). */
function surfaces(id: string): Array<[string, string]> {
  const p = THEMES.find((t) => t.id === id)!.palette;
  const under = [p.bg, ...p.glow.filter((g) => g !== 'transparent')];
  return [
    ['bg', p.bg],
    ['panel', p.panel],
    ['panel-edge', p.panelEdge],
    ...under.flatMap((u, i): Array<[string, string]> => [
      [`card/${i}`, over(p.card, u)],
      [`card-top/${i}`, over(p.cardTop, u)],
    ]),
  ];
}

describe('themes', () => {
  it('give every theme and accent AA contrast for text and the colours used as text', () => {
    const misses: string[] = [];
    for (const t of THEMES) {
      for (const accent of ACCENT_IDS) {
        const v = tokens(t.id, accent);
        const fg = ['--text', '--muted', '--accent-cpu', '--accent-gpu', '--ok', '--warn', '--hot'];
        for (const [where, bg] of surfaces(t.id)) {
          for (const name of fg) {
            const c = contrast(v[name]!, bg);
            if (c < AA) misses.push(`${t.id}/${accent}: ${name} on ${where} ${c.toFixed(2)}`);
          }
        }
      }
    }
    expect(misses).toEqual([]);
  });

  it('keep text readable on the tinted strips and buttons', () => {
    // color-mix(in srgb, X n%, surface), as App.svelte and the cards use them.
    const mix = (x: string, n: number, s: string) => over(`rgb(${parseColor(x).slice(0, 3).join(' ')} / ${n}%)`, s);
    for (const t of THEMES) {
      for (const accent of ACCENT_IDS) {
        const v = tokens(t.id, accent);
        const tints = [
          mix(v['--accent-cpu']!, 16, v['--bg']!),
          // --accent-soft: the selected tab.
          mix(v['--accent-cpu']!, 16, v['--panel']!),
          mix(v['--warn']!, 8, v['--panel']!),
          mix(v['--hot']!, 10, v['--panel']!),
        ];
        for (const bg of tints) expect(contrast(v['--text']!, bg), `${t.id}/${accent} on ${bg}`).toBeGreaterThanOrEqual(7);
      }
    }
  });

  it('keep warning colours apart from the accents', () => {
    // An accent must never read as a warning: no accent hue near amber or red.
    const hue = (c: string) => {
      const [r, g, b] = parseColor(c).map((x) => x / 255) as [number, number, number];
      const max = Math.max(r, g, b);
      const d = max - Math.min(r, g, b);
      if (d === 0) return 0;
      const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
      return (h * 60 + 360) % 360;
    };
    for (const t of THEMES) {
      for (const accent of ACCENT_IDS) {
        for (const c of accentPair(t, accent)) {
          const h = hue(c);
          expect(h > 70 && h < 335, `${t.id}/${accent} ${c} hue ${h.toFixed(0)}`).toBe(true);
        }
      }
    }
  });

  it('fall back to Midnight and the theme accent for unknown names', () => {
    expect(tokens('nope', 'nope')).toEqual(tokens('midnight', 'theme'));
    expect(tokens('daylight', 'violet')['--accent-cpu']).toBe(ACCENTS.find((a) => a.id === 'violet')!.light[0]);
    expect(tokens('cyber', 'theme')['--accent-cpu']).toBe(THEMES.find((t) => t.id === 'cyber')!.accent[0]);
  });

  it('match the window colours in the app', () => {
    // src-tauri/src/look.rs paints the window before the page loads; same backgrounds.
    for (const t of THEMES) {
      const name = t.id[0]!.toUpperCase() + t.id.slice(1);
      const m = new RegExp(`Theme::${name} => \\(0x([0-9a-f]{2}), 0x([0-9a-f]{2}), 0x([0-9a-f]{2})\\)`).exec(rust);
      expect(m, t.id).not.toBeNull();
      expect(`#${m![1]}${m![2]}${m![3]}`).toBe(t.palette.bg);
    }
  });

  it('works out the page state from Health and the game', () => {
    const snap = (overall: string, game: string | null) =>
      ({ health: { overall }, game: game ? { state: game } : null }) as unknown as SensorSnapshot;
    expect(uiState(null)).toBe('idle');
    expect(uiState(snap('ok', null))).toBe('idle');
    expect(uiState(snap('ok', 'unavailable'))).toBe('idle');
    expect(uiState(snap('ok', 'capturing'))).toBe('gaming');
    expect(uiState(snap('warn', 'capturing'))).toBe('warning');
    expect(uiState(snap('fail', null))).toBe('critical');
  });

  it('blends and measures colours', () => {
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(over('rgb(255 255 255 / 50%)', '#000000')).toBe('#808080');
    expect(parseColor('transparent')[3]).toBe(0);
    expect(() => parseColor('red')).toThrow();
  });
});

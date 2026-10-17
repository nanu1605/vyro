import { describe, expect, it } from 'vitest';
import { fromLch } from './color';
import {
  ACCENTS,
  THEMES,
  accentFills,
  accentPair,
  contrast,
  customPair,
  nearWarning,
  over,
  parseColor,
  textSurfaces,
  tokens,
  uiState,
} from './themes';
import type { SensorSnapshot } from './types';
import rust from '../src-tauri/src/look.rs?raw';

const AA = 4.5;
const ACCENT_IDS = ['theme', ...ACCENTS.map((a) => a.id)];

/** Every surface text sits on, made opaque (Glass cards over the bare page and both washes). */
const surfaces = (id: string) => textSurfaces(THEMES.find((t) => t.id === id)!);

/** Custom accents (1.3): a grid round the colour space, and the hard cases by name. */
const CUSTOM = [
  ...Array.from({ length: 24 }, (_, i) => i * 15).flatMap((h) =>
    [0.04, 0.1, 0.16, 0.3].flatMap((c) => [0.15, 0.35, 0.55, 0.75, 0.95].map((l) => fromLch({ l, c, h }))),
  ),
  '#ff0000',
  '#0000ff',
  '#1a237e',
  '#000000',
  '#ffffff',
  '#fff59d',
  '#808080',
];

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

  it('give any custom accent AA text and readable tints, and show fills as picked', () => {
    const misses: string[] = [];
    const mix = (x: string, n: number, s: string) => over(`rgb(${parseColor(x).slice(0, 3).join(' ')} / ${n}%)`, s);
    for (const t of THEMES) {
      const on = surfaces(t.id);
      for (const accent of CUSTOM) {
        const v = tokens(t.id, accent);
        if (v['--accent-cpu-fill'] !== accent) misses.push(`${t.id}/${accent}: fill ${v['--accent-cpu-fill']}`);
        for (const name of ['--accent-cpu', '--accent-gpu']) {
          for (const [where, bg] of on) {
            const c = contrast(v[name]!, bg);
            if (c < AA) misses.push(`${t.id}/${accent}: ${name} on ${where} ${c.toFixed(2)}`);
          }
        }
        for (const bg of [mix(v['--accent-cpu']!, 16, v['--bg']!), mix(v['--accent-cpu']!, 16, v['--panel']!)]) {
          const c = contrast(v['--text']!, bg);
          if (c < 7) misses.push(`${t.id}/${accent}: text on tint ${c.toFixed(2)}`);
        }
      }
    }
    expect(misses.slice(0, 20)).toEqual([]);
  });

  it('give a custom accent a GPU colour away from the warning hues', () => {
    for (const t of THEMES) {
      for (const accent of CUSTOM) {
        const [, second] = customPair(t, accent).fill;
        if (second !== t.accent[1]) expect(nearWarning(second), `${t.id}/${accent} → ${second}`).toBe(false);
      }
    }
    // Greys keep the theme's own second colour.
    expect(accentFills(THEMES[0]!, '#808080')[1]).toBe(THEMES[0]!.accent[1]);
    expect(nearWarning('#ff0000')).toBe(true);
    expect(nearWarning('#4cc9f0')).toBe(false);
    expect(nearWarning('#808080')).toBe(false);
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

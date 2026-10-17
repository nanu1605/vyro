// Themes and accents (P25). Each theme is a set of CSS variables put on the page root, so every
// view and canvas reads the same tokens; OK / warning / problem keep their meaning in every
// theme and only change shade to keep contrast (themes.test.ts checks WCAG AA for each pair).

import { isHex, readableOn, toLch, fromLch, over } from './color';
import type { SensorSnapshot } from './types';

export type ThemeId = 'midnight' | 'amoled' | 'cyber' | 'glass' | 'minimal' | 'daylight';
/** `theme`: the theme's own accent. */
export type AccentId = 'theme' | 'cyan' | 'blue' | 'violet' | 'magenta' | 'green';
/** Any colour (1.3), as `#rrggbb`: fills show it as picked, text a shade that stays readable. */
export type CustomAccent = `#${string}`;
export type AccentChoice = AccentId | CustomAccent;
/** Which tab the window opens on, at launch and when opened from the tray. */
export type OpenOn = 'dashboard' | 'last';
/** The page's state, shown by the header rail and the background wash. */
export type UiState = 'idle' | 'gaming' | 'warning' | 'critical';

export interface Palette {
  bg: string;
  /** Opaque surfaces: dialogs, toasts, sticky table heads, inner boxes. */
  panel: string;
  panelEdge: string;
  /** Card surfaces (`.panel`), top and body; translucent on Glass. */
  card: string;
  cardTop: string;
  line: string;
  text: string;
  muted: string;
  ok: string;
  warn: string;
  hot: string;
  gaugeTrack: string;
  gaugeTick: string;
  /** History series 3–6 (1 and 2 are the accents). */
  series: [string, string, string, string];
  /** The background wash, top left and top right; `transparent` for none. */
  glow: [string, string];
  radius: string;
  shadow: string;
}

export interface Theme {
  id: ThemeId;
  name: string;
  description: string;
  light: boolean;
  palette: Palette;
  /** The theme's own accent: main (CPU, links, focus) and second (GPU). */
  accent: [string, string];
}

export interface Accent {
  id: Exclude<AccentId, 'theme'>;
  name: string;
  dark: [string, string];
  light: [string, string];
}

const MIDNIGHT: Palette = {
  bg: '#0a0d12',
  panel: '#10161e',
  panelEdge: '#1a2330',
  card: '#10161e',
  cardTop: '#1a2330',
  line: '#243040',
  text: '#e6edf3',
  muted: '#8593a5',
  ok: '#57d68d',
  warn: '#f5b942',
  hot: '#ff5d5d',
  gaugeTrack: '#18212c',
  gaugeTick: '#334155',
  series: ['#c792ea', '#ffb86b', '#ff79c6', '#f1fa8c'],
  glow: ['#122033', '#0f2419'],
  radius: '14px',
  shadow: '0 8px 24px rgb(0 0 0 / 45%)',
};

export const THEMES: readonly Theme[] = [
  {
    id: 'midnight',
    name: 'Midnight',
    description: 'Deep blue instrument panel.',
    light: false,
    palette: MIDNIGHT,
    accent: ['#4cc9f0', '#7ee787'],
  },
  {
    id: 'amoled',
    name: 'AMOLED',
    description: 'Pure black, no glow.',
    light: false,
    palette: {
      ...MIDNIGHT,
      bg: '#000000',
      panel: '#050607',
      panelEdge: '#121417',
      card: '#050607',
      cardTop: '#050607',
      line: '#25282d',
      text: '#f1f3f5',
      muted: '#9aa0a9',
      gaugeTrack: '#15171a',
      gaugeTick: '#33373d',
      glow: ['transparent', 'transparent'],
    },
    accent: ['#4cc9f0', '#7ee787'],
  },
  {
    id: 'cyber',
    name: 'Cyber',
    description: 'Violet night with neon.',
    light: false,
    palette: {
      ...MIDNIGHT,
      bg: '#0a0614',
      panel: '#120b22',
      panelEdge: '#1e1338',
      card: '#120b22',
      cardTop: '#1e1338',
      line: '#35225c',
      text: '#f3ecff',
      muted: '#a897cc',
      ok: '#3ef0a0',
      warn: '#ffd23f',
      hot: '#ff6b6b',
      gaugeTrack: '#1a1030',
      gaugeTick: '#3d2a66',
      series: ['#b388ff', '#ffb86b', '#7ee787', '#f1fa8c'],
      glow: ['#2a0b3d', '#062a33'],
    },
    accent: ['#29e7ff', '#f15bd8'],
  },
  {
    id: 'glass',
    name: 'Glass',
    description: 'See-through cards over a colour wash.',
    light: false,
    palette: {
      ...MIDNIGHT,
      bg: '#0b1222',
      panel: '#131c30',
      panelEdge: '#1d2942',
      card: 'rgb(22 32 54 / 62%)',
      cardTop: 'rgb(34 47 76 / 62%)',
      line: '#2b3a58',
      text: '#eef3fa',
      muted: '#a3b3c9',
      hot: '#ff7a7a',
      gaugeTrack: 'rgb(255 255 255 / 8%)',
      gaugeTick: 'rgb(255 255 255 / 22%)',
      glow: ['#1d3b7a', '#4a1d66'],
      radius: '16px',
    },
    accent: ['#5fd4f5', '#8eeb96'],
  },
  {
    id: 'minimal',
    name: 'Minimal',
    description: 'Flat graphite, no gradients.',
    light: false,
    palette: {
      ...MIDNIGHT,
      bg: '#16181c',
      panel: '#1c1f24',
      panelEdge: '#262a31',
      card: '#1c1f24',
      cardTop: '#1c1f24',
      line: '#30353d',
      text: '#e8eaed',
      muted: '#9ca3ad',
      ok: '#6dd58c',
      warn: '#f2c25c',
      hot: '#f47a70',
      gaugeTrack: '#262a31',
      gaugeTick: '#3d434c',
      glow: ['transparent', 'transparent'],
      radius: '8px',
      shadow: '0 6px 18px rgb(0 0 0 / 35%)',
    },
    accent: ['#8ab4f8', '#81c995'],
  },
  {
    id: 'daylight',
    name: 'Daylight',
    description: 'Light, for bright rooms.',
    light: true,
    palette: {
      bg: '#eef1f5',
      panel: '#ffffff',
      panelEdge: '#e8edf3',
      card: '#ffffff',
      cardTop: '#f4f7fa',
      line: '#d0d7e0',
      text: '#17202b',
      muted: '#535f70',
      ok: '#15722f',
      warn: '#8a5a00',
      hot: '#c4232f',
      gaugeTrack: '#e2e8f0',
      gaugeTick: '#b4bfcc',
      series: ['#8250df', '#bc4c00', '#bf3989', '#6f6300'],
      glow: ['#dbe7f7', '#dff1e5'],
      radius: '14px',
      shadow: '0 8px 24px rgb(23 32 43 / 18%)',
    },
    accent: ['#0b62c4', '#15722f'],
  },
];

export const ACCENTS: readonly Accent[] = [
  { id: 'cyan', name: 'Cyan', dark: ['#4cc9f0', '#7ee787'], light: ['#0a6c8c', '#15722f'] },
  { id: 'blue', name: 'Blue', dark: ['#6cb6ff', '#7ee787'], light: ['#0b5fbf', '#15722f'] },
  { id: 'violet', name: 'Violet', dark: ['#b392f0', '#4cc9f0'], light: ['#6f42c1', '#0a6c8c'] },
  { id: 'magenta', name: 'Magenta', dark: ['#f47fd8', '#4cc9f0'], light: ['#a5248f', '#0a6c8c'] },
  { id: 'green', name: 'Green', dark: ['#7ee787', '#4cc9f0'], light: ['#15722f', '#0b5fbf'] },
];

export const DEFAULT_THEME: ThemeId = 'midnight';
export const DEFAULT_ACCENT: AccentId = 'theme';

export const theme = (id: string): Theme => THEMES.find((t) => t.id === id) ?? THEMES[0]!;

/** Every surface text sits on in a theme, made opaque (Glass cards over the bare page and both
 * washes): what a custom accent's text shade must be readable on. */
export function textSurfaces(t: Theme): Array<[string, string]> {
  const p = t.palette;
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

/** OKLCH hues that read as a warning (reds, oranges, ambers): a custom GPU colour avoids them,
 * and the picker says when the main colour is among them. */
export const nearWarning = (hex: string): boolean => {
  const { c, h } = toLch(hex);
  return c > 0.05 && (h < 100 || h > 345);
};

/** A custom accent on a theme: the colours as picked (fills) and the shades text takes. The GPU
 * colour is the main one turned round the hue circle (away from warning hues); a grey keeps
 * the theme's own second colour. */
export function customPair(t: Theme, hex: string): { fill: [string, string]; text: [string, string] } {
  const x = toLch(hex);
  let second = t.accent[1];
  if (x.c >= 0.03) {
    const hue = [-75, 75, 150, -150].map((d) => (x.h + d + 360) % 360).find((h) => !(h < 100 || h > 345));
    second = fromLch({ ...x, h: hue ?? (x.h + 180) % 360 });
  }
  const on = textSurfaces(t).map(([, c]) => c);
  return { fill: [hex, second], text: [readableOn(hex, on), readableOn(second, on)] };
}

/** The accent pair (main, second) for a theme and accent choice, as text takes it. */
export function accentPair(t: Theme, accent: string): [string, string] {
  if (isHex(accent)) return customPair(t, accent).text;
  const a = ACCENTS.find((x) => x.id === accent);
  return a ? (t.light ? a.light : a.dark) : t.accent;
}

/** The accent pair as fills show it: the colour picked for a custom accent, else the same as
 * [`accentPair`]. */
export function accentFills(t: Theme, accent: string): [string, string] {
  return isHex(accent) ? customPair(t, accent).fill : accentPair(t, accent);
}

/** The CSS variables for a theme and accent. */
export function tokens(themeId: string, accent: string): Record<string, string> {
  const t = theme(themeId);
  const p = t.palette;
  const [main, second] = accentPair(t, accent);
  const [mainFill, secondFill] = accentFills(t, accent);
  return {
    '--bg': p.bg,
    '--panel': p.panel,
    '--panel-edge': p.panelEdge,
    '--card': p.card,
    '--card-top': p.cardTop,
    '--line': p.line,
    '--text': p.text,
    '--muted': p.muted,
    '--accent-cpu': main,
    '--accent-gpu': second,
    // Gauges, graphs and the 3-D view: a custom accent exactly as picked (1.3).
    '--accent-cpu-fill': mainFill,
    '--accent-gpu-fill': secondFill,
    '--ok': p.ok,
    '--warn': p.warn,
    '--hot': p.hot,
    '--series-3': p.series[0],
    '--series-4': p.series[1],
    '--series-5': p.series[2],
    '--series-6': p.series[3],
    '--gauge-track': p.gaugeTrack,
    '--gauge-tick': p.gaugeTick,
    '--glow-1': p.glow[0],
    '--glow-2': p.glow[1],
    '--radius': p.radius,
    '--shadow': p.shadow,
    // The cards' drop (P28): soft on a light page, deep on a dark one.
    '--shadow-color': t.light ? 'rgb(23 32 43 / 14%)' : 'rgb(0 0 0 / 70%)',
  };
}

const LOOK_KEY = 'vyro.look';

export interface Look {
  theme: ThemeId;
  accent: AccentChoice;
  openOn: OpenOn;
}

/** The look last applied, kept in the page's storage so the first paint already has it (the
 * settings file stays the source and corrects it once loaded). */
export function savedLook(): Look {
  const fallback: Look = { theme: DEFAULT_THEME, accent: DEFAULT_ACCENT, openOn: 'dashboard' };
  try {
    const v = JSON.parse(localStorage.getItem(LOOK_KEY) ?? 'null') as Partial<Look> | null;
    if (!v || typeof v !== 'object') return fallback;
    return {
      theme: THEMES.some((t) => t.id === v.theme) ? v.theme! : fallback.theme,
      accent:
        v.accent === 'theme' || ACCENTS.some((a) => a.id === v.accent) || isHex(String(v.accent))
          ? v.accent!
          : fallback.accent,
      openOn: v.openOn === 'last' ? 'last' : 'dashboard',
    };
  } catch {
    return fallback;
  }
}

function remember(look: Look): void {
  try {
    localStorage.setItem(LOOK_KEY, JSON.stringify(look));
  } catch {
    /* storage unavailable: the settings file still has it */
  }
}

let applied = '';

/** Puts a look on the page and tells canvases to redraw; nothing happens when it is unchanged. */
export function applyLook(look: Look, root: HTMLElement = document.documentElement): void {
  remember(look);
  const key = `${look.theme}/${look.accent}`;
  if (key === applied) return;
  applied = key;
  const t = theme(look.theme);
  for (const [k, v] of Object.entries(tokens(t.id, look.accent))) root.style.setProperty(k, v);
  root.style.colorScheme = t.light ? 'light' : 'dark';
  root.dataset.theme = t.id;
  window.dispatchEvent(new Event(THEME_EVENT));
}

const THEME_EVENT = 'vyro-theme';

/** Calls `cb` after each theme change (canvases cache colours). */
export function onTheme(cb: () => void): () => void {
  window.addEventListener(THEME_EVENT, cb);
  return () => window.removeEventListener(THEME_EVENT, cb);
}

/** The page's state: Health first, then a measured game. */
export function uiState(snap: Pick<SensorSnapshot, 'health' | 'game'> | null): UiState {
  const overall = snap?.health.overall;
  if (overall === 'fail') return 'critical';
  if (overall === 'warn') return 'warning';
  if (snap?.game && snap.game.state !== 'unavailable') return 'gaming';
  return 'idle';
}

// WCAG 2 contrast and colour maths live in color.ts; the tests and swatches import them here too.
export { contrast, over, parseColor } from './color';

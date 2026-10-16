// Themes and accents (P25). Each theme is a set of CSS variables put on the page root, so every
// view and canvas reads the same tokens; OK / warning / problem keep their meaning in every
// theme and only change shade to keep contrast (themes.test.ts checks WCAG AA for each pair).

import type { SensorSnapshot } from './types';

export type ThemeId = 'midnight' | 'amoled' | 'cyber' | 'glass' | 'minimal' | 'daylight';
/** `theme`: the theme's own accent. */
export type AccentId = 'theme' | 'cyan' | 'blue' | 'violet' | 'magenta' | 'green';
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

/** The accent pair (main, second) for a theme and accent choice. */
export function accentPair(t: Theme, accent: string): [string, string] {
  const a = ACCENTS.find((x) => x.id === accent);
  return a ? (t.light ? a.light : a.dark) : t.accent;
}

/** The CSS variables for a theme and accent. */
export function tokens(themeId: string, accent: string): Record<string, string> {
  const t = theme(themeId);
  const p = t.palette;
  const [main, second] = accentPair(t, accent);
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
  };
}

const LOOK_KEY = 'vyro.look';

export interface Look {
  theme: ThemeId;
  accent: AccentId;
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
      accent: v.accent === 'theme' || ACCENTS.some((a) => a.id === v.accent) ? v.accent! : fallback.accent,
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

// WCAG 2 contrast, for the tests and the theme picker's swatches.

type Rgba = [number, number, number, number];

/** `#rrggbb` or `rgb(r g b / a%)`; `transparent` is fully clear. */
export function parseColor(c: string): Rgba {
  if (c === 'transparent') return [0, 0, 0, 0];
  const hex = /^#([0-9a-f]{6})$/i.exec(c);
  if (hex) {
    const n = parseInt(hex[1]!, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const rgb = /^rgb\((\d+) (\d+) (\d+)(?: \/ (\d+)%)?\)$/.exec(c);
  if (rgb) return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), rgb[4] != null ? Number(rgb[4]) / 100 : 1];
  throw new Error(`unknown colour ${c}`);
}

/** `top` over an opaque `under`. */
export function over(top: string, under: string): string {
  const [r, g, b, a] = parseColor(top);
  const [r2, g2, b2] = parseColor(under);
  const mix = (x: number, y: number) => Math.round(x * a + y * (1 - a));
  return `#${[mix(r, r2), mix(g, g2), mix(b, b2)].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
}

function luminance(c: string): number {
  const [r, g, b] = parseColor(c).map((v, i) => (i < 3 ? v / 255 : v)) as Rgba;
  const lin = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** Contrast ratio of two opaque colours, 1–21. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p) as [number, number];
  return (x + 0.05) / (y + 0.05);
}

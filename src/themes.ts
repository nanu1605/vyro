// Themes and accents (P25). Each theme is a set of CSS variables put on the page root, so every
// view and canvas reads the same tokens; OK / warning / problem keep their meaning in every
// theme and only change shade to keep contrast (themes.test.ts checks WCAG AA for each pair).

import { contrast, isHex, readableOn, toLch, fromLch, over, parseColor } from './color';
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
  /** The glass (2.5), used while Glass effects are on. */
  glass: Glass;
}

/** The glass (2.5): see-through surfaces over a colourful wash, in `rgb(r g b / a%)`. Only
 * layers that float over content (dialogs, menus, toasts, the top bar) blur what is under them;
 * cards sit on the soft wash, where a blur would look the same and cost frames. */
export interface Glass {
  /** The wash: top left, top right and lower middle; `transparent` for none. */
  aurora: [string, string, string];
  /** Cards (`.panel`), body and top. */
  card: string;
  cardTop: string;
  /** The sidebar, which nothing passes under. */
  bar: string;
  /** Layers over content, blurred: dialogs, Ctrl K, menus, toasts, the top bar. */
  float: string;
  /** The window's own colour, in %, over Windows' Acrylic (the see-through window). */
  base: number;
  /** Muted text and the problem colour a shade lighter (darker on a light theme) where the wash
   * is brighter than the theme's own page; the theme's own when absent. */
  muted?: string;
  hot?: string;
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
  glass: {
    aurora: ['#18355f', '#10402d', '#38256e'],
    card: 'rgb(14 20 28 / 60%)',
    cardTop: 'rgb(20 28 42 / 62%)',
    bar: 'rgb(14 20 28 / 66%)',
    float: 'rgb(14 20 28 / 80%)',
    base: 70,
    muted: '#a1aec0',
    hot: '#ff7f7f',
  },
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
      // Black stays black: no wash, and the window is not see-through.
      glass: {
        aurora: ['transparent', 'transparent', 'transparent'],
        card: 'rgb(255 255 255 / 5%)',
        cardTop: 'rgb(255 255 255 / 8%)',
        bar: 'rgb(255 255 255 / 4%)',
        float: 'rgb(14 15 17 / 82%)',
        base: 100,
      },
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
      glass: {
        aurora: ['#3d1260', '#06323d', '#4c1350'],
        card: 'rgb(16 10 30 / 60%)',
        cardTop: 'rgb(26 16 50 / 62%)',
        bar: 'rgb(16 10 30 / 66%)',
        float: 'rgb(16 10 30 / 80%)',
        base: 70,
        muted: '#b8a8da',
        hot: '#ff8080',
      },
    },
    accent: ['#29e7ff', '#f15bd8'],
  },
  {
    id: 'glass',
    name: 'Glass',
    description: 'The clearest glass, over a blue-violet wash.',
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
      glass: {
        aurora: ['#173470', '#461a6c', '#0b3f50'],
        card: 'rgb(20 30 52 / 52%)',
        cardTop: 'rgb(30 42 70 / 54%)',
        bar: 'rgb(20 30 52 / 56%)',
        float: 'rgb(18 26 46 / 76%)',
        base: 62,
        muted: '#b3c1d4',
        hot: '#ff9a9a',
      },
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
      // Neutral grey glass, no wash.
      glass: {
        aurora: ['transparent', 'transparent', 'transparent'],
        card: 'rgb(34 37 43 / 60%)',
        cardTop: 'rgb(34 37 43 / 60%)',
        bar: 'rgb(28 31 36 / 60%)',
        float: 'rgb(28 31 36 / 82%)',
        base: 72,
      },
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
      // White frosted glass.
      glass: {
        aurora: ['#d4e4fa', '#d6f0e0', '#e8def8'],
        card: 'rgb(255 255 255 / 66%)',
        cardTop: 'rgb(255 255 255 / 74%)',
        bar: 'rgb(255 255 255 / 55%)',
        float: 'rgb(250 251 253 / 88%)',
        base: 72,
        muted: '#4a5566',
        hot: '#b81f2b',
      },
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

/** `#rrggbb` at `pct` % opacity, as `rgb(r g b / pct%)`. */
export const alpha = (hex: string, pct: number): string => `rgb(${parseColor(hex).slice(0, 3).join(' ')} / ${pct}%)`;

/** Every surface text sits on in a theme, made opaque (Glass cards over the bare page and both
 * washes): what a custom accent's text shade must be readable on. With `glass`, the glass's
 * surfaces too ([`glassSurfaces`]). */
export function textSurfaces(t: Theme, glass = false): Array<[string, string]> {
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
    ...(glass ? glassSurfaces(t) : []),
  ];
}

/** The pane's edge and the light it catches (2.5): a faint line all round, a lit rim along the
 * top and a dimmer one down the left, and a sheen fading down from the top. */
export function glassEdge(t: Theme): { line: string; rim: string; rimLow: string; sheen: string } {
  return t.light
    ? { line: 'rgb(23 32 43 / 11%)', rim: 'rgb(255 255 255 / 95%)', rimLow: 'rgb(255 255 255 / 55%)', sheen: 'rgb(255 255 255 / 55%)' }
    : { line: 'rgb(255 255 255 / 9%)', rim: 'rgb(255 255 255 / 20%)', rimLow: 'rgb(255 255 255 / 7%)', sheen: 'rgb(255 255 255 / 3%)' };
}

/** What the see-through window's base lies on: Windows' Acrylic over a black and over a white
 * wallpaper. WinUI's own Acrylic keeps 96 % (dark) or 85 % (light) of its tint's luminosity
 * over the wallpaper; this takes 85 % for both, so the contrast tests keep a margin. */
export function acrylicUnder(t: Theme): string[] {
  const tint = t.light ? 'rgb(252 252 252 / 85%)' : 'rgb(44 44 44 / 85%)';
  return ['#000000', '#ffffff'].map((w) => over(tint, w));
}

/** The glass's surfaces (2.5), made opaque: the page (its base, the wash's colours and, when
 * the window is see-through, the base over Acrylic), cards, the sidebar over each, and a
 * floating layer over the haze a blur makes of what is under it. */
export function glassSurfaces(t: Theme): Array<[string, string]> {
  let out = glassOf.get(t);
  if (!out) glassOf.set(t, (out = glassOn(t)));
  return out;
}

const glassOf = new WeakMap<Theme, Array<[string, string]>>();

function glassOn(t: Theme): Array<[string, string]> {
  const p = t.palette;
  const g = p.glass;
  const base = alpha(p.bg, g.base);
  const page = [
    p.bg,
    ...g.aurora.filter((a) => a !== 'transparent'),
    ...(g.base < 100 ? acrylicUnder(t).map((u) => over(base, u)) : []),
  ];
  // A card's top carries the sheen, as does a floating layer's.
  const { sheen } = glassEdge(t);
  const out = page.flatMap((u, i): Array<[string, string]> => [
    [`glass-page/${i}`, u],
    [`glass-card/${i}`, over(g.card, u)],
    [`glass-card-top/${i}`, over(sheen, over(g.cardTop, u))],
    [`glass-bar/${i}`, over(g.bar, u)],
  ]);
  // A blurred layer shows a haze of what is under it: the surface nearest the text's own
  // lightness, with a fifth of text blurred into it, and under a dialog the dim as well.
  const near = out.map(([, c]) => c).sort((a, b) => contrast(b, p.text) - contrast(a, p.text)).at(-1)!;
  const haze = over(alpha(p.text, 20), near);
  const float = (under: string) => over(sheen, over(g.float, under));
  out.push(['glass-float', float(haze)], ['glass-float/dimmed', float(over('rgb(0 0 0 / 55%)', haze))]);
  return out;
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
export function customPair(t: Theme, hex: string, glass = false): { fill: [string, string]; text: [string, string] } {
  // Asked for again and again with the same colours (each theme's swatch in Settings, every
  // look applied): worked out once each. Few are ever picked; dragging the wheel makes many.
  const key = `${t.id}|${hex.toLowerCase()}|${glass ? 1 : 0}`;
  const had = pairs.get(key);
  if (had) return had;
  if (pairs.size >= 512) pairs.clear();
  const made = workPair(t, hex, glass);
  pairs.set(key, made);
  return made;
}
const pairs = new Map<string, { fill: [string, string]; text: [string, string] }>();

function workPair(t: Theme, hex: string, glass: boolean): { fill: [string, string]; text: [string, string] } {
  const x = toLch(hex);
  let second = t.accent[1];
  if (x.c >= 0.03) {
    const hue = [-75, 75, 150, -150].map((d) => (x.h + d + 360) % 360).find((h) => !(h < 100 || h > 345));
    second = fromLch({ ...x, h: hue ?? (x.h + 180) % 360 });
  }
  const on = textSurfaces(t, glass).map(([, c]) => c);
  return { fill: [hex, second], text: [readableOn(hex, on), readableOn(second, on)] };
}

/** The accent pair (main, second) for a theme and accent choice, as text takes it; with
 * `glass`, readable on the glass's surfaces too. */
export function accentPair(t: Theme, accent: string, glass = false): [string, string] {
  if (isHex(accent)) return customPair(t, accent, glass).text;
  const a = ACCENTS.find((x) => x.id === accent);
  return a ? (t.light ? a.light : a.dark) : t.accent;
}

/** The accent pair as fills show it: the colour picked for a custom accent, else the same as
 * [`accentPair`]. */
export function accentFills(t: Theme, accent: string, glass = false): [string, string] {
  return isHex(accent) ? customPair(t, accent, glass).fill : accentPair(t, accent, glass);
}

/** The CSS variables for a theme and accent, with Glass effects on or off. Off gives 2.4's
 * values exactly; the `--glass-*` ones are there either way and only used while on. */
export function tokens(themeId: string, accent: string, glass = false): Record<string, string> {
  const t = theme(themeId);
  const p = t.palette;
  const g = p.glass;
  const [main, second] = accentPair(t, accent, glass);
  const [mainFill, secondFill] = accentFills(t, accent, glass);
  const edge = glassEdge(t);
  return {
    '--bg': p.bg,
    '--panel': p.panel,
    '--panel-edge': p.panelEdge,
    '--card': p.card,
    '--card-top': p.cardTop,
    '--line': p.line,
    '--text': p.text,
    '--muted': glass ? (g.muted ?? p.muted) : p.muted,
    '--accent-cpu': main,
    '--accent-gpu': second,
    // Gauges, graphs and the 3-D view: a custom accent exactly as picked (1.3).
    '--accent-cpu-fill': mainFill,
    '--accent-gpu-fill': secondFill,
    '--ok': p.ok,
    '--warn': p.warn,
    '--hot': glass ? (g.hot ?? p.hot) : p.hot,
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
    // The logo's main strokes (2.2): its own white, or the light logo's near-black (brand.ts's
    // INK_DARK and INK_LIGHT, written here so the pages that only show the logo skip brand.ts).
    '--logo-ink': t.light ? '#15151F' : '#FFFFFF',
    // The glass (2.5), used under `:root[data-glass='on']` (theme.css, app.css).
    '--glass-aurora-1': g.aurora[0],
    '--glass-aurora-2': g.aurora[1],
    '--glass-aurora-3': g.aurora[2],
    '--glass-card': g.card,
    '--glass-card-top': g.cardTop,
    '--glass-bar': g.bar,
    '--glass-float': g.float,
    // The page's own colour under a see-through window.
    '--glass-base': alpha(p.bg, g.base),
    '--glass-line': edge.line,
    '--glass-rim': edge.rim,
    '--glass-rim-low': edge.rimLow,
    '--glass-sheen': edge.sheen,
  };
}

const LOOK_KEY = 'clokrix.look';

export interface Look {
  theme: ThemeId;
  accent: AccentChoice;
  openOn: OpenOn;
  /** Glass effects (2.5); on unless set off. */
  glass?: boolean;
}

/** The look last applied, kept in the page's storage so the first paint already has it (the
 * settings file stays the source and corrects it once loaded). */
export function savedLook(): Look & { glass: boolean } {
  const fallback = { theme: DEFAULT_THEME, accent: DEFAULT_ACCENT, openOn: 'dashboard' as OpenOn, glass: true };
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
      glass: v.glass !== false,
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

/** The system asks for less see-through (Windows' Transparency effects off, or a phone's
 * Reduce Transparency), where the browser says so. */
export function reducedTransparency(): boolean {
  try {
    return matchMedia('(prefers-reduced-transparency: reduce)').matches;
  } catch {
    return false;
  }
}

let applied = '';

/** Puts a look on the page and tells canvases to redraw; nothing happens when it is unchanged.
 * Glass is on unless the look turns it off or the system asks for less transparency. */
export function applyLook(look: Look, root: HTMLElement = document.documentElement): void {
  remember(look);
  const glass = look.glass !== false && !reducedTransparency();
  const key = `${look.theme}/${look.accent}/${glass}`;
  if (key === applied) return;
  applied = key;
  const t = theme(look.theme);
  for (const [k, v] of Object.entries(tokens(t.id, look.accent, glass))) root.style.setProperty(k, v);
  root.style.colorScheme = t.light ? 'light' : 'dark';
  root.dataset.theme = t.id;
  root.dataset.glass = glass ? 'on' : 'off';
  window.dispatchEvent(new Event(THEME_EVENT));
}

/** Marks the page as the see-through window's (Windows' Acrylic behind it), or not. */
export function applySeeThrough(on: boolean, root: HTMLElement = document.documentElement): void {
  if (on) root.dataset.window = 'see-through';
  else delete root.dataset.window;
}

const THEME_EVENT = 'clokrix-theme';

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

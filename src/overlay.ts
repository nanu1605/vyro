// Overlay 2.0 (P14) on the window's side: formatting the rows the app sends, and, for the
// browser preview and the Settings preview, the same rows and window size as
// `vyro_core::gaming::overlay` and `src-tauri/src/overlay.rs`.

import { fmt } from './format';
import { isHex } from './color';
import { ACCENTS as APP_ACCENTS, customPair, theme as themeOf } from './themes';
import type { FramePoint, GameSnapshot, OverlayMetric, OverlayMode, OverlayRow, SensorSnapshot } from './types';

export type OverlayCorner = 'topLeft' | 'topRight' | 'bottomLeft' | 'bottomRight';
/** `app`: the app's own accent (1.1), in a shade that reads on the dark plate. */
export type OverlayAccent = 'app' | 'green' | 'cyan' | 'amber' | 'pink' | 'white';

export interface OverlaySettings {
  enabled: boolean;
  mode: OverlayMode;
  corner: OverlayCorner;
  /** From the corner inward, logical px (0–200). */
  offsetX: number;
  offsetY: number;
  /** Percent, 75–200. */
  scale: number;
  /** Background opacity, percent. */
  opacity: number;
  /** Readings shown; empty: the mode's. */
  metrics: OverlayMetric[];
  /** Frame-time graph in Performance and Full mode (Graph mode always has it). */
  graph: boolean;
  /** Refreshes at most this often, ms. */
  updateMs: number;
  accent: OverlayAccent;
  compact: boolean;
}

export const DEFAULT_OVERLAY: OverlaySettings = {
  enabled: false, mode: 'performance', corner: 'topLeft', offsetX: 0, offsetY: 0, scale: 100, opacity: 72,
  metrics: [], graph: true, updateMs: 1000, accent: 'green', compact: false,
};

/** What the overlay page shows (`OverlayData` in `src-tauri/src/overlay.rs`). */
export interface OverlayData {
  app: string;
  paused: boolean;
  mode: OverlayMode;
  rows: OverlayRow[];
  timeline: Array<FramePoint | null>;
  corner: OverlayCorner;
  scale: number;
  opacity: number;
  accent: OverlayAccent;
  compact: boolean;
}

export const MODES: Array<{ id: OverlayMode; label: string; text: string }> = [
  { id: 'minimal', label: 'Minimal', text: 'Frame rate on one line.' },
  { id: 'performance', label: 'Performance', text: 'Frame rate, 1% low, CPU and GPU use and temperature.' },
  { id: 'full', label: 'Full', text: 'Everything: frame time, power, VRAM, latency, what limits the frame rate.' },
  { id: 'graph', label: 'Graph', text: 'Frame rate over a large frame-time graph.' },
];

export const ACCENTS: Record<Exclude<OverlayAccent, 'app'>, string> = {
  green: '#7ee787', cyan: '#4cc9f0', amber: '#f5b942', pink: '#ff79c6', white: '#f2f6fa',
};

/** The plate's colour. For `app`: the app accent's dark-theme shade, since the plate is always
 * dark; a light theme's own accent (made for white) gives way to cyan; a custom accent takes
 * the shade it reads in on Midnight. */
export function plateColor(accent: OverlayAccent, look: { theme: string; accent: string }): string {
  if (accent !== 'app') return ACCENTS[accent] ?? ACCENTS.green;
  if (isHex(look.accent)) return customPair(themeOf('midnight'), look.accent).text[0];
  const named = APP_ACCENTS.find((a) => a.id === look.accent);
  if (named) return named.dark[0];
  const t = themeOf(look.theme);
  return t.light ? APP_ACCENTS[0]!.dark[0] : t.accent[0];
}

export const UPDATE_CHOICES = [500, 1000, 2000] as const;

/** Every reading, in the order the overlay shows them, with its label. */
export const METRICS: Array<{ id: OverlayMetric; label: string }> = [
  { id: 'fps', label: 'FPS' },
  { id: 'low1', label: '1% low' },
  { id: 'low01', label: '0.1% low' },
  { id: 'frameTime', label: 'Frame' },
  { id: 'latency', label: 'Latency' },
  { id: 'cpuUsage', label: 'CPU' },
  { id: 'cpuTemp', label: 'CPU temp' },
  { id: 'cpuPower', label: 'CPU power' },
  { id: 'gpuUsage', label: 'GPU' },
  { id: 'gpuTemp', label: 'GPU temp' },
  { id: 'gpuPower', label: 'GPU power' },
  { id: 'vram', label: 'VRAM' },
  { id: 'bound', label: 'Limit' },
];

const DEFAULTS: Record<OverlayMode, OverlayMetric[]> = {
  minimal: ['fps'],
  performance: ['fps', 'low1', 'cpuUsage', 'cpuTemp', 'gpuUsage', 'gpuTemp'],
  full: METRICS.map((m) => m.id),
  graph: ['fps', 'low1'],
};

/** The readings a mode shows unless the owner picked their own. */
export const defaultMetrics = (mode: OverlayMode): OverlayMetric[] => DEFAULTS[mode];

/** The readings shown: the custom pick (in the fixed order), else the mode's. */
export function shownMetrics(mode: OverlayMode, custom: OverlayMetric[]): OverlayMetric[] {
  if (custom.length === 0) return DEFAULTS[mode];
  return METRICS.map((m) => m.id).filter((id) => custom.includes(id));
}

export const graphShown = (s: Pick<OverlaySettings, 'mode' | 'graph'>): boolean =>
  s.mode === 'graph' || (s.graph && s.mode !== 'minimal');

/** Room the overlay window needs, logical px (mirrors `vyro_core::gaming::overlay::size`). */
export function overlaySize(mode: OverlayMode, shown: OverlayMetric[], graph: boolean, compact: boolean, scale: number): [number, number] {
  const rest = shown.filter((m) => m !== 'fps' && m !== 'low1').length;
  const [row, pad] = compact ? [17, 10] : [21, 16];
  let w: number;
  let h: number;
  if (mode === 'minimal') {
    // The frame rate, then each other reading, wrapping past 744 px (the plate's widest).
    const need = 130 + 111 * Math.max(0, shown.length - 1);
    const lines = Math.max(1, Math.ceil(need / 744));
    w = Math.min(760, need);
    h = (compact ? 38 : 46) + (lines - 1) * 20;
  } else if (mode === 'graph') {
    w = 320;
    h = 52 + 120 + Math.ceil(rest / 2) * row + pad;
  } else {
    // Full is wider: its two columns hold the longer labels ("GPU power").
    w = mode === 'full' ? (compact ? 260 : 300) : compact ? 236 : 268;
    h = 52 + Math.ceil(rest / 2) * row + (graph ? 52 : 0) + pad;
  }
  const s = Math.min(200, Math.max(75, scale)) / 100;
  return [Math.ceil(w * s), Math.ceil(h * s)];
}

/** Rows from a snapshot (mirrors `vyro_core::gaming::overlay::rows`). */
export function overlayRows(shown: OverlayMetric[], snap: SensorSnapshot, game: GameSnapshot): OverlayRow[] {
  const live = game.state === 'capturing';
  const frames = (v: number | null) => (live ? v : null);
  const gpu = snap.gpu[0];
  const value: Record<OverlayMetric, number | null> = {
    fps: frames(game.fps),
    low1: frames(game.low1Fps),
    low01: frames(game.low01Fps),
    frameTime: frames(game.frameMs),
    latency: frames(game.latencyMs),
    cpuUsage: snap.cpu.usageTotal,
    cpuTemp: snap.cpu.tempPackage,
    cpuPower: snap.cpu.powerPkgW,
    gpuUsage: gpu?.usage ?? null,
    gpuTemp: gpu?.tempEdge ?? null,
    gpuPower: gpu?.powerW ?? null,
    vram: gpu?.vramUsedMB ?? null,
    bound: null,
  };
  const bound = live && game.bound ? { gpu: 'GPU', cpu: 'CPU', capped: 'Capped' }[game.bound] : null;
  return shown.map((metric) => ({
    metric,
    label: METRICS.find((m) => m.id === metric)!.label,
    value: value[metric],
    text: metric === 'bound' ? bound : null,
  }));
}

/** What the overlay shows for these settings (mirrors `OverlayData::new`). */
export function overlayData(s: OverlaySettings, snap: SensorSnapshot, game: GameSnapshot): OverlayData {
  return {
    app: game.app,
    paused: game.state !== 'capturing',
    mode: s.mode,
    rows: overlayRows(shownMetrics(s.mode, s.metrics), snap, game),
    timeline: graphShown(s) ? game.timeline : [],
    corner: s.corner,
    scale: s.scale,
    opacity: s.opacity,
    accent: s.accent,
    compact: s.compact,
  };
}

/** A row's value as shown; temperatures through `temp` (the owner's unit). */
export function formatRow(row: OverlayRow, temp: (c: number) => string | null): string {
  if (row.metric === 'bound') return row.text ?? '—';
  const v = row.value;
  if (v == null) return '—';
  switch (row.metric) {
    case 'frameTime':
    case 'latency':
      return `${fmt(v, 1)} ms`;
    case 'cpuUsage':
    case 'gpuUsage':
      return `${fmt(v)}%`;
    case 'cpuTemp':
    case 'gpuTemp':
      return temp(v) ?? '—';
    case 'cpuPower':
    case 'gpuPower':
      return `${fmt(v)} W`;
    case 'vram':
      return v >= 1024 ? `${fmt(v / 1024, 1)} GB` : `${fmt(v)} MB`;
    default:
      return fmt(v);
  }
}

const STEPS = [4, 5, 8, 10, 12.5, 16.7, 20, 25, 33.3, 50, 100, 200];

/** Top of the frame-time graph, ms: a round value above twice the median frame. */
export function graphScale(points: Array<FramePoint | null>): number {
  const avgs = points.flatMap((p) => (p ? [p.avgMs] : [])).sort((a, b) => a - b);
  if (!avgs.length) return 16.7;
  const need = avgs[Math.floor(avgs.length / 2)]! * 2.2;
  return STEPS.find((s) => s >= need) ?? STEPS[STEPS.length - 1]!;
}

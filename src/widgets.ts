// Desktop widgets (P30) on the page's side: the kinds, sizes and defaults, the same rules as
// `src-tauri/src/widgets.rs` (which checks what is saved). Pure, so it is unit-tested.

import { freshId } from './dashboard-edit';
import type { ProcessSnapshot } from './types';

export type DeskKind = 'gauge' | 'reading' | 'graph' | 'stack' | 'clock' | 'network' | 'game' | 'top';
export type DeskStyle = 'app' | 'glass' | 'solid';
export type DeskLayer = 'desktop' | 'top';

export interface DesktopWidget {
  id: string;
  kind: DeskKind;
  /** Reading ids, as the Dashboard's; none for "network" and "game"; for "top", what it ranks by. */
  sources: string[];
  /** Top left on the desktop, logical px; kept by the app as the widget is dragged. */
  x: number;
  y: number;
  placed: boolean;
  /** 50–200 %. */
  scale: number;
  /** The card's background, 0–100 %. */
  opacity: number;
  style: DeskStyle;
  layer: DeskLayer;
  locked: boolean;
  /** While locked, the mouse goes through to what is under the widget. */
  clickThrough: boolean;
}

export interface Widgets {
  version: number;
  /** On the desktop (the Widgets page's switch and the tray's). */
  shown: boolean;
  widgets: DesktopWidget[];
}

export const MAX_WIDGETS = 8;
export const MAX_SOURCES = 6;

/** What a widget's form picks: readings, what "top" ranks by, or nothing (it shows a fixed set). */
export type DeskPicks = 'readings' | 'rank' | 'none';

export const DESK_KINDS: ReadonlyArray<{ id: DeskKind; name: string; text: string; sources: number; defaults: string[]; picks: DeskPicks }> = [
  { id: 'gauge', name: 'Gauge', text: 'One reading on a dial.', sources: 1, defaults: ['cpu.usage'], picks: 'readings' },
  { id: 'reading', name: 'Reading', text: 'One reading, large.', sources: 1, defaults: ['cpu.temp'], picks: 'readings' },
  { id: 'graph', name: 'Graph', text: 'A reading and its last minute.', sources: 1, defaults: ['gpu.0.usage'], picks: 'readings' },
  {
    id: 'stack',
    name: 'Stack',
    text: 'Up to six readings, one under the other.',
    sources: MAX_SOURCES,
    defaults: ['cpu.usage', 'cpu.temp', 'gpu.0.usage', 'gpu.0.temp', 'mem.load'],
    picks: 'readings',
  },
  { id: 'clock', name: 'Clock', text: 'The time and date, with two readings.', sources: 2, defaults: ['cpu.temp', 'gpu.0.temp'], picks: 'readings' },
  { id: 'network', name: 'Network', text: 'Download and upload, and their last minute.', sources: 0, defaults: [], picks: 'none' },
  { id: 'game', name: 'Game FPS', text: "The running game's frame rate and its last minute.", sources: 0, defaults: [], picks: 'none' },
  { id: 'top', name: 'Top processes', text: 'The five busiest programs by CPU, memory or GPU.', sources: 1, defaults: ['proc.cpu'], picks: 'rank' },
];

export const kindOf = (k: string) => DESK_KINDS.find((x) => x.id === k) ?? DESK_KINDS[1]!;

export const EMPTY: Widgets = { version: 1, shown: true, widgets: [] };

/** A new widget of a kind, with its usual readings and a fresh id. */
export function newWidget(kind: DeskKind, taken: Iterable<string>): DesktopWidget {
  return {
    id: freshId(kind, taken),
    kind,
    sources: [...kindOf(kind).defaults],
    x: 0,
    y: 0,
    placed: false,
    scale: 100,
    opacity: 85,
    style: 'app',
    layer: 'desktop',
    locked: false,
    clickThrough: false,
  };
}

/** The window's size at the widget's scale, logical px (as `widgets::size`). */
export function widgetSize(w: Pick<DesktopWidget, 'kind' | 'sources' | 'scale'>): [number, number] {
  const [bw, bh] =
    w.kind === 'gauge'
      ? [200, 200]
      : w.kind === 'graph'
        ? [300, 150]
        : w.kind === 'stack'
          ? [250, 52 + 36 * Math.max(1, w.sources.length)]
          : w.kind === 'clock'
            ? [260, 120]
            : w.kind === 'network'
              ? [280, 150]
              : w.kind === 'game'
                ? [260, 150]
                : w.kind === 'top'
                  ? [260, 200]
                  : [220, 104];
  const k = w.scale / 100;
  return [bw * k, bh * k];
}

/** Sets a widget's readings, keeping as many as its kind shows. */
export function withSources(w: DesktopWidget, sources: string[]): DesktopWidget {
  return { ...w, sources: [...new Set(sources)].slice(0, kindOf(w.kind).sources) };
}

/** The widget's id from the window's address (`#widget=<id>`), or null. */
export function widgetIdFromHash(hash: string): string | null {
  const m = /^#widget=([a-z0-9-]{1,24})$/.exec(hash);
  return m ? m[1]! : null;
}

/** What a "top" widget ranks by (`PROC_SORTS` in `widgets.rs`). */
export type ProcSort = 'proc.cpu' | 'proc.ram' | 'proc.gpu';
export const PROC_SORTS: ReadonlyArray<{ id: ProcSort; name: string }> = [
  { id: 'proc.cpu', name: 'CPU' },
  { id: 'proc.ram', name: 'Memory' },
  { id: 'proc.gpu', name: 'GPU' },
];
/** A "top" widget's ranking; CPU when it has none. */
export const sortOf = (w: Pick<DesktopWidget, 'sources'>): ProcSort => PROC_SORTS.find((s) => s.id === w.sources[0])?.id ?? 'proc.cpu';

export interface ProcRow {
  name: string;
  /** Processes of that name. */
  count: number;
  /** % of the CPU or GPU, or MB of memory, added up. */
  value: number;
}

/**
 * The `n` busiest programs by a measure, one row per name with its processes added up (a browser
 * runs many). The snapshot lists the busiest processes by each measure, so the sums hold what
 * counts; programs at zero are left out.
 */
export function topProcesses(list: ReadonlyArray<ProcessSnapshot>, by: ProcSort, n = 5): ProcRow[] {
  const key = by === 'proc.ram' ? 'ramMB' : by === 'proc.gpu' ? 'gpuPct' : 'cpuPct';
  const rows = new Map<string, ProcRow>();
  for (const p of list) {
    const v = p[key];
    if (v == null) continue;
    const r = rows.get(p.name) ?? { name: p.name, count: 0, value: 0 };
    r.count += 1;
    r.value += v;
    rows.set(p.name, r);
  }
  return [...rows.values()]
    .filter((r) => r.value > 0)
    .sort((a, b) => b.value - a.value || a.name.localeCompare(b.name))
    .slice(0, n);
}

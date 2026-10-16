// Desktop widgets (P30) on the page's side: the kinds, sizes and defaults, the same rules as
// `src-tauri/src/widgets.rs` (which checks what is saved). Pure, so it is unit-tested.

import { freshId } from './dashboard-edit';

export type DeskKind = 'gauge' | 'reading' | 'graph' | 'stack' | 'clock';
export type DeskStyle = 'app' | 'glass' | 'solid';
export type DeskLayer = 'desktop' | 'top';

export interface DesktopWidget {
  id: string;
  kind: DeskKind;
  /** Reading ids, as the Dashboard's. */
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
}

export interface Widgets {
  version: number;
  /** On the desktop (the Widgets page's switch and the tray's). */
  shown: boolean;
  widgets: DesktopWidget[];
}

export const MAX_WIDGETS = 8;
export const MAX_SOURCES = 6;

export const DESK_KINDS: ReadonlyArray<{ id: DeskKind; name: string; text: string; sources: number; defaults: string[] }> = [
  { id: 'gauge', name: 'Gauge', text: 'One reading on a dial.', sources: 1, defaults: ['cpu.usage'] },
  { id: 'reading', name: 'Reading', text: 'One reading, large.', sources: 1, defaults: ['cpu.temp'] },
  { id: 'graph', name: 'Graph', text: 'A reading and its last minute.', sources: 1, defaults: ['gpu.0.usage'] },
  { id: 'stack', name: 'Stack', text: 'Up to six readings, one under the other.', sources: MAX_SOURCES, defaults: ['cpu.usage', 'cpu.temp', 'gpu.0.usage', 'gpu.0.temp', 'mem.load'] },
  { id: 'clock', name: 'Clock', text: 'The time and date, with two readings.', sources: 2, defaults: ['cpu.temp', 'gpu.0.temp'] },
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

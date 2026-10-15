// Dashboard editor (P24): the palette, sources and layout edits. Loaded with the editor, so the
// Dashboard itself does not carry it. Each edit returns a new layout; the editor keeps them until
// "Done".

import { GPU_PARTS, isKind, MAX_LAYOUTS, MAX_ROWS, MAX_WIDGETS, NAME_MAX, readingDef, STATIC, WIDTHS, widgetsOf, type CardId, type WidgetKind } from './dashboard';
import { isPreset, PRESETS } from './dashboard-presets';
import type { Dashboard, Layout, SensorSnapshot, Widget } from './types';

export const KINDS: Record<WidgetKind, { label: string; text: string; w: number; h: number; source: string | null }> = {
  gauge: { label: 'Gauge', text: 'Usage dial with clock and a short graph', w: 6, h: 3, source: 'cpu' },
  reading: { label: 'Reading', text: 'One live value', w: 3, h: 1, source: 'cpu.temp' },
  graph: { label: 'Graph', text: 'The last minute of a reading', w: 4, h: 2, source: 'cpu.temp' },
  history: { label: 'History', text: 'The last hour of a recorded sensor', w: 6, h: 3, source: 'cpu.temp.package' },
  game: { label: 'Game', text: 'FPS, 1% low and frame times while a game runs', w: 6, h: 2, source: null },
  health: { label: 'Health', text: 'The grade and what needs attention', w: 4, h: 2, source: null },
  events: { label: 'Events', text: 'The latest health changes', w: 4, h: 2, source: null },
  card: { label: 'Card', text: 'One of the detailed cards', w: 4, h: 2, source: 'cores' },
};

export const CARDS: Record<CardId, string> = {
  cores: 'CPU cores',
  cpuSensors: 'CPU sensors',
  gpus: 'GPUs',
  memory: 'Memory',
  power: 'Power',
  board: 'Board and fans',
  self: 'Vyro itself',
};
export const isCard = (c: string | null): c is CardId => c != null && c in CARDS;

/** Every reading this PC has, for the palette. */
export function readingIds(snap: SensorSnapshot | null): string[] {
  const gpus = (snap?.gpu ?? []).flatMap((_, i) => Object.keys(GPU_PARTS).map((p) => `gpu.${i}.${p}`));
  const fans = ['fan.cpu', 'fan.pump', ...(snap?.fans ?? []).map((_, i) => `fan.${i}`)];
  return [...STATIC.map((r) => r.id), ...(gpus.length ? gpus : Object.keys(GPU_PARTS).map((p) => `gpu.0.${p}`)), ...fans];
}

/** "cpu" or "gpu.N" for a gauge. */
export function gaugeSources(snap: SensorSnapshot | null): string[] {
  return ['cpu', ...(snap?.gpu.length ? snap.gpu.map((_, i) => `gpu.${i}`) : ['gpu.0'])];
}

/** A short name for a widget, for its edit bar and screen readers. */
export function widgetTitle(w: Widget, snap: SensorSnapshot | null): string {
  if (!isKind(w.kind)) return 'Widget from a newer Vyro';
  const kind = KINDS[w.kind].label;
  switch (w.kind) {
    case 'gauge':
      return w.source === 'cpu' ? 'CPU gauge' : `GPU ${Number(w.source?.split('.')[1] ?? 0) + 1} gauge`;
    case 'reading':
    case 'graph': {
      const r = readingDef(w.source ?? '', snap);
      return `${kind}: ${r?.label ?? 'unknown reading'}`;
    }
    case 'history':
      return `History: ${w.source ?? 'no sensor'}`;
    case 'card':
      return `${isCard(w.source) ? CARDS[w.source] : 'Unknown'} card`;
    default:
      return kind;
  }
}

/** The first `prefix-N` not taken. */
export function freshId(prefix: string, taken: Iterable<string>): string {
  const used = new Set(taken);
  for (let n = 1; ; n++) if (!used.has(`${prefix}${n}`)) return `${prefix}${n}`;
}

const at = (l: Layout, id: string): number => l.widgets.findIndex((w) => w.id === id);

/** Moves a widget to index `to` (clamped). */
export function move(l: Layout, id: string, to: number): Layout {
  const from = at(l, id);
  if (from < 0) return l;
  const widgets = [...l.widgets];
  const [w] = widgets.splice(from, 1);
  widgets.splice(Math.max(0, Math.min(to, widgets.length)), 0, w!);
  return { ...l, widgets };
}

/** One step wider (+1) or narrower (-1) through the allowed widths. */
export function stepWidth(w: number, dir: 1 | -1): number {
  const i = WIDTHS.findIndex((x) => x >= w);
  const cur = i < 0 ? WIDTHS.length - 1 : i;
  return WIDTHS[Math.max(0, Math.min(WIDTHS.length - 1, cur + dir))]!;
}

/** The nearest allowed width (the narrower on a tie). */
export function snapWidth(w: number): number {
  return WIDTHS.reduce((best, x) => (Math.abs(x - w) < Math.abs(best - w) ? x : best), WIDTHS[0] as number);
}

export function resize(l: Layout, id: string, w: number, h: number): Layout {
  return {
    ...l,
    widgets: l.widgets.map((x) => (x.id === id ? { ...x, w: snapWidth(w), h: Math.max(1, Math.min(MAX_ROWS, Math.round(h))) } : x)),
  };
}

export function setSource(l: Layout, id: string, source: string | null): Layout {
  return { ...l, widgets: l.widgets.map((x) => (x.id === id ? { ...x, source } : x)) };
}

export function duplicate(l: Layout, id: string): Layout {
  const i = at(l, id);
  if (i < 0 || l.widgets.length >= MAX_WIDGETS) return l;
  const copy = { ...l.widgets[i]!, id: freshId('w', l.widgets.map((w) => w.id)) };
  const widgets = [...l.widgets];
  widgets.splice(i + 1, 0, copy);
  return { ...l, widgets };
}

export function remove(l: Layout, id: string): Layout {
  return { ...l, widgets: l.widgets.filter((w) => w.id !== id) };
}

/** Adds a widget of a kind at the end, with its usual size and source. */
export function add(l: Layout, kind: WidgetKind, source?: string | null): Layout {
  if (l.widgets.length >= MAX_WIDGETS) return l;
  const k = KINDS[kind];
  const w: Widget = { id: freshId('w', l.widgets.map((x) => x.id)), kind, w: k.w, h: k.h, source: source === undefined ? k.source : source };
  return { ...l, widgets: [...l.widgets, w] };
}

/** Back to the preset it was made from (Default for one made from scratch); name and lock kept. */
export function reset(l: Layout): Layout {
  return { ...l, widgets: widgetsOf(PRESETS[isPreset(l.preset) ? l.preset : 'default'].widgets) };
}

/** A copy of `from` as a new layout, shown; null when there are already as many as allowed. */
export function newLayout(d: Dashboard, from: Layout, name: string): Dashboard | null {
  if (d.layouts.length >= MAX_LAYOUTS) return null;
  const id = freshId('layout-', d.layouts.map((l) => l.id));
  const copy: Layout = { ...from, id, name: name.trim().slice(0, NAME_MAX) || `Layout ${d.layouts.length + 1}`, locked: false, widgets: from.widgets.map((w) => ({ ...w })) };
  return { ...d, active: id, layouts: [...d.layouts, copy] };
}

/** Removes a layout (never the last); the first left is shown if it was the shown one. */
export function deleteLayout(d: Dashboard, id: string): Dashboard {
  if (d.layouts.length <= 1) return d;
  const layouts = d.layouts.filter((l) => l.id !== id);
  return { ...d, layouts, active: d.active === id ? layouts[0]!.id : d.active };
}


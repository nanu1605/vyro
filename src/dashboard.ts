// Dashboard builder (P24): widget kinds, live readings and presets, for drawing the Dashboard.
// The editor's part is in `dashboard-edit.ts`. Pure, so it is unit-tested; `dashboard.json` (app
// config folder) keeps the layouts.

import type { Dashboard, Layout, PawnIoStatus, SensorSnapshot, TempUnit, Widget } from './types';
import { tempSymbol, toUnit } from './units';

export type WidgetKind = 'gauge' | 'reading' | 'graph' | 'history' | 'game' | 'health' | 'events' | 'card';
export type CardId = 'cores' | 'cpuSensors' | 'gpus' | 'memory' | 'power' | 'board' | 'self';

/** Columns of 12; the same list the app checks the file against. */
export const WIDTHS = [3, 4, 6, 8, 12] as const;
export const MAX_ROWS = 4;
export const MAX_LAYOUTS = 12;
export const MAX_WIDGETS = 40;
export const NAME_MAX = 40;

const KIND_IDS: ReadonlySet<string> = new Set<WidgetKind>(['gauge', 'reading', 'graph', 'history', 'game', 'health', 'events', 'card']);
export const isKind = (k: string): k is WidgetKind => KIND_IDS.has(k);

/** Why a driver's reading is missing, by what reads it: the CPU (its temperature and power, and
 * the system total made with them) or the board's sensor chip (fans). */
export interface Needs {
  cpu: string;
  board: string;
}
export const NEEDS_PAWNIO = 'needs PawnIO';
export const NOT_REPORTED = 'not reported';
export const NOT_THIS_CPU = 'not read on this CPU';

/** The reasons from PawnIO's state: "not reported" once it is installed, and a CPU Vyro does not
 * read (an Intel one) says so whatever its state. Both said "needs PawnIO" until 2.0.4. */
export function needsFor(pawnio: PawnIoStatus | null): Needs {
  const board = pawnio?.state === 'ready' ? NOT_REPORTED : NEEDS_PAWNIO;
  return { cpu: pawnio?.cpuUnread ? NOT_THIS_CPU : board, board };
}

/** Readings that need PawnIO (or the service) to have a value, and what reads them. */
export const needsDriver = (id: string): keyof Needs | null =>
  id === 'cpu.temp' || id === 'cpu.power' || id === 'power.system' ? 'cpu' : id.startsWith('fan.') ? 'board' : null;

const reason = (needs: Needs, id: string): string | null => {
  const from = needsDriver(id);
  return from ? needs[from] : null;
};

/** Cards the compact profiles leave out, as the Dashboard always has. */
export const DETAIL_CARDS: ReadonlySet<string> = new Set(['cores', 'memory', 'power', 'board', 'self']);

// ---------------------------------------------------------------------------------------------
// Readings
// ---------------------------------------------------------------------------------------------

export interface Reading {
  id: string;
  label: string;
  /** "°" for a temperature in °C (shown in the chosen unit). */
  unit: string;
  digits: number;
  /** Colour limits, in the reading's own unit (°C for temperatures). */
  warn?: number;
  hot?: number;
  /** Only while a game is measured (hidden otherwise outside the editor). */
  game?: boolean;
  /** For the palette. */
  group: string;
  value: (s: SensorSnapshot) => number | null;
  sub?: (s: SensorSnapshot) => string | null;
}

const gb = (mb: number | null | undefined): number | null => (mb == null ? null : mb / 1024);
const ofGb = (mb: number | null | undefined): string | null => (mb == null ? null : `of ${(mb / 1024).toFixed(1)} GB`);
const primaryNet = (s: SensorSnapshot) => s.network.find((n) => n.primary && n.connected) ?? s.network.find((n) => n.connected);
const sum = (xs: Array<number | null>): number | null => (xs.some((x) => x != null) ? xs.reduce<number>((a, x) => a + (x ?? 0), 0) : null);
const max = (xs: Array<number | null>): number | null => {
  const v = xs.filter((x): x is number => x != null);
  return v.length ? Math.max(...v) : null;
};

export const STATIC: Reading[] = [
  { id: 'cpu.usage', label: 'CPU usage', unit: '%', digits: 0, group: 'CPU', value: (s) => s.cpu.usageTotal },
  { id: 'cpu.clock', label: 'CPU clock', unit: 'MHz', digits: 0, group: 'CPU', value: (s) => s.cpu.mhzCurrent },
  { id: 'cpu.temp', label: 'CPU package', unit: '°', digits: 0, warn: 80, hot: 95, group: 'CPU', value: (s) => s.cpu.tempPackage },
  { id: 'cpu.power', label: 'CPU power', unit: 'W', digits: 1, group: 'CPU', value: (s) => s.cpu.powerPkgW },
  { id: 'mem.used', label: 'Memory', unit: 'GB', digits: 1, group: 'Memory', value: (s) => gb(s.memory.usedMB), sub: (s) => ofGb(s.memory.totalMB) },
  {
    id: 'mem.load',
    label: 'Memory load',
    unit: '%',
    digits: 0,
    warn: 85,
    hot: 95,
    group: 'Memory',
    value: (s) => (s.memory.usedMB != null && s.memory.totalMB ? (s.memory.usedMB / s.memory.totalMB) * 100 : null),
  },
  { id: 'power.system', label: 'System power', unit: 'W', digits: 0, group: 'Power', value: (s) => s.power.estTotalW, sub: (s) => (s.power.estTotalW == null ? null : 'estimated') },
  { id: 'net.down', label: 'Download', unit: 'Mbps', digits: 1, group: 'Network', value: (s) => primaryNet(s)?.downMbps ?? null },
  { id: 'net.up', label: 'Upload', unit: 'Mbps', digits: 1, group: 'Network', value: (s) => primaryNet(s)?.upMbps ?? null },
  { id: 'net.ping', label: 'Ping', unit: 'ms', digits: 0, warn: 80, hot: 150, group: 'Network', value: (s) => s.ping.rttMs, sub: (s) => s.ping.host || null },
  { id: 'disk.read', label: 'Disk read', unit: 'MB/s', digits: 1, group: 'Storage', value: (s) => sum(s.storage.map((d) => d.readMBs)) },
  { id: 'disk.write', label: 'Disk write', unit: 'MB/s', digits: 1, group: 'Storage', value: (s) => sum(s.storage.map((d) => d.writeMBs)) },
  { id: 'disk.temp', label: 'Hottest drive', unit: '°', digits: 0, warn: 60, hot: 70, group: 'Storage', value: (s) => max(s.storage.map((d) => d.tempC)) },
  {
    id: 'game.fps',
    label: 'Game FPS',
    unit: 'FPS',
    digits: 0,
    game: true,
    group: 'Game',
    value: (s) => s.game?.fps ?? null,
    sub: (s) => (s.game ? (s.game.low1Fps != null ? `1% low ${Math.round(s.game.low1Fps)} · ${s.game.app}` : s.game.app) : null),
  },
  { id: 'game.low1', label: '1% low', unit: 'FPS', digits: 0, game: true, group: 'Game', value: (s) => s.game?.low1Fps ?? null },
  { id: 'game.frametime', label: 'Frame time', unit: 'ms', digits: 1, game: true, group: 'Game', value: (s) => s.game?.frameMs ?? null },
];

export const GPU_PARTS: Record<string, Omit<Reading, 'id' | 'label' | 'group' | 'value' | 'sub'> & { name: string; value: (g: SensorSnapshot['gpu'][number]) => number | null; sub?: (g: SensorSnapshot['gpu'][number]) => string | null }> = {
  usage: { name: 'usage', unit: '%', digits: 0, value: (g) => g.usage },
  clock: { name: 'clock', unit: 'MHz', digits: 0, value: (g) => g.mhzCore },
  temp: { name: 'temp', unit: '°', digits: 0, warn: 80, hot: 90, value: (g) => g.tempEdge },
  hotspot: { name: 'hotspot', unit: '°', digits: 0, warn: 90, hot: 105, value: (g) => g.tempHotspot },
  power: { name: 'power', unit: 'W', digits: 0, value: (g) => g.powerW },
  vram: { name: 'VRAM', unit: 'GB', digits: 1, value: (g) => gb(g.vramUsedMB), sub: (g) => ofGb(g.vramTotalMB) },
  fan: { name: 'fan', unit: 'RPM', digits: 0, value: (g) => g.fanRpm },
};

/** The reading for an id, or null for an id this version does not know. */
export function readingDef(id: string, snap: SensorSnapshot | null): Reading | null {
  const fixed = STATIC.find((r) => r.id === id);
  if (fixed) return fixed;
  const g = /^gpu\.(\d+)\.(\w+)$/.exec(id);
  if (g) {
    const i = Number(g[1]);
    const part = GPU_PARTS[g[2]!];
    if (!part) return null;
    const many = (snap?.gpu.length ?? 1) > 1;
    const name = part.name === 'VRAM' ? 'VRAM' : `GPU ${part.name}`;
    const gpu = (s: SensorSnapshot) => s.gpu[i];
    return {
      id,
      label: many || i > 0 ? `${name} (GPU ${i + 1})` : name,
      unit: part.unit,
      digits: part.digits,
      warn: part.warn,
      hot: part.hot,
      group: many ? `GPU ${i + 1}` : 'GPU',
      value: (s) => {
        const x = gpu(s);
        return x ? part.value(x) : null;
      },
      sub: part.sub
        ? (s) => {
            const x = gpu(s);
            return x ? part.sub!(x) : null;
          }
        : undefined,
    };
  }
  const f = /^fan\.(\d+|cpu|pump)$/.exec(id);
  if (f) {
    const which = (s: SensorSnapshot) =>
      f[1] === 'cpu' ? s.fans.find((x) => x.kind === 'fan') : f[1] === 'pump' ? s.fans.find((x) => x.kind === 'pump') : s.fans[Number(f[1])];
    const now = snap ? which(snap) : undefined;
    const fallback = f[1] === 'cpu' ? 'CPU fan' : f[1] === 'pump' ? 'Pump' : `Fan ${Number(f[1]) + 1}`;
    return { id, label: now?.name ?? fallback, unit: 'RPM', digits: 0, group: 'Fans', value: (s) => which(s)?.rpm ?? null };
  }
  return null;
}

// ---------------------------------------------------------------------------------------------
// Presets
// ---------------------------------------------------------------------------------------------

export type Spec = [WidgetKind, number, number, string?];

export const r3 = (source: string): Spec => ['reading', 3, 1, source];

/** Today's Dashboard, widget for widget: what shows before any layout is saved. The other
 * presets load on first run and with the editor (`dashboard-presets.ts`). */
export const DEFAULT_SPEC: Spec[] = [
      ['gauge', 6, 3, 'cpu'],
      ['gauge', 6, 3, 'gpu.0'],
      r3('game.fps'),
      r3('cpu.temp'),
      r3('cpu.power'),
      r3('gpu.0.temp'),
      r3('gpu.0.hotspot'),
      r3('gpu.0.vram'),
      r3('mem.used'),
      r3('power.system'),
      r3('fan.cpu'),
      ['card', 4, 2, 'cores'],
      ['card', 4, 2, 'cpuSensors'],
      ['card', 4, 2, 'gpus'],
      ['card', 4, 2, 'memory'],
      ['card', 4, 2, 'power'],
      ['card', 4, 2, 'board'],
      ['card', 4, 2, 'self'],
];

export function widgetsOf(specs: Spec[]): Widget[] {
  return specs.map(([kind, w, h, source], i) => ({ id: `w${i + 1}`, kind, w, h, source: source ?? null }));
}

export const defaultLayout = (): Layout => ({ id: 'default', name: 'Default', preset: 'default', locked: false, widgets: widgetsOf(DEFAULT_SPEC) });

/** Before anything is saved: Default, until the other presets load. */
export const initial = (): Dashboard => ({ version: 1, active: 'default', layouts: [defaultLayout()] });

export function replaceLayout(d: Dashboard, l: Layout): Dashboard {
  return { ...d, layouts: d.layouts.map((x) => (x.id === l.id ? l : x)) };
}

export function activeLayout(d: Dashboard): Layout {
  return d.layouts.find((l) => l.id === d.active) ?? d.layouts[0] ?? defaultLayout();
}

/** Span on the 6-column grid of narrower windows: half for small widgets, else the full row. */
export const narrowSpan = (w: number): number => (w >= 6 ? 6 : 3);

/** A game is being measured (the Dashboard's game readings show then). */
export const gameShown = (snap: SensorSnapshot | null): boolean => !!snap?.game && snap.game.state !== 'unavailable';

/** Whether a widget shows outside the editor: game readings only with a game, and the
 * detail cards not under a compact profile. */
export function shown(w: Widget, snap: SensorSnapshot | null, compact: boolean): boolean {
  if (compact && w.kind === 'card' && DETAIL_CARDS.has(w.source ?? '')) return false;
  if (w.kind === 'reading' && readingDef(w.source ?? '', snap)?.game && !gameShown(snap)) return false;
  return true;
}

/** A reading as shown: temperatures in the chosen unit, limits likewise, and why a driver reading
 * is missing ("needs PawnIO"). */
export function readingView(r: Reading, snap: SensorSnapshot | null, needs: Needs, unit: TempUnit) {
  const raw = snap ? r.value(snap) : null;
  const isTemp = r.unit === '°';
  const conv = (c: number | undefined) => (c == null ? undefined : isTemp ? toUnit(c, unit) : c);
  return {
    value: raw == null ? null : isTemp ? toUnit(raw, unit) : raw,
    unit: isTemp ? tempSymbol(unit) : r.unit,
    warn: conv(r.warn),
    hot: conv(r.hot),
    sub: (snap && r.sub?.(snap)) ?? (raw == null ? reason(needs, r.id) : null),
  };
}

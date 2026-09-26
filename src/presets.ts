// Which history-tracked sensors the History view charts together. One unit per chart.

import type { Sensor, SensorTree, Unit } from './types';

export interface ChartDef {
  key: string;
  title: string;
  unit: Unit;
  /** Axis starts at 0 (usage, power, RPM); otherwise it hugs the data. */
  zero: boolean;
  /** Axis reaches at least this (100 for percentages). */
  max?: number;
  metrics: Array<{ id: string; name: string }>;
}

interface Preset {
  key: string;
  title: string;
  unit: Unit;
  zero: boolean;
  max?: number;
  match: (s: Sensor) => boolean;
  /** Leave out sensors that never read above 0 (unplugged fan headers, idle adapters). */
  skipZero?: boolean;
}

const re = (...xs: RegExp[]) => (s: Sensor) => xs.some((x) => x.test(s.id));

const PRESETS: Preset[] = [
  { key: 'usage', title: 'Usage', unit: '%', zero: true, max: 100, match: re(/^cpu\.usage\.total$/, /^gpu\.\d+\.usage$/) },
  { key: 'temps', title: 'CPU and GPU temperatures', unit: '°C', zero: false, match: re(/^cpu\.temp\.package$/, /^gpu\.\d+\.temp\.edge$/) },
  { key: 'power', title: 'Power', unit: 'W', zero: true, match: re(/^cpu\.power\.package$/, /^gpu\.\d+\.power$/) },
  { key: 'clocks', title: 'Clocks', unit: 'MHz', zero: true, match: re(/^cpu\.clock\.(current|effective)$/, /^gpu\.\d+\.clock\.core$/) },
  { key: 'memory', title: 'Memory and VRAM', unit: 'MB', zero: true, match: re(/^memory\.used$/, /^gpu\.\d+\.vram\.used$/) },
  { key: 'board', title: 'Board and memory temperatures', unit: '°C', zero: false, match: re(/^board\.temp\d+$/, /^memory\.dimm/) },
  { key: 'drives', title: 'Drive temperatures', unit: '°C', zero: false, match: re(/^storage\..+\.temp$/) },
  { key: 'fans', title: 'Fans', unit: 'RPM', zero: true, match: re(/^fan\.\d+$/), skipZero: true },
  {
    key: 'volts',
    title: 'Core voltages',
    unit: 'V',
    zero: false,
    // Low rails only: 12 V on the same axis would flatten them.
    match: (s) => s.id === 'cpu.vid' || (s.id.startsWith('voltage.') && (s.max ?? s.value ?? 0) < 2.5),
  },
  { key: 'disks', title: 'Disk throughput', unit: 'MB/s', zero: true, match: re(/^storage\..+\.(read|write)$/), skipZero: true },
  { key: 'network', title: 'Network', unit: 'Mbps', zero: true, match: re(/^network\.if\d+\.(down|up)$/), skipZero: true },
  { key: 'ping', title: 'Ping', unit: 'ms', zero: true, match: re(/^network\.ping$/) },
];

export function chartsFor(tree: SensorTree | null): ChartDef[] {
  const sensors = (tree?.groups ?? []).flatMap((g) => g.sensors).filter((s) => s.history);
  return PRESETS.map(({ match, skipZero, ...def }) => ({
    ...def,
    metrics: sensors
      .filter((s) => match(s) && s.unit === def.unit && !(skipZero && !((s.max ?? 0) > 0)))
      .map((s) => ({ id: s.id, name: s.name })),
  })).filter((c) => c.metrics.length > 0);
}

/** A one-sensor chart, for a sensor opened from the explorer. */
export function chartForSensor(s: Sensor): ChartDef {
  return {
    key: `focus:${s.id}`,
    title: s.name,
    unit: s.unit,
    zero: ['%', 'W', 'RPM', 'MB', 'MB/s', 'Mbps', 'ms', '/s'].includes(s.unit),
    max: s.unit === '%' ? 100 : undefined,
    metrics: [{ id: s.id, name: s.name }],
  };
}

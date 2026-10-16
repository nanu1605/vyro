// The remote companion page's logic (P26): the pairing code, the device's name, and the view's
// readings as tiles and lists, in the owner's temperature unit.

import { fmt, NA } from '../format';
import { LIMITS } from '../limits';
import type { RemoteView, TempUnit } from '../types';
import { tempSymbol, toUnit } from '../units';

const ALPHABET = '23456789ABCDEFGHJKLMNPQRSTUVWXYZ';

/** What someone typed, canonical (upper case, no dashes or spaces); `null` if it cannot be a
 * code. Mirrors `vyro_core::remote::auth::parse`. */
export function canonCode(typed: string): string | null {
  const s = typed.replace(/[\s‐-―-]/g, '').toUpperCase();
  return s.length === 8 && [...s].every((c) => ALPHABET.includes(c)) ? s : null;
}

/** The code as it is typed, with a dash after four characters: "K7QM-4X". */
export function showCode(typed: string): string {
  const s = typed
    .replace(/[\s‐-―-]/g, '')
    .toUpperCase()
    .slice(0, 8);
  return s.length > 4 ? `${s.slice(0, 4)}-${s.slice(4)}` : s;
}

/** The code in `#pair=K7QM4XPA`, if any. */
export function codeFromHash(hash: string): string | null {
  const m = /^#pair=([A-Za-z0-9-]{8,9})$/.exec(hash);
  return m ? canonCode(m[1]!) : null;
}

/** "Android phone · Chrome": a first guess at this device's name, from its browser. */
export function deviceName(ua: string): string {
  const os = /iPhone/.test(ua)
    ? 'iPhone'
    : /iPad/.test(ua)
      ? 'iPad'
      : /Android/.test(ua)
        ? /Mobile/.test(ua)
          ? 'Android phone'
          : 'Android tablet'
        : /CrOS/.test(ua)
          ? 'Chromebook'
          : /Windows/.test(ua)
            ? 'Windows PC'
            : /Macintosh|Mac OS X/.test(ua)
              ? 'Mac'
              : /Linux/.test(ua)
                ? 'Linux PC'
                : 'Device';
  const browser = /EdgA?\/|EdgiOS/.test(ua)
    ? 'Edge'
    : /SamsungBrowser/.test(ua)
      ? 'Samsung Internet'
      : /Firefox|FxiOS/.test(ua)
        ? 'Firefox'
        : /CriOS|Chrome\//.test(ua)
          ? 'Chrome'
          : /Safari\//.test(ua)
            ? 'Safari'
            : '';
  return browser ? `${os} · ${browser}` : os;
}

export type Level = 'ok' | 'warn' | 'hot' | null;

/** A temperature against Health's [warning, problem] limits. */
export function level(c: number | null | undefined, limits: readonly [number, number]): Level {
  if (c == null || !Number.isFinite(c)) return null;
  return c >= limits[1] ? 'hot' : c >= limits[0] ? 'warn' : 'ok';
}

/** "64 °C", "147 °F", "—". */
export function temp(c: number | null | undefined, unit: TempUnit): string {
  return c == null || !Number.isFinite(c) ? NA : `${fmt(toUnit(c, unit))} ${tempSymbol(unit)}`;
}

const withUnit = (v: number | null | undefined, unit: string, digits = 0) =>
  v == null || !Number.isFinite(v) ? NA : `${fmt(v, digits)} ${unit}`;
const gb = (mb: number | null | undefined) => (mb == null ? NA : fmt(mb / 1024, 1));
const share = (used: number | null | undefined, total: number | null | undefined) =>
  used != null && total != null && total > 0 ? (used / total) * 100 : null;

export interface Tile {
  id: string;
  title: string;
  /** The model, under the title (GPUs). */
  sub: string | null;
  value: string;
  unit: string;
  /** 0–100 for the bar under the value. */
  bar: number | null;
  level: Level;
  lines: Array<[string, string]>;
}

/** The big readings, in the order the page shows them. */
export function tiles(v: RemoteView, unit: TempUnit): Tile[] {
  const out: Tile[] = [
    {
      id: 'cpu',
      title: 'CPU',
      sub: null,
      value: fmt(v.cpu.usage),
      unit: '%',
      bar: v.cpu.usage,
      level: level(v.cpu.temp, LIMITS.cpu),
      lines: [
        ['Temperature', temp(v.cpu.temp, unit)],
        ['Clock', withUnit(v.cpu.mhz, 'MHz')],
        ['Power', withUnit(v.cpu.powerW, 'W')],
      ],
    },
  ];
  // Integrated graphics only when there is nothing else to show.
  const discrete = v.gpus.filter((g) => !g.integrated);
  const gpus = discrete.length ? discrete : v.gpus;
  gpus.forEach((g, i) => {
    const hot = level(g.hotspot, LIMITS.hotspot);
    const core = level(g.temp, LIMITS.gpu);
    const worst = [hot, core].includes('hot') ? 'hot' : [hot, core].includes('warn') ? 'warn' : (core ?? hot);
    out.push({
      id: `gpu${i}`,
      title: gpus.length > 1 ? `GPU ${i + 1}` : 'GPU',
      sub: g.name,
      value: fmt(g.usage),
      unit: '%',
      bar: g.usage,
      level: worst,
      lines: [
        ['Temperature', temp(g.temp, unit)],
        ...(g.hotspot != null ? ([['Hotspot', temp(g.hotspot, unit)]] as Array<[string, string]>) : []),
        ['VRAM', g.vramTotalMb != null ? `${gb(g.vramUsedMb)} / ${gb(g.vramTotalMb)} GB` : NA],
        ['Power', withUnit(g.powerW, 'W')],
      ],
    });
  });
  const used = share(v.memory.usedMb, v.memory.totalMb);
  out.push({
    id: 'memory',
    title: 'Memory',
    sub: null,
    value: fmt(used),
    unit: '%',
    bar: used,
    level: null,
    lines: [['In use', v.memory.totalMb != null ? `${gb(v.memory.usedMb)} / ${gb(v.memory.totalMb)} GB` : NA]],
  });
  out.push({
    id: 'power',
    title: 'Power',
    sub: null,
    value: fmt(v.power.totalW),
    unit: 'W',
    bar: null,
    level: null,
    lines: [
      ['CPU', withUnit(v.power.cpuW, 'W')],
      ['GPU', withUnit(v.power.gpuW, 'W')],
    ],
  });
  out.push({
    id: 'network',
    title: 'Network',
    sub: 'Download',
    value: fmt(v.network.downMbps, 1),
    unit: 'Mbit/s',
    bar: null,
    level: null,
    lines: [
      ['Up', withUnit(v.network.upMbps, 'Mbit/s', 1)],
      ['Ping', withUnit(v.network.pingMs, 'ms')],
      ...(v.network.lossPct ? ([['Lost', withUnit(v.network.lossPct, '%')]] as Array<[string, string]>) : []),
    ],
  });
  return out;
}

export interface Row {
  name: string;
  value: string;
  level: Level;
  /** Extra text after the value (a drive's fill, a fan's duty). */
  note: string | null;
}

const BOARD: Record<string, readonly [number, number]> = {
  System: LIMITS.system,
  Chipset: LIMITS.chipset,
  VRM: LIMITS.vrm,
};

/** Board and memory temperatures. */
export function tempRows(v: RemoteView, unit: TempUnit): Row[] {
  return v.temps.map((t) => {
    const limits = t.name.startsWith('RAM') ? LIMITS.ram : BOARD[t.name];
    return { name: t.name, value: temp(t.value, unit), level: limits ? level(t.value, limits) : null, note: null };
  });
}

export function fanRows(v: RemoteView): Row[] {
  return v.fans.map((f) => ({
    name: f.pump ? `${f.name} (pump)` : f.name,
    value: withUnit(f.rpm, 'rpm'),
    level: null,
    note: f.pct != null ? `${fmt(f.pct)} %` : null,
  }));
}

export function driveRows(v: RemoteView, unit: TempUnit): Row[] {
  return v.drives.map((d) => {
    const heat = level(d.temp, d.kind === 'NVMe' ? LIMITS.nvme : LIMITS.sata);
    const health: Level = d.health === 'fail' ? 'hot' : d.health === 'warn' ? 'warn' : null;
    return {
      name: d.model,
      value: temp(d.temp, unit),
      level: health ?? heat,
      note: d.usedPct != null ? `${fmt(d.usedPct)} % full` : null,
    };
  });
}

export interface Notice {
  level: 'warn' | 'hot';
  text: string;
}

/** What heads the page: alerts that still hold, then Health's problems and warnings. */
export function notices(v: RemoteView): Notice[] {
  return [
    ...v.alerts.map((a) => ({ level: 'hot' as const, text: `Alert: ${a.name}` })),
    ...v.health.issues.map((i) => ({
      level: i.status === 'fail' ? ('hot' as const) : ('warn' as const),
      text: i.message ? `${i.part}: ${i.message}` : `${i.part}: ${i.status === 'fail' ? 'problem' : 'warning'}`,
    })),
  ];
}

/** The tab's title: the PC's name, with the number of alerts first. */
export function pageTitle(pc: string, v: RemoteView | null): string {
  const n = v ? v.alerts.length : 0;
  return `${n ? `(${n}) ` : ''}${pc ? `${pc} · Vyro` : 'Vyro'}`;
}

/** Seconds since the last reading, past which the page says it is waiting. */
export const STALE_SECS = 6;

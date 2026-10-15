// PC change history (P19): how each change reads, grouped by day, and which ones are new.

import { fmtDate } from './format';
import type { ChangeCategory, ComponentClass, HardwareChange } from './types';

export const CATEGORIES: { id: ChangeCategory; label: string; color: string }[] = [
  { id: 'hardware', label: 'Hardware', color: 'var(--accent-cpu)' },
  { id: 'firmware', label: 'Firmware', color: 'var(--accent-gpu)' },
  { id: 'driver', label: 'Drivers', color: 'var(--warn)' },
  { id: 'windows', label: 'Windows', color: 'var(--muted)' },
  { id: 'settings', label: 'Settings', color: 'color-mix(in srgb, var(--accent-cpu) 45%, var(--hot))' },
];

const CLASS: Record<ComponentClass, string> = {
  cpu: 'Processor',
  board: 'Motherboard',
  memory: 'Memory module',
  gpu: 'Graphics card',
  drive: 'Drive',
  removableDrive: 'USB drive',
  monitor: 'Monitor',
  network: 'Network adapter',
  windows: 'Windows',
  security: 'Firmware security',
};

/** A change as listed: identical changes found together on several modules are one entry. */
export interface Entry extends HardwareChange {
  /** How many devices it covers (their slots joined in `slot`). */
  count: number;
}

/** Folds identical changes noticed at the same moment (EXPO on two modules) into one entry. */
export function merge(list: HardwareChange[]): Entry[] {
  const out: Entry[] = [];
  // One check's changes share a timestamp and sit together in the list.
  let ts: number | null = null;
  let same = new Map<string, Entry>();
  for (const c of list) {
    if (c.ts !== ts) {
      ts = c.ts;
      same = new Map();
    }
    const key = c.slot ? JSON.stringify([c.kind, c.class, c.name, c.field, c.before, c.after]) : null;
    const into = key ? same.get(key) : undefined;
    if (into?.slot && c.slot) {
      into.slot = [...into.slot.split(', '), c.slot].sort().join(', ');
      into.count += 1;
      continue;
    }
    const entry = { ...c, count: 1 };
    out.push(entry);
    if (key) same.set(key, entry);
  }
  return out;
}

/** "Monitor" → "monitor", but "USB drive" stays. */
function lower(s: string): string {
  return /^.[A-Z]/.test(s) ? s : s.charAt(0).toLowerCase() + s.slice(1);
}

export interface Described {
  title: string;
  /** The device, where the title does not already say it. */
  device: string | null;
  /** Shown as `from → to`; `label` names what the two sides list. */
  from: string | null;
  to: string | null;
  label: string | null;
  note: string | null;
}

function changedTitle(c: HardwareChange): string {
  const field = c.field ?? '';
  switch (c.class) {
    case 'gpu':
      return field === 'Driver' ? 'Graphics driver' : field === 'VBIOS' ? 'Graphics card BIOS' : field;
    case 'board':
      return field;
    case 'memory':
      return `Memory ${field.toLowerCase()}`;
    case 'drive':
      return `Drive ${field.toLowerCase()}`;
    case 'windows':
      return field === 'Build' ? 'Windows update' : `Windows ${field.toLowerCase()}`;
    default:
      return field;
  }
}

/** What a change says, in words. */
export function describe(c: HardwareChange & { count?: number }): Described {
  const one = c.class ? CLASS[c.class] : '';
  const cls = (c.count ?? 1) > 1 ? `${one}s` : one;
  const at = c.slot ? `${c.slot}: ` : '';
  const none = { device: null, from: null, to: null, label: null, note: null };
  switch (c.kind) {
    case 'tracking':
      return { ...none, title: 'Tracking started', note: "Vyro started keeping track of this PC's hardware." };
    case 'added':
      return { ...none, title: `${cls} added`, device: `${at}${c.name}` };
    case 'removed':
      return { ...none, title: `${cls} removed`, device: `${at}${c.name}` };
    case 'replaced':
      return { ...none, title: `${cls} replaced`, device: c.slot, from: c.before, to: c.after };
    case 'changed': {
      const device = c.class === 'memory' ? `${at}${c.name}` : c.class === 'windows' || c.class === 'security' ? null : c.name;
      return { ...none, title: changedTitle(c), device, from: c.before, to: c.after };
    }
    case 'firstSeen':
      return {
        ...none,
        title: `New ${lower(cls)} connected`,
        device: c.name,
        from: c.before,
        to: c.after,
        label: `${cls}s before and now`,
      };
    case 'missing':
      return { ...none, title: `${cls} not seen for 30 days`, device: c.name, note: `Last seen ${fmtDate(c.lastSeen)}.` };
  }
}

function dayKey(ts: number): string {
  const d = new Date(ts * 1000);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

/** Changes by local day, newest first: "Today", "Yesterday", then dates. */
export function groupByDay<T extends HardwareChange>(list: T[], now = Date.now() / 1000): { key: string; label: string; items: T[] }[] {
  const today = dayKey(now);
  const yesterday = dayKey(now - 86_400);
  const out: { key: string; label: string; items: T[] }[] = [];
  for (const c of list) {
    const key = dayKey(c.ts);
    let day = out.at(-1);
    if (day?.key !== key) {
      day = { key, label: key === today ? 'Today' : key === yesterday ? 'Yesterday' : fmtDate(c.ts), items: [] };
      out.push(day);
    }
    day.items.push(c);
  }
  return out;
}

/** Local time of day, e.g. "14:05". */
export function timeOf(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

/** The id of the newest change worth a dot (the tracking entry is not one); 0 when none. */
export function newestId(list: HardwareChange[]): number {
  return list.reduce((m, c) => (c.kind === 'tracking' ? m : Math.max(m, c.id)), 0);
}

const SEEN_KEY = 'vyro.changesSeen';

/** The newest change id the Hardware tab has shown. */
export function loadSeen(): number {
  try {
    return Number(localStorage.getItem(SEEN_KEY)) || 0;
  } catch {
    return 0;
  }
}

export function saveSeen(id: number): void {
  try {
    localStorage.setItem(SEEN_KEY, String(id));
  } catch {
    /* storage unavailable: the dot comes back next start */
  }
}

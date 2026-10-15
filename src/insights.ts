// Insights (P16): how vital signs, evidence and causes read, and which findings are new.

import { NA } from './format';
import { tempSymbol, toUnit } from './units';
import type { Basis, CauseRole, Confidence, Evidence, Finding, Status, TempUnit, Vital, VitalKind } from './types';

export const VITALS: Record<VitalKind, { label: string; hint: string }> = {
  headroomCpu: { label: 'CPU headroom', hint: 'Degrees left before the CPU reaches 99 °C' },
  headroomGpu: { label: 'GPU headroom', hint: 'Degrees left before the GPU reaches 92 °C' },
  tempForLoad: { label: 'CPU heat for its load', hint: 'Against its usual temperature at the same load' },
  fanResponse: { label: 'CPU fan', hint: 'Its speed against its usual at the same CPU temperature' },
  efficiency: { label: 'Frames per watt', hint: 'Last game session, CPU and GPU power together' },
  clockStability: { label: 'Clock steadiness', hint: 'How much the CPU clock varies under full load; lower is steadier' },
};

export const BASIS: Record<Basis, string> = { measured: 'Measured', inferred: 'Inferred' };
export const BASIS_HINT: Record<Basis, string> = {
  measured: 'Read from the sensors or the logs',
  inferred: 'Worked out from measurements',
};
export const ROLE: Record<CauseRole, string> = {
  likely: 'Likely cause',
  possible: 'Possible cause',
  related: 'Around the same time',
};
export const CONFIDENCE: Record<Confidence, string> = {
  high: 'high confidence',
  medium: 'medium confidence',
  low: 'low confidence',
};

/** A temperature difference (headroom, "+9 °C for its load") in `unit`. */
export function fmtTempDelta(c: number, unit: TempUnit, sign = false): string {
  const v = unit === 'f' ? c * 1.8 : c;
  const n = Math.round(v);
  return `${sign && n > 0 ? '+' : ''}${n === 0 ? 0 : n} ${tempSymbol(unit)}`;
}

/** A vital sign's value and usual, in words. */
export function fmtVital(v: Vital, unit: TempUnit): { value: string; usual: string | null } {
  const f = (x: number | null): string | null => {
    if (x == null) return null;
    switch (v.kind) {
      case 'headroomCpu':
      case 'headroomGpu':
        return fmtTempDelta(x, unit);
      case 'tempForLoad':
        return fmtTempDelta(x, unit, true);
      case 'fanResponse':
        return `${Math.round(x).toLocaleString('en-US')} RPM`;
      case 'efficiency':
        return `${x.toFixed(2)} FPS/W`;
      case 'clockStability':
        return `${x.toFixed(1)} %`;
    }
  };
  // "+9 °C" is already against usual.
  return { value: f(v.value) ?? NA, usual: v.kind === 'tempForLoad' ? null : f(v.usual) };
}

/** Whether a vital sign is worth a look. */
export function vitalTone(v: Vital): Status {
  const x = v.value;
  if (x == null) return 'unknown';
  const u = v.usual;
  switch (v.kind) {
    case 'headroomCpu':
    case 'headroomGpu':
      return x < 5 ? 'warn' : 'ok';
    case 'tempForLoad':
      return x >= 6 ? 'warn' : 'ok';
    case 'fanResponse':
      return u != null && u >= 300 && x <= 0.6 * u ? 'warn' : 'ok';
    case 'efficiency':
      return u != null && x <= 0.9 * u ? 'warn' : 'ok';
    case 'clockStability':
      return u != null && x >= Math.max(3, 2 * u) ? 'warn' : 'ok';
  }
}

function amount(v: number, u: string, unit: TempUnit): string {
  switch (u) {
    case '°C':
      return `${Math.round(toUnit(v, unit))} ${tempSymbol(unit)}`;
    case '/min':
      return `${v.toFixed(1)} a minute`;
    case 'min':
      return `${Math.round(v)} min`;
    case 'FPS/W':
      return `${v.toFixed(2)} FPS/W`;
    default:
      return `${Math.round(v).toLocaleString('en-US')} ${u}`.trimEnd();
  }
}

/** An evidence item's value and what it usually is. */
export function fmtEvidence(e: Evidence, unit: TempUnit): { value: string; usual: string | null } {
  return { value: amount(e.value, e.unit, unit), usual: e.baseline == null ? null : amount(e.baseline, e.unit, unit) };
}

/** When a finding happened, in words. */
export function findingTime(f: Finding, fmt: (ts: number) => string): string {
  if (f.since != null) return `since ${fmt(f.since)}`;
  if (f.ts != null) return fmt(f.ts);
  return 'last 30 minutes';
}

/** How much history the baselines saw, when it is still thin (under three days). */
export function thinHistory(secs: number): string | null {
  if (secs >= 3 * 86_400) return null;
  const days = Math.floor(secs / 86_400);
  const have = days === 0 ? 'less than a day' : days === 1 ? '1 day' : `${days} days`;
  return `Insights improve over the first few days of history; Vyro has ${have} so far.`;
}

/** Keys of findings worth a look that have not been shown yet. */
export function unseen(attention: string[], seen: string[]): string[] {
  return attention.filter((k) => !seen.includes(k));
}

const SEEN_KEY = 'vyro.insightsSeen';
/** Keys remembered, newest last. */
const MAX_SEEN = 100;

export function loadSeenKeys(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(SEEN_KEY) ?? '[]');
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

/** Adds `keys` to the seen list (bounded) and returns it. */
export function markSeen(seen: string[], keys: string[]): string[] {
  const next = [...seen.filter((k) => !keys.includes(k)), ...keys].slice(-MAX_SEEN);
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify(next));
  } catch {
    /* storage unavailable: the dot comes back next start */
  }
  return next;
}

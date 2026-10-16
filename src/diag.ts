// Diagnostics (P21) in words. Mirrors `vyro_core::diag` (`fmt_citation`, `fmt_ago`, the labels),
// so the report on screen reads exactly like its exports.

import type { CheckId, CheckStatus, Citation, DiagReport, Origin, TempUnit } from './types';
import { cToF, tempsIn } from './units';

export const CHECKS: Record<CheckId, string> = {
  thermals: 'Thermals',
  clockStability: 'Clock stability',
  powerLimits: 'Power limits',
  throttling: 'Throttling',
  cooling: 'Fans and pump',
  memory: 'Memory',
  storage: 'Storage',
  network: 'Network',
  hardwareErrors: 'Hardware errors',
  changes: 'Changes from usual',
  drivers: 'Drivers',
  sensors: 'Sensors',
  service: 'Service',
};

export const STATUS: Record<CheckStatus, { label: string; tone: 'ok' | 'warn' | 'info' | 'muted' }> = {
  pass: { label: 'Pass', tone: 'ok' },
  attention: { label: 'Attention', tone: 'warn' },
  info: { label: 'Info', tone: 'info' },
  unavailable: { label: 'Unavailable', tone: 'muted' },
};

export const ORIGIN: Record<Origin, string> = {
  history: '7-day history',
  digests: 'minute digests',
  healthLog: 'Health log',
  smart: 'drive health (SMART)',
  eventLog: 'Windows System log',
  live: 'live reading',
  insights: 'Insights',
  inventory: 'inventory',
  service: 'Vyro',
};

/** "17 °C", "61%", "0.4%", "good"; temperatures in `unit` (a difference without the offset). */
export function fmtCitation(c: Citation, unit: TempUnit): string {
  if (c.text != null) return tempsIn(c.text, unit);
  if (c.value == null) return '—';
  let v = c.value;
  let u = c.unit;
  if (u === '°C' && unit === 'f') {
    v = c.difference ? v * 1.8 : cToF(v);
    u = '°F';
  }
  const n = v.toFixed(c.digits);
  if (!u) return n;
  return u === '%' ? `${n}%` : `${n} ${u}`;
}

/** "12 min before", "5 h before", "3 d before". */
export function fmtAgo(secs: number): string {
  const s = Math.max(0, secs);
  if (s < 3600) return `${Math.max(1, Math.floor(s / 60))} min before`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h before`;
  return `${Math.floor(s / 86_400)} d before`;
}

/** "7-day history" or "Health log, 3 d before". */
export function sourceText(c: Citation): string {
  return c.ago != null ? `${ORIGIN[c.source]}, ${fmtAgo(c.ago)}` : ORIGIN[c.source];
}

export function count(r: DiagReport, s: CheckStatus): number {
  return r.checks.filter((c) => c.status === s).length;
}

/** "2 attention · 8 pass · 2 info · 1 unavailable". */
export function countsText(r: DiagReport): string {
  return (['attention', 'pass', 'info', 'unavailable'] as const).map((s) => `${count(r, s)} ${s}`).join(' · ');
}

/** Attention first, then the rest in the report's order. */
export function ordered(r: DiagReport): DiagReport['checks'] {
  return [...r.checks.filter((c) => c.status === 'attention'), ...r.checks.filter((c) => c.status !== 'attention')];
}

/** "20261015-1024": the export's file name stamp, local time. */
export function stamp(ts: number): string {
  const d = new Date(ts * 1000);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}`;
}

export const FORMATS = [
  { id: 'pdf', label: 'PDF' },
  { id: 'html', label: 'HTML' },
  { id: 'md', label: 'Markdown' },
  { id: 'json', label: 'JSON' },
] as const;
export type ExportFormat = (typeof FORMATS)[number]['id'];

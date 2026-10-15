// Browser preview of alerts: the first-run rules plus three P18 rules (a compound rule with a
// fan boost and a record that holds right now, a fast rise, a temperature above its usual), and
// a log where a later alert about the CPU repeated it and was not shown.

import { DEFAULT_ACTIONS } from './alerts';
import type { AlertBrief, AlertFired, AlertRule } from './types';

const plain = { ...DEFAULT_ACTIONS };

export const SIM_RULES: AlertRule[] = [
  { id: 'cpu-temp', name: 'CPU temperature', enabled: true, kind: { type: 'threshold', sensor: 'cpu.temp.package', above: true, value: 95 }, forSecs: 30, cooldownSecs: 600, sound: true, actions: plain },
  { id: 'gpu-temp', name: 'GPU temperature', enabled: true, kind: { type: 'threshold', sensor: 'gpu.0.temp.edge', above: true, value: 90 }, forSecs: 30, cooldownSecs: 600, sound: true, actions: plain },
  { id: 'health-problems', name: 'Hardware problems', enabled: true, kind: { type: 'health', problems: true }, forSecs: 0, cooldownSecs: 600, sound: true, actions: plain },
  { id: 'health-warnings', name: 'Health warnings', enabled: false, kind: { type: 'health', problems: false }, forSecs: 0, cooldownSecs: 1800, sound: false, actions: plain },
  {
    id: 'hot-under-load',
    name: 'Hot under load',
    enabled: true,
    kind: {
      type: 'conditions',
      any: false,
      conditions: [
        { sensor: 'cpu.temp.package', test: { type: 'above', value: 80 }, hysteresis: 3 },
        { sensor: 'gpu.0.usage', test: { type: 'above', value: 90 }, hysteresis: 0 },
      ],
    },
    forSecs: 30,
    cooldownSecs: 600,
    sound: false,
    actions: { notify: true, tray: true, profile: null, fanBoost: 15, record: true },
  },
  {
    id: 'cpu-spike',
    name: 'CPU heating fast',
    enabled: true,
    kind: { type: 'conditions', any: false, conditions: [{ sensor: 'cpu.temp.package', test: { type: 'rises', by: 15, secs: 30 }, hysteresis: 0 }] },
    forSecs: 0,
    cooldownSecs: 900,
    sound: true,
    actions: { ...plain, record: true },
  },
  {
    id: 'hotter-than-usual',
    name: 'Hotter than usual',
    enabled: false,
    kind: { type: 'conditions', any: false, conditions: [{ sensor: 'cpu.temp.package', test: { type: 'aboveUsual', by: 8 }, hysteresis: 2 }] },
    forSecs: 300,
    cooldownSecs: 3600,
    sound: false,
    actions: { ...plain, notify: false, tray: true, profile: 'silent' },
  },
];

/** Rules of the preview that hold right now (and since when, seconds ago). */
const HOLDING: Record<string, number> = { 'hot-under-load': 190 };

/** What holds in the preview, from the rules as edited. */
export function simAlertState(rules: AlertRule[], now: number): AlertBrief {
  return {
    active: rules
      .filter((r) => r.enabled && r.id in HOLDING)
      .map((r) => ({
        ruleId: r.id,
        name: r.name,
        since: now - (HOLDING[r.id] ?? 0),
        tray: r.actions.tray,
        profile: r.actions.profile,
        fanBoost: r.actions.fanBoost,
      })),
  };
}

export function simAlertLog(now: number): AlertFired[] {
  const fired = (ago: number, ruleId: string, title: string, message: string, extra: Partial<AlertFired> = {}): AlertFired => ({
    ts: now - ago, ruleId, title, message, sound: false, notify: true, also: null, actions: [], diagnostic: null, ...extra,
  });
  return [
    fired(3890, 'cpu-temp', 'CPU temperature', 'CPU (Tdie) at 95.4 °C (above 95.0 °C for 30 s)', { sound: true }),
    fired(2410, 'cpu-spike', 'CPU heating fast', 'CPU (Tdie) rose from 52.0 °C to 69.5 °C within 30 s', {
      sound: true,
      diagnostic: {
        readings: [
          { id: 'cpu.temp.package', name: 'CPU (Tdie)', value: 69.5, unit: '°C' },
          { id: 'cpu.usage.total', name: 'CPU Total', value: 96.2, unit: '%' },
          { id: 'cpu.power.package', name: 'CPU Package Power', value: 118.4, unit: 'W' },
          { id: 'fan.0', name: 'CPU Fan', value: 1240, unit: 'RPM' },
        ],
        programs: [
          { name: 'HandBrake.exe', cpuPct: 88.1, gpuPct: null },
          { name: 'chrome.exe', cpuPct: 3.4, gpuPct: 1.2 },
          { name: 'explorer.exe', cpuPct: 0.6, gpuPct: null },
        ],
      },
    }),
    fired(190, 'hot-under-load', 'Hot under load', 'CPU (Tdie) at 83.1 °C (above 80.0 °C) and GPU #0 Usage at 97.0% (above 90.0%) for 30 s', {
      actions: ['Fans Vyro drives +15 % while it holds', 'Shown in the tray while it holds'],
      diagnostic: {
        readings: [
          { id: 'cpu.temp.package', name: 'CPU (Tdie)', value: 83.1, unit: '°C' },
          { id: 'gpu.0.usage', name: 'GPU #0 Usage', value: 97, unit: '%' },
          { id: 'gpu.0.temp.edge', name: 'GPU #0 Temperature', value: 74, unit: '°C' },
          { id: 'fan.0', name: 'CPU Fan', value: 1380, unit: 'RPM' },
        ],
        programs: [
          { name: 'Cyberpunk2077.exe', cpuPct: 31.5, gpuPct: 96.8 },
          { name: 'MsMpEng.exe', cpuPct: 12.2, gpuPct: null },
          { name: 'Discord.exe', cpuPct: 1.1, gpuPct: 0.4 },
        ],
      },
    }),
    fired(150, 'cpu-temp', 'CPU temperature', 'CPU (Tdie) at 95.2 °C (above 95.0 °C for 30 s)', { sound: true, notify: false, also: 'Hot under load' }),
  ];
}

/** Usual readings the preview has a day of history for. */
export function simUsual(sensor: string): number | null {
  if (sensor.includes('temp')) return 58.4;
  if (sensor.startsWith('fan.')) return 1180;
  if (sensor.endsWith('.usage') || sensor.endsWith('.total')) return 21;
  return null;
}

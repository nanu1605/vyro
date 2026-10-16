// Browser preview of Diagnostics: reports for the reference PC (Ryzen 7 9700X, RTX 5060 Ti),
// one of them with a fan that stopped two days ago.

import type { Citation, DiagCheck, DiagReport, DiagSummary, Origin } from './types';

const n = (label: string, value: number, unit: string, digits: number, source: Origin, extra: Partial<Citation> = {}): Citation => ({
  label, value, unit, difference: false, digits, text: null, source, metric: null, ago: null, ...extra,
});
const t = (label: string, text: string, source: Origin, extra: Partial<Citation> = {}): Citation => ({
  label, value: null, unit: '', difference: false, digits: 0, text, source, metric: null, ago: null, ...extra,
});
const c = (id: DiagCheck['id'], status: DiagCheck['status'], summary: string, citations: Citation[], advice: string | null = null): DiagCheck => ({
  id, status, summary, advice, citations,
});

function checks(fanStopped: boolean): DiagCheck[] {
  return [
    c('thermals', 'pass', 'Every device stays well below its limit under load.', [
      n('CPU median under load', 78, '°C', 0, 'history', { metric: 'cpu.temp.package' }),
      n('CPU headroom to its limit', 17, '°C', 0, 'history', { difference: true }),
      n('GPU #0 median under load', 62, '°C', 0, 'history', { metric: 'gpu.0.temp.edge' }),
      n('GPU #0 headroom to its limit', 23, '°C', 0, 'history', { difference: true }),
    ]),
    c('clockStability', 'pass', 'Clocks hold steady through long full load.', [
      n('CPU clock sag over long full load', 1.4, '%', 1, 'history', { metric: 'cpu.clock.effective' }),
      n('GPU #0 clock sag over long full load', 0.8, '%', 1, 'history', { metric: 'gpu.0.clock.core' }),
    ]),
    c('powerLimits', 'info', 'How much of the loaded time power limits held the clocks. This is how CPUs and GPUs normally keep to their power budget, not a fault.', [
      n('CPU at its power limit (PPT), share of loaded time', 42, '%', 1, 'digests'),
      n('GPU at its power target, share of loaded time', 12, '%', 1, 'digests'),
    ]),
    c('throttling', 'pass', 'The hardware rarely or never limited itself for heat or current.', [
      n('Loaded time limited by heat or current', 0, '%', 1, 'digests'),
    ]),
    fanStopped
      ? c('cooling', 'attention', 'Health logged a fan or pump problem in the past week.', [
          n('CPU fan now', 1104, 'RPM', 0, 'live'),
          n('Rear fan now', 830, 'RPM', 0, 'live'),
          n('Pump now', 2650, 'RPM', 0, 'live'),
          t('Health: Fans', 'Rear fan stopped (0 RPM)', 'healthLog', { ago: 2 * 86_400 + 3 * 3600 }),
          n('CPU against its usual for the load', 1.0, '°C', 1, 'insights', { difference: true, metric: 'cpu.temp.package' }),
          n('CPU fan against its usual at that temperature', 98, '%', 0, 'insights'),
        ], 'Check the fan and pump cables and that nothing blocks them; if a curve is set, check it in Cooling.')
      : c('cooling', 'pass', 'Fans and pump spin, and cooling keeps up as usual.', [
          n('CPU fan now', 1104, 'RPM', 0, 'live'),
          n('Rear fan now', 830, 'RPM', 0, 'live'),
          n('Pump now', 2650, 'RPM', 0, 'live'),
        ]),
    c('memory', 'pass', 'There is memory to spare most of the time.', [
      n('RAM in use, 95th percentile', 61, '%', 0, 'history', { metric: 'memory.used' }),
      n('RAM in use now', 38, '%', 0, 'live', { metric: 'memory.used' }),
    ]),
    c('storage', 'pass', 'Drives report good health, with life, room and temperatures to spare.', [
      t('Samsung SSD 990 PRO 2TB: health', 'good', 'smart', { metric: 'storage.disk0.health' }),
      n('Samsung SSD 990 PRO 2TB: life left', 96, '%', 0, 'smart'),
      n('Samsung SSD 990 PRO 2TB: temperature now', 41, '°C', 0, 'live', { metric: 'storage.disk0.temp' }),
      n('Free on the system drive', 41, '%', 0, 'live'),
    ]),
    c('network', 'pass', 'The connection answers quickly and reliably.', [
      n('Ping, 95th percentile', 18, 'ms', 0, 'history', { metric: 'network.ping' }),
      n('Lost replies', 0.1, '%', 1, 'history', { metric: 'network.loss' }),
    ]),
    c('hardwareErrors', 'pass', 'No crashes and no serious hardware errors in 30 days.', [
      n('Unexpected shutdowns (Kernel-Power 41), 30 days', 0, '', 0, 'eventLog'),
      n('Corrected hardware errors (WHEA), 30 days', 0, '', 0, 'eventLog'),
      n('Fatal hardware errors (WHEA), 30 days', 0, '', 0, 'eventLog'),
    ]),
    c('changes', 'pass', 'Nothing has changed from usual.', []),
    c('drivers', 'info', 'The versions in use, for comparing with a later report or a support request.', [
      t('NVIDIA GeForce RTX 5060 Ti driver', '576.02 (2025-09-30)', 'inventory'),
      t('AMD Radeon(TM) Graphics driver', '32.0.21013.1000 (2025-06-12)', 'inventory'),
      t('Windows', 'Windows 11 Pro 24H2 26100.4652', 'inventory'),
    ]),
    c('sensors', 'pass', 'The key sensors report.', [
      t('PawnIO', 'ready', 'live'),
      t('CPU temperature', 'read', 'live'),
      t('CPU power', 'read', 'live'),
      n('Board fans and pumps read', 3, '', 0, 'live'),
      n('Voltages read', 7, '', 0, 'live'),
      n('RAM modules with a temperature', 2, '', 0, 'live'),
      n('Drives with health data', 2, '', 0, 'smart'),
    ]),
    c('service', 'pass', 'The app reaches the engine through the Vyro service.', [
      t('Vyro app', '0.4.0', 'service'),
      t('Vyro service', '0.4.0', 'service'),
    ]),
  ];
}

const PC = [
  { label: 'CPU', value: 'AMD Ryzen 7 9700X 8-Core Processor' },
  { label: 'GPU', value: 'NVIDIA GeForce RTX 5060 Ti' },
  { label: 'GPU', value: 'AMD Radeon(TM) Graphics' },
  { label: 'Board', value: 'Micro-Star International Co., Ltd. B650M GAMING PLUS WIFI (MS-7E24)' },
  { label: 'BIOS', value: '1.A0 05/12/2025' },
  { label: 'RAM', value: '32 GB at 6000 MT/s' },
  { label: 'Windows', value: 'Windows 11 Pro 24H2 26100.4652' },
];

let reports: DiagReport[] | null = null;
const all = () =>
  (reports ??= [
    { id: 1, ts: Math.floor(Date.now() / 1000) - 5 * 86_400, version: 1, appVersion: '0.4.0', engine: 'Vyro service 0.4.0', pc: PC, checks: checks(false) },
  ]);

const summary = (r: DiagReport): DiagSummary => ({
  id: r.id,
  ts: r.ts,
  version: r.version,
  attention: r.checks.filter((x) => x.status === 'attention').length,
  unavailable: r.checks.filter((x) => x.status === 'unavailable').length,
});

export async function simRunDiagnostics(): Promise<DiagReport> {
  await new Promise((r) => setTimeout(r, 700));
  const id = Math.max(0, ...all().map((r) => r.id)) + 1;
  const r: DiagReport = { id, ts: Math.floor(Date.now() / 1000), version: 1, appVersion: '0.4.0', engine: 'Vyro service 0.4.0', pc: PC, checks: checks(true) };
  all().push(r);
  return r;
}

export const simDiagnosticRuns = (): DiagSummary[] => [...all()].sort((a, b) => b.id - a.id).map(summary);
export const simDiagnosticRun = (id: number): DiagReport | null => all().find((r) => r.id === id) ?? null;
export function simDeleteDiagnostic(id: number): boolean {
  const before = all().length;
  reports = all().filter((r) => r.id !== id);
  return reports.length < before;
}

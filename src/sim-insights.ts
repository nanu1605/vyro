// Browser preview of Insights: a PC running hotter since a BIOS update reset its fan curve, a
// game slowed down after a driver and a Windows update, and an antivirus scan during play.

import type { Cause, Evidence, Finding, Insights, Vital } from './types';

const NOW = Math.floor(Date.now() / 1000);
const H = 3600;
const D = 86_400;
const GAME = 'Cyberpunk2077.exe';
const BIOS_AT = NOW - 3 * D - 2 * H;
/** The Cyberpunk session of `sim-game.ts`. */
const SESSION = NOW - 7_500;

const ev = (label: string, metric: string | null, from: number, to: number, value: number, baseline: number | null, unit: string): Evidence => ({
  label, metric, from, to, value, baseline, unit,
});

const bios: Cause = {
  role: 'likely', basis: 'inferred', confidence: 'medium', kind: 'change',
  text: 'BIOS: 7E24v1A (07/12/2024) → 7E24v1B (09/30/2024)', ts: BIOS_AT + 90, changeId: 7, evidence: [], because: [],
};
const fanSlower: Cause = {
  role: 'likely', basis: 'measured', confidence: 'high', kind: 'measured', text: 'CPU Fan slower than usual', ts: BIOS_AT,
  changeId: null, evidence: [ev('CPU Fan speed', 'fan.0', NOW - 1800, NOW, 820, 1310, 'RPM')], because: [bios],
};
const hotterEvidence = [
  ev('CPU temperature', 'cpu.temp.package', NOW - 1800, NOW, 71, 62, '°C'),
  ev('CPU load', 'cpu.usage.total', NOW - 1800, NOW, 22, null, '%'),
];

const FINDINGS: Finding[] = [
  {
    key: `hotter:cpu:${Math.floor(BIOS_AT / D)}`, kind: 'hotter', status: 'warn', basis: 'measured', confidence: 'high',
    title: 'CPU running hotter than usual',
    detail: 'Over the last 30 minutes: 71 °C at 22 % load. At that load it ran near 62 °C before this began.',
    ts: null, since: BIOS_AT, evidence: hotterEvidence, causes: [fanSlower],
    related: [{ role: 'related', basis: 'inferred', confidence: 'low', kind: 'change', text: 'New monitor connected: DELL U2724D', ts: BIOS_AT + 300, changeId: 8, evidence: [], because: [] }],
  },
  {
    key: `throttling:cpu:${Math.floor(NOW / D)}`, kind: 'throttling', status: 'warn', basis: 'inferred', confidence: 'medium',
    title: 'CPU slowed down by heat',
    detail: '3 times in the last 24 hours it ran at full load at 96 °C and only 4300 MHz, below its usual 5150 MHz at full load. It does not report heat limiting itself, so this is worked out from its clocks.',
    ts: NOW - 5 * H, since: null,
    evidence: [
      ev('Clock at full load', 'cpu.clock.effective', NOW - 5 * H, NOW - 5 * H + 144, 4300, 5150, 'MHz'),
      ev('Temperature', 'cpu.temp.package', NOW - 5 * H, NOW - 5 * H + 144, 96, null, '°C'),
    ],
    causes: [{ role: 'likely', basis: 'measured', confidence: 'high', kind: 'measured', text: 'CPU running hotter than usual', ts: BIOS_AT, changeId: null, evidence: hotterEvidence, because: [fanSlower] }],
    related: [],
  },
  {
    key: `background:3:MsMpEng.exe`, kind: 'backgroundLoad', status: 'warn', basis: 'measured', confidence: 'high',
    title: `Frame drops in ${GAME} while MsMpEng.exe was busy`,
    detail: 'MsMpEng.exe used about 24 % of the CPU for 9 of the 121 minutes played. The frame rate dropped 17 times in those minutes, against 9 in the other 112.',
    ts: SESSION + 1200, since: null,
    evidence: [
      ev('MsMpEng.exe CPU use', null, SESSION + 1200, SESSION + 1740, 24, null, '%'),
      ev('Frame drops per minute (busy vs other minutes)', 'game.fps', SESSION, SESSION + 7260, 1.9, 0.08, '/min'),
    ],
    causes: [{ role: 'likely', basis: 'inferred', confidence: 'medium', kind: 'program', text: 'MsMpEng.exe used about 24 % of the CPU during play', ts: SESSION + 1200, changeId: null, evidence: [], because: [] }],
    related: [],
  },
  {
    key: `slower:${GAME}:2`, kind: 'gameSlower', status: 'warn', basis: 'measured', confidence: 'high',
    title: `${GAME} running slower than usual`,
    detail: 'The last 2 sessions averaged 114 FPS; earlier sessions averaged 131 FPS (median of 6). In-game settings, the map or the scene can also explain this.',
    ts: SESSION, since: NOW - 2 * D,
    evidence: [ev('Average FPS, last session', 'game.fps', SESSION, SESSION + 7260, 112.9, 131, 'FPS')],
    causes: [
      { role: 'possible', basis: 'inferred', confidence: 'low', kind: 'change', text: 'Graphics driver: 32.0.15.7640 (576.40) → 32.0.15.7700 (577.00)', ts: NOW - 2 * D - 4 * H, changeId: 9, evidence: [], because: [] },
      { role: 'possible', basis: 'inferred', confidence: 'low', kind: 'change', text: 'Windows update: 26100.4061 (24H2) → 26100.4202 (24H2)', ts: NOW - 2 * D - 5 * H, changeId: 10, evidence: [], because: [] },
    ],
    related: [{ role: 'related', basis: 'inferred', confidence: 'low', kind: 'change', text: 'New USB drive connected: SanDisk Ultra (57.3 GB, USB)', ts: NOW - 2 * D - 3 * H, changeId: 11, evidence: [], because: [] }],
  },
  {
    key: `bound:3`, kind: 'bottleneck', status: 'ok', basis: 'measured', confidence: 'high',
    title: `GPU set the frame rate in ${GAME}`,
    detail: 'In the last session (121 min) the GPU was the limit 92 % of the time: it was busy for nearly all of every frame. Lower graphics settings or a faster GPU raise the frame rate.',
    ts: SESSION, since: null,
    evidence: [
      ev('GPU load (session average)', 'gpu.0.usage', SESSION, SESSION + 7260, 97.1, null, '%'),
      ev('CPU load (session average)', 'cpu.usage.total', SESSION, SESSION + 7260, 34, null, '%'),
    ],
    causes: [], related: [],
  },
  {
    key: `power:${NOW - 90 * 60}`, kind: 'powerChange', status: 'ok', basis: 'measured', confidence: 'high',
    title: 'Power rose to 156 W',
    detail: 'CPU + GPU power went from 48 W to 156 W: CPU +104 W (load 4 % → 96 %), GPU #0 +4 W (load 2 % → 3 %).',
    ts: NOW - 90 * 60, since: null,
    evidence: [ev('CPU + GPU power', 'cpu.power.package', NOW - 90 * 60 - 576, NOW - 90 * 60 + 576, 156, 48, 'W')],
    causes: [{ role: 'likely', basis: 'inferred', confidence: 'medium', kind: 'program', text: 'HandBrake.exe: about 0 % → 91 % CPU', ts: NOW - 90 * 60, changeId: null, evidence: [], because: [] }],
    related: [],
  },
];

const VITALS: Vital[] = [
  { kind: 'headroomCpu', value: 28, usual: 37, unit: '°C', basis: 'measured', detail: 'at 22 % load', metric: 'cpu.temp.package' },
  { kind: 'headroomGpu', value: 44, usual: 45, unit: '°C', basis: 'measured', detail: 'at 3 % load', metric: 'gpu.0.temp.edge' },
  { kind: 'tempForLoad', value: 9, usual: 0, unit: '°C', basis: 'measured', detail: 'at 22 % load', metric: 'cpu.temp.package' },
  { kind: 'fanResponse', value: 820, usual: 1310, unit: 'RPM', basis: 'measured', detail: 'CPU Fan at 71 °C', metric: 'fan.0' },
  { kind: 'efficiency', value: 0.27, usual: 0.31, unit: 'FPS/W', basis: 'measured', detail: GAME, metric: null },
  { kind: 'clockStability', value: 1.6, usual: 1.1, unit: '%', basis: 'measured', detail: 'under full load, last 24 h', metric: 'cpu.clock.effective' },
];

/** Keys of the preview's findings worth a look (the snapshot's brief). */
export const SIM_ATTENTION = FINDINGS.filter((f) => f.status === 'warn').map((f) => f.key);

export function simInsights(): Insights {
  return { ts: Math.floor(Date.now() / 1000), vitals: VITALS, findings: FINDINGS, historySecs: 7 * D };
}

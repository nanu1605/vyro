// Browser preview of the PC change history: a month of a PC's life, newest first.

import type { HardwareChange } from './types';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

function change(id: number, ago: number, c: Partial<HardwareChange> & Pick<HardwareChange, 'category' | 'kind'>): HardwareChange {
  return { id, ts: NOW - ago, class: null, slot: null, name: '', field: null, before: null, after: null, lastSeen: null, ...c };
}

const GPU = 'NVIDIA GeForce RTX 5060 Ti';
const BOARD = 'Micro-Star International Co., Ltd. B650M GAMING PLUS WIFI (MS-7E24)';
const KIT = 'G.Skill F5-6000J3038F16G, 16 GB DDR5';

export const SAMPLE_CHANGES: HardwareChange[] = [
  change(14, 2 * 3600, { category: 'windows', kind: 'changed', class: 'windows', name: 'Windows', field: 'Build', before: '26100.4061 (24H2)', after: '26100.4202 (24H2)' }),
  change(13, DAY + 5 * 3600, { category: 'driver', kind: 'changed', class: 'gpu', name: GPU, field: 'Driver', before: '32.0.15.7216 (572.16)', after: '32.0.15.7640 (576.40)' }),
  change(12, 3 * DAY, { category: 'hardware', kind: 'firstSeen', class: 'monitor', name: 'DELL U2724D', before: 'LG ULTRAGEAR', after: 'LG ULTRAGEAR; DELL U2724D' }),
  change(11, 9 * DAY, { category: 'settings', kind: 'changed', class: 'memory', slot: 'DIMM_A2', name: KIT, field: 'Speed', before: '4800 MT/s', after: '6000 MT/s' }),
  change(10, 9 * DAY, { category: 'settings', kind: 'changed', class: 'memory', slot: 'DIMM_A2', name: KIT, field: 'Voltage', before: '1.10 V', after: '1.35 V' }),
  change(9, 9 * DAY, { category: 'settings', kind: 'changed', class: 'memory', slot: 'DIMM_B2', name: KIT, field: 'Speed', before: '4800 MT/s', after: '6000 MT/s' }),
  change(8, 9 * DAY, { category: 'settings', kind: 'changed', class: 'memory', slot: 'DIMM_B2', name: KIT, field: 'Voltage', before: '1.10 V', after: '1.35 V' }),
  change(7, 9 * DAY + 60, { category: 'firmware', kind: 'changed', class: 'board', name: BOARD, field: 'BIOS', before: '1.A0 (07/12/2024)', after: '1.B0 (09/30/2024)' }),
  change(6, 16 * DAY, { category: 'hardware', kind: 'replaced', class: 'memory', slot: 'DIMM_A2', name: KIT, before: 'Kingston KF560C30-16, 16 GB DDR5', after: KIT }),
  change(5, 16 * DAY, { category: 'hardware', kind: 'replaced', class: 'memory', slot: 'DIMM_B2', name: KIT, before: 'Kingston KF560C30-16, 16 GB DDR5', after: KIT }),
  change(4, 18 * DAY, { category: 'hardware', kind: 'firstSeen', class: 'removableDrive', name: 'SanDisk Ultra (57.3 GB, USB)', after: 'SanDisk Ultra (57.3 GB, USB)' }),
  change(3, 21 * DAY, { category: 'settings', kind: 'changed', class: 'security', name: 'Firmware security', field: 'Secure Boot', before: 'Off', after: 'On' }),
  change(2, 25 * DAY, { category: 'driver', kind: 'changed', class: 'gpu', name: GPU, field: 'Driver', before: '32.0.15.6636 (566.36)', after: '32.0.15.7216 (572.16)' }),
  change(1, 30 * DAY, { category: 'hardware', kind: 'tracking' }),
];

// My PC (P23): a stylized ATX tower with its side off, laid out from what Vyro knows about this PC,
// and the thermal map's colours. Pure, so it is unit-tested; the 3-D scene and the 2-D plan both
// draw these parts.
//
// Units are millimetres. x runs from the back of the case (0) to the front (460), y from the
// floor (0) up, z from the board (0) out towards the open side.

import { detectCard, type DetectedCard, type GpuShape } from './gpuspecs';
import type { DriveInfo, GpuSnapshot, HardwareId, SensorSnapshot } from './types';

export type PartKind = 'board' | 'cpu' | 'cooler' | 'aio' | 'radiator' | 'vrm' | 'chipset' | 'ram' | 'slot' | 'gpu' | 'm2' | 'drive' | 'fan' | 'psu';
export type HeatGroup = 'cpu' | 'gpu' | 'board' | 'storage';
export type ThermalMode = 'full' | HeatGroup;
export type Vec3 = [number, number, number];

export interface Part {
  /** Stable within a session: "cpu", "gpu.0", "ram.1", "disk0", "fan.2". */
  id: string;
  kind: PartKind;
  /** Short name for its label. */
  label: string;
  /** Box centre and size. */
  pos: Vec3;
  size: Vec3;
  /** Which thermal mode colours it; null: never coloured (fans, PSU). */
  group: HeatGroup | null;
  /** Where its label points. */
  anchor: Vec3;
  /** Draw a label (small parts carry theirs on the part they belong to). */
  labelled: boolean;
  /** The tab with its details. */
  tab: 'hardware' | 'cooling' | 'storage' | null;
  /** A fan's axis (its face points along it) and which reading spins it. */
  fan?: { axis: 'x' | 'y' | 'z'; rpm: (s: SensorSnapshot) => number | null };
  gpu?: { index: number; card: DetectedCard; shape: GpuShape; key: string };
  /** A RAM stick's place in the hardware list, and its slot name. */
  dimm?: number;
  slot?: string;
}

export const CASE: Vec3 = [460, 460, 220];
const BOARD = { x0: 20, x1: 264, y0: 130, y1: 435 };
const CPU_AT: Vec3 = [150, 345, 0];
const SLOT_Y = 270;
const SLOT_MM = 20.32;

export interface PcInput {
  hw: HardwareId | null;
  snap: SensorSnapshot | null;
  /** Drive details (SSD or HDD); optional. */
  drives?: DriveInfo[];
  /** The owner's corrections by card key. */
  shapes?: Record<string, GpuShape>;
  cardKey: (g: { vendorId: number; deviceId: number; subsysId: number }) => string;
}

const box = (part: Omit<Part, 'anchor' | 'labelled'> & { anchor?: Vec3; labelled?: boolean }): Part => ({
  anchor: part.anchor ?? [part.pos[0], part.pos[1] + part.size[1] / 2, part.pos[2] + part.size[2] / 2],
  labelled: part.labelled ?? true,
  ...part,
});

/** The discrete GPUs: hardware identity matched to the live reading by name. */
export function discreteGpus(hw: HardwareId | null, snap: SensorSnapshot | null): Array<{ id: HardwareId['gpus'][number]; live: number | null }> {
  const live = snap?.gpu ?? [];
  return (hw?.gpus ?? [])
    .map((id) => {
      const i = live.findIndex((g) => g.name === id.name);
      return { id, live: i < 0 ? null : i };
    })
    .filter(({ id, live: i }) => {
      const g = i == null ? null : live[i];
      if (g?.integrated != null) return !g.integrated;
      return (id.vramMB ?? 0) >= 2048;
    });
}

/** A RAM slot's place, 0..3 (A1 A2 B1 B2), from its name; null when the name does not say. */
export function slotIndex(name: string): number | null {
  const n = name.toUpperCase();
  const plain = /([AB])[-_ ]?([12])\b/.exec(n); // DIMM_A2, A1
  if (plain) return (plain[1] === 'A' ? 0 : 2) + Number(plain[2]) - 1;
  const channel = /CHANNEL\s*([AB]).*?DIMM\s*([01])/.exec(n); // ChannelA-DIMM1
  if (channel) return (channel[1] === 'A' ? 0 : 2) + Number(channel[2]);
  return null;
}

/** The parts of this PC, back to front. */
export function buildPc(input: PcInput): Part[] {
  const { hw, snap } = input;
  const parts: Part[] = [];
  const fans = snap?.fans ?? [];
  const pump = fans.findIndex((f) => f.kind === 'pump');
  const cpuFan = fans.findIndex((f) => f.kind === 'fan' && /cpu/i.test(f.name));
  const rpmOf = (i: number) => (s: SensorSnapshot) => s.fans[i]?.rpm ?? null;

  parts.push(
    box({
      id: 'board',
      kind: 'board',
      label: hw?.board.product ?? 'Motherboard',
      pos: [(BOARD.x0 + BOARD.x1) / 2, (BOARD.y0 + BOARD.y1) / 2, 1.5],
      size: [BOARD.x1 - BOARD.x0, BOARD.y1 - BOARD.y0, 3],
      group: 'board',
      anchor: [BOARD.x0 + 20, BOARD.y0 + 20, 3],
      tab: 'hardware',
    }),
    box({ id: 'vrm', kind: 'vrm', label: 'VRM', pos: [92, 375, 14], size: [18, 90, 24], group: 'board', tab: 'hardware', anchor: [92, 360, 26] }),
    box({ id: 'vrm.top', kind: 'vrm', label: 'VRM', pos: [150, 410, 14], size: [90, 18, 24], group: 'board', tab: 'hardware', labelled: false }),
    box({ id: 'chipset', kind: 'chipset', label: 'Chipset', pos: [205, 175, 8], size: [42, 42, 10], group: 'board', tab: 'hardware' }),
    box({ id: 'cpu', kind: 'cpu', label: 'CPU', pos: [CPU_AT[0], CPU_AT[1], 5], size: [45, 45, 6], group: 'cpu', tab: 'hardware', labelled: false }),
  );

  // Cooler: an AIO block and a top radiator when a pump is reported, else a tower.
  if (pump >= 0) {
    parts.push(
      box({ id: 'aio', kind: 'aio', label: 'CPU', pos: [CPU_AT[0], CPU_AT[1], 24], size: [62, 62, 34], group: 'cpu', tab: 'cooling', fan: { axis: 'z', rpm: rpmOf(pump) } }),
      box({ id: 'radiator', kind: 'radiator', label: 'Radiator', pos: [235, 448, 110], size: [280, 24, 122], group: null, tab: 'cooling', labelled: false }),
    );
    for (const [i, x] of [175, 295].entries())
      parts.push(
        box({
          id: `radfan.${i}`,
          kind: 'fan',
          label: 'Radiator fan',
          pos: [x, 422, 110],
          size: [120, 25, 120],
          group: null,
          tab: 'cooling',
          labelled: false,
          fan: { axis: 'y', rpm: rpmOf(cpuFan >= 0 ? cpuFan : pump) },
        }),
      );
  } else {
    parts.push(
      box({ id: 'cooler', kind: 'cooler', label: 'CPU', pos: [CPU_AT[0], CPU_AT[1], 85], size: [52, 125, 150], group: 'cpu', tab: 'hardware', anchor: [CPU_AT[0], CPU_AT[1] + 63, 160] }),
      box({
        id: 'cooler.fan',
        kind: 'fan',
        label: 'CPU fan',
        pos: [CPU_AT[0] + 39, CPU_AT[1], 85],
        size: [25, 120, 120],
        group: null,
        tab: 'cooling',
        labelled: false,
        fan: { axis: 'x', rpm: rpmOf(cpuFan >= 0 ? cpuFan : fans.findIndex((f) => f.kind === 'fan')) },
      }),
    );
  }

  // RAM: four slots right of the CPU; sticks where DIMMs are fitted.
  const total = Math.max(2, Math.min(4, hw?.memory.slotsTotal ?? 4));
  const xs = [222, 231, 240, 249].slice(0, total);
  const dimms = hw?.memory.dimms ?? [];
  const used = new Set<number>();
  for (const [n, d] of dimms.entries()) {
    let at = slotIndex(d.slot) ?? -1;
    if (at < 0 || at >= total || used.has(at)) at = [1, 3, 0, 2].find((i) => i < total && !used.has(i)) ?? n;
    used.add(at);
    parts.push(
      box({
        id: `ram.${n}`,
        kind: 'ram',
        label: 'RAM',
        dimm: n,
        slot: d.slot,
        pos: [xs[at] ?? 222, CPU_AT[1], 24],
        size: [7, 133, 42],
        group: 'board',
        tab: 'hardware',
        labelled: n === 0,
        anchor: [xs[at] ?? 222, CPU_AT[1] + 66, 45],
      }),
    );
  }
  for (const [i, x] of xs.entries())
    if (!used.has(i)) parts.push(box({ id: `slot.${i}`, kind: 'slot', label: 'Empty slot', pos: [x, CPU_AT[1], 4], size: [6, 140, 6], group: null, tab: null, labelled: false }));

  // GPUs, top slot down.
  let y = SLOT_Y;
  for (const [n, g] of discreteGpus(hw, snap).entries()) {
    const key = input.cardKey(g.id);
    const card = detectCard(g.id);
    const shape = input.shapes?.[key] ?? card.shape;
    const thick = shape.slots * SLOT_MM;
    const index = g.live ?? n;
    const len = shape.lengthMm;
    parts.push(
      box({
        id: `gpu.${index}`,
        kind: 'gpu',
        label: (snap?.gpu.length ?? 0) > 1 || n > 0 ? `GPU ${index + 1}` : 'GPU',
        pos: [BOARD.x0 + len / 2, y - thick / 2, 70],
        size: [len, thick, 126],
        group: 'gpu',
        tab: 'hardware',
        anchor: [BOARD.x0 + len * 0.6, y, 133],
        gpu: { index, card, shape, key },
        fan: { axis: 'y', rpm: (s) => s.gpu[index]?.fanRpm ?? null },
      }),
    );
    y -= thick + SLOT_MM;
  }

  // Drives: M.2 on the board, SATA in the cage under the shroud.
  const storage = snap?.storage ?? [];
  const m2Y = [310, 205, 160];
  let m2 = 0;
  let sata = 0;
  for (const d of storage) {
    if (d.type === 'NVMe' && m2 < m2Y.length) {
      parts.push(box({ id: d.id, kind: 'm2', label: d.model, pos: [125, m2Y[m2]!, 6], size: [80, 22, 6], group: 'storage', tab: 'storage', labelled: m2 === 0 || storage.length <= 3 }));
      m2++;
    } else if (d.type === 'SATA' && sata < 3) {
      const ssd = input.drives?.find((x) => x.id === d.id)?.ssd ?? false;
      const size: Vec3 = ssd ? [100, 7, 70] : [147, 26, 101];
      parts.push(box({ id: d.id, kind: 'drive', label: d.model, pos: [360, 22 + sata * 30, 110], size, group: 'storage', tab: 'storage' }));
      sata++;
    }
  }

  // Case fans: front, then rear, then top.
  const spots: Array<{ pos: Vec3; axis: 'x' | 'y' }> = [
    { pos: [446, 390, 110], axis: 'x' },
    { pos: [446, 265, 110], axis: 'x' },
    { pos: [14, 360, 110], axis: 'x' },
    { pos: [446, 140, 110], axis: 'x' },
  ];
  let spot = 0;
  for (const [i, f] of fans.entries()) {
    if (i === pump || i === cpuFan || f.kind !== 'fan' || spot >= spots.length) continue;
    const s = spots[spot++]!;
    parts.push(
      box({
        id: `fan.${i}`,
        kind: 'fan',
        label: f.name,
        pos: s.pos,
        size: s.axis === 'x' ? [25, 120, 120] : [120, 25, 120],
        group: null,
        tab: 'cooling',
        fan: { axis: s.axis, rpm: rpmOf(i) },
      }),
    );
  }

  parts.push(box({ id: 'psu', kind: 'psu', label: 'PSU', pos: [100, 50, 110], size: [150, 86, 140], group: null, tab: null }));
  return parts;
}

// ---------------------------------------------------------------------------------------------
// Readings and the thermal map
// ---------------------------------------------------------------------------------------------

/** Health's limits, °C: [warning, problem]. */
export const LIMITS = {
  cpu: [90, 99],
  gpu: [85, 92],
  hotspot: [100, 108],
  ram: [75, 85],
  nvme: [75, 85],
  sata: [60, 70],
  vrm: [95, 105],
  chipset: [80, 90],
  system: [55, 65],
} as const satisfies Record<string, readonly [number, number]>;

export interface PartTemp {
  /** °C. */
  temp: number;
  limits: readonly [number, number];
  /** What was measured, e.g. "package", "hotspot". */
  what: string;
}

const gpuOf = (p: Part, s: SensorSnapshot): GpuSnapshot | undefined => (p.gpu ? s.gpu[p.gpu.index] : undefined);

/** A part's temperature with the limits it is judged by; null when it has none. */
export function partTemp(p: Part, s: SensorSnapshot | null): PartTemp | null {
  if (!s) return null;
  const t = (temp: number | null | undefined, limits: readonly [number, number], what: string): PartTemp | null =>
    temp == null ? null : { temp, limits, what };
  switch (p.kind) {
    case 'cpu':
    case 'cooler':
    case 'aio':
      return t(s.cpu.tempPackage, LIMITS.cpu, 'package');
    case 'gpu': {
      const g = gpuOf(p, s);
      return t(g?.tempEdge, LIMITS.gpu, 'core');
    }
    case 'ram': {
      // The live list names slots as the hardware list does; by position otherwise.
      const want = p.dimm ?? -1;
      const d = s.memory.dimms.find((x) => x.slot === p.slot) ?? s.memory.dimms[want];
      return t(d?.tempC, LIMITS.ram, 'module');
    }
    case 'm2':
    case 'drive': {
      const d = s.storage.find((x) => x.id === p.id);
      return t(d?.tempC, d?.type === 'NVMe' ? LIMITS.nvme : LIMITS.sata, 'drive');
    }
    case 'vrm':
      return t(s.board.tempVrmMos ?? s.board.tempVrm, LIMITS.vrm, 'VRM');
    case 'chipset':
      return t(s.board.tempChipset ?? s.board.tempPch, LIMITS.chipset, 'chipset');
    case 'board':
      return t(s.board.tempSystem, LIMITS.system, 'system');
    default:
      return null;
  }
}

/** 0 when cool, 0.5 at the warning limit, 1 at the problem limit. */
export function heat(temp: number, [warn, hot]: readonly [number, number]): number {
  if (temp < warn) return Math.max(0, 0.5 * (1 - (warn - temp) / 40));
  return Math.min(1, 0.5 + (0.5 * (temp - warn)) / Math.max(1, hot - warn));
}

/** The ramp's stops: cool blue, normal green, warning amber, problem red. */
export const RAMP: Array<[number, [number, number, number]]> = [
  [0, [59, 130, 246]],
  [0.3, [34, 197, 94]],
  [0.5, [245, 185, 66]],
  [1, [255, 77, 77]],
];

export function heatColor(h: number): [number, number, number] {
  const x = Math.max(0, Math.min(1, h));
  for (let i = 1; i < RAMP.length; i++) {
    const [b, cb] = RAMP[i]!;
    const [a, ca] = RAMP[i - 1]!;
    if (x <= b) {
      const f = (x - a) / (b - a);
      return [0, 1, 2].map((k) => Math.round(ca[k]! + (cb[k]! - ca[k]!) * f)) as [number, number, number];
    }
  }
  return RAMP.at(-1)![1];
}

/** Whether a part is coloured in a thermal mode. */
export const inMode = (p: Part, mode: ThermalMode): boolean => p.group != null && (mode === 'full' || p.group === mode);

export const MODES: Array<{ id: ThermalMode; label: string }> = [
  { id: 'full', label: 'Full' },
  { id: 'cpu', label: 'CPU' },
  { id: 'gpu', label: 'GPU' },
  { id: 'board', label: 'Board' },
  { id: 'storage', label: 'Storage' },
];

/** Spin rate shown for a fan, revolutions per second: the real rate scaled down so it reads
 * as speed, not a blur (2000 RPM → about 1.1 turns a second). */
export const spinRate = (rpm: number | null): number => (rpm == null || rpm <= 0 ? 0 : Math.min(2.5, Math.sqrt(rpm) / 40));

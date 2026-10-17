// My PC (P23, P29): a stylized ATX tower with its side off, or a laptop seen through its keyboard
// deck, laid out from what Vyro knows about this PC, and the thermal map's colours. Pure, so it is
// unit-tested; the 3-D scene and the 2-D plan both draw these parts.
//
// Units are millimetres. Tower: x runs from the back of the case (0) to the front (460), y from
// the floor (0) up, z from the board (0) out towards the open side. Laptop: x from left to
// right (0–360), y from the bottom (0) up to the deck (22), z from the hinge (0) to the front
// edge (250).

import { detectCard, type DetectedCard, type GpuShape } from './gpuspecs';
import type { DriveInfo, GpuSnapshot, HardwareId, SensorSnapshot } from './types';
import { fmt } from './format';
import { LIMITS } from './limits';

export type PartKind =
  | 'board'
  | 'cpu'
  | 'cooler'
  | 'aio'
  | 'radiator'
  | 'vrm'
  | 'chipset'
  | 'ram'
  | 'slot'
  | 'gpu'
  | 'm2'
  | 'drive'
  | 'fan'
  | 'psu'
  // Laptop (P29).
  | 'die'
  | 'blower'
  | 'heatpipe'
  | 'sodimm'
  | 'battery'
  | 'deck'
  | 'lid';
/** Which model to draw. */
export type Form = 'desktop' | 'laptop';
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
  /** A laptop GPU chip: its place in the live GPU list. */
  gpuIndex?: number;
  /** A heat pipe's course (laptop). */
  path?: Vec3[];
  /** Where it moves in the exploded view, from its place. */
  explode?: Vec3;
}

export const CASE: Vec3 = [460, 460, 220];
export const LAPTOP: Vec3 = [360, 22, 250];
/** The laptop's lid: open this far from closed, degrees. */
export const LID_OPEN = 110;
export const LID: { width: number; height: number; depth: number } = { width: 360, height: 230, depth: 7 };

/** Desktop or laptop: the owner's pick, else what the firmware says (a tower when it is not a
 * laptop, or says nothing). */
export function formOf(hw: HardwareId | null, pick: 'auto' | 'desktop' | 'laptop' = 'auto'): Form {
  if (pick !== 'auto') return pick;
  return hw?.board.formFactor === 'laptop' ? 'laptop' : 'desktop';
}
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
  /** Board fan headers (by index) seen turning this session; else those turning now. */
  spunFans?: ReadonlySet<number>;
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

  // Case fans: front, then rear, then low front. The board's headers seen turning take the places
  // first; a tower almost always has two at the front and one at the back, so those are drawn
  // either way, turning slowly with "Speed not reported" when nothing reads them. A header that
  // has never turned is empty, or its fans run from a hub: its 0 RPM stood the front fans still
  // (1.2's first field check).
  const spots: Array<{ pos: Vec3; axis: 'x' | 'y' }> = [
    { pos: [446, 390, 110], axis: 'x' },
    { pos: [446, 265, 110], axis: 'x' },
    { pos: [14, 360, 110], axis: 'x' },
    { pos: [446, 140, 110], axis: 'x' },
  ];
  const caseFan = (id: string, label: string, s: (typeof spots)[number], rpm: (snap: SensorSnapshot) => number | null) =>
    box({
      id,
      kind: 'fan',
      label,
      pos: s.pos,
      size: s.axis === 'x' ? [25, 120, 120] : [120, 25, 120],
      group: null,
      tab: 'cooling',
      fan: { axis: s.axis, rpm },
    });
  let spot = 0;
  const spun = (i: number) => input.spunFans?.has(i) ?? (fans[i]?.rpm ?? 0) > 0;
  for (const [i, f] of fans.entries()) {
    if (i === pump || i === cpuFan || f.kind !== 'fan' || !spun(i) || spot >= spots.length) continue;
    parts.push(caseFan(`fan.${i}`, f.name, spots[spot++]!, rpmOf(i)));
  }
  const DEFAULT_SPOTS = 3;
  for (; spot < DEFAULT_SPOTS; spot++) parts.push(caseFan(`casefan.${spot}`, spot === 2 ? 'Rear fan' : 'Front fan', spots[spot]!, () => null));

  parts.push(box({ id: 'psu', kind: 'psu', label: 'PSU', pos: [100, 50, 110], size: [150, 86, 140], group: null, tab: null }));
  for (const p of parts) p.explode = towerExplode(p);
  return parts;
}

/** The exploded view (P29): parts lift off the board towards the open side, the case fans move
 * out of their walls. */
function towerExplode(p: Part): Vec3 {
  switch (p.kind) {
    case 'cooler':
    case 'aio':
      return [0, 0, 150];
    case 'cpu':
      return [0, 0, 80];
    case 'ram':
      return [0, 0, 95];
    case 'gpu':
      return [0, -10, 120];
    case 'vrm':
    case 'chipset':
      return [0, 0, 35];
    case 'm2':
      return [0, 0, 55];
    case 'drive':
    case 'psu':
      return [0, 0, 110];
    case 'radiator':
      return [0, 80, 0];
    case 'fan':
      if (p.id === 'cooler.fan') return [40, 0, 150];
      if (p.id.startsWith('radfan')) return [0, 55, 0];
      return [p.pos[0] > CASE[0] / 2 ? 80 : -80, 0, 0];
    default:
      return [0, 0, 0];
  }
}

// ---------------------------------------------------------------------------------------------
// Laptop (P29)
// ---------------------------------------------------------------------------------------------

/** A heat pipe's part: its course, boxed. */
function pipe(id: string, path: Vec3[]): Part {
  const lo = [0, 1, 2].map((a) => Math.min(...path.map((v) => v[a]!)));
  const hi = [0, 1, 2].map((a) => Math.max(...path.map((v) => v[a]!)));
  return box({
    id,
    kind: 'heatpipe',
    label: 'Heat pipe',
    pos: [0, 1, 2].map((a) => (lo[a]! + hi[a]!) / 2) as Vec3,
    size: [0, 1, 2].map((a) => Math.max(6, hi[a]! - lo[a]!)) as Vec3,
    group: null,
    tab: null,
    labelled: false,
    path,
  });
}

/** The parts of a laptop: the board, its CPU and GPU chips, the heat pipes to two blower fans
 * and their fins at the back, memory, drives and the battery, under the keyboard deck, with
 * the lid open behind. */
export function buildLaptop(input: PcInput): Part[] {
  const { hw, snap } = input;
  const fans = snap?.fans ?? [];
  const named = (re: RegExp) => fans.findIndex((f) => f.kind === 'fan' && re.test(f.name));
  const cpuFan = named(/cpu/i) >= 0 ? named(/cpu/i) : fans.findIndex((f) => f.kind === 'fan');
  const gpuFanBoard = named(/gpu/i);
  const dgpu = discreteGpus(hw, snap)[0];
  const gpuIndex = dgpu ? (dgpu.live ?? 0) : null;
  const parts: Part[] = [
    box({
      id: 'board',
      kind: 'board',
      label: hw?.board.systemProduct ?? hw?.board.product ?? 'Motherboard',
      pos: [180, 5, 80],
      size: [330, 2, 140],
      group: 'board',
      tab: 'hardware',
      anchor: [40, 6, 140],
    }),
    box({ id: 'cpu', kind: 'die', label: 'CPU', pos: [135, 7.5, 85], size: [24, 3, 20], group: 'cpu', tab: 'hardware', anchor: [135, 12, 85] }),
  ];
  if (gpuIndex != null)
    parts.push(
      box({ id: `gpu.${gpuIndex}`, kind: 'die', label: 'GPU', pos: [225, 7.5, 85], size: [30, 3, 30], group: 'gpu', tab: 'hardware', gpuIndex, anchor: [225, 12, 85] }),
    );

  // Copper heat pipes from the chips to the fins at each back corner.
  parts.push(
    pipe('pipe.0', [
      [135, 10, 85],
      [105, 10, 62],
      [72, 10, 30],
      [52, 10, 10],
    ]),
    pipe('pipe.1', [
      [140, 11.5, 90],
      [200, 11.5, 70],
      [275, 11.5, 32],
      [300, 11.5, 10],
    ]),
  );
  if (gpuIndex != null)
    parts.push(
      pipe('pipe.2', [
        [225, 10, 85],
        [255, 10, 60],
        [290, 10, 28],
        [312, 10, 10],
      ]),
    );

  // Two blower fans with their fins at the back vents.
  const blower = (id: string, label: string, x: number, rpm: (s: SensorSnapshot) => number | null) =>
    box({ id, kind: 'blower', label, pos: [x, 11, 42], size: [64, 14, 64], group: null, tab: 'cooling', labelled: false, fan: { axis: 'y', rpm } });
  parts.push(
    blower('fan.cpu', 'CPU fan', 70, (s) => (cpuFan >= 0 ? (s.fans[cpuFan]?.rpm ?? null) : null)),
    blower('fan.gpu', 'GPU fan', 290, (s) => {
      if (gpuFanBoard >= 0) return s.fans[gpuFanBoard]?.rpm ?? null;
      return gpuIndex != null ? (s.gpu[gpuIndex]?.fanRpm ?? null) : null;
    }),
    box({ id: 'fins.0', kind: 'radiator', label: 'Fins', pos: [70, 11, 6], size: [62, 12, 10], group: null, tab: null, labelled: false }),
    box({ id: 'fins.1', kind: 'radiator', label: 'Fins', pos: [290, 11, 6], size: [62, 12, 10], group: null, tab: null, labelled: false }),
  );

  // Memory: up to two modules, stacked; soldered memory shows none.
  const dimms = (hw?.memory.dimms ?? []).slice(0, 2);
  for (const [n, d] of dimms.entries())
    parts.push(
      box({
        id: `ram.${n}`,
        kind: 'sodimm',
        label: 'RAM',
        dimm: n,
        slot: d.slot,
        pos: [190, 7.5 + n * 4, 125],
        size: [70, 3, 30],
        group: 'board',
        tab: 'hardware',
        labelled: n === 0,
        anchor: [190, 14, 125],
      }),
    );

  // NVMe drives on the board.
  const nvme = (snap?.storage ?? []).filter((d) => d.type === 'NVMe').slice(0, 2);
  for (const [i, d] of nvme.entries())
    parts.push(box({ id: d.id, kind: 'm2', label: d.model, pos: [i === 0 ? 95 : 265, 7, 128], size: [80, 3, 22], group: 'storage', tab: 'storage', labelled: i === 0 }));

  parts.push(
    box({ id: 'battery', kind: 'battery', label: 'Battery', pos: [180, 6, 205], size: [300, 8, 70], group: null, tab: null, labelled: false }),
    box({ id: 'deck', kind: 'deck', label: 'Keyboard deck', pos: [180, 21, 125], size: [360, 2, 250], group: null, tab: null, labelled: false }),
  );
  // The lid stands open behind the hinge; its box is the screen's centre.
  const a = ((LID_OPEN - 90) * Math.PI) / 180;
  parts.push(
    box({
      id: 'lid',
      kind: 'lid',
      label: 'Display',
      pos: [180, LAPTOP[1] + (LID.height / 2) * Math.cos(a), -(LID.height / 2) * Math.sin(a)],
      size: [LID.width, LID.height * Math.cos(a), LID.height * Math.sin(a) + LID.depth],
      group: null,
      tab: null,
      labelled: false,
    }),
  );
  for (const p of parts) p.explode = laptopExplode(p);
  return parts;
}

function laptopExplode(p: Part): Vec3 {
  switch (p.kind) {
    case 'deck':
      return [0, 110, 0];
    case 'heatpipe':
      return [0, 60, 0];
    case 'blower':
    case 'radiator':
      return [0, 45, 0];
    case 'die':
      return [0, 30, 0];
    case 'sodimm':
    case 'm2':
      return [0, 22, 0];
    case 'battery':
      return [0, -30, 0];
    default:
      return [0, 0, 0];
  }
}

// ---------------------------------------------------------------------------------------------
// Readings and the thermal map
// ---------------------------------------------------------------------------------------------

export { LIMITS };

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
    case 'die':
      if (p.group === 'gpu') return t(p.gpuIndex != null ? s.gpu[p.gpuIndex]?.tempEdge : null, LIMITS.gpu, 'core');
      return t(s.cpu.tempPackage, LIMITS.cpu, 'package');
    case 'ram':
    case 'sodimm': {
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

/** What a fan's reading says: a speed, stopped (a real 0 RPM, e.g. a GPU resting at idle), or
 * nothing (no sensor, or the board's sensors need PawnIO). */
export type FanState = 'measured' | 'stopped' | 'unknown';

export const fanState = (rpm: number | null): FanState => (rpm == null ? 'unknown' : rpm <= 0 ? 'stopped' : 'measured');

/** Turns a second for a fan whose speed is not reported: it is almost surely running, so it
 * turns slowly, and its details say the speed is not known. */
export const IDLE_SPIN = 0.35;

/** Spin rate shown for a fan, revolutions per second: the real rate scaled down so it reads
 * as speed, not a blur (2000 RPM → about 1.1 turns a second). */
export const spinRate = (rpm: number | null): number =>
  rpm == null ? IDLE_SPIN : rpm <= 0 ? 0 : Math.min(2.5, Math.sqrt(rpm) / 40);

/** A fan's speed for its details. */
export function fanSpeedText(rpm: number | null): string {
  const state = fanState(rpm);
  if (state === 'unknown') return 'Speed not reported';
  if (state === 'stopped') return 'Stopped (0 RPM)';
  return `${fmt(rpm)} RPM`;
}

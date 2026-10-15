import { describe, expect, it } from 'vitest';
import { cardKey, detectCard, tier, validShape, AMD, INTEL, NVIDIA } from './gpuspecs';
import { buildPc, CASE, discreteGpus, heat, heatColor, inMode, LIMITS, partTemp, RAMP, slotIndex, spinRate, type Part } from './pcmodel';
import { sampleHardware } from './sample';
import { initialSimState, nextSnapshot } from './sim';
import type { GpuId, SensorSnapshot } from './types';

const snap = nextSnapshot(initialSimState(), 1_700_000_000, 1, () => 0.5);
const gpu = (over: Partial<GpuId>): GpuId => ({ ...sampleHardware.gpus[0]!, ...over });

describe('GPU shapes', () => {
  it('know reference designs by device id when the board is the chip maker’s own', () => {
    const fe = detectCard(gpu({ name: 'NVIDIA GeForce RTX 4090', vendorId: NVIDIA, deviceId: 0x2684, subsysId: 0x16f110de }));
    expect(fe).toMatchObject({ source: 'reference', design: 'Founders Edition' });
    expect(fe.shape).toMatchObject({ fans: 2, flow: true, slots: 3, power: '16' });
    const amd = detectCard(gpu({ name: 'AMD Radeon RX 7900 XTX', vendorId: AMD, deviceId: 0x744c, subsysId: 0x0e3b1002 }));
    expect(amd).toMatchObject({ source: 'reference', design: 'AMD reference' });
    expect(amd.shape.fans).toBe(3);
    const arc = detectCard(gpu({ name: 'Intel(R) Arc(TM) A770 Graphics', vendorId: INTEL, deviceId: 0x56a0, subsysId: 0x10208086 }));
    expect(arc.design).toBe('Limited Edition');
  });

  it('give partner cards a shape from the tier, and name the partner', () => {
    // The sample is a Gigabyte RTX 4070 (subsystem vendor 0x1458).
    const g = detectCard(sampleHardware.gpus[0]!);
    expect(g).toMatchObject({ source: 'partner', design: 'Gigabyte card' });
    expect(g.shape).toMatchObject({ fans: 3, power: '16', flow: false });
    const big = detectCard(gpu({ name: 'NVIDIA GeForce RTX 3090', vendorId: NVIDIA, deviceId: 0x2204, subsysId: 0x87af1043 }));
    expect(big).toMatchObject({ design: 'ASUS card' });
    expect(big.shape).toMatchObject({ fans: 3, slots: 3.5, power: '3x8' });
    const small = detectCard(gpu({ name: 'NVIDIA GeForce RTX 3050', vendorId: NVIDIA, deviceId: 0x2507, subsysId: 0x12341462 }));
    expect(small.shape.fans).toBe(2);
    const unknown = detectCard(gpu({ name: 'Matrox Something', vendorId: 0x102b, deviceId: 1, subsysId: 0 }));
    expect(unknown.source).toBe('generic');
  });

  it('read the tier from the model number', () => {
    expect(tier('NVIDIA GeForce RTX 4090', NVIDIA)).toBe(90);
    expect(tier('NVIDIA GeForce RTX 3060 Ti', NVIDIA)).toBe(60);
    expect(tier('NVIDIA GeForce GTX 1660 SUPER', NVIDIA)).toBe(60);
    expect(tier('AMD Radeon RX 7900 XT', AMD)).toBe(90);
    expect(tier('AMD Radeon RX 6650 XT', AMD)).toBe(60);
    expect(tier('AMD Radeon RX 9070 XT', AMD)).toBe(70);
    expect(tier('Intel(R) Arc(TM) B580 Graphics', INTEL)).toBe(50);
    expect(tier('Microsoft Basic Render Driver', NVIDIA)).toBeNull();
  });

  it('keys a card by vendor, device and subsystem, and only accepts valid shapes back', () => {
    expect(cardKey({ vendorId: 0x10de, deviceId: 0x2786, subsysId: 0x51411458 })).toBe('10de:2786:51411458');
    const s = detectCard(sampleHardware.gpus[0]!).shape;
    expect(validShape(s)).toEqual(s);
    expect(validShape({ ...s, fans: 4 })).toBeNull();
    expect(validShape({ ...s, slots: 5 })).toBeNull();
    expect(validShape({ ...s, power: 'molex' })).toBeNull();
    expect(validShape(null)).toBeNull();
  });
});

describe('My PC layout', () => {
  const parts = buildPc({ hw: sampleHardware, snap, cardKey });
  const byKind = (k: Part['kind']) => parts.filter((p) => p.kind === k);

  it('fits every part inside the case, ids unique', () => {
    expect(new Set(parts.map((p) => p.id)).size).toBe(parts.length);
    for (const p of parts) {
      for (const a of [0, 1, 2]) {
        expect(p.pos[a]! - p.size[a]! / 2, `${p.id} axis ${a}`).toBeGreaterThanOrEqual(-0.01);
        expect(p.pos[a]! + p.size[a]! / 2, `${p.id} axis ${a}`).toBeLessThanOrEqual(CASE[a]! + 0.01);
      }
    }
  });

  it('draws what this PC has', () => {
    // The sample reports a pump: an AIO block with a radiator, not a tower.
    expect(snap.fans.some((f) => f.kind === 'pump')).toBe(true);
    expect(byKind('aio')).toHaveLength(1);
    expect(byKind('cooler')).toHaveLength(0);
    expect(byKind('ram')).toHaveLength(sampleHardware.memory.dimms.length);
    expect(byKind('ram').length + byKind('slot').length).toBe(sampleHardware.memory.slotsTotal);
    expect(byKind('gpu').map((p) => p.gpu!.card.design)).toEqual(['Gigabyte card']);
    expect(byKind('m2').length).toBe(snap.storage.filter((d) => d.type === 'NVMe').length);
    // Without a pump, a tower cooler with its own fan.
    const noPump = { ...snap, fans: snap.fans.filter((f) => f.kind !== 'pump') };
    const tower = buildPc({ hw: sampleHardware, snap: noPump, cardKey });
    expect(tower.filter((p) => p.kind === 'cooler')).toHaveLength(1);
    expect(tower.some((p) => p.id === 'cooler.fan')).toBe(true);
  });

  it('uses the owner’s card shape and stacks a second GPU below the first', () => {
    const key = cardKey(sampleHardware.gpus[0]!);
    const mine = { ...detectCard(sampleHardware.gpus[0]!).shape, fans: 1, lengthMm: 170 };
    const p = buildPc({ hw: sampleHardware, snap, cardKey, shapes: { [key]: mine } }).find((x) => x.kind === 'gpu')!;
    expect([p.size[0], p.gpu!.shape.fans]).toEqual([170, 1]);
    const two = { ...sampleHardware, gpus: [sampleHardware.gpus[0]!, { ...sampleHardware.gpus[0]!, name: 'Second' }] };
    const live = { ...snap, gpu: [snap.gpu[0]!, { ...snap.gpu[0]!, name: 'Second', integrated: false }] };
    const gpus = buildPc({ hw: two, snap: live, cardKey }).filter((x) => x.kind === 'gpu');
    expect(gpus.map((g) => g.id)).toEqual(['gpu.0', 'gpu.1']);
    expect(gpus[1]!.pos[1]).toBeLessThan(gpus[0]!.pos[1] - gpus[0]!.size[1] / 2);
    expect(gpus.map((g) => g.label)).toEqual(['GPU 1', 'GPU 2']);
  });

  it('leaves integrated graphics out', () => {
    const igpu = { ...sampleHardware, gpus: [{ ...sampleHardware.gpus[0]!, name: 'Intel UHD 770', vramMB: 128 }] };
    expect(discreteGpus(igpu, { ...snap, gpu: [{ ...snap.gpu[0]!, name: 'Intel UHD 770', integrated: true }] })).toEqual([]);
    expect(discreteGpus(igpu, null)).toEqual([]);
  });

  it('places RAM by slot name', () => {
    expect(['DIMM_A1', 'DIMM_A2', 'DIMM_B1', 'DIMM_B2'].map(slotIndex)).toEqual([0, 1, 2, 3]);
    expect(['ChannelA-DIMM0', 'ChannelB-DIMM1', 'Controller0-ChannelA-DIMM1'].map(slotIndex)).toEqual([0, 3, 1]);
    expect(slotIndex('P0 CHANNEL A')).toBeNull();
  });
});

describe('thermal map', () => {
  const parts = buildPc({ hw: sampleHardware, snap, cardKey });

  it('reads each part’s temperature against Health’s limits', () => {
    const cpu = partTemp(parts.find((p) => p.kind === 'aio')!, snap)!;
    expect(cpu).toEqual({ temp: snap.cpu.tempPackage, limits: LIMITS.cpu, what: 'package' });
    const g = partTemp(parts.find((p) => p.kind === 'gpu')!, snap)!;
    expect(g.temp).toBe(snap.gpu[0]!.tempEdge);
    const nvme = parts.find((p) => p.kind === 'm2')!;
    expect(partTemp(nvme, snap)!.limits).toEqual(LIMITS.nvme);
    expect(partTemp(parts.find((p) => p.kind === 'psu')!, snap)).toBeNull();
    expect(partTemp(nvme, null)).toBeNull();
    const cold: SensorSnapshot = { ...snap, cpu: { ...snap.cpu, tempPackage: null } };
    expect(partTemp(parts.find((p) => p.kind === 'aio')!, cold)).toBeNull();
  });

  it('heat is 0.5 at the warning limit and 1 at the problem limit, rising throughout', () => {
    const lim = [90, 99] as const;
    expect(heat(90, lim)).toBe(0.5);
    expect(heat(99, lim)).toBe(1);
    expect(heat(120, lim)).toBe(1);
    expect(heat(20, lim)).toBe(0);
    let last = -1;
    for (let t = 30; t <= 105; t += 1) {
      const h = heat(t, lim);
      expect(h).toBeGreaterThanOrEqual(last);
      last = h;
    }
  });

  it('colours run blue, green, amber, red', () => {
    expect(heatColor(0)).toEqual(RAMP[0]![1]);
    expect(heatColor(0.5)).toEqual([245, 185, 66]);
    expect(heatColor(1)).toEqual([255, 77, 77]);
    expect(heatColor(-1)).toEqual(heatColor(0));
    const mid = heatColor(0.4);
    expect(mid[0]).toBeGreaterThan(34);
    expect(mid[0]).toBeLessThan(245);
  });

  it('modes colour only their parts', () => {
    const ids = (mode: Parameters<typeof inMode>[1]) => parts.filter((p) => inMode(p, mode)).map((p) => p.kind);
    expect(new Set(ids('gpu'))).toEqual(new Set(['gpu']));
    expect(new Set(ids('cpu'))).toEqual(new Set(['cpu', 'aio']));
    expect(ids('board')).toContain('ram');
    expect(ids('full')).not.toContain('fan');
    expect(ids('full')).not.toContain('psu');
  });

  it('fans spin slower than real, faster with more RPM', () => {
    expect(spinRate(null)).toBe(0);
    expect(spinRate(0)).toBe(0);
    expect(spinRate(400)).toBeLessThan(spinRate(1600));
    expect(spinRate(100_000)).toBe(2.5);
  });
});

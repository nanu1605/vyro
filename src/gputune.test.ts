import { describe as group, expect, it } from 'vitest';
import { STOCK, clamp, describe, inForce, isStock, nameProblem, newId, runningText, same, testedText, withPreset, withoutPreset } from './gputune';
import type { TuneConfig, TuneLimits, TunePreset, TuneStatus } from './types';

const LIMITS: TuneLimits = {
  core: { min: -500, max: 1000 },
  mem: { min: -1000, max: 1500 },
  power: { min: 50, max: 115 },
  powerDefaultW: 180,
};
const PLUS = { coreMhz: 90, memMhz: 500, powerPct: 110 };
const mine = (id: string, name = id): TunePreset => ({ id, name, settings: PLUS, kind: 'custom', note: null });
const CONFIG: TuneConfig = {
  enabled: true,
  preset: 'mine',
  applyAtStart: false,
  presets: [mine('mine', 'Mine')],
  profiles: { gaming: 'mine', silent: 'quiet' },
  gpu: 'GPU-1',
  tested: [],
};

group('GPU tuning', () => {
  it('describes settings', () => {
    expect(describe(STOCK)).toBe('stock');
    expect(describe(PLUS)).toBe('core +90 MHz, memory +500 MHz, power 110 %');
    expect(describe({ coreMhz: -30, memMhz: 0, powerPct: 100 })).toBe('core −30 MHz');
    expect(isStock(STOCK)).toBe(true);
    expect(same(PLUS, { ...PLUS })).toBe(true);
    expect(same(PLUS, null)).toBe(false);
  });

  it('keeps settings within what the driver allows', () => {
    expect(clamp({ coreMhz: 5000, memMhz: -9000, powerPct: 200 }, LIMITS)).toEqual({ coreMhz: 1000, memMhz: -1000, powerPct: 115 });
    // A laptop without a power limit to change: power stays at 100 %.
    expect(clamp({ coreMhz: 10.6, memMhz: 0, powerPct: 80 }, { ...LIMITS, power: null })).toEqual({ coreMhz: 11, memMhz: 0, powerPct: 100 });
  });

  it('names and ids of the owner presets', () => {
    expect(newId([{ id: 'custom-1' }, { id: 'custom-3' }])).toBe('custom-2');
    expect(nameProblem('  ', [], null)).toMatch(/name/);
    expect(nameProblem('x'.repeat(33), [], null)).toMatch(/32/);
    expect(nameProblem('mine', CONFIG.presets, null)).toMatch(/Another/);
    expect(nameProblem('Mine', CONFIG.presets, 'mine')).toBeNull();
  });

  it('adds, replaces and removes presets', () => {
    const added = withPreset(CONFIG, mine('custom-1', 'Second'));
    expect(added.presets.map((p) => p.id)).toEqual(['mine', 'custom-1']);
    const replaced = withPreset(added, { ...mine('mine', 'Renamed') });
    expect(replaced.presets.map((p) => p.name)).toEqual(['Renamed', 'Second']);
    const removed = withoutPreset(CONFIG, 'mine');
    expect(removed.presets).toEqual([]);
    expect(removed.preset).toBe('stock');
    expect(removed.profiles).toEqual({ gaming: 'stock', silent: 'quiet' });
  });

  it('says what runs', () => {
    const st = {
      config: CONFIG,
      presets: [
        { id: 'stock', name: 'Stock', settings: STOCK, kind: 'stock', note: null, testedAt: 0 },
        { id: 'mine', name: 'Mine', settings: PLUS, kind: 'custom', note: null, testedAt: null },
      ],
      applied: 'mine',
      profile: 'gaming',
      trial: null,
    } as unknown as TuneStatus;
    expect(runningText(st)).toBe('Mine · core +90 MHz, memory +500 MHz, power 110 %');
    expect(inForce(st)?.id).toBe('mine');
    expect(inForce(st, 'desktop')?.id).toBe('mine');
    expect(runningText({ ...st, applied: null })).toBe('Stock');
    expect(runningText({ ...st, trial: { settings: PLUS, step: '', done: 0 } })).toMatch(/^On test/);
    expect(runningText({ ...st, config: { ...CONFIG, enabled: false } })).toMatch(/off/);
    expect(testedText(null)).toBeNull();
    expect(testedText(0)).toBe('stock');
    expect(testedText(1_760_000_000)).toMatch(/^tested /);
  });
});

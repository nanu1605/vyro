import { describe as group, expect, it } from 'vitest';
import { describe, groupByDay, merge, newestId } from './changes';
import { SAMPLE_CHANGES } from './sim-changes';
import type { HardwareChange } from './types';

const base: HardwareChange = {
  id: 1, ts: 0, category: 'hardware', kind: 'added', class: null, slot: null, name: '', field: null, before: null, after: null, lastSeen: null,
};
const c = (x: Partial<HardwareChange>): HardwareChange => ({ ...base, ...x });

group('PC change history', () => {
  it('says what each change was', () => {
    expect(describe(c({ kind: 'changed', category: 'driver', class: 'gpu', name: 'RTX 5060 Ti', field: 'Driver', before: 'a', after: 'b' }))).toMatchObject({
      title: 'Graphics driver', device: 'RTX 5060 Ti', from: 'a', to: 'b',
    });
    expect(describe(c({ kind: 'changed', category: 'windows', class: 'windows', name: 'Windows', field: 'Build' }))).toMatchObject({
      title: 'Windows update', device: null,
    });
    expect(describe(c({ kind: 'changed', category: 'settings', class: 'memory', slot: 'DIMM_A2', name: 'Kit', field: 'Speed' }))).toMatchObject({
      title: 'Memory speed', device: 'DIMM_A2: Kit',
    });
    expect(describe(c({ kind: 'replaced', class: 'memory', slot: 'DIMM_B2', before: 'Old', after: 'New' }))).toMatchObject({
      title: 'Memory module replaced', device: 'DIMM_B2', from: 'Old', to: 'New',
    });
    expect(describe(c({ kind: 'added', class: 'gpu', name: 'RTX' })).title).toBe('Graphics card added');
    expect(describe(c({ kind: 'firstSeen', class: 'monitor', name: 'DELL', before: 'LG', after: 'LG; DELL' }))).toMatchObject({
      title: 'New monitor connected', label: 'Monitors before and now', from: 'LG',
    });
    expect(describe(c({ kind: 'missing', class: 'network', name: 'Intel Wi-Fi', lastSeen: 1_700_000_000 })).title).toBe('Network adapter not seen for 30 days');
    expect(describe(c({ kind: 'tracking' })).title).toBe('Tracking started');
  });

  it('folds the same change on several modules into one entry', () => {
    const expo = (id: number, slot: string, field: string) =>
      c({ id, ts: 50, kind: 'changed', category: 'settings', class: 'memory', slot, name: 'Kit', field, before: 'a', after: 'b' });
    // As the engine stores them: per module, speed then voltage; listed newest first.
    const list = merge([expo(4, 'DIMM_B2', 'Voltage'), expo(3, 'DIMM_B2', 'Speed'), expo(2, 'DIMM_A2', 'Voltage'), expo(1, 'DIMM_A2', 'Speed')]);
    expect(list.map((x) => [x.id, x.field, x.slot, x.count])).toEqual([
      [4, 'Voltage', 'DIMM_A2, DIMM_B2', 2],
      [3, 'Speed', 'DIMM_A2, DIMM_B2', 2],
    ]);
    // The same change at another moment stays its own entry.
    expect(merge([expo(2, 'DIMM_A2', 'Speed'), { ...expo(1, 'DIMM_B2', 'Speed'), ts: 40 }])).toHaveLength(2);
    const swap = merge(SAMPLE_CHANGES).find((x) => x.kind === 'replaced')!;
    expect(describe(swap)).toMatchObject({ title: 'Memory modules replaced', device: 'DIMM_A2, DIMM_B2' });
    expect(describe(c({ kind: 'firstSeen', class: 'removableDrive', name: 'Stick' })).title).toBe('New USB drive connected');
  });

  it('groups by local day, newest first', () => {
    const now = new Date(2026, 9, 15, 12).getTime() / 1000;
    const days = groupByDay(
      [c({ id: 4, ts: now - 3600 }), c({ id: 3, ts: now - 7200 }), c({ id: 2, ts: now - 86_400 }), c({ id: 1, ts: now - 5 * 86_400 })],
      now,
    );
    expect(days.map((d) => [d.label, d.items.map((x) => x.id)])).toEqual([
      ['Today', [4, 3]],
      ['Yesterday', [2]],
      [expect.stringMatching(/10/), [1]],
    ]);
  });

  it('the dot counts changes, not the start of tracking', () => {
    expect(newestId([c({ id: 1, kind: 'tracking' })])).toBe(0);
    expect(newestId(SAMPLE_CHANGES)).toBe(14);
    // The preview covers every category and the newest is first.
    expect(new Set(SAMPLE_CHANGES.map((x) => x.category)).size).toBe(5);
    expect(SAMPLE_CHANGES.every((x, i, a) => i === 0 || a[i - 1]!.id > x.id)).toBe(true);
  });
});

import { describe, expect, it } from 'vitest';
import { activeLayout, defaultLayout, initial, isKind, MAX_LAYOUTS, MAX_WIDGETS, narrowSpan, needsFor, readingDef, readingView, WIDTHS } from './dashboard';
import { PRESET_IDS, PRESETS, presetLayout, seed } from './dashboard-presets';
import {
  add,
  CARDS,
  deleteLayout,
  duplicate,
  freshId,
  gaugeSources,
  isCard,
  KINDS,
  move,
  newLayout,
  readingIds,
  remove,
  reset,
  resize,
  snapWidth,
  stepWidth,
  widgetTitle,
} from './dashboard-edit';
import { initialSimState, nextSnapshot } from './sim';

const snap = nextSnapshot(initialSimState(), 1_700_000_000, 1, () => 0.5);

describe('dashboard presets', () => {
  it('are all valid layouts whose widgets resolve on this PC', () => {
    for (const id of PRESET_IDS) {
      const l = presetLayout(id);
      expect(l.widgets.length).toBeGreaterThan(0);
      expect(l.widgets.length).toBeLessThanOrEqual(MAX_WIDGETS);
      expect(new Set(l.widgets.map((w) => w.id)).size).toBe(l.widgets.length);
      for (const w of l.widgets) {
        expect(isKind(w.kind), `${id}: ${w.kind}`).toBe(true);
        expect(WIDTHS as readonly number[]).toContain(w.w);
        expect(w.h).toBeGreaterThanOrEqual(1);
        expect(w.h).toBeLessThanOrEqual(4);
        if (w.kind === 'reading' || w.kind === 'graph') expect(readingDef(w.source!, snap), `${id}: ${w.source}`).not.toBeNull();
        if (w.kind === 'gauge') expect(gaugeSources(snap)).toContain(w.source);
        if (w.kind === 'card') expect(isCard(w.source)).toBe(true);
      }
    }
  });

  it('Default is today’s Dashboard: two gauges, the nine tiles, then the cards', () => {
    const d = presetLayout('default').widgets;
    expect(d.slice(0, 2).map((w) => w.source)).toEqual(['cpu', 'gpu.0']);
    expect(d.filter((w) => w.kind === 'reading').map((w) => w.source)).toEqual([
      'game.fps', 'cpu.temp', 'cpu.power', 'gpu.0.temp', 'gpu.0.hotspot', 'gpu.0.vram', 'mem.used', 'power.system', 'fan.cpu',
    ]);
    expect(d.filter((w) => w.kind === 'card').map((w) => w.source)).toEqual(Object.keys(CARDS));
  });

  it('seed holds every preset, Default shown', () => {
    const s = seed();
    expect(s.layouts.map((l) => l.name)).toEqual(['Default', 'Gaming', 'Work', 'Streaming', 'Benchmark', 'Monitoring', 'Minimal']);
    expect(activeLayout(s).id).toBe('default');
    expect(PRESETS.minimal.widgets.length).toBeLessThan(PRESETS.default.widgets.length);
    // Before the presets load, Default alone is the same layout.
    expect(initial().layouts).toEqual([defaultLayout()]);
    expect(defaultLayout()).toEqual(presetLayout('default'));
  });
});

describe('readings', () => {
  it('read the snapshot', () => {
    expect(readingDef('cpu.temp', snap)!.value(snap)).toBe(snap.cpu.tempPackage);
    expect(readingDef('gpu.0.vram', snap)!.value(snap)).toBeCloseTo(snap.gpu[0]!.vramUsedMB! / 1024);
    expect(readingDef('gpu.0.vram', snap)!.sub!(snap)).toMatch(/^of \d+\.\d GB$/);
    expect(readingDef('mem.load', snap)!.value(snap)).toBeCloseTo((snap.memory.usedMB! / snap.memory.totalMB!) * 100);
    const fan = snap.fans.find((f) => f.kind === 'fan')!;
    expect(readingDef('fan.cpu', snap)!.label).toBe(fan.name);
    expect(readingDef('fan.cpu', snap)!.value(snap)).toBe(fan.rpm);
    expect(readingDef('disk.read', snap)!.value(snap)).toBeCloseTo(snap.storage.reduce((a, d) => a + (d.readMBs ?? 0), 0));
    // A game reading is empty without a game, and marked as game-only.
    const r = readingDef('game.fps', snap)!;
    expect(r.game).toBe(true);
    expect(r.value({ ...snap, game: null })).toBeNull();
  });

  it('name GPUs only when there are several, and survive a missing part', () => {
    expect(readingDef('gpu.0.temp', snap)!.label).toBe('GPU temp');
    const two = { ...snap, gpu: [snap.gpu[0]!, snap.gpu[0]!] };
    expect(readingDef('gpu.1.temp', two)!.label).toBe('GPU temp (GPU 2)');
    expect(readingDef('gpu.3.temp', snap)!.value(snap)).toBeNull();
    expect(readingDef('gpu.0.sparkle', snap)).toBeNull();
    expect(readingDef('nope', snap)).toBeNull();
    expect(readingDef('fan.7', snap)!.label).toBe('Fan 8');
  });

  it('every listed id resolves, with no duplicates', () => {
    const ids = readingIds(snap);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(readingDef(id, snap), id).not.toBeNull();
    expect(ids.length).toBeGreaterThan(25);
    expect(readingIds(null)).toContain('gpu.0.temp');
  });

  it('say why a driver reading is missing: an Intel CPU’s is not read, with PawnIO or without (2.0.4)', () => {
    const ready = { state: 'ready', version: '2.2.0', detail: null, cpuUnread: false } as const;
    expect(needsFor(null)).toEqual({ cpu: 'needs PawnIO', board: 'needs PawnIO' });
    expect(needsFor({ ...ready, state: 'notInstalled' })).toEqual({ cpu: 'needs PawnIO', board: 'needs PawnIO' });
    expect(needsFor(ready)).toEqual({ cpu: 'not reported', board: 'not reported' });
    expect(needsFor({ ...ready, cpuUnread: true })).toEqual({ cpu: 'not read on this CPU', board: 'not reported' });
    expect(needsFor({ ...ready, state: 'notInstalled', cpuUnread: true })).toEqual({ cpu: 'not read on this CPU', board: 'needs PawnIO' });

    const needs = needsFor({ ...ready, cpuUnread: true });
    const bare = { ...snap, cpu: { ...snap.cpu, tempPackage: null, powerPkgW: null }, power: { ...snap.power, cpuW: null, estTotalW: null } };
    const sub = (id: string, s = bare) => readingView(readingDef(id, s)!, s, needs, 'c').sub;
    expect(sub('cpu.temp')).toBe('not read on this CPU');
    expect(sub('cpu.power')).toBe('not read on this CPU');
    expect(sub('power.system')).toBe('not read on this CPU');
    expect(sub('cpu.temp', snap)).toBeNull();
    // "estimated" only under a value: a missing one said "— estimated".
    expect(sub('power.system', snap)).toBe('estimated');
    const noFans = { ...bare, fans: bare.fans.map((f) => ({ ...f, rpm: null })) };
    expect(sub('fan.cpu', noFans)).toBe('not reported');
    expect(sub('mem.load')).toBeNull();
  });
});

describe('layout edits', () => {
  const l = presetLayout('minimal');

  it('move, resize, duplicate, remove and add', () => {
    const ids = (x: typeof l) => x.widgets.map((w) => w.id);
    expect(ids(move(l, 'w1', 3))).toEqual(['w2', 'w3', 'w4', 'w1', 'w5']);
    expect(ids(move(l, 'w5', -4))).toEqual(['w5', 'w1', 'w2', 'w3', 'w4']);
    expect(move(l, 'missing', 0)).toBe(l);
    const r = resize(l, 'w1', 7, 9).widgets[0]!;
    expect([r.w, r.h]).toEqual([6, 4]);
    const d = duplicate(l, 'w2');
    expect(ids(d)).toEqual(['w1', 'w2', 'w6', 'w3', 'w4', 'w5']);
    expect(d.widgets[2]!.source).toBe('gpu.0.temp');
    expect(ids(remove(l, 'w3'))).toEqual(['w1', 'w2', 'w4', 'w5']);
    const a = add(l, 'graph');
    expect(a.widgets.at(-1)).toMatchObject({ id: 'w6', kind: 'graph', w: KINDS.graph.w, h: KINDS.graph.h, source: 'cpu.temp' });
    expect(add(l, 'health').widgets.at(-1)!.source).toBeNull();
    // Capped.
    let full = l;
    while (full.widgets.length < MAX_WIDGETS) full = add(full, 'reading');
    expect(add(full, 'reading')).toBe(full);
    expect(duplicate(full, 'w1')).toBe(full);
  });

  it('widths step and snap through the grid', () => {
    expect([3, 4, 6, 8, 12].map((w) => stepWidth(w, 1))).toEqual([4, 6, 8, 12, 12]);
    expect([3, 4, 6, 8, 12].map((w) => stepWidth(w, -1))).toEqual([3, 3, 4, 6, 8]);
    expect([0, 5, 7, 10, 11, 40].map(snapWidth)).toEqual([3, 4, 6, 8, 12, 12]);
    expect([3, 4, 6, 8, 12].map(narrowSpan)).toEqual([3, 3, 6, 6, 6]);
  });

  it('reset returns to the preset, keeping name and lock', () => {
    const changed = { ...remove(l, 'w1'), name: 'Mine', locked: true };
    const back = reset(changed);
    expect(back.widgets).toEqual(presetLayout('minimal').widgets);
    expect([back.name, back.locked]).toEqual(['Mine', true]);
    expect(reset({ ...changed, preset: null }).widgets).toEqual(presetLayout('default').widgets);
  });

  it('new and deleted layouts', () => {
    const s = seed();
    const n = newLayout(s, activeLayout(s), '  My desk  ')!;
    expect(n.active).toBe('layout-1');
    const made = activeLayout(n);
    expect([made.name, made.preset, made.locked]).toEqual(['My desk', 'default', false]);
    expect(made.widgets).toEqual(activeLayout(s).widgets);
    expect(made.widgets[0]).not.toBe(activeLayout(s).widgets[0]);
    expect(newLayout(n, made, '')!.layouts.at(-1)!.name).toBe('Layout 9');
    let many = n;
    while (many.layouts.length < MAX_LAYOUTS) many = newLayout(many, made, 'x')!;
    expect(newLayout(many, made, 'x')).toBeNull();

    const del = deleteLayout(n, 'layout-1');
    expect(del.active).toBe('default');
    expect(del.layouts).toHaveLength(7);
    const one = { ...s, layouts: [s.layouts[0]!] };
    expect(deleteLayout(one, 'default')).toBe(one);
    expect(freshId('w', ['w1', 'w3'])).toBe('w2');
  });

  it('titles name what a widget shows', () => {
    const t = (kind: string, source: string | null) => widgetTitle({ id: 'x', kind, w: 3, h: 1, source }, snap);
    expect(t('gauge', 'gpu.0')).toBe('GPU 1 gauge');
    expect(t('reading', 'cpu.temp')).toBe('Reading: CPU package');
    expect(t('card', 'board')).toBe('Board and fans card');
    expect(t('hologram', null)).toBe('Widget from a newer Vyro');
  });
});

import { describe, expect, it } from 'vitest';
import rust from '../src-tauri/src/widgets.rs?raw';
import { DESK_KINDS, MAX_SOURCES, MAX_WIDGETS, PROC_SORTS, newWidget, sortOf, topProcesses, widgetIdFromHash, widgetSize, withSources } from './widgets';
import type { ProcessSnapshot } from './types';

describe('desktop widgets', () => {
  it('match the app’s kinds and limits', () => {
    const kinds = /KINDS: \[&str; \d+\] = \[([^\]]+)\]/.exec(rust)![1]!.match(/"(\w+)"/g)!.map((k) => k.slice(1, -1));
    expect(DESK_KINDS.map((k) => k.id)).toEqual(kinds);
    expect(rust).toContain(`MAX_WIDGETS: usize = ${MAX_WIDGETS};`);
    expect(rust).toContain(`MAX_SOURCES: usize = ${MAX_SOURCES};`);
    const sorts = /PROC_SORTS: \[&str; \d+\] = \[([^\]]+)\]/.exec(rust)![1]!.match(/"([\w.]+)"/g)!.map((k) => k.slice(1, -1));
    expect(PROC_SORTS.map((s) => s.id)).toEqual(sorts);
  });

  it('size like the app does', () => {
    // The Rust tests pin these too.
    expect(widgetSize({ kind: 'gauge', sources: ['a'], scale: 100 })).toEqual([200, 200]);
    expect(widgetSize({ kind: 'gauge', sources: ['a'], scale: 150 })).toEqual([300, 300]);
    expect(widgetSize({ kind: 'stack', sources: ['a', 'b', 'c'], scale: 100 })).toEqual([250, 160]);
    expect(widgetSize({ kind: 'reading', sources: [], scale: 50 })).toEqual([110, 52]);
    expect(widgetSize({ kind: 'network', sources: [], scale: 100 })).toEqual([280, 150]);
    expect(widgetSize({ kind: 'game', sources: [], scale: 100 })).toEqual([260, 150]);
    expect(widgetSize({ kind: 'top', sources: ['proc.cpu'], scale: 100 })).toEqual([260, 200]);
  });

  it('make new widgets with fresh ids and their usual readings', () => {
    const a = newWidget('gauge', []);
    const b = newWidget('gauge', [a.id]);
    expect(a.id).not.toBe(b.id);
    expect(a.id).toMatch(/^[a-z0-9-]{1,24}$/);
    expect(a.sources).toEqual(['cpu.usage']);
    expect(newWidget('stack', []).sources.length).toBeLessThanOrEqual(MAX_SOURCES);
    expect(withSources(a, ['x', 'y']).sources).toEqual(['x']);
    expect(withSources(newWidget('stack', []), ['x', 'x', 'y']).sources).toEqual(['x', 'y']);
    expect(a.clickThrough).toBe(false);
    expect(newWidget('network', []).sources).toEqual([]);
    expect(newWidget('top', []).sources).toEqual(['proc.cpu']);
    expect(withSources(newWidget('game', []), ['game.fps']).sources).toEqual([]);
  });

  it('rank programs by a measure, a name once with its processes added up', () => {
    const p = (name: string, pid: number, cpuPct: number | null, ramMB: number | null, gpuPct: number | null): ProcessSnapshot => ({ pid, name, cpuPct, ramMB, gpuPct });
    const list = [
      p('chrome.exe', 1, 4, 900, 1),
      p('game.exe', 2, 30, 5000, 80),
      p('chrome.exe', 3, 3, 700, 0),
      p('idle.exe', 4, 0, 10, 0),
      p('new.exe', 5, null, 50, null),
    ];
    expect(topProcesses(list, 'proc.cpu')).toEqual([
      { name: 'game.exe', count: 1, value: 30 },
      { name: 'chrome.exe', count: 2, value: 7 },
    ]);
    expect(topProcesses(list, 'proc.ram', 2).map((r) => r.name)).toEqual(['game.exe', 'chrome.exe']);
    expect(topProcesses(list, 'proc.ram').at(-1)).toEqual({ name: 'idle.exe', count: 1, value: 10 });
    expect(topProcesses(list, 'proc.gpu').map((r) => r.value)).toEqual([80, 1]);
    expect(sortOf({ sources: ['proc.gpu'] })).toBe('proc.gpu');
    expect(sortOf({ sources: [] })).toBe('proc.cpu');
  });

  it('read their id from the window address only when it is well formed', () => {
    expect(widgetIdFromHash('#widget=gauge-2')).toBe('gauge-2');
    expect(widgetIdFromHash('#widget=../x')).toBeNull();
    expect(widgetIdFromHash('#overlay')).toBeNull();
  });
});

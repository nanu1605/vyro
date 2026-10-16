import { describe, expect, it } from 'vitest';
import rust from '../src-tauri/src/widgets.rs?raw';
import { DESK_KINDS, MAX_SOURCES, MAX_WIDGETS, newWidget, widgetIdFromHash, widgetSize, withSources } from './widgets';

describe('desktop widgets', () => {
  it('match the app’s kinds and limits', () => {
    const kinds = /KINDS: \[&str; \d+\] = \[([^\]]+)\]/.exec(rust)![1]!.match(/"(\w+)"/g)!.map((k) => k.slice(1, -1));
    expect(DESK_KINDS.map((k) => k.id)).toEqual(kinds);
    expect(rust).toContain(`MAX_WIDGETS: usize = ${MAX_WIDGETS};`);
    expect(rust).toContain(`MAX_SOURCES: usize = ${MAX_SOURCES};`);
  });

  it('size like the app does', () => {
    // The Rust tests pin these too.
    expect(widgetSize({ kind: 'gauge', sources: ['a'], scale: 100 })).toEqual([200, 200]);
    expect(widgetSize({ kind: 'gauge', sources: ['a'], scale: 150 })).toEqual([300, 300]);
    expect(widgetSize({ kind: 'stack', sources: ['a', 'b', 'c'], scale: 100 })).toEqual([250, 160]);
    expect(widgetSize({ kind: 'reading', sources: [], scale: 50 })).toEqual([110, 52]);
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
  });

  it('read their id from the window address only when it is well formed', () => {
    expect(widgetIdFromHash('#widget=gauge-2')).toBe('gauge-2');
    expect(widgetIdFromHash('#widget=../x')).toBeNull();
    expect(widgetIdFromHash('#overlay')).toBeNull();
  });
});

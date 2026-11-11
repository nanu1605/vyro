import { describe, expect, it } from 'vitest';
import { renameKeys } from './rename-storage';

/** A Storage over a Map, in insertion order like a browser's. */
function store(entries: Record<string, string>, full = false): Storage {
  const m = new Map(Object.entries(entries));
  return {
    get length() {
      return m.size;
    },
    key: (i: number) => [...m.keys()][i] ?? null,
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => {
      if (full) throw new DOMException('full', 'QuotaExceededError');
      m.set(k, v);
    },
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  };
}

describe('renameKeys', () => {
  it('moves every vyro. key to clokrix. and drops the old ones', () => {
    const s = store({ 'vyro.look': '{"theme":"cyber"}', 'vyro.cards.at': '[3,2]', other: 'x' });
    expect(renameKeys(s)).toBe(2);
    expect(s.getItem('clokrix.look')).toBe('{"theme":"cyber"}');
    expect(s.getItem('clokrix.cards.at')).toBe('[3,2]');
    expect(s.getItem('vyro.look')).toBeNull();
    expect(s.getItem('other')).toBe('x');
    expect(s.length).toBe(3);
  });

  it('keeps a key already saved under the new name', () => {
    const s = store({ 'vyro.tab': 'cooling', 'clokrix.tab': 'network' });
    expect(renameKeys(s)).toBe(0);
    expect(s.getItem('clokrix.tab')).toBe('network');
    expect(s.getItem('vyro.tab')).toBeNull();
  });

  it('leaves a key it cannot write, for the next start', () => {
    const s = store({ 'vyro.dashboard': '{}' }, true);
    expect(renameKeys(s)).toBe(0);
    expect(s.getItem('vyro.dashboard')).toBe('{}');
  });

  it('does nothing twice', () => {
    const s = store({ 'vyro.rail': '1' });
    renameKeys(s);
    expect(renameKeys(s)).toBe(0);
    expect(s.getItem('clokrix.rail')).toBe('1');
  });
});

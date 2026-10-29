import { describe, expect, it } from 'vitest';
import { arrange, moved, remember } from './card-order.svelte';

describe('card order', () => {
  const VIEW = ['appearance', 'profile', 'general', 'gaming', 'overlay', 'remote'];

  it('keeps the view’s own order until a card is moved', () => {
    expect(arrange([], VIEW)).toEqual(VIEW);
    expect(arrange(['remote', 'appearance', 'profile', 'general', 'gaming', 'overlay'], VIEW)).toEqual([
      'remote', 'appearance', 'profile', 'general', 'gaming', 'overlay',
    ]);
  });

  it('puts a card it has not seen after the card before it in the view', () => {
    // A second drive plugged in after the owner put the first one last.
    expect(arrange(['b', 'a'], ['a', 'a2', 'b'])).toEqual(['b', 'a', 'a2']);
    // One that comes first in the view (Hardware's share bar) comes first.
    expect(arrange(['b', 'a'], ['share', 'a', 'b'])).toEqual(['share', 'b', 'a']);
    // A saved list with a card twice counts it once.
    expect(arrange(['a', 'b', 'a'], ['a', 'b'])).toEqual(['a', 'b']);
  });

  it('moves a card into the place of the one it is dragged over', () => {
    const o = ['a', 'b', 'c', 'd'];
    expect(moved(o, 'a', 2)).toEqual(['b', 'c', 'a', 'd']); // over c, from before: after it
    expect(moved(o, 'd', 1)).toEqual(['a', 'd', 'b', 'c']); // over b, from after: before it
    expect(moved(o, 'b', -3)).toEqual(['b', 'a', 'c', 'd']);
    expect(moved(o, 'b', 99)).toEqual(['a', 'c', 'd', 'b']);
    expect(moved(o, 'x', 0)).toEqual(o);
  });

  it('keeps the place of a card gone away', () => {
    // Drive "usb" unplugged; the owner moves "c" to the front; "usb" stays before "d".
    expect(remember(['a', 'b', 'usb', 'd', 'c'], ['c', 'a', 'b', 'd'])).toEqual(['c', 'a', 'b', 'usb', 'd']);
    // Last of all, it stays last.
    expect(remember(['a', 'b', 'usb'], ['b', 'a'])).toEqual(['b', 'a', 'usb']);
    // And it comes back there.
    expect(arrange(['c', 'a', 'b', 'usb', 'd'], ['a', 'b', 'c', 'd', 'usb'])).toEqual(['c', 'a', 'b', 'usb', 'd']);
  });

  it('remembers at most 32 cards gone away', () => {
    const gone = Array.from({ length: 40 }, (_, i) => `drive${i}`);
    const kept = remember([...gone, 'a'], ['a']);
    expect(kept).toHaveLength(33);
    expect(kept[0]).toBe('drive8');
    expect(kept.at(-1)).toBe('a');
  });
});

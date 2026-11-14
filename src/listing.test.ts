import { describe, expect, it } from 'vitest';
import type { HealthEvent } from './bindings/HealthEvent';
import { eventCategories, moreLabel, newestEvents, preview } from './listing';

const ev = (ts: number, subsystem: string): HealthEvent => ({ ts, subsystem, status: 'warn', message: `${subsystem} ${ts}` });

describe('long lists shown short', () => {
  it('shows the first few until all are asked for', () => {
    const list = [1, 2, 3, 4, 5, 6, 7];
    expect(preview(list, 3, false)).toEqual([1, 2, 3]);
    expect(preview(list, 3, true)).toEqual(list);
    expect(preview([1, 2], 3, false)).toEqual([1, 2]);
  });

  it('offers the rest only when there is more', () => {
    expect(moreLabel(3, 3, false, 'actions')).toBeNull();
    expect(moreLabel(7, 3, false, 'actions')).toBe('Show all 7 actions');
    expect(moreLabel(7, 3, true, 'actions')).toBe('Show fewer');
  });
});

describe("Health's recent events by part", () => {
  const events = [ev(1, 'GPU'), ev(2, 'CPU'), ev(3, 'GPU'), ev(4, 'Fans'), ev(5, 'Something new')];

  it('lists All, then the parts with events in a fixed order, with counts', () => {
    expect(eventCategories(events)).toEqual([
      { id: null, label: 'All', count: 5 },
      { id: 'CPU', label: 'CPU', count: 1 },
      { id: 'GPU', label: 'GPU', count: 2 },
      { id: 'Fans', label: 'Fans', count: 1 },
      { id: 'Something new', label: 'Something new', count: 1 },
    ]);
    expect(eventCategories([])).toEqual([{ id: null, label: 'All', count: 0 }]);
  });

  it('puts the newest first, of one part or all', () => {
    expect(newestEvents(events, null).map((e) => e.ts)).toEqual([5, 4, 3, 2, 1]);
    expect(newestEvents(events, 'GPU').map((e) => e.ts)).toEqual([3, 1]);
    expect(newestEvents(events, 'Memory')).toEqual([]);
    // The list given is not changed.
    expect(events.map((e) => e.ts)).toEqual([1, 2, 3, 4, 5]);
  });
});

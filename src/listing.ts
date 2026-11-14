// Long lists shown short (2.5): the newest few, then "Show all", and Health's events by the part
// of the PC they are about.

import type { HealthEvent } from './bindings/HealthEvent';

/** The first `n` of a list unless all are wanted. */
export function preview<T>(list: readonly T[], n: number, all: boolean): readonly T[] {
  return all || list.length <= n ? list : list.slice(0, n);
}

/** What the "Show all" button says, or null where everything shows anyway. */
export function moreLabel(total: number, n: number, all: boolean, noun: string): string | null {
  if (total <= n) return null;
  return all ? 'Show fewer' : `Show all ${total} ${noun}`;
}

/** Health's parts of the PC (`HealthEvent.subsystem`), in the order their chips show. */
export const EVENT_CATEGORIES = ['CPU', 'GPU', 'Memory', 'Storage', 'Board thermals', 'Fans', 'Voltages'] as const;

export interface Category {
  /** The subsystem, or null for every event. */
  id: string | null;
  label: string;
  count: number;
}

/** "All", then each part with events, in the order above; a part Clokrix doesn't know comes last. */
export function eventCategories(events: readonly HealthEvent[]): Category[] {
  const counts = new Map<string, number>();
  for (const e of events) counts.set(e.subsystem, (counts.get(e.subsystem) ?? 0) + 1);
  const known: readonly string[] = EVENT_CATEGORIES;
  const order = [...known.filter((c) => counts.has(c)), ...[...counts.keys()].filter((c) => !known.includes(c)).sort()];
  return [{ id: null, label: 'All', count: events.length }, ...order.map((id) => ({ id, label: id, count: counts.get(id) ?? 0 }))];
}

/** Newest first, of one part or all. `events` come oldest first, as Clokrix keeps them. */
export function newestEvents(events: readonly HealthEvent[], category: string | null): HealthEvent[] {
  return events.filter((e) => category === null || e.subsystem === category).reverse();
}

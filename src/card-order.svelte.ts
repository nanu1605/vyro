// Card order (2.0.2): each view's cards in the order the owner dragged them into, kept in this
// window's storage as the folded sidebar is. By id, not by place: a card that comes later (another
// drive, a second GPU) goes after the card it follows in the view's own order, and one that goes
// away (an unplugged drive) keeps its place for when it comes back. The sums are pure, so they are
// unit-tested.

const KEY = 'vyro.cards';
/** Cards gone away remembered per view, at most: drives and displays come and go for years. */
const KEEP_GONE = 32;

/** The cards `present`, in the `saved` order; a card not saved goes after the one before it in `present`. */
export function arrange(saved: readonly string[], present: readonly string[]): string[] {
  const out = [...new Set(saved)].filter((id) => present.includes(id));
  present.forEach((id, i) => {
    if (out.includes(id)) return;
    let at = 0;
    for (let j = i - 1; j >= 0; j--) {
      const k = out.indexOf(present[j]!);
      if (k >= 0) {
        at = k + 1;
        break;
      }
    }
    out.splice(at, 0, id);
  });
  return out;
}

/** `order` with `id` at `to` (clamped): past a later card it goes after it, past an earlier one before. */
export function moved(order: readonly string[], id: string, to: number): string[] {
  const out = order.filter((x) => x !== id);
  if (out.length === order.length) return [...order];
  out.splice(Math.max(0, Math.min(to, out.length)), 0, id);
  return out;
}

/** What to keep after the cards shown were put in `order`: that, with the cards not shown where they were. */
export function remember(saved: readonly string[], order: readonly string[]): string[] {
  const gone = [...new Set(saved)].filter((id) => !order.includes(id)).slice(-KEEP_GONE);
  const out = [...order];
  for (const id of gone) {
    const next = saved.slice(saved.indexOf(id) + 1).find((x) => order.includes(x));
    out.splice(next === undefined ? out.length : out.indexOf(next), 0, id);
  }
  return out;
}

function load(): Record<string, string[]> {
  try {
    const all: unknown = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    if (!all || typeof all !== 'object' || Array.isArray(all)) return {};
    return Object.fromEntries(
      Object.entries(all).filter((e): e is [string, string[]] => Array.isArray(e[1]) && e[1].every((x) => typeof x === 'string')),
    );
  } catch {
    return {};
  }
}

/** Each view's saved order; a view never rearranged has none. */
export const cards = $state<{ views: Record<string, string[]> }>({ views: load() });

function keep(): void {
  try {
    if (Object.keys(cards.views).length) localStorage.setItem(KEY, JSON.stringify(cards.views));
    else localStorage.removeItem(KEY);
  } catch {
    /* kept for this session only */
  }
}

/** The view's cards `present`, as the owner put them. */
export function ordered(view: string, present: readonly string[]): string[] {
  return arrange(cards.views[view] ?? [], present);
}

/** The view's cards shown are now in `order`. */
export function saveOrder(view: string, order: readonly string[]): void {
  cards.views[view] = remember(cards.views[view] ?? [], order);
  keep();
}

/** Every view's cards back where Vyro puts them. */
export function resetOrder(): void {
  cards.views = {};
  keep();
}

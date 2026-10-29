// Card order (2.0.2): each view's cards in the order the owner dragged them into, kept in this
// window's storage as the folded sidebar is. By id, not by place: a card that comes later (another
// drive, a second GPU) goes after the card it follows in the view's own order, and one that goes
// away (an unplugged drive) keeps its place for when it comes back. With the order goes the window
// size it was made at (the columns there was room for, and those used): there the cards stay
// exactly as left, and at other sizes card-layout.ts starts from the order. The sums are pure, so
// they are unit-tested.

const KEY = 'vyro.cards';
const AT = 'vyro.cards.at';
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

/** Where a view's cards were last moved: the columns the window had room for, and those used. */
export interface MovedAt {
  fit: number;
  cols: number;
}

function loadAt(): Record<string, MovedAt> {
  try {
    const all: unknown = JSON.parse(localStorage.getItem(AT) ?? '{}');
    if (!all || typeof all !== 'object' || Array.isArray(all)) return {};
    return Object.fromEntries(
      Object.entries(all).filter((e): e is [string, MovedAt] => {
        const { fit, cols } = (e[1] ?? {}) as Partial<MovedAt>;
        return typeof fit === 'number' && typeof cols === 'number' && Number.isInteger(cols) && cols >= 1 && cols <= fit;
      }),
    );
  } catch {
    return {};
  }
}

/** Each view's saved order, and where it was made; a view never rearranged has neither. */
export const cards = $state<{ views: Record<string, string[]>; at: Record<string, MovedAt> }>({ views: load(), at: loadAt() });

function keep(): void {
  try {
    if (Object.keys(cards.views).length) localStorage.setItem(KEY, JSON.stringify(cards.views));
    else localStorage.removeItem(KEY);
    if (Object.keys(cards.at).length) localStorage.setItem(AT, JSON.stringify(cards.at));
    else localStorage.removeItem(AT);
  } catch {
    /* kept for this session only */
  }
}

/** The view's cards `present`, as the owner put them. */
export function ordered(view: string, present: readonly string[]): string[] {
  return arrange(cards.views[view] ?? [], present);
}

/** The view's cards shown are now in `order`, moved with the window as `at` says. */
export function saveOrder(view: string, order: readonly string[], at?: MovedAt): void {
  cards.views[view] = remember(cards.views[view] ?? [], order);
  if (at) cards.at[view] = { fit: at.fit, cols: at.cols };
  keep();
}

/** Every view's cards back where Vyro puts them. */
export function resetOrder(): void {
  cards.views = {};
  cards.at = {};
  keep();
}

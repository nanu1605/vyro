<script lang="ts" module>
  /** Columns a card takes: one, two once the grid has three or more, or all of them. */
  export type CardWidth = 1 | 2 | 'full';
  export interface CardItem {
    id: string;
    title: string;
    w?: CardWidth;
  }
  /** What a card's grip asks of the grid it sits in. */
  export interface GridContext {
    has(id: string): boolean;
    /** The card's name, for its grip ("Move …") and what is said when it moves. */
    title(id: string): string | undefined;
    grab(e: PointerEvent, id: string, grip: HTMLElement): void;
    key(e: KeyboardEvent, id: string, grip: HTMLElement): void;
  }
  export const GRID = Symbol('card-grid');
</script>

<script lang="ts">
  // A view's cards (2.0.2): fitted to the window by card-layout.ts (as many columns as they fill,
  // no gap, one straight last line), in the order the owner put them by dragging a card's grip, or
  // with the arrow keys on it. The cards are moved in the page itself, so the order you read, Tab
  // through and hear is the order you see.
  import { setContext, tick, type Snippet } from 'svelte';
  import { fitCards } from '../card-layout';
  import { arrange, cards, moved, ordered, saveOrder } from '../card-order.svelte';

  interface Props {
    /** Whose order this is. */
    view: string;
    items: CardItem[];
    /** One card, by id. */
    card: Snippet<[string]>;
    /** The narrowest a column gets (22rem). */
    min?: string;
    /** The cards in the order they show in (Settings' side list follows it). */
    arranged?: string[];
  }
  let { view, items, card, min, arranged = $bindable([]) }: Props = $props();

  const byId = $derived(new Map(items.map((i) => [i.id, i])));
  /** The order while a card is dragged, before it is let go. */
  let draft = $state.raw<string[] | null>(null);
  const present = $derived(items.map((i) => i.id));
  /** The owner's order, or the one being dragged: what the cards are fitted from. */
  const base = $derived(draft ? arrange(draft, present) : ordered(view, present));
  /** The order card-layout.ts shows them in, and the columns: in use, and there is room for. */
  let shown = $state.raw<string[]>([]);
  let cols = 0;
  let fit = 0;
  const order = $derived(draft ? base : arrange(shown.length ? shown : base, present));
  let dragging = $state<string | null>(null);
  let said = $state('');
  let grid: HTMLElement;

  function onarrange(next: string[], c: number, f: number): void {
    cols = c;
    fit = f;
    if (next.join() === shown.join()) return;
    // A card moved in the page loses the focus inside it: it comes back.
    const had = document.activeElement instanceof HTMLElement && grid?.contains(document.activeElement) ? document.activeElement : null;
    shown = next;
    arranged = next;
    if (had) void tick().then(() => had.isConnected && document.activeElement !== had && had.focus({ preventScroll: true }));
  }

  const where = (id: string, o: readonly string[]) => `${byId.get(id)?.title ?? 'Card'}: ${o.indexOf(id) + 1} of ${o.length}.`;

  function grab(e: PointerEvent, id: string, grip: HTMLElement): void {
    if (e.button !== 0 || dragging) return;
    const cell = grip.closest<HTMLElement>('.card-cell');
    if (!cell) return;
    e.preventDefault();
    grip.focus();
    const x0 = e.clientX;
    const y0 = e.clientY;
    let x = x0;
    let y = y0;
    let ghost: HTMLElement | null = null;
    let before: string[] = [];
    let dx = 0;
    let dy = 0;
    // The card just moved past: ignored until the pointer leaves it, so two cards don't trade
    // places back and forth under a still pointer.
    let passed: Element | null = null;
    let raf = 0;
    const mine = (ev: PointerEvent) => ev.pointerId === e.pointerId;

    const start = () => {
      // From the order shown: the cards don't move as the drag starts.
      before = [...order];
      const r = cell.getBoundingClientRect();
      dx = x0 - r.left;
      dy = y0 - r.top;
      ghost = cell.cloneNode(true) as HTMLElement;
      ghost.className = `${cell.className} card-ghost`;
      ghost.removeAttribute('data-card');
      ghost.querySelectorAll('[id]').forEach((n) => n.removeAttribute('id'));
      // A copied radio button with its group's name would take the checked mark from the real one.
      ghost.querySelectorAll('[name]').forEach((n) => n.removeAttribute('name'));
      ghost.setAttribute('aria-hidden', 'true');
      ghost.inert = true;
      ghost.style.width = `${r.width}px`;
      ghost.style.setProperty('--fill', '0px');
      document.body.append(ghost);
      follow();
      draft = before;
      dragging = id;
      document.documentElement.classList.add('card-dragging');
      raf = requestAnimationFrame(edge);
    };
    const follow = () => {
      if (ghost) ghost.style.transform = `translate(${x - dx}px, ${y - dy}px)`;
    };
    const over = () => {
      const el = document.elementFromPoint(x, y)?.closest<HTMLElement>('.card-cell');
      if (passed && el !== passed) passed = null;
      if (!el || el === passed || el.parentElement !== grid || el.dataset.card === id || !draft) return;
      const to = draft.indexOf(el.dataset.card ?? '');
      if (to < 0) return;
      passed = el;
      // card-layout.ts puts the cards in the new order, and they glide there.
      draft = moved(draft, id, to);
    };
    // Near the window's top or bottom the page scrolls, faster the closer.
    const edge = () => {
      const band = 72;
      const top = (document.querySelector('.main > header.top')?.getBoundingClientRect().bottom ?? 0) + band;
      const bottom = window.innerHeight - band;
      const v = y < top ? -Math.min(1, (top - y) / band) : y > bottom ? Math.min(1, (y - bottom) / band) : 0;
      if (v) {
        window.scrollBy(0, v * 18);
        over();
      }
      raf = requestAnimationFrame(edge);
    };
    const onMove = (ev: PointerEvent) => {
      if (!mine(ev)) return;
      x = ev.clientX;
      y = ev.clientY;
      if (!ghost) {
        if (Math.hypot(x - x0, y - y0) < 6) return;
        start();
      }
      follow();
      over();
    };
    const end = (keep: boolean) => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
      window.removeEventListener('keydown', onKey, true);
      if (grid.hasPointerCapture(e.pointerId)) grid.releasePointerCapture(e.pointerId);
      cancelAnimationFrame(raf);
      if (!ghost) return;
      ghost.remove();
      document.documentElement.classList.remove('card-dragging');
      const now = draft ?? before;
      const changed = keep && now.join() !== before.join();
      // Saved with the window's size: there the cards stay exactly as left.
      if (changed) saveOrder(view, now, { fit, cols });
      said = changed ? where(id, now) : `${byId.get(id)?.title ?? 'Card'}: not moved.`;
      draft = null;
      dragging = null;
      // Its grip can take the focus once the card shows again.
      void tick().then(() => grip.focus({ preventScroll: true }));
    };
    const onUp = (ev: PointerEvent) => mine(ev) && end(true);
    const onCancel = (ev: PointerEvent) => mine(ev) && end(false);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key !== 'Escape') return;
      ev.preventDefault();
      ev.stopPropagation();
      end(false);
    };
    // Captured by the grid and heard on the window, not the grip: the grip is moved with its card
    // while dragged, which would end its capture, and is hidden in its outlined place.
    grid.setPointerCapture(e.pointerId);
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    window.addEventListener('keydown', onKey, true);
  }

  async function key(e: KeyboardEvent, id: string, grip: HTMLElement): Promise<void> {
    if (dragging || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const now = order;
    const i = now.indexOf(id);
    const to =
      e.key === 'ArrowUp' || e.key === 'ArrowLeft'
        ? i - 1
        : e.key === 'ArrowDown' || e.key === 'ArrowRight'
          ? i + 1
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? now.length - 1
              : null;
    if (to === null || i < 0) return;
    e.preventDefault();
    if (to < 0 || to >= now.length || to === i) {
      said = where(id, now);
      return;
    }
    const next = moved(now, id, to);
    saveOrder(view, next, { fit, cols });
    said = where(id, next);
    await tick();
    // Moving a card can take the focus with it.
    if (document.activeElement !== grip) grip.focus();
  }

  setContext<GridContext>(GRID, { has: (id) => byId.has(id), title: (id) => byId.get(id)?.title, grab, key });
</script>

<div
  class="card-grid"
  bind:this={grid}
  use:fitCards={{ order: base, mine: cards.at[view] ?? null, still: dragging !== null, onarrange }}
  style:--card-min={min}
>
  {#each order as id (id)}
    <div class="card-cell" class:lifted={dragging === id} data-card={id} data-w={String(byId.get(id)?.w ?? 1)}>
      {@render card(id)}
    </div>
  {/each}
</div>
<div class="sr-only" aria-live="polite">{said}</div>

<style>
  /* As many columns as the cards fill (card-layout.ts sets them, never more than fit; this is
     the first paint's), each card as tall as it is and placed in fine rows the way the Dashboard's
     widgets are, then reaching down to the next card (`--fill`, in Card.svelte), so there is no
     gap and the last line is straight. A card is a container: what is inside it can follow its
     width, not the window's. Here rather than in the theme, which the phone pages load too. */
  .card-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, var(--card-min, 22rem)), 1fr));
    grid-auto-rows: 0.25rem;
    column-gap: 1rem;
  }
  .card-cell {
    position: relative;
    align-self: start;
    min-width: 0;
    grid-row: auto / span var(--span, 40);
    container-type: inline-size;
  }
  .card-cell > :global(*) {
    margin: 0;
  }
  /* Being dragged: its place, outlined down to where the card reaches. The card itself rides
     under the pointer. */
  .card-cell.lifted::after {
    content: '';
    position: absolute;
    inset: 0 0 calc(-1 * var(--fill, 0px));
    border-radius: var(--radius);
    outline: 2px dashed color-mix(in srgb, var(--accent) 55%, transparent);
    outline-offset: -2px;
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .card-cell.lifted > :global(*) {
    visibility: hidden;
  }
  /* The top of a tall card is enough to see which it is. A copy of the cell, so over its rules. */
  .card-cell:global(.card-ghost) {
    position: fixed;
    z-index: 50;
    top: 0;
    left: 0;
    max-height: 60vh;
    overflow: hidden;
    border-radius: var(--radius);
    /* Solid: a see-through theme's card would show the page under it. */
    background: var(--bg);
    box-shadow: var(--shadow-lift);
    pointer-events: none;
  }
  :global(:root.card-dragging),
  :global(:root.card-dragging *) {
    cursor: grabbing !important;
    user-select: none;
  }
</style>

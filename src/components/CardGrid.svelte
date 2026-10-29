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
  // A view's cards (2.0.2): packed without gaps (masonry), in the order the owner put them by
  // dragging a card's grip, or with the arrow keys on it. The cards are moved in the page itself,
  // so the order you read, Tab through and hear is the order you see.
  import { setContext, tick, type Snippet } from 'svelte';
  import { animator } from '../anim';
  import { arrange, moved, ordered, saveOrder } from '../card-order.svelte';
  import { pack } from '../packing';

  interface Props {
    /** Whose order this is. */
    view: string;
    items: CardItem[];
    /** One card, by id. */
    card: Snippet<[string]>;
    /** The narrowest a column gets (22rem). */
    min?: string;
  }
  let { view, items, card, min }: Props = $props();

  const byId = $derived(new Map(items.map((i) => [i.id, i])));
  /** The order while a card is dragged, before it is let go. */
  let draft = $state.raw<string[] | null>(null);
  const present = $derived(items.map((i) => i.id));
  const order = $derived(draft ? arrange(draft, present) : ordered(view, present));
  let dragging = $state<string | null>(null);
  let said = $state('');
  let grid: HTMLElement;

  const still = () => animator.snap || document.documentElement.dataset.motion === 'reduced';
  const cellsNow = () => [...grid.querySelectorAll<HTMLElement>(':scope > .card-cell')];

  /** Puts the cards in a new order; the ones that moved glide from where they were (FLIP). */
  async function reorder(apply: () => void): Promise<void> {
    const cells = cellsNow();
    const was = new Map(cells.map((c) => [c, c.getBoundingClientRect()]));
    apply();
    // Svelte moves the cells, then `pack` places them (its observer runs before this resumes).
    await tick();
    if (still()) return;
    for (const c of cells) {
      const a = was.get(c)!;
      const b = c.getBoundingClientRect();
      const dx = a.left - b.left;
      const dy = a.top - b.top;
      if (Math.abs(dx) + Math.abs(dy) < 1) continue;
      c.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], {
        duration: 180,
        easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)',
      });
    }
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
      const next = moved(draft, id, to);
      void reorder(() => (draft = next));
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
      if (changed) saveOrder(view, now);
      said = changed ? where(id, now) : `${byId.get(id)?.title ?? 'Card'}: not moved.`;
      // Its grip can take the focus once the card shows again.
      void reorder(() => {
        draft = null;
        dragging = null;
      }).then(() => grip.focus({ preventScroll: true }));
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
    await reorder(() => saveOrder(view, next));
    said = where(id, next);
    // Moving a card can take the focus with it.
    if (document.activeElement !== grip) grip.focus();
  }

  setContext<GridContext>(GRID, { has: (id) => byId.has(id), title: (id) => byId.get(id)?.title, grab, key });
</script>

<div class="card-grid" bind:this={grid} use:pack={{ cell: 'card-cell', dense: true }} style:--card-min={min}>
  {#each order as id (id)}
    <div class="card-cell" class:lifted={dragging === id} data-card={id} data-w={String(byId.get(id)?.w ?? 1)}>
      {@render card(id)}
    </div>
  {/each}
</div>
<div class="sr-only" aria-live="polite">{said}</div>

<style>
  /* As many columns as fit, each card as tall as it is, packed in order the way the Dashboard's
     widgets are (`pack` sets each one's span and place), so the next card moves up into the room a
     short one leaves. A card is a container: what is inside it can follow its width, not the
     window's. Here rather than in the theme, which the phone pages load too. */
  .card-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, var(--card-min, 22rem)), 1fr));
    grid-auto-rows: 0.25rem;
    column-gap: 1rem;
  }
  .card-cell {
    align-self: start;
    min-width: 0;
    grid-row: auto / span var(--span, 40);
    container-type: inline-size;
  }
  .card-cell > :global(*) {
    margin: 0;
  }
  /* Being dragged: its place, outlined. The card itself rides under the pointer. */
  .card-cell.lifted {
    border-radius: var(--radius);
    outline: 2px dashed color-mix(in srgb, var(--accent) 55%, transparent);
    outline-offset: -2px;
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .card-cell.lifted > :global(*) {
    visibility: hidden;
  }
  /* The top of a tall card is enough to see which it is. */
  :global(.card-ghost) {
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

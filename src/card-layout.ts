// Cards that fill the window (2.0.2): a view's cards in as many columns as they fill, in the owner's
// order where that leaves no gap and a few places out of it where that closes one; then each card
// reaches down to the next one under it, or to the last line, so the grid has no hole and ends in
// one straight line. The grid's columns are set here, never more than fit, so the window never
// scrolls sideways. The sums are pure, so they are unit-tested; `fitCards` measures and places.

import type { Action } from 'svelte/action';
import { animator } from './anim';
import { cardSpan, dense, rowSpan } from './packing';

/** A card as the sums see it: its `data-w` and the grid rows it spans (its height and the gap). */
export interface Box {
  w: string | undefined;
  rows: number;
}

/** Where a card goes, 0-based; `end` is the row after its gap. */
export interface Spot {
  col: number;
  span: number;
  row: number;
  end: number;
}

/** Columns of at least `min` px with `gap` px between them, in `width` px. */
export function fitColumns(width: number, min: number, gap: number): number {
  return Math.max(1, Math.floor((width + gap) / (min + gap)));
}

/** The most columns, up to `fit`, the cards fill, so none is left empty; a card across all counts for none. */
export function mostColumns(boxes: ReadonlyArray<Box>, fit: number): number {
  for (let n = fit; n > 1; n--) if (boxes.reduce((s, b) => s + (b.w === 'full' ? 0 : cardSpan(b.w, n)), 0) >= n) return n;
  return 1;
}

/** The cards in `order` (indices into `boxes`), each where it sits highest (`dense`). */
export function placeCards(boxes: ReadonlyArray<Box>, order: readonly number[], cols: number, slack: number): Spot[] {
  const items = order.map((i) => ({ w: cardSpan(boxes[i]!.w, cols), rows: boxes[i]!.rows }));
  return dense(items, cols, slack).map(({ col, row }, k) => ({ col, span: items[k]!.w, row, end: row + items[k]!.rows }));
}

/**
 * How far each card reaches down (`fill`, rows): to the next card under it in every column it
 * spans, or to the last line. `holes` is the room no card reaches (rows × columns): under a wide
 * card that stops short in one of its columns, or a column with no card.
 */
export function stretch(spots: ReadonlyArray<Spot>, cols: number): { fill: number[]; holes: number; bottom: number } {
  const bottom = spots.reduce((b, s) => Math.max(b, s.end), 0);
  const fill = spots.map((s) => {
    let to = bottom;
    for (let c = s.col; c < s.col + s.span; c++)
      for (const t of spots) if (t !== s && t.col <= c && c < t.col + t.span && t.row >= s.end && t.row < to) to = t.row;
    return to - s.end;
  });
  const covered = spots.reduce((a, s, i) => a + (s.end + fill[i]! - s.row) * s.span, 0);
  return { fill, holes: bottom * cols - covered, bottom };
}

/**
 * The order for `cols` columns that leaves the least room: the grid's height, room no card reaches
 * counting four times, and a row for each place a card is from `boxes`' order. One card moved at a
 * time, at most `reach` places, until no move helps (or `budget` layouts were tried): the same
 * cards always come out the same way.
 */
export function tighten(boxes: ReadonlyArray<Box>, cols: number, slack: number, reach = 4, budget = 1200): number[] {
  const cost = (order: readonly number[]) => {
    const { holes, bottom } = stretch(placeCards(boxes, order, cols, slack), cols);
    return bottom + (4 * holes) / cols + order.reduce((m, id, at) => m + Math.abs(at - id), 0);
  };
  let order = boxes.map((_, i) => i);
  let best = cost(order);
  let tried = 1;
  for (;;) {
    let better: number[] | null = null;
    for (let i = 0; i < order.length; i++)
      for (let j = Math.max(0, i - reach); j <= Math.min(order.length - 1, i + reach); j++) {
        if (j === i || tried >= budget) continue;
        const next = order.slice();
        next.splice(j, 0, ...next.splice(i, 1));
        tried++;
        const c = cost(next);
        if (c < best - 1e-9) {
          best = c;
          better = next;
        }
      }
    if (!better) return order;
    order = better;
  }
}

/** Columns and order for the cards in a grid `width` px wide. */
export interface Plan {
  cols: number;
  order: number[];
}

/**
 * Of the column counts the cards fill (down to two fewer, none wider than three times `min`), the
 * one whose tightened layout the cards cover best: room no card reaches counts three times against
 * it, each column fewer five points (more columns show more at once), columns wider than two and
 * a half times `min` more, and fewer columns must win by more than a point. `rowsAt` gives the
 * cards' rows in a count of columns (they are taller in narrower ones); `boxes`' own by default.
 */
export function planCards(
  boxes: ReadonlyArray<Box>,
  width: number,
  min: number,
  gap: number,
  slack: number,
  rowsAt: (cols: number) => readonly number[] = () => boxes.map((b) => b.rows),
): Plan {
  const most = mostColumns(boxes, fitColumns(width, min, gap));
  let best: (Plan & { score: number }) | null = null;
  for (let n = most; n >= Math.max(1, most - 2); n--) {
    const wide = (width - (n - 1) * gap) / n / min;
    if (n < most && wide > 3) break;
    const rows = rowsAt(n);
    const here = boxes.map((b, i) => ({ w: b.w, rows: rows[i] ?? b.rows }));
    const order = tighten(here, n, slack);
    const spots = placeCards(here, order, n, slack);
    const { holes, bottom } = stretch(spots, n);
    const cards = spots.reduce((a, s) => a + (s.end - s.row) * s.span, 0);
    const score = (cards - 3 * holes) / Math.max(1, bottom * n) - 0.05 * (most - n) - 0.5 * Math.max(0, wide - 2.5);
    if (!best || score > best.score + 0.01) best = { cols: n, order, score };
  }
  return { cols: best?.cols ?? 1, order: best?.order ?? [] };
}

/** What `fitCards` places by. */
export interface FitCards {
  /** The cards' ids in the owner's order. */
  order: readonly string[];
  /** Where the owner last moved a card: the columns the window had room for, and those used. */
  mine: { fit: number; cols: number } | null;
  /** A card is being dragged: the cards go in `order` exactly, in the columns there are. */
  still: boolean;
  /** Told the order the cards show in, the columns and the columns there is room for, on a change. */
  onarrange?: (shown: string[], cols: number, fit: number) => void;
}

/** A CSS length in px: rem, em or px. */
function px(value: string, el: Element): number {
  const n = parseFloat(value);
  if (!Number.isFinite(n)) return NaN;
  if (value.trim().endsWith('rem')) return n * parseFloat(getComputedStyle(document.documentElement).fontSize);
  if (value.trim().endsWith('em')) return n * parseFloat(getComputedStyle(el).fontSize);
  return n;
}

/**
 * On a view's card grid (`.card-cell` children with `data-card` and `data-w`): sets its columns
 * and puts each card in place, as tall as its room (`--fill`). It plans afresh (columns, then
 * order, measuring the cards in each count of columns it weighs) when the view first shows, or
 * shows again with the window or a card's height changed meanwhile; when the window has stopped
 * resizing for 150 ms with room for more or fewer columns than before (until then it only drops
 * a column that no longer fits); when a card comes, goes or changes width; when the owner's order
 * changes; and when a card's height changes a lot before anyone has used the grid (its data
 * arriving). The order alone once more when the cards have their heights in new columns. Never
 * because a card the owner is using grew or shrank: the cards keep their order and only reach
 * down anew. At the window size where the owner last moved a card (`mine`), the cards are exactly
 * as left.
 */
export const fitCards: Action<HTMLElement, FitCards> = (grid, first) => {
  let opts = first;
  let width = 0; // what the plan was made for; 0 until the grid shows
  let hidden = false; // another view shows: plan again on coming back, if anything changed
  let room = 0; // the columns there was room for then
  let chosen = 0; // and the columns it chose
  let cols = 0;
  let shown: string[] = [];
  let made = ''; // the cards, widths, order and arrangement the plan was made from
  let rows = new Map<string, number>(); // each card's rows when planned
  let settle = false; // new columns: plan the order again once the cards have their heights there
  let due = false; // the window stopped resizing
  let used = false; // someone pointed or typed in the grid since the plan
  let timer: ReturnType<typeof setTimeout> | undefined;
  let told = '';

  const cells = () => [...grid.children].filter((c): c is HTMLElement => c instanceof HTMLElement && c.classList.contains('card-cell'));
  const glides = () => !animator.snap && document.documentElement.dataset.motion !== 'reduced';
  const set = (el: HTMLElement, k: string, v: string) => el.style.getPropertyValue(k) !== v && el.style.setProperty(k, v);

  const place = () => {
    const all = cells();
    const w = grid.clientWidth;
    // Hidden (another view shows): nothing to measure.
    if (!all.length || !w) {
      hidden = true;
      return;
    }
    const back = hidden;
    hidden = false;
    const style = getComputedStyle(grid);
    const unit = parseFloat(style.gridAutoRows) || 4;
    const gap = parseFloat(style.columnGap) || 16;
    const min = px(style.getPropertyValue('--card-min'), grid) || px('22rem', grid);
    const slack = Math.ceil(gap / unit);
    const byId = new Map(all.map((c) => [c.dataset.card ?? '', c]));
    const ids = [...opts.order.filter((id) => byId.has(id)), ...[...byId.keys()].filter((id) => !opts.order.includes(id))];
    // Where the cards are, for them to glide from if they move (FLIP); before any is measured.
    const rects = glides() ? new Map(all.map((c) => [c, c.getBoundingClientRect()])) : null;
    // offsetHeight: the card's own height (its reach down is outside its cell), not the rise
    // animation's scaled box.
    let heights = ids.map((id) => byId.get(id)!.offsetHeight);
    const rowsOf = (h: readonly number[]) => h.map((x) => rowSpan(x, unit, gap));
    let boxes = ids.map((id, i) => ({ w: byId.get(id)!.dataset.w, rows: rowSpan(heights[i]!, unit, gap) }));
    // The cards' heights in `n` columns: laid out there for a moment, never painted.
    const seen = new Map<number, number[]>();
    const measure = (n: number) => {
      let h = seen.get(n);
      if (!h) {
        set(grid, 'grid-template-columns', `repeat(${n}, minmax(0, 1fr))`);
        for (const c of all) {
          set(c, 'grid-column', `auto / span ${cardSpan(c.dataset.w, n)}`);
          set(c, 'grid-row-start', 'auto');
        }
        h = ids.map((id) => byId.get(id)!.offsetHeight);
        seen.set(n, h);
      }
      return h;
    };
    const fit = fitColumns(w, min, gap);
    const here = opts.mine?.fit === fit ? opts.mine : null;
    const from = `${ids.map((id, i) => `${id}:${boxes[i]!.w ?? '1'}`).join()}|${opts.mine?.fit}:${opts.mine?.cols}|${opts.still}`;
    const was = { cols, shown: shown.join() };
    const plan = (order: readonly number[]) => order.map((i) => ids[i]!);
    let planned = false;
    if (opts.still) {
      // Dragged: the order being made, exactly, in the columns there are.
      cols = Math.min(cols || fit, fit, mostColumns(boxes, fit));
      shown = ids;
      made = from;
      width = w;
    } else if (
      !width ||
      from !== made ||
      due ||
      shown.length !== ids.length ||
      // A card's data arrived (much taller or shorter) before anyone used the grid.
      (!used && boxes.some((b, i) => Math.abs(b.rows - (rows.get(ids[i]!) ?? b.rows)) > Math.max(40, 0.3 * b.rows))) ||
      // Back from another view: as planned, unless the window or a card's height changed meanwhile.
      (back && (w !== width || boxes.some((b, i) => rows.get(ids[i]!) !== b.rows)))
    ) {
      if (here) {
        cols = Math.min(here.cols, mostColumns(boxes, fit));
        shown = ids;
      } else {
        const p = planCards(boxes, w, min, gap, slack, (n) => rowsOf(measure(n)));
        cols = p.cols;
        shown = plan(p.order);
      }
      settle = !here && cols !== was.cols;
      made = from;
      width = w;
      room = fit;
      chosen = cols;
      due = false;
      used = false;
      planned = true;
    } else if (w !== width) {
      // Resizing: never more columns than fit; plan once it stops, if there is room for more or
      // fewer columns than at the plan (or much more or less width), or a column had to go.
      clearTimeout(timer);
      cols = Math.min(cols, fit);
      if (fit !== room || cols !== chosen || Math.abs(w - width) > 0.1 * width)
        timer = setTimeout(() => {
          due = true;
          place();
        }, 150);
      else width = w;
    } else if (settle) {
      // The cards have their heights in the new columns: the order once more (not the columns,
      // which would change the heights again).
      settle = false;
      if (!here) shown = plan(tighten(boxes, cols, slack));
      planned = true;
    }
    // In new columns, the cards' heights there.
    if (cols !== was.cols || seen.size) {
      heights = measure(cols);
      boxes = ids.map((id, i) => ({ w: byId.get(id)!.dataset.w, rows: rowSpan(heights[i]!, unit, gap) }));
    }
    if (planned) rows = new Map(ids.map((id, i) => [id, boxes[i]!.rows]));
    const order = shown.map((id) => ids.indexOf(id));
    const spots = placeCards(boxes, order, cols, slack);
    const { fill } = stretch(spots, cols);
    // Cards that move glide from where they were; a card that only grew doesn't.
    const moved = cols !== was.cols || shown.join() !== was.shown;
    set(grid, 'grid-template-columns', `repeat(${cols}, minmax(0, 1fr))`);
    spots.forEach((s, k) => {
      const c = byId.get(shown[k]!)!;
      const reach = Math.max(0, (s.end + fill[k]!) * unit - gap - (s.row * unit + heights[order[k]!]!));
      set(c, 'grid-column', `${s.col + 1} / span ${s.span}`);
      set(c, 'grid-row-start', String(s.row + 1));
      set(c, '--span', String(s.end - s.row));
      set(c, '--fill', `${reach}px`);
    });
    if (moved && rects)
      for (const c of all) {
        const a = rects.get(c)!;
        if (!a.width) continue;
        const b = c.getBoundingClientRect();
        const dx = a.left - b.left;
        const dy = a.top - b.top;
        if (Math.abs(dx) + Math.abs(dy) < 1) continue;
        c.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], {
          duration: 180,
          easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)',
        });
      }
    const now = `${shown.join()}|${cols}|${fit}`;
    if (now !== told) {
      told = now;
      opts.onarrange?.(shown, cols, fit);
    }
  };

  const sizes = new ResizeObserver(place);
  for (const c of cells()) sizes.observe(c);
  // The grid's own width (the window, the sidebar), not its height, which follows the placing.
  let across = -1;
  const frame = new ResizeObserver(([e]) => {
    if (!e || e.contentRect.width === across) return;
    across = e.contentRect.width;
    place();
  });
  frame.observe(grid);
  const list = new MutationObserver((records) => {
    for (const r of records) {
      r.removedNodes.forEach((n) => n instanceof Element && sizes.unobserve(n));
      r.addedNodes.forEach((n) => n instanceof HTMLElement && n.classList.contains('card-cell') && sizes.observe(n));
    }
    place();
  });
  list.observe(grid, { childList: true });
  // A card that takes more or fewer columns (a fan header showing its curve) plans again.
  const widths = new MutationObserver(place);
  widths.observe(grid, { subtree: true, attributeFilter: ['data-w'] });
  const use = () => (used = true);
  grid.addEventListener('pointerdown', use, true);
  grid.addEventListener('keydown', use, true);
  return {
    update(next) {
      opts = next;
      place();
    },
    destroy() {
      clearTimeout(timer);
      sizes.disconnect();
      frame.disconnect();
      list.disconnect();
      widths.disconnect();
      grid.removeEventListener('pointerdown', use, true);
      grid.removeEventListener('keydown', use, true);
      grid.style.removeProperty('grid-template-columns');
    },
  };
};

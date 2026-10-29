// Dashboard space (1.2): each widget as tall as it needs, placed in order wherever it sits highest
// (masonry), so the next one moves up into the room a short card leaves, rather than every card in
// a row stretching to the tallest. And rows of equal items laid out evenly across their card. The
// sums are pure, so they are unit-tested.

import type { Action } from 'svelte/action';

/** Grid rows a cell spans: its height and the gap under it, in the grid's fine rows. */
export function rowSpan(height: number, unit: number, gap: number): number {
  // A hair over a whole row is that row: layout heights come with rounding dust.
  return Math.max(1, Math.ceil((height + gap) / unit - 0.01));
}

/**
 * Where each item goes, in order: the columns where it sits highest, the leftmost of those within
 * `slack` rows of the highest (so items of about one height keep reading left to right). `w` is in
 * columns, `rows` in grid rows; the answer is 0-based.
 */
export function skyline(items: ReadonlyArray<{ w: number; rows: number }>, cols: number, slack = 0): Array<{ col: number; row: number }> {
  const sky = new Array<number>(Math.max(1, cols)).fill(0);
  return items.map(({ w, rows }) => {
    const span = Math.min(Math.max(1, w), sky.length);
    const tops: number[] = [];
    for (let c = 0; c + span <= sky.length; c++) tops.push(Math.max(...sky.slice(c, c + span)));
    const best = Math.min(...tops);
    const col = tops.findIndex((t) => t <= best + slack);
    const row = tops[col]!;
    sky.fill(row + rows, col, col + span);
    return { col, row };
  });
}

/**
 * As `skyline`, but an item may also go up into a hole a wider one left (2.0.2, the views' cards):
 * a card two columns wide starts below the taller of the two it spans, and the room under the
 * shorter one is then taken by the next card that fits there. Each goes where it sits highest, the
 * leftmost of those within `slack` rows.
 */
export function dense(items: ReadonlyArray<{ w: number; rows: number }>, cols: number, slack = 0): Array<{ col: number; row: number }> {
  const n = Math.max(1, cols);
  const taken: Array<{ col: number; span: number; row: number; end: number }> = [];
  const free = (col: number, span: number, row: number, rows: number) =>
    taken.every((t) => t.col >= col + span || col >= t.col + t.span || t.row >= row + rows || row >= t.end);
  return items.map(({ w, rows }) => {
    const span = Math.min(Math.max(1, w), n);
    // Where an item can start: the top, or right under one already placed.
    const starts = [...new Set([0, ...taken.map((t) => t.end)])].sort((a, b) => a - b);
    const tops: number[] = [];
    for (let c = 0; c + span <= n; c++) tops.push(starts.find((r) => free(c, span, r, rows))!);
    const best = Math.min(...tops);
    const col = tops.findIndex((t) => t <= best + slack);
    const row = tops[col]!;
    taken.push({ col, span, row, end: row + rows });
    return { col, row };
  });
}

/**
 * Columns for `n` equal items in `width` px, each at least `min` px with `gap` px between: as few
 * rows as fit, then as few columns as fill them, so the last row is never one or two strays.
 */
export function balancedColumns(n: number, width: number, min: number, gap: number): number {
  if (n <= 0) return 1;
  const fit = Math.max(1, Math.floor((width + gap) / (min + gap)));
  const rows = Math.ceil(n / fit);
  return Math.ceil(n / rows);
}

/**
 * Columns a view's card takes (2.0.2), from its `data-w`: `full` every one; `2` two once the grid
 * has three or more (beside one other card, a wide one would leave a gap the height of that one);
 * else one.
 */
export function cardSpan(w: string | undefined, cols: number): number {
  if (w === 'full') return Math.max(1, cols);
  return w === '2' && cols >= 3 ? 2 : 1;
}

/**
 * On the Dashboard's grid, and the views' card grids (2.0.2, `cell: 'card-cell'`, `dense`): places
 * its cells by `skyline` (or `dense`) whenever one changes size or the cells change. A cell sits at
 * the top of its area (`align-self: start`), so its height is its widget's (or the least its h asks
 * for) and never the area's: placing can't change what it measured. A cell with `data-w` (a card)
 * has its span set here too, so it never asks for more columns than the grid has.
 */
export const pack: Action<HTMLElement, { cell?: string; dense?: boolean } | undefined> = (grid, opts) => {
  const cls = opts?.cell ?? 'dash-cell';
  const placer = opts?.dense ? dense : skyline;
  const cells = () => [...grid.children].filter((c): c is HTMLElement => c instanceof HTMLElement && c.classList.contains(cls));
  const place = () => {
    const all = cells();
    // Hidden (another view shows): nothing to measure. Showing it again resizes every cell.
    if (!all.length || !grid.clientWidth) return;
    const style = getComputedStyle(grid);
    const cols = style.gridTemplateColumns.split(' ').filter(Boolean).length;
    const unit = parseFloat(style.gridAutoRows) || 4;
    const gap = parseFloat(style.columnGap) || 12;
    // offsetHeight: the layout height, not the rise animation's scaled box.
    const items = all.map((c) => ({
      w: c.dataset.w === undefined ? Number(/span (\d+)/.exec(getComputedStyle(c).gridColumnEnd)?.[1] ?? 1) : cardSpan(c.dataset.w, cols),
      rows: rowSpan(c.offsetHeight, unit, gap),
    }));
    const at = placer(items, cols, Math.ceil(gap / unit));
    all.forEach((c, i) => {
      const set = (k: string, v: string) => c.style.getPropertyValue(k) !== v && c.style.setProperty(k, v);
      if (c.dataset.w !== undefined) set('grid-column-end', `span ${items[i]!.w}`);
      set('grid-column-start', String(at[i]!.col + 1));
      set('grid-row-start', String(at[i]!.row + 1));
      set('--span', String(items[i]!.rows));
    });
  };
  // The cells only: the grid's own height follows the placing, and watching it would loop.
  const sizes = new ResizeObserver(place);
  for (const c of cells()) sizes.observe(c);
  const list = new MutationObserver((records) => {
    for (const r of records) {
      r.removedNodes.forEach((n) => n instanceof Element && sizes.unobserve(n));
      r.addedNodes.forEach((n) => n instanceof HTMLElement && n.classList.contains(cls) && sizes.observe(n));
    }
    place();
  });
  list.observe(grid, { childList: true });
  // A card that takes more or fewer columns (a fan header showing its curve) places again.
  const widths = new MutationObserver(place);
  widths.observe(grid, { subtree: true, attributeFilter: ['data-w'] });
  return {
    destroy() {
      sizes.disconnect();
      list.disconnect();
      widths.disconnect();
    },
  };
};

import { describe, expect, it } from 'vitest';
import { balancedColumns, cardSpan, dense, rowSpan, skyline } from './packing';

describe('Dashboard packing', () => {
  it('spans the rows a widget is tall, with its gap', () => {
    // 0.25rem rows and a 0.75rem gap at 16 px: one 5rem row is 23 fine rows, two are 46.
    expect(rowSpan(80, 4, 12)).toBe(23);
    expect(rowSpan(172, 4, 12)).toBe(46);
    // Taller by a pixel takes the next row; rounding dust doesn't.
    expect(rowSpan(81, 4, 12)).toBe(24);
    expect(rowSpan(80.02, 4, 12)).toBe(23);
    expect(rowSpan(0, 4, 0)).toBe(1);
  });

  it('moves the next card up into the room a short one leaves', () => {
    // The owner's Dashboard: CPU sensors (short), GPUs (two of them, tall) and Memory side by
    // side, then Power, Board and Vyro itself.
    const at = skyline(
      [
        { w: 4, rows: 50 }, // CPU sensors
        { w: 4, rows: 215 }, // GPUs
        { w: 4, rows: 140 }, // Memory
        { w: 4, rows: 45 }, // Power
        { w: 4, rows: 85 }, // Board
        { w: 4, rows: 110 }, // Vyro itself
      ],
      12,
      3,
    );
    expect(at).toEqual([
      { col: 0, row: 0 },
      { col: 4, row: 0 },
      { col: 8, row: 0 },
      { col: 0, row: 50 }, // under CPU sensors, not under the GPUs
      { col: 0, row: 95 }, // and Board under Power
      { col: 8, row: 140 }, // under Memory
    ]);
  });

  it('keeps tiles of one height in rows, left to right', () => {
    const tiles = Array.from({ length: 9 }, () => ({ w: 3, rows: 23 }));
    expect(skyline(tiles, 12, 3).map((p) => [p.col, p.row])).toEqual([
      [0, 0], [3, 0], [6, 0], [9, 0],
      [0, 23], [3, 23], [6, 23], [9, 23],
      [0, 46],
    ]);
    // Within the slack, order wins over a row or two of height.
    expect(skyline([{ w: 6, rows: 70 }, { w: 6, rows: 68 }, { w: 3, rows: 23 }], 12, 3)[2]).toEqual({ col: 0, row: 70 });
  });

  it('fits anything to the columns there are', () => {
    // A 12-wide card on a phone's one column, and a 4-wide one on six.
    expect(skyline([{ w: 12, rows: 10 }, { w: 4, rows: 5 }], 1)).toEqual([
      { col: 0, row: 0 },
      { col: 0, row: 10 },
    ]);
    expect(skyline([{ w: 4, rows: 5 }, { w: 4, rows: 5 }], 6)).toEqual([
      { col: 0, row: 0 },
      { col: 0, row: 5 },
    ]);
  });

  it('lays a row of thread bars across the whole card', () => {
    // 16 threads on a 12-column card (about 1450 px), bars at least 3.1rem with 0.5rem gaps.
    expect(balancedColumns(16, 1450, 49.6, 8)).toBe(16);
    // On a 4-column card: three rows, the last not a stray two.
    expect(balancedColumns(16, 436, 49.6, 8)).toBe(6);
    expect(balancedColumns(32, 436, 49.6, 8)).toBe(7);
    expect(balancedColumns(24, 720, 49.6, 8)).toBe(12);
  });

  it('splits eight cores four and four rather than seven and one', () => {
    expect(balancedColumns(8, 430, 72, 12)).toBe(4);
    expect(balancedColumns(8, 1400, 72, 12)).toBe(8);
  });

  it('copes with nothing to lay out and no room', () => {
    expect(balancedColumns(0, 500, 50, 8)).toBe(1);
    // No room for two side by side: one column.
    expect(balancedColumns(5, 0, 50, 8)).toBe(1);
    expect(skyline([], 12)).toEqual([]);
  });

  it('gives a view’s card the columns it asks for, never more than the grid has', () => {
    expect(cardSpan(undefined, 5)).toBe(1);
    expect(cardSpan('1', 5)).toBe(1);
    // Two once there are three columns: beside a single other card it would leave a gap.
    expect(cardSpan('2', 5)).toBe(2);
    expect(cardSpan('2', 3)).toBe(2);
    expect(cardSpan('2', 2)).toBe(1);
    expect(cardSpan('2', 1)).toBe(1);
    expect(cardSpan('full', 4)).toBe(4);
    expect(cardSpan('full', 1)).toBe(1);
    expect(cardSpan('full', 0)).toBe(1);
  });

  it('packs Settings at five columns without a stretch under the short cards', () => {
    // The owner's 2.0.1 screenshot: Appearance tall, Profile, General and Gaming short, Remote
    // (two wide) very tall; then Units, Overlay (two wide), Hidden sensors, Service, Updates.
    const rows = [190, 90, 30, 70, 160, 40, 120, 30, 45, 50];
    const w = [1, 1, 1, 1, 2, 1, 2, 1, 1, 1];
    const at = skyline(rows.map((r, i) => ({ w: w[i]!, rows: r })), 5, 4);
    expect(at[4]).toEqual({ col: 2, row: 70 }); // Remote under General and Gaming
    expect(at[5]).toEqual({ col: 4, row: 0 }); // Units up in the free column
    expect(at[6]).toEqual({ col: 0, row: 190 }); // Overlay under Appearance
    // Rows as tall as their tallest card took 190 + 160 + 120 (and more); packed, 310.
    expect(Math.max(...at.map((p, i) => p.row + rows[i]!))).toBe(310);
  });

  it('fills the room a wide card leaves under a shorter one', () => {
    // As above, packed the views' way: Hidden sensors goes up under General, where Remote (two
    // wide, below Gaming) left room, instead of last in the fifth column.
    const rows = [190, 90, 30, 70, 160, 40, 120, 30, 45, 50];
    const w = [1, 1, 1, 1, 2, 1, 2, 1, 1, 1];
    const at = dense(rows.map((r, i) => ({ w: w[i]!, rows: r })), 5, 4);
    expect(at[7]).toEqual({ col: 2, row: 30 });
    // Nothing on top of anything.
    for (let i = 0; i < at.length; i++)
      for (let j = i + 1; j < at.length; j++) {
        const [a, b] = [at[i]!, at[j]!];
        const across = a.col < b.col + w[j]! && b.col < a.col + w[i]!;
        const down = a.row < b.row + rows[j]! && b.row < a.row + rows[i]!;
        expect(across && down, `${i} and ${j}`).toBe(false);
      }
    // Without wide cards it places as skyline does.
    const one = rows.map((r) => ({ w: 1, rows: r }));
    expect(dense(one, 4, 4)).toEqual(skyline(one, 4, 4));
  });
});

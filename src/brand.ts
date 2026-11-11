// The Clokrix logo's own shapes (the owner's artwork, assets/brand/clokrix-logo.svg, 2026-11-11),
// in its 640 x 200 units: the X and the word, each drawn three times, magenta and cyan offset
// either way and the main strokes on top. The app draws them itself (Logo.svelte, the share card)
// so that the main strokes follow the theme: white, or near-black on a light one.

export const MAGENTA = '#FF2E93';
export const CYAN = '#22D3EE';
/** The main strokes on a dark theme and on a light one (the owner's light logo). */
export const INK_DARK = '#FFFFFF';
export const INK_LIGHT = '#15151F';
/** Each colour layer's opacity. */
export const LAYER = 0.85;

/** The X, around (0, 0), placed at `at` and `scale`; the colour layers move by `shift` each way. */
export const MARK = {
  d: 'M-58,-58 L58,58 M58,-58 L-58,58',
  at: [104, 95],
  scale: 0.72,
  width: 30,
  shift: [7, 5],
} as const;

/** The seven letters, slanted by `skew` degrees, placed at `at` and `scale`. */
export const WORD = {
  d:
    'M48,4 L16,4 L4,16 L4,50 L16,62 L48,62 M68,4 L68,62 L108,62 ' +
    'M124,16 L136,4 L160,4 L172,16 L172,50 L160,62 L136,62 L124,50 Z M192,4 L192,62 ' +
    'M234,4 L194,34 M208,27 L236,62 M258,62 L258,4 L290,4 L298,12 L298,28 L290,36 L258,36 ' +
    'M278,36 L300,62 M314,4 L330,4 M322,4 L322,62 M314,62 L330,62 M350,4 L390,62 ' +
    'M390,4 L350,62',
  at: [215, 67],
  scale: 0.9,
  width: 11,
  skew: -10,
  shift: [5, 3],
} as const;

/** What a logo shows, in its units (x, y, width, height): the X alone, the X and the word, and
 * those with the pixel scatter. */
export const VIEW = {
  mark: [48, 39, 112, 112],
  logo: [48, 40, 528, 110],
  scatter: [48, 40, 534, 116],
} as const;

/** Squares as x, y, size, colour (0 the main strokes' ink, 1 cyan, 2 magenta) and opacity in %. */
export type Pixel = readonly [number, number, number, number, number];
/** The pixel scatter (brand-scatter.ts), in the X's and the word's own units. */
export interface Scatter {
  mark: readonly Pixel[];
  word: readonly Pixel[];
}

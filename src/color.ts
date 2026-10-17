// Colour maths (1.3): WCAG contrast, OKLCH for an accent of any colour, and HSV for the colour
// wheel. A custom accent keeps its hue and chroma; only its lightness moves, and only as far as
// text in it needs to stay readable.

type Rgba = [number, number, number, number];

/** `#rrggbb` or `rgb(r g b / a%)`; `transparent` is fully clear. */
export function parseColor(c: string): Rgba {
  if (c === 'transparent') return [0, 0, 0, 0];
  const hex = /^#([0-9a-f]{6})$/i.exec(c);
  if (hex) {
    const n = parseInt(hex[1]!, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  const rgb = /^rgb\((\d+) (\d+) (\d+)(?: \/ (\d+)%)?\)$/.exec(c);
  if (rgb) return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), rgb[4] != null ? Number(rgb[4]) / 100 : 1];
  throw new Error(`unknown colour ${c}`);
}

const hex2 = (v: number) =>
  Math.round(Math.min(255, Math.max(0, v)))
    .toString(16)
    .padStart(2, '0');

export const toHex = (r: number, g: number, b: number): string => `#${hex2(r)}${hex2(g)}${hex2(b)}`;

/** `#rrggbb`, lowercase: what a custom accent is stored as. */
export const isHex = (s: string): boolean => /^#[0-9a-f]{6}$/.test(s);

/** `top` over an opaque `under`. */
export function over(top: string, under: string): string {
  const [r, g, b, a] = parseColor(top);
  const [r2, g2, b2] = parseColor(under);
  const mix = (x: number, y: number) => x * a + y * (1 - a);
  return toHex(mix(r, r2), mix(g, g2), mix(b, b2));
}

const lin = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const gam = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);

/** WCAG 2 relative luminance (its own 0.03928 knee, as the contrast tests have always used). */
function luminance(c: string): number {
  const ch = (v: number) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  const [r, g, b] = parseColor(c).map((v, i) => (i < 3 ? ch(v / 255) : v)) as Rgba;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio of two opaque colours, 1–21. */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p) as [number, number];
  return (x + 0.05) / (y + 0.05);
}

// OKLab / OKLCH (Björn Ottosson), on linear sRGB.

export interface Lch {
  /** 0–1 */
  l: number;
  c: number;
  /** Degrees. */
  h: number;
}

export function toLch(hex: string): Lch {
  const [r, g, b] = parseColor(hex)
    .slice(0, 3)
    .map((v) => lin(v / 255)) as [number, number, number];
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return { l: L, c: Math.hypot(A, B), h: ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360 };
}

/** Linear sRGB of an OKLCH colour; may be outside 0–1. */
function linear({ l: L, c, h }: Lch): [number, number, number] {
  const A = c * Math.cos((h * Math.PI) / 180);
  const B = c * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

const inGamut = (rgb: number[]) => rgb.every((v) => v >= -1e-4 && v <= 1 + 1e-4);

/** An OKLCH colour as `#rrggbb`, with its chroma lowered until it fits sRGB. */
export function fromLch(x: Lch): string {
  let lo = 0;
  let hi = x.c;
  let rgb = linear(x);
  if (!inGamut(rgb)) {
    for (let i = 0; i < 24; i++) {
      const mid = (lo + hi) / 2;
      if (inGamut(linear({ ...x, c: mid }))) lo = mid;
      else hi = mid;
    }
    rgb = linear({ ...x, c: lo });
  }
  const [r, g, b] = rgb.map((v) => gam(Math.min(1, Math.max(0, v))) * 255) as [number, number, number];
  return toHex(r, g, b);
}

/** The least contrast `c` has on any of `surfaces`. */
const worst = (c: string, surfaces: string[]) => Math.min(...surfaces.map((s) => contrast(c, s)));

/**
 * `color` as text on `surfaces`: unchanged when it reaches `min` on every one; otherwise the
 * same hue and chroma, lightened (dark surfaces) or darkened (light ones) just enough.
 */
export function readableOn(color: string, surfaces: string[], min = 4.5): string {
  if (worst(color, surfaces) >= min) return color;
  const lch = toLch(color);
  const dark = surfaces.every((s) => luminance(s) < 0.18);
  // Bisect the lightness between the colour's own and the end that reaches the most contrast.
  let ok = dark ? 1 : 0;
  let bad = lch.l;
  for (let i = 0; i < 24; i++) {
    const mid = (ok + bad) / 2;
    if (worst(fromLch({ ...lch, l: mid }), surfaces) >= min) ok = mid;
    else bad = mid;
  }
  return fromLch({ ...lch, l: ok });
}

/** A colour turned `deg` degrees round the OKLCH hue circle, at the same lightness and chroma. */
export const turn = (hex: string, deg: number): string => {
  const x = toLch(hex);
  return fromLch({ ...x, h: (x.h + deg) % 360 });
};

// HSV, for the colour wheel: hue round it, saturation out from the middle, value on a slider.

export interface Hsv {
  /** Degrees. */
  h: number;
  /** 0–1 */
  s: number;
  v: number;
}

export function toHsv(hex: string): Hsv {
  const [r, g, b] = parseColor(hex)
    .slice(0, 3)
    .map((x) => x / 255) as [number, number, number];
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  let h = 0;
  if (d > 0) h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return { h: (h * 60 + 360) % 360, s: max === 0 ? 0 : d / max, v: max };
}

export function fromHsv({ h, s, v }: Hsv): string {
  const f = (n: number) => {
    const k = (n + h / 60) % 6;
    return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
  };
  return toHex(f(5) * 255, f(3) * 255, f(1) * 255);
}

// Shareable PC cards (P25): the specs and the Performance Score as a PNG, in three sizes and three
// styles. The model takes only the fields named here from the inventory (never a serial
// number or the computer's name), and the layout is pure so the tests can check every size;
// `paint` draws it on a canvas with the theme's colours.

import { BANDS } from './score';
import { parseColor } from './themes';
import type { DriveInfo, HardwareId, OsInfo, PerformanceScore } from './types';

export type CardSize = 'wide' | 'square' | 'story';
export type CardStyle = 'full' | 'minimal' | 'transparent';

export const SIZES: Record<CardSize, { label: string; w: number; h: number }> = {
  wide: { label: '16:9', w: 1920, h: 1080 },
  square: { label: 'Square', w: 1080, h: 1080 },
  story: { label: 'Story', w: 1080, h: 1920 },
};

export const STYLES: Record<CardStyle, { label: string; hint: string }> = {
  full: { label: 'Full', hint: 'Every spec on the theme background' },
  minimal: { label: 'Minimal', hint: 'CPU, GPU, RAM and the score' },
  transparent: { label: 'Transparent', hint: 'No background, for streams and wallpapers' },
};

export const TITLE_MAX = 40;
export const DEFAULT_TITLE = 'My PC';
export const POWERED_BY = 'Powered by Vyro';

export interface CardLine {
  label: string;
  value: string;
}

export interface CardModel {
  title: string;
  lines: CardLine[];
  score: { value: number; band: string } | null;
}

/** "2 TB", "512 GB": drives are sold in decimal sizes. */
export function driveSize(mib: number): string {
  const bytes = mib * 1024 * 1024;
  if (bytes >= 1e12) return `${Math.round(bytes / 1e11) / 10} TB`;
  return `${Math.round(bytes / 1e9)} GB`;
}

/** "NVMe", "SSD" or "HDD", as people name drives. */
function driveKind(d: DriveInfo): string {
  if (d.type === 'NVMe') return 'NVMe';
  if (d.ssd != null) return d.ssd ? 'SSD' : 'HDD';
  return d.type === 'other' ? '' : d.type;
}

const clean = (s: string | null | undefined) => (s ?? '').replace(/\s+/g, ' ').trim();

/** "AMD Ryzen 7 9800X3D 8-Core Processor" → "AMD Ryzen 7 9800X3D"; drops (R) and (TM). */
export function shortName(s: string | null | undefined): string {
  return clean(
    clean(s)
      .replace(/\((R|TM|C)\)/gi, '')
      .replace(/\s+CPU\s+@.*$/i, '')
      .replace(/\s+\d+-Core Processor$/i, '')
      .replace(/\s+Processor$/i, ''),
  );
}

/** The card's text: only these inventory fields, and the score if there is one. */
export function cardModel(
  hw: HardwareId | null,
  os: OsInfo | null,
  drives: DriveInfo[],
  score: PerformanceScore | null,
  opts: { title: string; style: CardStyle; withScore: boolean },
): CardModel {
  const lines: CardLine[] = [];
  const add = (label: string, value: string) => {
    if (value) lines.push({ label, value });
  };
  const cpu = hw?.cpu;
  const cores = opts.style !== 'minimal' && cpu?.cores != null && cpu.threads != null ? `${cpu.cores}C/${cpu.threads}T` : '';
  add('CPU', [shortName(cpu?.brand), cores].filter(Boolean).join(' · '));
  // A graphics card hides the processor's own graphics.
  const gpus = hw?.gpus ?? [];
  const cards = gpus.filter((g) => (g.vramMB ?? 0) >= 2048);
  for (const g of cards.length ? cards : gpus) {
    const vram = g.vramMB != null && g.vramMB >= 1024 ? `${Math.round(g.vramMB / 1024)} GB` : '';
    add('GPU', [shortName(g.name), vram].filter(Boolean).join(' · '));
  }
  const mem = hw?.memory;
  if (mem?.totalMb != null) {
    const dimm = mem.dimms[0];
    const speed = dimm?.configuredSpeedMts ?? dimm?.speedMts;
    add('RAM', [`${Math.round(mem.totalMb / 1024)} GB`, clean(dimm?.memoryType), speed ? `${speed} MT/s` : ''].filter(Boolean).join(' · '));
  }
  if (opts.style !== 'minimal') {
    add('Motherboard', [clean(hw?.board.manufacturer), clean(hw?.board.product)].filter(Boolean).join(' '));
    const fixed = drives.filter((d) => !d.removable && d.capacityMB != null && d.capacityMB > 0);
    add('Storage', fixed.map((d) => [driveSize(d.capacityMB!), driveKind(d)].filter(Boolean).join(' ')).join(' + '));
    add('Windows', [clean(os?.name), clean(os?.displayVersion)].filter(Boolean).join(' '));
  }
  const s = opts.withScore && score?.score != null && score.band ? { value: Math.round(score.score), band: BANDS[score.band].label } : null;
  return { title: clean(opts.title).slice(0, TITLE_MAX) || DEFAULT_TITLE, lines, score: s };
}

// Layout: a list of drawing steps in pixels. Colours are roles, resolved by `paint`.

export type Role = 'text' | 'muted' | 'accent' | 'second';

export type Op =
  | { t: 'background' }
  | { t: 'panel'; x: number; y: number; w: number; h: number; r: number }
  | { t: 'text'; x: number; y: number; text: string; size: number; weight: number; role: Role; align: 'left' | 'center' | 'right'; spacing?: number }
  | { t: 'ring'; cx: number; cy: number; r: number; width: number; value: number };

/** Text width in pixels at a size and weight (the canvas in the app, a stand-in in tests). */
export type Measure = (text: string, size: number, weight: number) => number;

/** Shortens text with "…" until it fits. */
export function fit(text: string, size: number, weight: number, max: number, measure: Measure): string {
  if (measure(text, size, weight) <= max) return text;
  let s = text;
  while (s.length > 1 && measure(`${s}…`, size, weight) > max) s = s.slice(0, -1).trimEnd();
  return `${s}…`;
}

/** Biggest size from `sizes` at which the text fits, else the smallest, shortened. */
function fitSize(text: string, sizes: number[], weight: number, max: number, measure: Measure): [string, number] {
  for (const size of sizes) if (measure(text, size, weight) <= max) return [text, size];
  const size = sizes[sizes.length - 1]!;
  return [fit(text, size, weight, max, measure), size];
}

export function layout(model: CardModel, size: CardSize, style: CardStyle, measure: Measure): Op[] {
  const { w, h } = SIZES[size];
  const ops: Op[] = [];
  const pad = size === 'wide' ? 96 : 80;
  if (style !== 'transparent') ops.push({ t: 'background' });
  if (style === 'full') ops.push({ t: 'panel', x: pad / 2, y: pad / 2, w: w - pad, h: h - pad, r: 36 });
  const inner = pad + (style === 'full' ? 24 : 0);
  const big = style === 'minimal' ? 1.15 : 1;

  // Header: wordmark, then the owner's title.
  ops.push({ t: 'text', x: inner, y: inner + 30, text: 'VYRO', size: 30, weight: 700, role: 'accent', align: 'left', spacing: 10 });

  // The score ring: right of the specs on 16:9, beside the title on square, centred on story.
  const ringR = size === 'wide' ? 190 : size === 'square' ? 105 : 210;
  const ring =
    model.score &&
    (size === 'wide'
      ? { cx: w - inner - ringR, cy: h / 2 - 20 }
      : size === 'square'
        ? { cx: w - inner - ringR, cy: inner + ringR }
        : { cx: w / 2, cy: inner + 300 + ringR });
  const titleMax = (ring && size !== 'story' ? ring.cx - ringR - 60 : w - inner) - inner;
  const [title, titleSize] = fitSize(model.title, [88, 76, 64, 54].map((s) => Math.round(s * big)), 700, titleMax, measure);
  ops.push({ t: 'text', x: inner, y: inner + 60 + titleSize, text: title, size: titleSize, weight: 700, role: 'text', align: 'left' });

  if (ring && model.score) {
    ops.push({ t: 'ring', cx: ring.cx, cy: ring.cy, r: ringR, width: Math.round(ringR * 0.13), value: model.score.value });
    ops.push({ t: 'text', x: ring.cx, y: ring.cy + ringR * 0.22, text: String(model.score.value), size: Math.round(ringR * 0.62), weight: 700, role: 'text', align: 'center' });
    ops.push({ t: 'text', x: ring.cx, y: ring.cy + ringR * 0.72, text: model.score.band, size: Math.round(ringR * 0.17), weight: 600, role: 'muted', align: 'center' });
    const caption = size === 'square' ? 20 : 24;
    ops.push({ t: 'text', x: ring.cx, y: ring.cy + ringR + caption * 2, text: 'PERFORMANCE SCORE', size: caption, weight: 600, role: 'muted', align: 'center', spacing: size === 'square' ? 2 : 3 });
  }

  // The specs, one label and value per line.
  const footer = h - inner;
  // Square cards start the specs below the ring, so every line has the full width.
  const below = ring && size === 'square' ? ring.cy + ringR + 110 : 0;
  const top = size === 'story' ? (ring ? ring.cy + ringR + 150 : inner + 300) : Math.max(inner + 60 + titleSize + 70, below);
  const right = size === 'wide' && ring ? ring.cx - ringR - 80 : w - inner;
  const lines = model.lines;
  const room = footer - 90 - top;
  const step = Math.min(Math.round(150 * big), lines.length ? Math.floor(room / lines.length) : 0);
  const valueSize = Math.max(26, Math.min(Math.round(46 * big), Math.round(step * 0.4)));
  const labelSize = Math.max(18, Math.round(valueSize * 0.55));
  lines.forEach((l, i) => {
    const y = top + i * step;
    // A long value gets a little smaller before it is shortened.
    const [value, px] = fitSize(l.value, [1, 0.9, 0.8, 0.72].map((f) => Math.round(valueSize * f)), 600, right - inner, measure);
    ops.push({ t: 'text', x: inner, y, text: l.label.toUpperCase(), size: labelSize, weight: 600, role: 'muted', align: 'left', spacing: 3 });
    ops.push({ t: 'text', x: inner, y: y + valueSize + 12, text: value, size: px, weight: 600, role: 'text', align: 'left' });
  });

  ops.push({ t: 'text', x: inner, y: footer, text: POWERED_BY, size: 28, weight: 600, role: 'muted', align: 'left' });
  return ops;
}

export interface CardPalette {
  bg: string;
  card: string;
  cardTop: string;
  line: string;
  text: string;
  muted: string;
  accent: string;
  second: string;
  track: string;
  glow: [string, string];
  /** Light theme: transparent cards get a light halo instead of a dark shadow. */
  light: boolean;
}

const FONT = "'Segoe UI Variable Display', 'Segoe UI', system-ui, sans-serif";
export const font = (size: number, weight: number) => `${weight} ${size}px ${FONT}`;

export function paint(ctx: CanvasRenderingContext2D, ops: Op[], size: CardSize, p: CardPalette, transparent: boolean): void {
  const { w, h } = SIZES[size];
  ctx.clearRect(0, 0, w, h);
  const color: Record<Role, string> = { text: p.text, muted: p.muted, accent: p.accent, second: p.second };
  for (const op of ops) {
    ctx.save();
    if (op.t === 'background') {
      ctx.fillStyle = p.bg;
      ctx.fillRect(0, 0, w, h);
      p.glow.forEach((g, i) => {
        if (g === 'transparent') return;
        const x = i === 0 ? w * 0.2 : w * 0.9;
        const grad = ctx.createRadialGradient(x, 0, 0, x, 0, Math.max(w, h) * 0.6);
        // Fade to the same colour, see-through: fading to clear black greys light themes.
        const [r, gr, b] = parseColor(g);
        grad.addColorStop(0, g);
        grad.addColorStop(1, `rgb(${r} ${gr} ${b} / 0%)`);
        ctx.fillStyle = grad;
        ctx.fillRect(0, 0, w, h);
      });
    } else if (op.t === 'panel') {
      const grad = ctx.createLinearGradient(0, op.y, 0, op.y + op.h * 0.25);
      grad.addColorStop(0, p.cardTop);
      grad.addColorStop(1, p.card);
      ctx.fillStyle = grad;
      ctx.strokeStyle = p.line;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.roundRect(op.x, op.y, op.w, op.h, op.r);
      ctx.fill();
      ctx.stroke();
    } else if (op.t === 'ring') {
      const start = 0.75 * Math.PI;
      const sweep = 1.5 * Math.PI;
      ctx.lineWidth = op.width;
      ctx.lineCap = 'round';
      ctx.strokeStyle = p.track;
      ctx.beginPath();
      ctx.arc(op.cx, op.cy, op.r, start, start + sweep);
      ctx.stroke();
      const grad = ctx.createLinearGradient(op.cx - op.r, op.cy, op.cx + op.r, op.cy);
      grad.addColorStop(0, p.accent);
      grad.addColorStop(1, p.second);
      ctx.strokeStyle = grad;
      ctx.beginPath();
      ctx.arc(op.cx, op.cy, op.r, start, start + (sweep * Math.max(0, Math.min(100, op.value))) / 100);
      ctx.stroke();
    } else {
      ctx.font = font(op.size, op.weight);
      ctx.fillStyle = color[op.role];
      ctx.textAlign = op.align;
      ctx.textBaseline = 'alphabetic';
      if (op.spacing) ctx.letterSpacing = `${op.spacing}px`;
      if (transparent) {
        ctx.shadowColor = p.light ? 'rgb(255 255 255 / 85%)' : 'rgb(0 0 0 / 70%)';
        ctx.shadowBlur = 12;
      }
      ctx.fillText(op.text, op.x, op.y);
    }
    ctx.restore();
  }
}

/** "vyro-card-20261015-1024-16x9.png". */
export function fileName(stamp: string, size: CardSize, style: CardStyle): string {
  const tag = size === 'wide' ? '16x9' : size === 'square' ? '1x1' : 'story';
  return `vyro-card-${stamp}-${tag}${style === 'full' ? '' : `-${style}`}.png`;
}

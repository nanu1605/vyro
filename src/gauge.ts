// Canvas speedometer: utilization on a 270° dial with the current clock shown prominently.
//
// The track, ticks and labels never change between frames, so they are rendered once per
// resize into an offscreen layer; each frame blits that layer and draws only the value arc,
// pointer and readouts.

import { NA, fmt } from './format';

const START = 0.75 * Math.PI; // bottom-left
const SWEEP = 1.5 * Math.PI; // clockwise to bottom-right

/** Canvas height as a fraction of its width; the dial's open bottom needs no square. */
export const GAUGE_ASPECT = 0.86;

export interface GaugeOptions {
  label: string;
  max?: number;
}

interface Palette {
  track: string;
  tick: string;
  text: string;
  muted: string;
  /** Text in the accent (the readable shade). */
  accent: string;
  /** The arc: the accent as picked (1.3). */
  fill: string;
  warn: string;
  hot: string;
  font: string;
}

function readPalette(el: Element): Palette {
  const cs = getComputedStyle(el);
  const v = (name: string, fallback: string) => cs.getPropertyValue(name).trim() || fallback;
  return {
    track: v('--gauge-track', '#1c2530'),
    tick: v('--gauge-tick', '#3a4758'),
    text: v('--text', '#e6edf3'),
    muted: v('--muted', '#8b98a8'),
    accent: v('--accent', '#4cc9f0'),
    fill: v('--accent-fill', '') || v('--accent', '#4cc9f0'),
    warn: v('--warn', '#f5b942'),
    hot: v('--hot', '#ff5d5d'),
    font: v('--font-num', 'system-ui, sans-serif'),
  };
}

export class Gauge {
  private readonly ctx: CanvasRenderingContext2D;
  private readonly layer: HTMLCanvasElement;
  private readonly max: number;
  private size = 0;
  private height = 0;
  private dpr = 1;
  private palette: Palette;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly opts: GaugeOptions,
  ) {
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('2D canvas unavailable');
    this.ctx = ctx;
    this.layer = document.createElement('canvas');
    this.max = opts.max ?? 100;
    this.palette = readPalette(canvas);
    this.resize();
  }

  /** Re-reads CSS size, device pixel ratio and theme, and rebuilds the static layer. */
  resize(): void {
    const rect = this.canvas.getBoundingClientRect();
    this.size = Math.max(1, rect.width);
    this.height = Math.max(1, rect.height);
    this.dpr = window.devicePixelRatio || 1;
    for (const c of [this.canvas, this.layer]) {
      c.width = Math.round(this.size * this.dpr);
      c.height = Math.round(this.height * this.dpr);
    }
    this.palette = readPalette(this.canvas);
    this.drawStatic();
  }

  private geometry() {
    const s = this.size;
    return { s, cx: s / 2, cy: s / 2, r: s * 0.38, w: s * 0.045 };
  }

  private drawStatic(): void {
    const g = this.layer.getContext('2d');
    if (!g) return;
    const { s, cx, cy, r, w } = this.geometry();
    const p = this.palette;
    g.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    g.clearRect(0, 0, s, this.height);

    g.lineCap = 'round';
    g.lineWidth = w;
    g.strokeStyle = p.track;
    g.beginPath();
    g.arc(cx, cy, r, START, START + SWEEP);
    g.stroke();

    g.lineCap = 'butt';
    g.textAlign = 'center';
    g.textBaseline = 'middle';
    g.font = `500 ${Math.round(s * 0.042)}px ${p.font}`;
    const inner = r - w * 0.95;
    for (let i = 0; i <= 50; i++) {
      const a = START + (i / 50) * SWEEP;
      const major = i % 5 === 0;
      const len = major ? w * 0.75 : w * 0.35;
      const cos = Math.cos(a);
      const sin = Math.sin(a);
      g.lineWidth = major ? 2 : 1;
      g.strokeStyle = p.tick;
      g.beginPath();
      g.moveTo(cx + cos * inner, cy + sin * inner);
      g.lineTo(cx + cos * (inner - len), cy + sin * (inner - len));
      g.stroke();
      if (i % 10 === 0) {
        const lr = r + w * 1.6;
        g.fillStyle = p.muted;
        g.fillText(String((i / 50) * this.max), cx + cos * lr, cy + sin * lr);
      }
    }
  }

  /** Renders one frame. `null` values render as unavailable. */
  draw(value: number | null, mhz: number | null): void {
    const ctx = this.ctx;
    const { s, cx, cy, r, w } = this.geometry();
    const p = this.palette;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
    ctx.drawImage(this.layer, 0, 0);
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);

    const frac = value == null ? 0 : Math.min(Math.max(value / this.max, 0), 1);
    if (value != null && frac > 0.002) {
      const end = START + frac * SWEEP;
      const grad = ctx.createConicGradient(START, cx, cy);
      grad.addColorStop(0, p.fill);
      grad.addColorStop(0.75 * 0.6, p.fill);
      grad.addColorStop(0.75 * 0.85, p.warn);
      grad.addColorStop(0.75, p.hot);
      // Past the sweep the gradient wraps back to START, where the round cap of a small arc
      // sits; return to the accent colour there.
      grad.addColorStop(0.8, p.hot);
      grad.addColorStop(0.9, p.fill);
      grad.addColorStop(1, p.fill);
      ctx.lineCap = 'round';
      ctx.strokeStyle = grad;
      // Soft halo, then the arc itself.
      ctx.globalAlpha = 0.18;
      ctx.lineWidth = w * 2.2;
      ctx.beginPath();
      ctx.arc(cx, cy, r, START, end);
      ctx.stroke();
      ctx.globalAlpha = 1;
      ctx.lineWidth = w;
      ctx.beginPath();
      ctx.arc(cx, cy, r, START, end);
      ctx.stroke();

      // Pointer: a blade between the readouts and the tick ring.
      const cos = Math.cos(end);
      const sin = Math.sin(end);
      const tip = r - w * 1.1;
      ctx.lineWidth = Math.max(2, s * 0.012);
      ctx.strokeStyle = p.text;
      ctx.beginPath();
      ctx.moveTo(cx + cos * r * 0.56, cy + sin * r * 0.56);
      ctx.lineTo(cx + cos * tip, cy + sin * tip);
      ctx.stroke();
    }

    ctx.textAlign = 'center';
    ctx.textBaseline = 'alphabetic';

    ctx.fillStyle = p.muted;
    ctx.font = `600 ${Math.round(s * 0.05)}px ${p.font}`;
    ctx.fillText(this.opts.label.toUpperCase(), cx, cy - r * 0.3);

    const text = value == null ? NA : fmt(value);
    const unit = value == null ? '' : '%';
    ctx.font = `600 ${Math.round(s * 0.17)}px ${p.font}`;
    const tw = ctx.measureText(text).width;
    ctx.font = `500 ${Math.round(s * 0.07)}px ${p.font}`;
    const uw = ctx.measureText(unit).width;
    const x0 = cx - (tw + uw) / 2;
    ctx.textAlign = 'left';
    ctx.fillStyle = p.text;
    ctx.font = `600 ${Math.round(s * 0.17)}px ${p.font}`;
    ctx.fillText(text, x0, cy + s * 0.06);
    ctx.fillStyle = p.muted;
    ctx.font = `500 ${Math.round(s * 0.07)}px ${p.font}`;
    ctx.fillText(unit, x0 + tw, cy + s * 0.06);

    ctx.textAlign = 'center';
    ctx.fillStyle = mhz == null ? p.muted : p.accent;
    ctx.font = `600 ${Math.round(s * 0.075)}px ${p.font}`;
    ctx.fillText(`${fmt(mhz)} MHz`, cx, cy + r * 0.62);
  }
}

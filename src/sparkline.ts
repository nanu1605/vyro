/** Fixed-capacity FIFO of recent readings; `null` marks a gap. */
export class Ring {
  private buf: (number | null)[] = [];

  constructor(readonly capacity: number) {}

  push(v: number | null): void {
    this.buf.push(v);
    if (this.buf.length > this.capacity) this.buf.shift();
  }

  values(): readonly (number | null)[] {
    return this.buf;
  }
}

export interface SparkStyle {
  color: string;
  min?: number;
  max?: number;
  /** Draw over what is already on the canvas (second series). */
  keep?: boolean;
}

/**
 * Draws `data` right-aligned across a `capacity`-slot width so the line scrolls in from the
 * right as it fills. Gaps break the line. `ctx` is expected to be pre-scaled for DPR.
 */
export function drawSparkline(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  data: readonly (number | null)[],
  capacity: number,
  style: SparkStyle,
): void {
  if (!style.keep) ctx.clearRect(0, 0, w, h);
  const nums = data.filter((v): v is number => v != null);
  if (nums.length < 2) return;
  const lo = style.min ?? Math.min(...nums);
  const hi = style.max ?? Math.max(...nums);
  const span = hi - lo || 1;
  const pad = 2;
  const dx = w / Math.max(capacity - 1, 1);
  const x0 = w - (data.length - 1) * dx;
  const y = (v: number) => pad + (1 - (v - lo) / span) * (h - pad * 2);

  ctx.lineWidth = 1.5;
  ctx.lineJoin = 'round';
  ctx.strokeStyle = style.color;
  ctx.fillStyle = style.color;

  let open = false;
  let first = 0;
  let lastX = 0;
  const closeFill = () => {
    ctx.stroke();
    ctx.lineTo(lastX, h);
    ctx.lineTo(first, h);
    ctx.closePath();
    ctx.globalAlpha = 0.12;
    ctx.fill();
    ctx.globalAlpha = 1;
    open = false;
  };
  data.forEach((v, i) => {
    const x = x0 + i * dx;
    if (v == null) {
      if (open) closeFill();
      return;
    }
    if (!open) {
      ctx.beginPath();
      ctx.moveTo(x, y(v));
      first = x;
      open = true;
    } else {
      ctx.lineTo(x, y(v));
    }
    lastX = x;
  });
  if (open) closeFill();
}

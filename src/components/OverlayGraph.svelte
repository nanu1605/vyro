<script lang="ts">
  // Frame times for the game overlay. The line is the average frame per 100 ms, the faint band
  // reaches the slowest frame, so stutters show as spikes. Readings arrive every 0.5–2 s; the
  // graph plays them back about one update late (`overlay-graph.ts`), so the line slides in
  // smoothly at up to 30 frames a second instead of jumping. With the profile's animation off
  // it is drawn only when readings arrive, as before.
  import { onMount } from 'svelte';
  import { fmt } from '../format';
  import { graphScale } from '../overlay';
  import { GraphFeed, MAX_LAG, monotoneSegments } from '../overlay-graph';
  import { profileSpec } from '../prefs.svelte';
  import type { FramePoint } from '../types';

  interface Props {
    /** Oldest first, one per 100 ms; `null` where no frames came. */
    points: Array<FramePoint | null>;
    width: number;
    height: number;
    color: string;
    /** The plate's zoom, so the canvas stays sharp when scaled up. */
    zoom?: number;
  }
  let { points, width, height, color, zoom = 1 }: Props = $props();

  let canvas = $state<HTMLCanvasElement | null>(null);
  const smooth = $derived(profileSpec().motion !== 'off');
  const target = $derived(graphScale(points));
  /** The scale drawn: eases to a new step instead of jumping. */
  let shown = 16.7;

  const feed = new GraphFeed();
  $effect(() => {
    feed.push(points, performance.now());
    kick();
  });

  /** Points across the window: what is always there behind the trailing edge. */
  const span = $derived(Math.max(10, points.length - MAX_LAG - 2));

  function draw(rows: Array<[number, FramePoint | null]>, scale: number): void {
    const c = canvas;
    if (!c) return;
    const dpr = (window.devicePixelRatio || 1) * zoom;
    const w = Math.round(width * dpr);
    const h = Math.round(height * dpr);
    if (c.width !== w || c.height !== h) {
      c.width = w;
      c.height = h;
    }
    const ctx = c.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const y = (ms: number) => height - 1 - (Math.min(ms, scale) / scale) * (height - 4);
    // Baseline.
    ctx.strokeStyle = 'rgb(255 255 255 / 18%)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, height - 0.5);
    ctx.lineTo(width, height - 0.5);
    ctx.stroke();
    // Runs of consecutive points: gaps (no frames) break the line.
    let run: Array<[number, FramePoint]> = [];
    const curve = (xs: number[], ys: number[], move: boolean) => {
      if (move) ctx.moveTo(xs[0]!, ys[0]!);
      else ctx.lineTo(xs[0]!, ys[0]!);
      for (const [a, b, c2, d, e, f] of monotoneSegments(xs, ys)) ctx.bezierCurveTo(a, b, c2, d, e, f);
    };
    const flush = () => {
      if (run.length > 1) {
        const xs = run.map(([x]) => x * width);
        const top = run.map(([, p]) => y(p.maxMs));
        const mid = run.map(([, p]) => y(p.avgMs));
        ctx.globalAlpha = 0.22;
        ctx.fillStyle = color;
        ctx.beginPath();
        curve(xs, top, true);
        curve([...xs].reverse(), [...mid].reverse(), false);
        ctx.closePath();
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.5;
        ctx.lineJoin = 'round';
        ctx.lineCap = 'round';
        ctx.beginPath();
        curve(xs, mid, true);
        ctx.stroke();
      }
      run = [];
    };
    for (const [x, p] of rows) {
      if (p) run.push([x, p]);
      else flush();
    }
    flush();
  }

  // Not animated: the latest readings across the whole width, drawn when they arrive.
  $effect(() => {
    if (smooth || !canvas) return;
    const n = points.length;
    shown = target;
    draw(
      points.map((p, i): [number, FramePoint | null] => [n > 1 ? i / (n - 1) : 0, p]),
      target,
    );
  });

  // Animated: one frame loop that stops once nothing moves (a paused game, the overlay
  // hidden) and starts again with the next readings.
  let raf = 0;
  let last = 0;
  let mounted = false;
  function kick(): void {
    if (mounted && !raf && smooth && !document.hidden) raf = requestAnimationFrame(frame);
  }
  function frame(t: number): void {
    raf = 0;
    if (!smooth || document.hidden) {
      last = 0;
      return;
    }
    // At most 30 frames a second over the game.
    if (last && t - last < 1000 / 30 - 2) {
      raf = requestAnimationFrame(frame);
      return;
    }
    const dt = last ? Math.min(250, t - last) : 0;
    last = t;
    const moving = feed.advance(dt);
    shown += (target - shown) * (1 - Math.exp(-dt / 400));
    const settled = Math.abs(target - shown) < 0.01;
    if (settled) shown = target;
    draw(feed.window(span), shown);
    if (moving || !settled || dt === 0) raf = requestAnimationFrame(frame);
    else last = 0;
  }

  onMount(() => {
    mounted = true;
    kick();
    const onVisible = () => kick();
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      mounted = false;
      cancelAnimationFrame(raf);
      raf = 0;
      document.removeEventListener('visibilitychange', onVisible);
    };
  });
</script>

<div class="graph" style:width="{width}px" style:height="{height}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px" aria-hidden="true"></canvas>
  <span>{fmt(target, target < 10 ? 1 : 0)} ms</span>
</div>

<style>
  .graph {
    position: relative;
  }
  canvas {
    display: block;
  }
  span {
    position: absolute;
    top: 0;
    right: 0;
    color: #c5ced8;
    font-size: 0.62rem;
    line-height: 1;
  }
</style>

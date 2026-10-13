<script lang="ts">
  // Frame times for the game overlay, on a canvas redrawn only when readings arrive (at most
  // twice a second): nothing animates over the game between updates. The line is the average
  // frame per 100 ms, the faint band reaches the slowest frame, so stutters show as spikes.
  import { fmt } from '../format';
  import { graphScale } from '../overlay';
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
  const scale = $derived(graphScale(points));

  $effect(() => {
    const c = canvas;
    if (!c) return;
    const dpr = (window.devicePixelRatio || 1) * zoom;
    c.width = Math.round(width * dpr);
    c.height = Math.round(height * dpr);
    const ctx = c.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const n = points.length;
    if (n < 2) return;
    const x = (i: number) => (i / (n - 1)) * width;
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
    const flush = () => {
      if (run.length > 1) {
        ctx.globalAlpha = 0.22;
        ctx.fillStyle = color;
        ctx.beginPath();
        run.forEach(([i, p], k) => (k ? ctx.lineTo(x(i), y(p.maxMs)) : ctx.moveTo(x(i), y(p.maxMs))));
        for (let k = run.length - 1; k >= 0; k--) ctx.lineTo(x(run[k]![0]), y(run[k]![1].avgMs));
        ctx.closePath();
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.strokeStyle = color;
        ctx.lineWidth = 1.5;
        ctx.lineJoin = 'round';
        ctx.beginPath();
        run.forEach(([i, p], k) => (k ? ctx.lineTo(x(i), y(p.avgMs)) : ctx.moveTo(x(i), y(p.avgMs))));
        ctx.stroke();
      }
      run = [];
    };
    points.forEach((p, i) => (p ? run.push([i, p]) : flush()));
    flush();
  });
</script>

<div class="graph" style:width="{width}px" style:height="{height}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px" aria-hidden="true"></canvas>
  <span>{fmt(scale, scale < 10 ? 1 : 0)} ms</span>
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

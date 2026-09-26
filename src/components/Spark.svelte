<script lang="ts">
  import { onMount } from 'svelte';
  import { Ring, drawSparkline } from '../sparkline';

  interface Series {
    value: number | null | undefined;
    /** CSS colour or custom property, e.g. `var(--accent-cpu)`. */
    color: string;
  }
  interface Props {
    /** Snapshot timestamp: each new one appends a point. */
    ts: number;
    series: Series[];
    /** Fixed scale; otherwise scaled to the visible data (never below `floor`). */
    max?: number;
    floor?: number;
    label: string;
  }
  let { ts, series, max, floor = 1, label }: Props = $props();

  const POINTS = 60;
  let canvas: HTMLCanvasElement;
  let rings: Ring[] = [];

  function resolve(color: string): string {
    const m = /^var\((--[\w-]+)\)$/.exec(color);
    return m ? getComputedStyle(canvas).getPropertyValue(m[1]!).trim() || '#4cc9f0' : color;
  }

  function draw(): void {
    const ctx = canvas?.getContext('2d');
    if (!ctx) return;
    const { width, height } = canvas.getBoundingClientRect();
    if (width === 0) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(width * dpr)) {
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const all = rings.flatMap((r) => r.values()).filter((v): v is number => v != null);
    const top = max ?? Math.max(floor, ...all);
    ctx.clearRect(0, 0, width, height);
    rings.forEach((r, i) =>
      drawSparkline(ctx, width, height, r.values(), POINTS, { color: resolve(series[i]!.color), min: 0, max: top, keep: true }),
    );
  }

  onMount(() => {
    const ro = new ResizeObserver(draw);
    ro.observe(canvas);
    return () => ro.disconnect();
  });

  $effect(() => {
    void ts;
    while (rings.length < series.length) rings.push(new Ring(POINTS));
    series.forEach((s, i) => rings[i]!.push(s.value ?? null));
    draw();
  });
</script>

<div class="wrap" role="img" aria-label={label}><canvas bind:this={canvas} class="spark" aria-hidden="true"></canvas></div>

<style>
  .wrap {
    margin: 0.3rem 0 0.5rem;
  }
  .spark {
    display: block;
    width: 100%;
    height: 3rem;
  }
</style>

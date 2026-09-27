<script lang="ts">
  // Frame times as a steady, scrolling line: one point per 100 ms (the average frame there) with
  // a faint band up to the worst frame, so stutters show without per-frame jitter. The scale
  // steps between round values and only shrinks when the data leaves plenty of room. Between
  // updates the graph glides left by the time that passed, so it scrolls instead of jumping.
  import { untrack } from 'svelte';
  import { fmt } from '../format';
  import { profileSpec } from '../prefs.svelte';
  import type { FramePoint } from '../types';

  interface Props {
    /** Oldest first, one per 100 ms; `null` where no frames came. */
    points: Array<FramePoint | null>;
    /** Buckets across the visible width; older ones are drawn left of it and scroll in. */
    span?: number;
    height?: number;
    /** Hide the axis labels (compact overlay). */
    bare?: boolean;
  }
  let { points, span = 100, height = 120, bare = false }: Props = $props();

  const W = 1000;
  const uid = Math.random().toString(36).slice(2);
  const BUCKET_MS = 100;
  const STEPS = [4, 5, 8, 10, 12.5, 16.7, 20, 25, 33.3, 50, 100, 200];

  /** The newest `span` buckets fill the width; older ones sit at negative x. */
  const first = $derived(Math.max(0, points.length - span));
  const visible = $derived(points.slice(first));

  let scale = $state(16.7);
  $effect(() => {
    const avgs = visible.flatMap((p) => (p ? [p.avgMs] : [])).sort((a, b) => a - b);
    if (!avgs.length) return;
    const median = avgs[Math.floor(avgs.length / 2)]!;
    const need = median * 2.2;
    const fit = STEPS.find((s) => s >= need) ?? STEPS[STEPS.length - 1]!;
    untrack(() => {
      // Grow at once; shrink only when the line would sit in the lower 40 %.
      if (fit > scale || need < scale * 0.4) scale = fit;
    });
  });

  const y = (ms: number) => height - 2 - (Math.min(ms, scale) / scale) * (height - 6);
  const x = (i: number) => ((i - (points.length - span)) / (span - 1)) * W;

  /** Runs of consecutive points, so gaps (no frames) break the line. */
  const runs = $derived.by(() => {
    const out: Array<Array<[number, FramePoint]>> = [];
    let cur: Array<[number, FramePoint]> = [];
    points.forEach((p, i) => {
      if (p) cur.push([i, p]);
      else if (cur.length) {
        out.push(cur);
        cur = [];
      }
    });
    if (cur.length) out.push(cur);
    return out;
  });
  const line = (run: Array<[number, FramePoint]>) =>
    run.map(([i, p]) => `${x(i).toFixed(1)},${y(p.avgMs).toFixed(1)}`).join(' ');
  const band = (run: Array<[number, FramePoint]>) =>
    [
      ...run.map(([i, p]) => `${x(i).toFixed(1)},${y(p.maxMs).toFixed(1)}`),
      ...run
        .slice()
        .reverse()
        .map(([i, p]) => `${x(i).toFixed(1)},${y(p.avgMs).toFixed(1)}`),
    ].join(' ');

  const filled = $derived(visible.filter((p): p is FramePoint => p != null));
  const avg = $derived(filled.length ? filled.reduce((a, p) => a + p.avgMs, 0) / filled.length : null);
  const worst = $derived(filled.length ? Math.max(...filled.map((p) => p.maxMs)) : null);

  // Scrolling: start shifted right by the time since the last update, then glide to rest.
  let shift = $state(0);
  let glideMs = $state(0);
  let lastAt = 0;
  $effect(() => {
    void points;
    untrack(() => {
      const now = performance.now();
      const dt = lastAt ? Math.min(now - lastAt, 6000) : 0;
      lastAt = now;
      if (!dt || profileSpec().motion === 'off' || points.length < 2) return;
      glideMs = 0;
      shift = (dt / (BUCKET_MS * (span - 1))) * W;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          glideMs = dt;
          shift = 0;
        }),
      );
    });
  });
</script>

<div class="graph" style:height="{height}px">
  <svg
    viewBox="0 0 {W} {height}"
    preserveAspectRatio="none"
    role="img"
    aria-label="Frame times over the last {span / 10} seconds: average {fmt(avg, 1)} ms, worst {fmt(worst, 1)} ms"
  >
    <defs>
      <!-- Fades the left edge, where old data scrolls out. -->
      <linearGradient id="ftg-fade-{uid}" x1="0" x2="1" y1="0" y2="0">
        <stop offset="0" stop-color="white" stop-opacity="0" />
        <stop offset="0.08" stop-color="white" stop-opacity="1" />
      </linearGradient>
      <mask id="ftg-mask-{uid}"><rect width={W} height={height} fill="url(#ftg-fade-{uid})" /></mask>
    </defs>
    <g mask="url(#ftg-mask-{uid})">
      <g class="glide" style:transform="translateX({shift}px)" style:transition-duration="{glideMs}ms">
        {#each runs as run, i (i)}
          <polygon class="band" points={band(run)} />
          {#if run.length > 1}<polyline points={line(run)} />{/if}
        {/each}
      </g>
    </g>
  </svg>
  {#if !bare}
    <span class="top">{fmt(scale, scale < 10 ? 1 : 0)} ms</span>
    <span class="bottom">0</span>
  {/if}
</div>

<style>
  .graph {
    position: relative;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
    overflow: hidden;
    border-bottom: 1px solid var(--line);
  }
  .glide {
    transition-property: transform;
    transition-timing-function: linear;
  }
  polyline {
    fill: none;
    stroke: var(--accent-gpu);
    stroke-width: 1.75;
    vector-effect: non-scaling-stroke;
    stroke-linejoin: round;
    stroke-linecap: round;
  }
  .band {
    fill: var(--accent-gpu);
    fill-opacity: 0.18;
    stroke: none;
  }
  span {
    position: absolute;
    left: 0.2rem;
    color: var(--muted);
    font-size: 0.68rem;
    font-variant-numeric: tabular-nums;
  }
  .top {
    top: 0;
  }
  .bottom {
    bottom: 0.1rem;
  }
</style>

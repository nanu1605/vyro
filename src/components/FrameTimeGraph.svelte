<script lang="ts">
  // Frame times of the latest frames as a line: flat is smooth, spikes are stutters. The scale
  // keeps the typical frame in the lower half so hitches stand out.
  import { fmt } from '../format';

  interface Props {
    /** Frame times in ms, oldest first. */
    frames: number[];
    height?: number;
    /** Hide the axis labels (compact overlay). */
    bare?: boolean;
  }
  let { frames, height = 120, bare = false }: Props = $props();

  const W = 480;
  const scale = $derived.by(() => {
    if (!frames.length) return 20;
    const sorted = [...frames].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)]!;
    const max = sorted[sorted.length - 1]!;
    // At least twice the median; more when a spike needs the room (capped so one huge hitch
    // does not flatten everything).
    return Math.max(median * 2, Math.min(max * 1.1, median * 6), 1);
  });
  const avg = $derived(frames.length ? frames.reduce((a, b) => a + b, 0) / frames.length : null);
  const points = $derived(
    frames
      .map((ms, i) => {
        const x = frames.length > 1 ? (i / (frames.length - 1)) * W : 0;
        const y = height - (Math.min(ms, scale) / scale) * (height - 4) - 2;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(' '),
  );
  const avgY = $derived(avg == null ? null : height - (Math.min(avg, scale) / scale) * (height - 4) - 2);
  const spikes = $derived(avg == null ? 0 : frames.filter((f) => f > avg * 2).length);
</script>

<div class="graph" style:height="{height}px">
  <svg
    viewBox="0 0 {W} {height}"
    preserveAspectRatio="none"
    role="img"
    aria-label="Frame times of the last {frames.length} frames: average {fmt(avg, 1)} ms, {spikes} spike{spikes === 1 ? '' : 's'} over twice the average"
  >
    {#if avgY != null}<line class="avg" x1="0" x2={W} y1={avgY} y2={avgY} />{/if}
    {#if frames.length > 1}<polyline points={points} />{/if}
  </svg>
  {#if !bare}
    <span class="top">{fmt(scale, 0)} ms</span>
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
    border-bottom: 1px solid var(--line);
  }
  polyline {
    fill: none;
    stroke: var(--accent-gpu);
    stroke-width: 1.5;
    vector-effect: non-scaling-stroke;
    stroke-linejoin: round;
  }
  .avg {
    stroke: var(--muted);
    stroke-dasharray: 3 4;
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
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

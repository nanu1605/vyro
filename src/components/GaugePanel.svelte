<script lang="ts">
  import { onMount } from 'svelte';
  import { GAUGE_ASPECT, Gauge } from '../gauge';
  import { Tween, animator } from '../anim';
  import { Ring, drawSparkline } from '../sparkline';
  import { fmt } from '../format';

  interface Props {
    label: string;
    /** Snapshot timestamp; a new value marks a new sample even if readings repeat. */
    ts: number;
    usage: number | null;
    mhz: number | null;
    detail?: string | null;
  }
  let { label, ts, usage, mhz, detail = null }: Props = $props();

  const SPARK_POINTS = 60;
  let dial: HTMLCanvasElement;
  let spark: HTMLCanvasElement;
  let gauge: Gauge | null = null;
  const tUsage = new Tween(0);
  const tMhz = new Tween(0);
  const recent = new Ring(SPARK_POINTS);

  function frame(dt: number): boolean {
    // Hidden tab: skip drawing and let the loop sleep; the resize observer wakes it on show.
    if (dial.offsetParent === null) return false;
    const snap = animator.reducedMotion;
    const a = tUsage.step(dt, snap);
    const b = tMhz.step(dt, snap);
    gauge?.draw(usage == null ? null : tUsage.cur, mhz == null ? null : tMhz.cur);
    return a || b;
  }

  function redrawSpark(): void {
    const ctx = spark?.getContext('2d');
    if (!ctx) return;
    const { width, height } = spark.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    if (spark.width !== Math.round(width * dpr)) {
      spark.width = Math.round(width * dpr);
      spark.height = Math.round(height * dpr);
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const color = getComputedStyle(spark).getPropertyValue('--accent').trim() || '#4cc9f0';
    drawSparkline(ctx, width, height, recent.values(), SPARK_POINTS, { color, min: 0, max: 100 });
  }

  onMount(() => {
    gauge = new Gauge(dial, { label });
    const ro = new ResizeObserver(() => {
      gauge?.resize();
      frame(0);
      redrawSpark();
      animator.wake();
    });
    ro.observe(dial);
    ro.observe(spark);
    const off = animator.add(frame);
    return () => {
      ro.disconnect();
      off();
    };
  });

  $effect(() => {
    void ts;
    if (usage != null) tUsage.set(usage);
    if (mhz != null) tMhz.set(mhz);
    recent.push(usage);
    redrawSpark();
    animator.wake();
  });
</script>

<section class="panel gauge-panel" aria-label="{label} utilization">
  <div class="dial" style:aspect-ratio="1 / {GAUGE_ASPECT}" role="img" aria-label="{label} {fmt(usage)}% at {fmt(mhz)} MHz">
    <canvas bind:this={dial} aria-hidden="true"></canvas>
  </div>
  <div class="spark-row">
    <span class="spark-label">60 s</span>
    <canvas class="spark" bind:this={spark} aria-hidden="true"></canvas>
  </div>
  {#if detail}<p class="detail">{detail}</p>{/if}
</section>

<style>
  .gauge-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
  }
  .dial {
    width: 100%;
    max-width: 22rem;
  }
  .dial canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
  .spark-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
  }
  .spark-label {
    color: var(--muted);
    font-size: 0.7rem;
  }
  .spark {
    flex: 1;
    height: 2.5rem;
    min-width: 0;
  }
  .detail {
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
    text-align: center;
  }
</style>

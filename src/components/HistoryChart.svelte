<script lang="ts">
  import { bandPath, linePath, nearest, niceScale, runs, timeLabel, timeTicks } from '../chart';
  import { fmt, fmtUnit } from '../format';
  import type { HistoryPoint } from '../types';

  interface Series {
    name: string;
    color: string;
    points: HistoryPoint[];
  }
  interface Props {
    title: string;
    /** Display unit, e.g. "°F" for a °C sensor shown in Fahrenheit. */
    unit: string;
    zero: boolean;
    max?: number;
    series: Series[];
    /** Visible window, unix seconds. */
    start: number;
    end: number;
    step: number;
    onclose?: () => void;
  }
  let { title, unit, zero, max, series, start, end, step, onclose }: Props = $props();

  const H = 170;
  const M = { l: 46, r: 10, t: 8, b: 22 };
  let w = $state(0);
  let hoverTs = $state<number | null>(null);

  const scale = $derived(
    niceScale(
      series.flatMap((s) => s.points.flatMap((p) => [p.min, p.max])),
      { zero, max },
    ),
  );
  const plotW = $derived(Math.max(1, w - M.l - M.r));
  const x = $derived((ts: number) => M.l + ((ts - start) / Math.max(1, end - start)) * plotW);
  const y = $derived((v: number) => M.t + (1 - (v - scale.lo) / (scale.hi - scale.lo || 1)) * (H - M.t - M.b));
  // Poll slows to 5 s while the window is hidden; only longer holes are gaps.
  const maxGap = $derived(Math.max(step * 3, 15));
  const drawn = $derived(
    series.map((s) => ({
      ...s,
      runs: runs(s.points, maxGap).map((r) => ({ line: linePath(r, x, y), band: step > 1 ? bandPath(r, x, y) : '' })),
    })),
  );
  const xTicks = $derived(timeTicks(start, end, Math.max(2, Math.floor(plotW / 110))));
  const tickDigits = $derived(Math.max(0, -Math.floor(Math.log10((scale.ticks[1] ?? 1) - (scale.ticks[0] ?? 0) || 1))));
  const empty = $derived(series.every((s) => s.points.length === 0));

  const hover = $derived.by(() => {
    if (hoverTs == null) return null;
    const rows = series.map((s) => {
      const i = nearest(s.points, hoverTs!);
      const p = i < 0 ? null : s.points[i]!;
      return { name: s.name, color: s.color, p: p && Math.abs(p.ts - hoverTs!) <= maxGap ? p : null };
    });
    const ts = rows.find((r) => r.p)?.p?.ts ?? hoverTs;
    return { ts, rows };
  });

  function move(e: PointerEvent): void {
    const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
    const px = e.clientX - rect.left;
    if (px < M.l || px > M.l + plotW) {
      hoverTs = null;
      return;
    }
    hoverTs = start + ((px - M.l) / plotW) * (end - start);
  }

  const latest = (s: Series) => s.points[s.points.length - 1]?.avg ?? null;
  const clock = (ts: number) =>
    new Date(ts * 1000).toLocaleString('en-US', {
      weekday: end - start > 86400 ? 'short' : undefined,
      hour: '2-digit',
      minute: '2-digit',
      second: step < 60 ? '2-digit' : undefined,
      hour12: false,
    });
</script>

<figure class="chart">
  <figcaption>
    <span class="title">{title}</span>
    <span class="unit">{unit}</span>
    {#if onclose}
      <button type="button" class="close" aria-label="Close {title} chart" onclick={onclose}>×</button>
    {/if}
  </figcaption>
  <div class="plot" bind:clientWidth={w}>
    {#if w > 0}
      <svg
        width={w}
        height={H}
        role="img"
        aria-label="{title} history: {series.map((s) => s.name).join(', ')}"
        onpointermove={move}
        onpointerleave={() => (hoverTs = null)}
      >
        {#each scale.ticks as t (t)}
          <line class="grid" x1={M.l} x2={M.l + plotW} y1={y(t)} y2={y(t)} />
          <text class="ylab" x={M.l - 6} y={y(t) + 3.5}>{fmt(t, tickDigits)}</text>
        {/each}
        {#each xTicks as t (t)}
          <text class="xlab" x={x(t)} y={H - 6}>{timeLabel(t, end - start)}</text>
        {/each}
        {#each drawn as s (s.name)}
          {#each s.runs as r, i (i)}
            {#if r.band}<path class="band" d={r.band} style:fill={s.color} />{/if}
            <path class="line" d={r.line} style:stroke={s.color} />
          {/each}
        {/each}
        {#if hover}
          <line class="cross" x1={x(hover.ts)} x2={x(hover.ts)} y1={M.t} y2={H - M.b} />
          {#each hover.rows as r (r.name)}
            {#if r.p}<circle cx={x(r.p.ts)} cy={y(r.p.avg)} r="3" style:fill={r.color} />{/if}
          {/each}
        {/if}
      </svg>
      {#if empty}
        <p class="empty">No history for this range yet.</p>
      {/if}
      {#if hover}
        <div class="tip" style:left="{Math.min(Math.max(x(hover.ts) + 10, 0), Math.max(0, w - 190))}px">
          <div class="when">{clock(hover.ts)}</div>
          {#each hover.rows as r (r.name)}
            <div class="row">
              <span class="dot" style:background={r.color}></span>
              <span class="name">{r.name}</span>
              <b>{r.p ? fmtUnit(r.p.avg, unit) : '—'}</b>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
  <ul class="legend">
    {#each series as s (s.name)}
      <li><span class="dot" style:background={s.color}></span>{s.name} <b>{fmtUnit(latest(s), unit)}</b></li>
    {/each}
  </ul>
</figure>

<style>
  .chart {
    margin: 0;
    padding: 0.7rem 0.8rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    min-width: 0;
  }
  figcaption {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    margin-bottom: 0.3rem;
    font-size: 0.85rem;
  }
  .title {
    font-weight: 600;
  }
  .unit {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .close {
    margin-left: auto;
    padding: 0 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: none;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .plot {
    position: relative;
    height: 170px;
  }
  svg {
    display: block;
    touch-action: pan-y;
  }
  .grid {
    stroke: var(--line);
    stroke-width: 1;
  }
  .cross {
    stroke: var(--muted);
    stroke-dasharray: 3 3;
  }
  .ylab,
  .xlab {
    fill: var(--muted);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .ylab {
    text-anchor: end;
  }
  .xlab {
    text-anchor: middle;
  }
  .line {
    fill: none;
    stroke-width: 1.6;
    stroke-linejoin: round;
  }
  .band {
    opacity: 0.15;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--muted);
    font-size: 0.82rem;
    pointer-events: none;
  }
  .tip {
    position: absolute;
    top: 4px;
    width: 180px;
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: color-mix(in srgb, var(--bg) 92%, transparent);
    font-size: 0.75rem;
    pointer-events: none;
  }
  .when {
    margin-bottom: 0.15rem;
    color: var(--muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }
  .row .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 0.9rem;
    margin: 0.35rem 0 0;
    padding: 0;
    list-style: none;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .legend b,
  .row b {
    color: var(--text);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .dot {
    display: inline-block;
    flex: none;
    width: 0.55rem;
    height: 0.55rem;
    margin-right: 0.3rem;
    border-radius: 2px;
  }
  .row .dot {
    margin-right: 0;
  }
</style>

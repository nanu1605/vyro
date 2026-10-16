<script lang="ts">
  // The overlay's plate (P14): readable over any scene, with large tabular figures; only the
  // graph moves (1.0.1). Used by the overlay window and by the preview in Settings.
  import OverlayGraph from './OverlayGraph.svelte';
  import { ACCENTS, formatRow, type OverlayData } from '../overlay';
  import { fmtTemp } from '../prefs.svelte';

  interface Props {
    data: OverlayData;
  }
  let { data }: Props = $props();

  const fps = $derived(data.rows.find((r) => r.metric === 'fps') ?? null);
  const low = $derived(data.rows.find((r) => r.metric === 'low1') ?? null);
  const rest = $derived(data.rows.filter((r) => r.metric !== 'fps' && r.metric !== 'low1'));
  const show = (r: (typeof data.rows)[number]) => formatRow(r, (c) => fmtTemp(c));
  // Inner widths match the window sizes in `vyro_core::gaming::overlay::size`.
  const graphW = $derived(
    data.mode === 'graph' ? 284 : data.mode === 'full' ? (data.compact ? 230 : 264) : data.compact ? 206 : 232,
  );
  const graphH = $derived(data.mode === 'graph' ? 120 : data.compact ? 38 : 44);
</script>

<div
  class="plate {data.mode}"
  class:compact={data.compact}
  style:--plate-accent={ACCENTS[data.accent]}
  style:--plate-bg="rgb(8 11 16 / {data.opacity}%)"
  style:zoom={data.scale / 100}
>
  {#if data.mode === 'minimal'}
    <div class="line">
      {#each data.rows as r (r.metric)}
        {#if r.metric === 'fps'}
          <span class="fps"><b>{show(r)}</b><small>FPS</small></span>
        {:else}
          <span class="item"><small>{r.label}</small> {show(r)}</span>
        {/if}
      {/each}
    </div>
  {:else}
    {#if fps || low}
      <div class="head">
        {#if fps}<span class="fps"><b>{show(fps)}</b><small>FPS</small></span>{/if}
        {#if low}<span class="low"><small>1% low</small> {show(low)}</span>{/if}
      </div>
    {/if}
    {#if data.timeline.length}
      <OverlayGraph points={data.timeline} width={graphW} height={graphH} color={ACCENTS[data.accent]} zoom={data.scale / 100} />
    {/if}
    {#if rest.length}
      <dl>
        {#each rest as r (r.metric)}
          <div><dt>{r.label}</dt><dd>{show(r)}</dd></div>
        {/each}
      </dl>
    {/if}
  {/if}
</div>

<style>
  .plate {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    box-sizing: border-box;
    padding: 0.45rem 0.6rem;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 10px;
    background: var(--plate-bg);
    color: #f2f6fa;
    font-family: var(--font-num);
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px 2px rgb(0 0 0 / 70%);
  }
  .performance {
    width: 252px;
  }
  .full {
    width: 284px;
  }
  .graph {
    width: 304px;
  }
  .performance.compact {
    width: 220px;
  }
  .full.compact {
    width: 244px;
  }
  .compact {
    gap: 0.2rem;
    padding: 0.3rem 0.45rem;
  }
  .minimal {
    width: max-content;
    max-width: 744px;
  }
  .line,
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.15rem 0.7rem;
  }
  .fps b {
    color: var(--plate-accent);
    font-size: 1.7rem;
    line-height: 1;
  }
  .compact .fps b {
    font-size: 1.35rem;
  }
  .fps small {
    margin-left: 0.25rem;
    font-size: 0.72rem;
    font-weight: 600;
  }
  .low,
  .item {
    font-size: 0.8rem;
    white-space: nowrap;
  }
  .low small,
  .item small {
    color: #c5ced8;
    font-size: 0.7rem;
  }
  dl {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.1rem 0.6rem;
    margin: 0;
    font-size: 0.75rem;
  }
  .compact dl {
    gap: 0 0.5rem;
    font-size: 0.7rem;
  }
  dl div {
    display: flex;
    justify-content: space-between;
    gap: 0.35rem;
    min-width: 0;
    white-space: nowrap;
  }
  dt {
    overflow: hidden;
    color: #c5ced8;
    font-weight: 600;
    text-overflow: ellipsis;
  }
  dd {
    margin: 0;
  }
</style>

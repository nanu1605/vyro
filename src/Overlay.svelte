<script lang="ts">
  // The game overlay: a click-through corner window over the game. Readable over any scene:
  // dark translucent plate, large tabular figures, no animation.
  import { onMount } from 'svelte';
  import FrameTimeGraph from './components/FrameTimeGraph.svelte';
  import { fmt } from './format';
  import { onOverlay, type OverlayData } from './ipc';
  import { fmtTemp, loadPrefs } from './prefs.svelte';

  let data = $state.raw<OverlayData | null>(null);

  onMount(() => {
    void loadPrefs().catch(() => {});
    let stop: (() => void) | undefined;
    let disposed = false;
    void onOverlay((d) => (data = d)).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });

  const g = $derived(data?.game ?? null);
  const paused = $derived(g != null && g.state !== 'capturing');
  const pct = (v: number | null) => (v == null ? '—' : `${fmt(v)}%`);
</script>

{#if g}
  <div class="plate" class:detailed={data?.detailed}>
    <div class="fps">
      <b>{paused ? '—' : fmt(g.fps)}</b><span>FPS</span>
      <i>1% {fmt(g.low1Fps)} · {fmt(g.frameMs, 1)} ms</i>
    </div>
    {#if data?.detailed}
      <FrameTimeGraph frames={g.recentMs.slice(-120)} height={44} bare />
      <dl>
        <dt>CPU</dt>
        <dd>{fmtTemp(data.cpuTemp) ?? '—'}</dd>
        <dd>{pct(data.cpuUsage)}</dd>
        <dt>GPU</dt>
        <dd>{fmtTemp(data.gpuTemp) ?? '—'}</dd>
        <dd>{pct(data.gpuUsage)}{data.gpuPower != null ? ` · ${fmt(data.gpuPower)} W` : ''}</dd>
      </dl>
    {/if}
  </div>
{/if}

<style>
  .plate {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.45rem 0.6rem;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 10px;
    background: rgb(8 11 16 / 72%);
    color: #f2f6fa;
    font-family: var(--font-num);
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px 2px rgb(0 0 0 / 70%);
  }
  .fps {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    column-gap: 0.35rem;
  }
  .fps b {
    color: var(--accent-gpu);
    font-size: 1.7rem;
    line-height: 1;
  }
  .fps span {
    font-size: 0.72rem;
    font-weight: 600;
  }
  .fps i {
    flex-basis: 100%;
    color: #c5ced8;
    font-size: 0.72rem;
    font-style: normal;
  }
  dl {
    display: grid;
    grid-template-columns: auto auto 1fr;
    gap: 0.1rem 0.5rem;
    margin: 0;
    font-size: 0.74rem;
  }
  dt {
    color: #c5ced8;
    font-weight: 600;
  }
  dd {
    margin: 0;
  }
</style>

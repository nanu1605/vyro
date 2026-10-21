<script lang="ts">
  // What runs on the PC (1.5): the busiest processes by CPU, memory or GPU, as the PC's
  // Processes view lists them. A phone allowed to control the PC can end one, after a sheet asks.
  import { fmt } from '../../format';
  import type { ProcessSnapshot } from '../../types';
  import Sheet from '../ui/Sheet.svelte';
  import { toast } from '../ui/toast.svelte';
  import { errText, every, type ScreenProps } from './ask';

  let { ask, can, pc }: ScreenProps = $props();

  type Sort = 'cpu' | 'ram' | 'gpu';
  const SORTS: Array<[Sort, string]> = [
    ['cpu', 'CPU'],
    ['ram', 'Memory'],
    ['gpu', 'GPU'],
  ];
  let list = $state.raw<ProcessSnapshot[] | null>(null);
  let error = $state<string | null>(null);
  let sort = $state<Sort>('cpu');
  let asking = $state<ProcessSnapshot | null>(null);
  let busy = $state(false);

  function load(): void {
    ask<ProcessSnapshot[]>('processes')
      .then((v) => {
        list = v;
        error = null;
      })
      .catch((e: unknown) => (error = errText(e)));
  }
  $effect(() => every(3000, load));

  const key = (p: ProcessSnapshot) => (sort === 'cpu' ? p.cpuPct : sort === 'ram' ? p.ramMB : p.gpuPct) ?? -1;
  const shown = $derived(list ? [...list].sort((a, b) => key(b) - key(a)) : []);
  /** How full each one's bar is: its share of the busiest one. */
  const top = $derived(Math.max(1e-9, ...shown.map(key)));

  async function end(p: ProcessSnapshot): Promise<void> {
    busy = true;
    try {
      await ask('process.end', { pid: p.pid, name: p.name });
      toast(`Ended ${p.name}.`);
      load();
    } catch (e) {
      toast(errText(e), 'hot');
    } finally {
      busy = false;
      asking = null;
    }
  }
</script>

<section class="panel rise" aria-labelledby="proc-title">
  <h1 id="proc-title">What runs on {pc}</h1>
  <div class="segmented" role="group" aria-label="Sort by">
    {#each SORTS as [id, label] (id)}
      <button type="button" class="ghost" aria-pressed={sort === id} onclick={() => (sort = id)}>{label}</button>
    {/each}
  </div>
  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if !list}
    <div class="skeleton" role="status" aria-label="Reading…"><span></span><span></span><span></span><span></span></div>
  {:else}
    <ul class="rows">
      {#each shown as p (p.pid)}
        <li>
          <div class="what">
            <b>{p.name}</b>
            <span class="muted small">
              CPU {fmt(p.cpuPct, 1)} % · {fmt(p.ramMB)} MB{p.gpuPct != null ? ` · GPU ${fmt(p.gpuPct)} %` : ''}
            </span>
            <span class="meter" aria-hidden="true"><i style:width="{Math.max(0, (key(p) / top) * 100)}%"></i></span>
          </div>
          {#if can('process.end')}
            <button type="button" class="ghost end" onclick={() => (asking = p)} aria-label="End {p.name}…">End…</button>
          {/if}
        </li>
      {/each}
    </ul>
    <p class="muted small foot">
      The busiest programs, every 3 seconds.{can('process.end')
        ? ' Ending one loses its unsaved work. Windows’ own processes and Vyro are never ended.'
        : ''}
    </p>
  {/if}
</section>

<Sheet open={asking != null} title={asking ? `End ${asking.name}?` : ''} onclose={() => (asking = null)}>
  {#if asking}
    <p class="muted">Its unsaved work is lost. Windows’ own processes and Vyro are never ended.</p>
    <div class="actions sheet-actions">
      <button type="button" disabled={busy} onclick={() => asking && void end(asking)} aria-label="End {asking.name} now">End</button>
      <button type="button" class="ghost" onclick={() => (asking = null)}>Keep</button>
    </div>
  {/if}
</Sheet>

<style>
  .segmented {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 0.35rem;
    margin-bottom: 0.6rem;
    padding: 0.25rem;
    border: 1px solid var(--line);
    border-radius: 14px;
  }
  .segmented button {
    min-height: 2.4rem;
    padding: 0.3rem;
    border-color: transparent;
    font-size: 0.85rem;
  }
  .rows li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
  }
  .what {
    display: grid;
    flex: 1;
    gap: 0.15rem;
    min-width: 0;
  }
  .meter {
    display: block;
    height: 4px;
    margin-top: 0.2rem;
    overflow: hidden;
    border-radius: 2px;
    background: var(--gauge-track);
  }
  .meter i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width var(--dur-3) var(--ease-out);
  }
  .end {
    flex: none;
    padding: 0.35rem 0.8rem;
    font-size: 0.85rem;
  }
  .sheet-actions button {
    flex: 1;
  }
  .foot {
    margin: 0.75rem 0 0;
  }
</style>

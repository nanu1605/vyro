<script lang="ts">
  // What runs on the PC (1.5): the busiest processes by CPU, memory or GPU, as the PC's
  // Processes view lists them. A phone allowed to control the PC can end one.
  import { fmt } from '../../format';
  import type { ProcessSnapshot } from '../../types';
  import { errText, every, type ScreenProps } from './ask';

  let { ask, can, pc }: ScreenProps = $props();

  type Sort = 'cpu' | 'ram' | 'gpu';
  let list = $state.raw<ProcessSnapshot[] | null>(null);
  let error = $state<string | null>(null);
  let sort = $state<Sort>('cpu');
  let asking = $state<number | null>(null);
  let result = $state<string | null>(null);
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

  async function end(p: ProcessSnapshot): Promise<void> {
    busy = true;
    try {
      await ask('process.end', { pid: p.pid, name: p.name });
      result = `Ended ${p.name}.`;
      load();
    } catch (e) {
      result = errText(e);
    } finally {
      busy = false;
      asking = null;
    }
  }
</script>

<section class="panel" aria-labelledby="proc-title">
  <h1 id="proc-title">What runs on {pc}</h1>
  <div class="sorts" role="group" aria-label="Sort by">
    {#each [['cpu', 'CPU'], ['ram', 'Memory'], ['gpu', 'GPU']] as [id, label] (id)}
      <button type="button" class="ghost" aria-pressed={sort === id} onclick={() => (sort = id as Sort)}>{label}</button>
    {/each}
  </div>
  {#if result}<p class="note" role="status">{result}</p>{/if}
  {#if error}
    <p class="error" role="alert">{error}</p>
  {:else if !list}
    <p class="muted" role="status">Reading…</p>
  {:else}
    <ul class="rows">
      {#each shown as p (p.pid)}
        <li>
          <div class="what">
            <b>{p.name}</b>
            <span class="muted small">
              CPU {fmt(p.cpuPct, 1)} % · {fmt(p.ramMB)} MB{p.gpuPct != null ? ` · GPU ${fmt(p.gpuPct)} %` : ''}
            </span>
          </div>
          {#if can('process.end')}
            {#if asking === p.pid}
              <span class="confirm">
                <button type="button" disabled={busy} onclick={() => void end(p)} aria-label="End {p.name} now">End</button>
                <button type="button" class="ghost" onclick={() => (asking = null)}>Keep</button>
              </span>
            {:else}
              <button type="button" class="ghost" onclick={() => ((asking = p.pid), (result = null))} aria-label="End {p.name}…">
                End…
              </button>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
    <p class="muted small">
      The busiest programs, every 3 seconds.{can('process.end')
        ? ' Ending one loses its unsaved work. Windows’ own processes and Vyro are never ended.'
        : ''}
    </p>
  {/if}
</section>

<style>
  .sorts {
    display: flex;
    gap: 0.4rem;
    margin-bottom: 0.6rem;
  }
  .sorts button {
    padding: 0.35rem 0.8rem;
    font-size: 0.85rem;
  }
  .sorts button[aria-pressed='true'] {
    border-color: var(--accent);
    font-weight: 700;
  }
  .rows {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .rows li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--line);
  }
  .what {
    display: grid;
    min-width: 0;
  }
  .what b {
    overflow-wrap: anywhere;
  }
  .rows button {
    padding: 0.35rem 0.75rem;
    font-size: 0.85rem;
  }
  .confirm {
    display: flex;
    flex: none;
    gap: 0.35rem;
  }
</style>

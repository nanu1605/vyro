<script lang="ts">
  import { fmtUnit, GROUP_LABEL, NA } from '../format';
  import { getSensorTree, resetSensorStats } from '../ipc';
  import type { Sensor, SensorSnapshot, SensorTree } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    /** Only refresh while the tab is shown. */
    active: boolean;
    /** Opens a history-tracked sensor in the History view. */
    onhistory?: (id: string) => void;
  }
  let { snap, active, onhistory }: Props = $props();

  let tree = $state<SensorTree | null>(null);
  let error = $state<string | null>(null);
  let filter = $state('');
  let collapsed = $state<Record<string, boolean>>({});
  let busy = false;

  async function refresh(): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      tree = await getSensorTree(snap);
      error = null;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  // One refresh per engine tick while visible.
  $effect(() => {
    if (active && snap?.ts != null) void refresh();
  });

  async function reset(): Promise<void> {
    await resetSensorStats();
    await refresh();
  }

  const groups = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return (tree?.groups ?? [])
      .map((g) => ({
        ...g,
        sensors: q ? g.sensors.filter((s) => s.name.toLowerCase().includes(q) || s.id.includes(q)) : g.sensors,
      }))
      .filter((g) => g.sensors.length > 0);
  });
  const total = $derived(tree?.groups.reduce((n, g) => n + g.sensors.length, 0) ?? 0);
  const live = $derived(tree?.groups.reduce((n, g) => n + g.sensors.filter((s) => s.available).length, 0) ?? 0);

  const v = (s: Sensor, x: number | null) => (x == null ? NA : fmtUnit(x, s.unit));
</script>

<section class="panel">
  <div class="bar">
    <p class="note">
      Every sensor Vyro reads, with minimum, maximum and average since start{tree ? ` · ${live} of ${total} reporting` : ''}.
    </p>
    <div class="tools">
      <input type="search" placeholder="Filter sensors" aria-label="Filter sensors" bind:value={filter} />
      <button type="button" class="reset" onclick={reset}>Reset min/max</button>
    </div>
  </div>

  {#if error}
    <p class="empty">Sensor list unavailable: {error}</p>
  {:else if !tree}
    <p class="empty">Waiting for data…</p>
  {:else if groups.length === 0}
    <p class="empty">No sensors match “{filter}”.</p>
  {/if}

  {#each groups as g (g.group)}
    {@const open = !collapsed[g.group] || filter.trim() !== ''}
    <div class="group">
      <h2>
        <button type="button" aria-expanded={open} onclick={() => (collapsed[g.group] = open)}>
          <span class="chev" aria-hidden="true">{open ? '▾' : '▸'}</span>
          {GROUP_LABEL[g.group] ?? g.group}
          <span class="count">{g.sensors.length}</span>
        </button>
      </h2>
      {#if open}
        <div class="scroll">
          <table>
            <colgroup>
              <col class="c-name" />
              <col class="c-num" />
              <col class="c-num opt" />
              <col class="c-num" />
              <col class="c-num opt" />
              <col class="c-src opt" />
            </colgroup>
            <thead>
              <tr>
                <th>Sensor</th>
                <th class="num">Value</th>
                <th class="num opt">Min</th>
                <th class="num">Max</th>
                <th class="num opt">Avg</th>
                <th class="opt">Source</th>
              </tr>
            </thead>
            <tbody>
              {#each g.sensors as s (s.id)}
                <tr class:off={!s.available}>
                  <td class="name" title={s.id}>
                    {#if s.history && onhistory}
                      <button type="button" class="link" onclick={() => onhistory(s.id)} title="Show history">{s.name}</button>
                    {:else}
                      {s.name}
                    {/if}
                  </td>
                  <td class="num val">{v(s, s.value)}</td>
                  <td class="num opt">{v(s, s.min)}</td>
                  <td class="num">{v(s, s.max)}</td>
                  <td class="num opt">{v(s, s.avg)}</td>
                  <td class="opt src">{s.source}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/each}
</section>

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    margin-bottom: 0.75rem;
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  input,
  .reset {
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
  }
  input {
    min-width: 12rem;
  }
  .reset {
    cursor: pointer;
  }
  .reset:hover {
    border-color: var(--muted);
  }
  .group {
    margin-bottom: 0.9rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  h2 {
    margin: 0;
  }
  h2 button {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    width: 100%;
    padding: 0.55rem 0.8rem;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }
  .chev {
    width: 0.8rem;
    color: var(--muted);
  }
  .count {
    color: var(--muted);
    font-weight: 400;
  }
  .scroll {
    overflow-x: auto;
  }
  table {
    width: 100%;
    table-layout: fixed;
    border-collapse: collapse;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
  }
  .c-num {
    width: 8.5rem;
  }
  .c-src {
    width: 5.5rem;
  }
  th {
    padding: 0.3rem 0.8rem;
    color: var(--muted);
    font-weight: 600;
    text-align: left;
  }
  td {
    padding: 0.28rem 0.8rem;
    border-top: 1px solid var(--line);
  }
  .num {
    text-align: right;
    white-space: nowrap;
  }
  .val {
    color: var(--text);
    font-weight: 600;
  }
  td.num:not(.val) {
    color: var(--muted);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    text-decoration: underline dotted var(--muted);
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .link:hover {
    color: var(--accent-cpu);
  }
  .src {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .off {
    opacity: 0.5;
  }
  .empty {
    color: var(--muted);
    font-size: 0.85rem;
  }
  @media (max-width: 560px) {
    .opt {
      display: none;
    }
    th,
    td {
      padding-inline: 0.45rem;
    }
    .c-num {
      width: 6.2rem;
    }
  }
</style>

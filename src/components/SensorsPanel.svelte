<script lang="ts">
  import { GROUP_LABEL, NA } from '../format';
  import { fmtReading, isHidden, prefs, savePrefs } from '../prefs.svelte';
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

  let tree = $state.raw<SensorTree | null>(null);
  let error = $state<string | null>(null);
  let filter = $state('');
  let collapsed = $state<Record<string, boolean>>({});
  let showHidden = $state(false);
  let busy = false;

  /** Hides a sensor from this list and from History (Settings lists them to bring back). */
  async function toggleHidden(s: Sensor): Promise<void> {
    const hidden = prefs.hiddenSensors;
    try {
      await savePrefs({ hiddenSensors: isHidden(s.id) ? hidden.filter((id) => id !== s.id) : [...hidden, s.id] });
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

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
    try {
      await resetSensorStats();
    } catch (e) {
      error = String(e);
      return;
    }
    await refresh();
  }

  const groups = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return (tree?.groups ?? [])
      .map((g) => ({
        ...g,
        sensors: g.sensors.filter(
          (s) => (showHidden || !isHidden(s.id)) && (!q || s.name.toLowerCase().includes(q) || s.id.includes(q)),
        ),
      }))
      .filter((g) => g.sensors.length > 0);
  });
  const total = $derived(tree?.groups.reduce((n, g) => n + g.sensors.length, 0) ?? 0);
  const live = $derived(tree?.groups.reduce((n, g) => n + g.sensors.filter((s) => s.available).length, 0) ?? 0);

  const hiddenCount = $derived(prefs.hiddenSensors.length);

  const v = (s: Sensor, x: number | null) => (x == null ? NA : fmtReading(x, s.unit));
</script>

<section class="panel">
  <div class="bar">
    <p class="note">
      Every sensor Vyro reads, with minimum, maximum and average since start{tree ? ` · ${live} of ${total} reporting` : ''}.
    </p>
    <div class="tools">
      <input type="search" placeholder="Filter sensors" aria-label="Filter sensors" bind:value={filter} />
      <button type="button" class="reset" onclick={reset}>Reset min/max</button>
      {#if hiddenCount > 0 || showHidden}
        <label class="reset">
          <input type="checkbox" bind:checked={showHidden} />
          Show hidden ({hiddenCount})
        </label>
      {/if}
    </div>
  </div>

  {#if error}
    <p class="empty" role="alert">Sensor list unavailable: {error}. Vyro keeps trying every reading.</p>
  {:else if !tree}
    <p class="empty" role="status">Waiting for the first reading…</p>
  {:else if groups.length === 0}
    <p class="empty" role="status">
      {#if filter.trim()}
        No sensors match “{filter.trim()}”.
      {:else if total > 0}
        Every sensor is hidden. Tick “Show hidden” to see them.
      {:else}
        No sensors reported yet.
      {/if}
    </p>
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
              <col class="c-act" />
            </colgroup>
            <thead>
              <tr>
                <th>Sensor</th>
                <th class="num">Value</th>
                <th class="num opt">Min</th>
                <th class="num">Max</th>
                <th class="num opt">Avg</th>
                <th class="opt">Source</th>
                <th><span class="sr">Hide</span></th>
              </tr>
            </thead>
            <tbody>
              {#each g.sensors as s (s.id)}
                {@const hidden = isHidden(s.id)}
                <tr class:off={!s.available || hidden}>
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
                  <td class="act">
                    <button
                      type="button"
                      class="hide"
                      onclick={() => toggleHidden(s)}
                      aria-label="{hidden ? 'Show' : 'Hide'} {s.name}"
                      title={hidden ? 'Show again in Sensors and History' : 'Hide from Sensors and History'}
                    >
                      {hidden ? '+' : '×'}
                    </button>
                  </td>
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
    container-type: inline-size;
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
  .c-act {
    width: 2.2rem;
  }
  .act {
    padding: 0 0.3rem;
    text-align: center;
  }
  .hide {
    width: 1.5rem;
    height: 1.5rem;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 6px;
    background: none;
    color: var(--muted);
    font: inherit;
    line-height: 1;
    cursor: pointer;
  }
  .hide:hover,
  .hide:focus-visible {
    border-color: var(--line);
    color: var(--text);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  label.reset {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  label.reset input {
    min-width: 0;
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
  /* Dimmed with the muted colour, not opacity, so the text keeps its contrast in every theme. */
  .off td,
  .off .val {
    color: var(--muted);
    font-weight: 400;
  }
  .empty {
    color: var(--muted);
    font-size: 0.85rem;
  }
  /* By the table's own width (2.0.2), not the window's: Min, Avg and Source step out before the
     names would be squeezed, so the table never scrolls sideways. */
  @container (max-width: 50rem) {
    .opt {
      display: none;
    }
  }
  @container (max-width: 30rem) {
    th,
    td {
      padding-inline: 0.45rem;
    }
    .c-num {
      width: 6.2rem;
    }
  }
</style>

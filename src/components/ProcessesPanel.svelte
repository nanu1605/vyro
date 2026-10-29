<script lang="ts">
  import { fmt, fmtMB } from '../format';
  import { endProcess } from '../ipc';
  import type { ProcessSnapshot } from '../types';

  interface Props {
    processes: ProcessSnapshot[];
    /** The view shows: while hidden the table keeps its rows rather than sorting every reading. */
    active: boolean;
  }
  let { processes, active }: Props = $props();
  const kept = { rows: [] as ProcessSnapshot[] };
  const shown = $derived(active ? (kept.rows = processes) : kept.rows);

  type Col = 'name' | 'pid' | 'cpuPct' | 'ramMB' | 'gpuPct';
  const COLS: Array<{ id: Col; label: string; num: boolean }> = [
    { id: 'name', label: 'Name', num: false },
    { id: 'pid', label: 'PID', num: true },
    { id: 'cpuPct', label: 'CPU', num: true },
    { id: 'ramMB', label: 'Memory', num: true },
    { id: 'gpuPct', label: 'GPU', num: true },
  ];

  let sortBy = $state<Col>('cpuPct');
  let desc = $state(true);
  let filter = $state('');

  function sort(col: Col): void {
    if (sortBy === col) desc = !desc;
    else {
      sortBy = col;
      desc = col !== 'name' && col !== 'pid';
    }
  }

  const rows = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = q
      ? shown.filter((p) => p.name.toLowerCase().includes(q) || String(p.pid).startsWith(q))
      : shown.slice();
    const dir = desc ? -1 : 1;
    list.sort((a, b) => {
      if (sortBy === 'name') return dir * a.name.localeCompare(b.name) || a.pid - b.pid;
      const x = a[sortBy] ?? -1;
      const y = b[sortBy] ?? -1;
      return dir * (x - y) || a.pid - b.pid;
    });
    return list;
  });

  // End task (1.5): asked once, then the row says what happened.
  let asking = $state<number | null>(null);
  let busy = $state(false);
  let said = $state<string | null>(null);
  async function end(p: ProcessSnapshot): Promise<void> {
    busy = true;
    try {
      await endProcess(p.pid, p.name);
      said = `Ended ${p.name}.`;
    } catch (e) {
      said = `${p.name}: ${String(e).replace(/^Error: /, '')}`;
    } finally {
      busy = false;
      asking = null;
    }
  }

  const cell = (p: ProcessSnapshot, col: Col): string => {
    switch (col) {
      case 'name':
        return p.name;
      case 'pid':
        return String(p.pid);
      case 'ramMB':
        return fmtMB(p.ramMB);
      default: {
        const v = p[col];
        return v == null ? '—' : `${fmt(v, 1)}%`;
      }
    }
  };
</script>

<section class="panel">
  <div class="bar">
    <p class="note">Busiest processes by CPU, memory and GPU. Memory is the private working set.</p>
    {#if said}<p class="note said" role="status">{said}</p>{/if}
    <input type="search" placeholder="Filter by name or PID" aria-label="Filter processes" bind:value={filter} />
  </div>
  <div class="scroll">
    <table>
      <thead>
        <tr>
          {#each COLS as c (c.id)}
            <th
              class:num={c.num}
              aria-sort={sortBy === c.id ? (desc ? 'descending' : 'ascending') : 'none'}
            >
              <button type="button" onclick={() => sort(c.id)}>
                {c.label}{sortBy === c.id ? (desc ? ' ▾' : ' ▴') : ''}
              </button>
            </th>
          {/each}
          <th><span class="sr-only">End task</span></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as p (p.pid)}
          <tr>
            {#each COLS as c (c.id)}
              <td class:num={c.num} class:name={c.id === 'name'}>{cell(p, c.id)}</td>
            {/each}
            <td class="end">
              {#if asking === p.pid}
                <button type="button" disabled={busy} onclick={() => void end(p)} aria-label="End {p.name} now">
                  End
                </button>
                <button type="button" onclick={() => (asking = null)}>Keep</button>
              {:else}
                <button type="button" onclick={() => ((asking = p.pid), (said = null))} aria-label="End task {p.name}…">
                  End task…
                </button>
              {/if}
            </td>
          </tr>
        {:else}
          <tr><td colspan={COLS.length + 1} class="empty">{shown.length ? 'No matches.' : 'Waiting for data…'}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<style>
  .end {
    white-space: nowrap;
    text-align: right;
  }
  .end button {
    padding: 0.15rem 0.55rem;
    font-size: 0.8rem;
  }
  .end button + button {
    margin-left: 0.3rem;
  }
  .said {
    margin: 0;
  }
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
  input {
    min-width: 12rem;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
  }
  .scroll {
    max-height: 70vh;
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.82rem;
    font-variant-numeric: tabular-nums;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--panel);
    text-align: left;
  }
  th button {
    padding: 0.4rem 0.5rem;
    border: 0;
    background: none;
    color: var(--muted);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  th.num button {
    width: 100%;
    text-align: right;
  }
  td {
    padding: 0.3rem 0.5rem;
    border-top: 1px solid var(--line);
  }
  .num {
    text-align: right;
    white-space: nowrap;
  }
  .name {
    max-width: 18rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty {
    color: var(--muted);
    text-align: center;
  }
  @media (max-width: 480px) {
    th:nth-child(2),
    td:nth-child(2):not(.empty) {
      display: none;
    }
    td,
    th button {
      padding-inline: 0.3rem;
    }
  }
</style>

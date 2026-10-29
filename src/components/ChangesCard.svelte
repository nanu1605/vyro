<script lang="ts">
  import Card from './Card.svelte';
  import { CATEGORIES, describe, groupByDay, merge, timeOf } from '../changes';
  import { fmtDate, fmtDateTime } from '../format';
  import { getHardwareChanges, onHardwareChanged } from '../ipc';
  import type { ChangeCategory, HardwareChange } from '../types';

  interface Props {
    /** Changes above this id are new since the tab was last opened. */
    newSince?: number;
  }
  let { newSince = Number.POSITIVE_INFINITY }: Props = $props();

  /** Entries shown before "Show all". */
  const PREVIEW = 5;

  let list = $state.raw<HardwareChange[] | null>(null);
  let error = $state<string | null>(null);
  let all = $state(false);
  let shown = $state<Record<ChangeCategory, boolean>>({ hardware: true, firmware: true, driver: true, windows: true, settings: true });

  function load(): void {
    getHardwareChanges()
      .then((v) => {
        list = v;
        error = null;
      })
      .catch((e: unknown) => (error = String(e)));
  }

  $effect(() => {
    load();
    let stop: (() => void) | undefined;
    let disposed = false;
    void onHardwareChanged(load).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });

  const changes = $derived(merge((list ?? []).filter((c) => c.kind !== 'tracking')));
  // Past the store's limit the start of tracking is gone; the oldest kept entry stands in.
  const since = $derived(list?.find((c) => c.kind === 'tracking')?.ts ?? list?.at(-1)?.ts ?? null);
  const counts = $derived(
    Object.fromEntries(CATEGORIES.map((g) => [g.id, changes.filter((c) => c.category === g.id).length])) as Record<ChangeCategory, number>,
  );
  const filtered = $derived(changes.filter((c) => shown[c.category]));
  const days = $derived(groupByDay(all ? filtered : filtered.slice(0, PREVIEW)));
  const color = (id: ChangeCategory) => CATEGORIES.find((g) => g.id === id)?.color;
  const label = (id: ChangeCategory) => CATEGORIES.find((g) => g.id === id)?.label;
</script>

<Card title="PC changes" id="hw-changes" subtitle={since != null ? `Tracking since ${fmtDate(since)}` : null}>
  {#if error}
    <p class="note" role="alert">Could not read the change history: {error}</p>
  {:else if !list}
    <p class="note" role="status">Loading…</p>
  {:else if list.length === 0}
    <p class="note">Vyro has not checked this PC yet; the first check runs shortly after it starts.</p>
  {:else if changes.length === 0}
    <p class="note">
      No changes since {fmtDate(since)}. New or swapped parts, BIOS, driver and Windows updates and firmware settings will
      show here.
    </p>
  {:else}
    <div class="filters" role="group" aria-label="Kinds of change shown">
      {#each CATEGORIES as g (g.id)}
        <label class="chip">
          <input type="checkbox" bind:checked={shown[g.id]} />
          <span class="dot" style:background={g.color}></span>{g.label} ({counts[g.id]})
        </label>
      {/each}
    </div>
    {#if days.length === 0}
      <p class="note">All changes are filtered out.</p>
    {/if}
    {#each days as d (d.key)}
      <h3>{d.label}</h3>
      <ol class="changes">
        {#each d.items as c (c.id)}
          {@const x = describe(c)}
          <li class:fresh={c.id > newSince}>
            <span class="cat" style:--c={color(c.category)}>{label(c.category)}</span>
            <div class="body">
              <p class="title">
                {x.title}{#if c.id > newSince}<span class="badge">New</span>{/if}
              </p>
              {#if x.device}<p class="device">{x.device}</p>{/if}
              {#if x.from != null || x.to != null}
                <p class="fromto">
                  {#if x.label}<span class="what">{x.label}:</span>{/if}
                  <span class="was">{x.from ?? 'none'}</span>
                  <span aria-hidden="true">→</span><span class="sr-only">changed to</span>
                  <span class="now">{x.to ?? 'none'}</span>
                </p>
              {/if}
              {#if x.note}<p class="device">{x.note}</p>{/if}
            </div>
            <time datetime={new Date(c.ts * 1000).toISOString()} title={fmtDateTime(c.ts)}>{timeOf(c.ts)}</time>
          </li>
        {/each}
      </ol>
    {/each}
    {#if filtered.length > PREVIEW}
      <button type="button" class="more" aria-expanded={all} onclick={() => (all = !all)}>
        {all ? 'Show fewer' : `Show all ${filtered.length} changes`}
      </button>
    {/if}
  {/if}
</Card>

<style>
  .note {
    margin: 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.4rem;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .dot {
    display: inline-block;
    flex: none;
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
  }
  h3 {
    margin: 0.9rem 0 0.4rem;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .changes {
    display: grid;
    gap: 0.45rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: 5.5rem minmax(0, 1fr) auto;
    align-items: start;
    gap: 0.75rem;
    padding: 0.55rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    font-size: 0.84rem;
  }
  li.fresh {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
  }
  .cat {
    justify-self: start;
    padding: 0.05rem 0.5rem;
    border: 1px solid color-mix(in srgb, var(--c) 55%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, var(--c) 14%, transparent);
    font-size: 0.72rem;
    white-space: nowrap;
  }
  .body p {
    margin: 0;
  }
  .title {
    font-weight: 600;
  }
  .badge {
    margin-left: 0.45rem;
    padding: 0 0.4rem;
    border-radius: 4px;
    background: var(--accent);
    color: var(--bg);
    font-size: 0.68rem;
    font-weight: 700;
    vertical-align: 0.08rem;
  }
  .device {
    margin-top: 0.1rem !important;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .fromto {
    margin-top: 0.2rem !important;
    font-family: var(--font-num);
    overflow-wrap: anywhere;
  }
  .what {
    margin-right: 0.3rem;
    color: var(--muted);
    font-family: var(--font-ui);
  }
  .was {
    color: var(--muted);
  }
  time {
    color: var(--muted);
    font-family: var(--font-num);
    font-size: 0.78rem;
    white-space: nowrap;
  }
  .more {
    margin-top: 0.7rem;
    padding: 0.3rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .more:hover {
    border-color: var(--accent);
  }
  @media (max-width: 560px) {
    li {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .cat {
      grid-column: 1 / -1;
    }
  }
</style>

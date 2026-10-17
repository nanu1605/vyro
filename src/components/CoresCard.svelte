<script lang="ts">
  import Card from './Card.svelte';
  import { fmt } from '../format';
  import { balancedColumns } from '../packing';
  import type { CpuSnapshot } from '../types';

  interface Props {
    cpu: CpuSnapshot | null;
  }
  let { cpu }: Props = $props();

  const cores = $derived(
    (cpu?.usagePerCore ?? []).map((usage, i) => ({ usage, mhz: cpu?.mhzPerCore[i] ?? null })),
  );

  // Across the whole card in even rows (1.2): 16 threads on a wide card are one row of 16, not
  // half a card of bars and half empty. A bar is 3.1rem at least, 7rem at most.
  let width = $state(0);
  const rem = () => parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  const cols = $derived(width ? balancedColumns(cores.length, width, 3.1 * rem(), 0.5 * rem()) : Math.min(cores.length, 8));
</script>

<Card title="Logical processors" subtitle={cores.length ? `${cores.length} threads` : null} wide>
  {#if cores.length === 0}
    <p class="empty">Per-core readings unavailable.</p>
  {:else}
    <ul class="cores" bind:clientWidth={width} style:--cols={cols}>
      {#each cores as c, i (i)}
        <li title="CPU {i}: {fmt(c.usage)}% at {fmt(c.mhz)} MHz">
          <span class="bar" aria-hidden="true"><span style:height="{Math.min(100, c.usage)}%"></span></span>
          <span class="pct">{fmt(c.usage)}%</span>
          <span class="mhz">{c.mhz != null ? fmt(c.mhz) : ''}</span>
          <span class="idx">{i}</span>
        </li>
      {/each}
    </ul>
  {/if}
</Card>

<style>
  .cores {
    display: grid;
    grid-template-columns: repeat(var(--cols), minmax(0, 7rem));
    justify-content: center;
    gap: 0.5rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    justify-items: center;
    gap: 0.15rem;
    font-size: 0.7rem;
    font-variant-numeric: tabular-nums;
  }
  .bar {
    display: flex;
    align-items: flex-end;
    width: 100%;
    height: 3rem;
    border-radius: 6px;
    background: var(--gauge-track);
    overflow: hidden;
  }
  .bar span {
    width: 100%;
    background: var(--accent-cpu);
    transition: height 0.4s ease-out;
  }
  .mhz,
  .idx {
    color: var(--muted);
  }
  .idx {
    font-size: 0.62rem;
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
  }
  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
</style>

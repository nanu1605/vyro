<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt } from '../format';
  import { balancedColumns } from '../packing';
  import { fmtTemp } from '../prefs.svelte';
  import type { CpuSnapshot } from '../types';

  interface Props {
    cpu: CpuSnapshot | null;
    /** Why its sensors are missing ("needs PawnIO", "not read on this CPU"). */
    needs: string;
  }
  let { cpu, needs }: Props = $props();

  const unit = (v: number | null | undefined, u: string, d = 1) => (v == null ? null : `${fmt(v, d)} ${u}`);
  const rows = $derived<Array<[string, string | null]>>([
    ['Package (Tdie)', fmtTemp(cpu?.tempPackage, 1)],
    ...(cpu?.tempCcd ?? []).map((t, i): [string, string | null] => [`CCD${i + 1}`, fmtTemp(t, 1)]),
    ['Package power', unit(cpu?.powerPkgW, 'W')],
    ['Cores power', unit(cpu?.powerCoreW, 'W')],
    ['Core VID', unit(cpu?.vidCore, 'V', 3)],
    ['Effective clock', unit(cpu?.mhzEffective, 'MHz', 0)],
  ]);
  const perCore = $derived(cpu?.powerPerCoreW ?? []);
  // Even rows (1.2): 8 cores read 4 and 4, never 7 and one left over.
  let width = $state(0);
  const rem = () => parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
  const cols = $derived(width ? balancedColumns(perCore.length, width, 4.5 * rem(), 0.75 * rem()) : 4);
</script>

<Card title="CPU sensors" subtitle={cpu?.tempPackage == null ? needs : null}>
  <KV {rows} />
  {#if perCore.length}
    <ul class="cores" aria-label="Power per core" bind:clientWidth={width} style:--cols={cols}>
      {#each perCore as w, i (i)}
        <li><span class="idx">C{i}</span> {fmt(w, 1)} W</li>
      {/each}
    </ul>
  {/if}
</Card>

<style>
  .cores {
    display: grid;
    grid-template-columns: repeat(var(--cols), minmax(0, 6rem));
    gap: 0.3rem 0.75rem;
    margin: 0.75rem 0 0;
    padding: 0;
    list-style: none;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }
  .idx {
    color: var(--muted);
  }
</style>

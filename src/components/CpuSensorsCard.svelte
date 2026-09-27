<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt } from '../format';
  import { fmtTemp } from '../prefs.svelte';
  import type { CpuSnapshot } from '../types';

  interface Props {
    cpu: CpuSnapshot | null;
  }
  let { cpu }: Props = $props();

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
</script>

<Card title="CPU sensors" subtitle={cpu?.tempPackage == null ? 'needs PawnIO' : null}>
  <KV {rows} />
  {#if perCore.length}
    <ul class="cores" aria-label="Power per core">
      {#each perCore as w, i (i)}
        <li><span class="idx">C{i}</span> {fmt(w, 1)} W</li>
      {/each}
    </ul>
  {/if}
</Card>

<style>
  .cores {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(4.5rem, 1fr));
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

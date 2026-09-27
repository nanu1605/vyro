<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt } from '../format';
  import { fmtTemp } from '../prefs.svelte';
  import type { BoardSnapshot, FanSnapshot, VoltageReading } from '../types';

  interface Props {
    board: BoardSnapshot | null;
    fans: FanSnapshot[];
    voltages: VoltageReading[];
    pending: string;
  }
  let { board, fans, voltages, pending }: Props = $props();

  const temp = (v: number | null | undefined) => fmtTemp(v, 1);
  const temps = $derived<Array<[string, string | null]>>(
    (
      [
        ['System', board?.tempSystem],
        ['VRM MOS', board?.tempVrmMos],
        ['Chipset', board?.tempChipset],
        ['CPU socket', board?.tempSocket],
        ['PCIe', board?.tempPcie],
        ['M.2', board?.tempM2],
      ] as Array<[string, number | null | undefined]>
    )
      .filter(([, v]) => v != null)
      .map(([k, v]) => [k, temp(v)]),
  );
  const empty = $derived(temps.length === 0 && fans.length === 0 && voltages.length === 0);
</script>

<Card title="Board" subtitle={empty ? pending : null}>
  {#if empty}
    <p class="empty">Fans, voltages and board temperatures come from the motherboard sensor chip.</p>
  {:else}
    {#if temps.length}
      <KV rows={temps} />
    {/if}
    {#if fans.length}
      <h3>Fans</h3>
      <KV rows={fans.map((f): [string, string | null] => [f.name, f.rpm == null ? null : `${fmt(f.rpm)} RPM`])} />
    {/if}
    {#if voltages.length}
      <h3>Voltages</h3>
      <KV rows={voltages.map((v): [string, string | null] => [v.name, v.value == null ? null : `${fmt(v.value, 3)} V`])} />
    {/if}
  {/if}
</Card>

<style>
  h3 {
    margin: 0.9rem 0 0.45rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .empty {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
  }
</style>

<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt } from '../format';
  import type { GpuSnapshot, PowerSnapshot } from '../types';

  interface Props {
    power: PowerSnapshot | null;
    gpus: GpuSnapshot[];
  }
  let { power, gpus }: Props = $props();

  const PENDING = 'needs PawnIO';
  const watts = (w: number | null | undefined) => (w == null ? null : `${fmt(w, 1)} W`);
</script>

<Card title="Power" subtitle={power?.estTotalW != null ? `${fmt(power.estTotalW)} W estimated` : power?.cpuW != null ? 'CPU + GPU' : 'Partial: GPU only'}>
  <KV
    rows={[
      ['System total', power?.estTotalW != null ? `${fmt(power.estTotalW)} W` : PENDING],
      ['CPU package', power?.cpuW != null ? watts(power.cpuW) : PENDING],
      ['GPU (all)', watts(power?.gpuW)],
      ...gpus.map((g): [string, string | null] => [
        g.name,
        g.powerW != null ? `${fmt(g.powerW, 1)} W${g.powerLimitW != null ? ` of ${fmt(g.powerLimitW)} W` : ''}` : null,
      ]),
    ]}
  />
  {#if power?.cpuW == null}
    <p class="note">CPU power and the system total need PawnIO (free, signed driver from pawnio.eu).</p>
  {/if}
</Card>

<style>
  .note {
    margin: 0.75rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
</style>

<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { NEEDS_PAWNIO, NOT_REPORTED } from '../dashboard';
  import { fmt } from '../format';
  import type { GpuSnapshot, PowerSnapshot } from '../types';

  interface Props {
    power: PowerSnapshot | null;
    gpus: GpuSnapshot[];
    /** Why the CPU's power is missing ("needs PawnIO", "not read on this CPU"). */
    needs: string;
  }
  let { power, gpus, needs }: Props = $props();

  const watts = (w: number | null | undefined) => (w == null ? null : `${fmt(w, 1)} W`);
</script>

<!-- The total needs both: CPU power without the GPU's said "CPU + GPU" (2.0.4). -->
<Card
  title="Power"
  subtitle={power?.estTotalW != null
    ? `${fmt(power.estTotalW)} W estimated`
    : power?.cpuW != null
      ? 'Partial: CPU only'
      : power?.gpuW != null
        ? 'Partial: GPU only'
        : null}
>
  <KV
    rows={[
      ['System total', power?.estTotalW != null ? `${fmt(power.estTotalW)} W` : power?.cpuW != null ? NOT_REPORTED : needs],
      ['CPU package', power?.cpuW != null ? watts(power.cpuW) : needs],
      ['GPU (all)', watts(power?.gpuW)],
      ...gpus.map((g): [string, string | null] => [
        g.name,
        g.powerW != null ? `${fmt(g.powerW, 1)} W${g.powerLimitW != null ? ` of ${fmt(g.powerLimitW)} W` : ''}` : null,
      ]),
    ]}
  />
  {#if power?.cpuW == null && needs === NEEDS_PAWNIO}
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

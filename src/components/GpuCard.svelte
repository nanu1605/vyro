<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import Meter from './Meter.svelte';
  import { fmt, fmtGB } from '../format';
  import { fmtTemp } from '../prefs.svelte';
  import type { GpuSnapshot } from '../types';

  interface Props {
    gpu: GpuSnapshot;
  }
  let { gpu }: Props = $props();

  const e = $derived(gpu.engine);
  const kind = $derived(gpu.integrated == null ? null : gpu.integrated ? 'Integrated' : 'Discrete');
  const unit = (v: number | null, u: string, digits = 0) => (v == null ? null : `${fmt(v, digits)} ${u}`);

  const meters = $derived<Array<[string, number | null]>>([
    ['Overall', gpu.usage],
    ['3D', e.graphics3d],
    ['Video decode', e.decode],
    ['Video encode', e.encode],
    ['Compute', e.compute],
    ['Copy', e.copy],
    ['Tensor', e.tensor],
    ['Ray tracing', e.raytrace],
  ]);
  const rows = $derived<Array<[string, string | null]>>([
    ['Core clock', unit(gpu.mhzCore, 'MHz')],
    ['Memory clock', unit(gpu.mhzMem, 'MHz')],
    ['Temperature', fmtTemp(gpu.tempEdge)],
    ['Hotspot', fmtTemp(gpu.tempHotspot)],
    ['Memory temp', fmtTemp(gpu.tempMem)],
    ['Core voltage', unit(gpu.voltageV, 'V', 3)],
    ['VRAM', gpu.vramUsedMB != null ? `${fmtGB(gpu.vramUsedMB)} / ${fmtGB(gpu.vramTotalMB)} GB` : null],
    ['Power', gpu.powerW != null ? `${fmt(gpu.powerW, 1)} W${gpu.powerLimitW != null ? ` of ${fmt(gpu.powerLimitW)} W` : ''}` : null],
    ['Fan', [gpu.fanPct != null ? `${fmt(gpu.fanPct)}%` : null, unit(gpu.fanRpm, 'RPM')].filter(Boolean).join(' · ') || null],
    ['PCIe link', gpu.pcieGen != null ? `Gen ${gpu.pcieGen} ×${gpu.pcieWidth ?? '?'}` : null],
    ['Limiters', gpu.throttleReasons.length ? gpu.throttleReasons.join(', ') : gpu.throttleThermal != null ? 'None' : null],
  ]);
  // What this GPU doesn't report is named once at the end (1.2), not a line of "—" each: a
  // processor's graphics reports little, and its card was mostly empty lines.
  const shownMeters = $derived(meters.filter(([, v]) => v != null));
  const shownRows = $derived(rows.filter(([, v]) => v != null));
  const lower = (l: string) => (/^[A-Z][a-z]/.test(l) ? l[0]!.toLowerCase() + l.slice(1) : l);
  const missing = $derived([...meters, ...rows].filter(([, v]) => v == null).map(([l]) => lower(l)));
</script>

<Card title={[gpu.vendor, kind].filter(Boolean).join(' · ') || 'GPU'} subtitle={gpu.name}>
  {#if shownMeters.length}
    <div class="engines">
      {#each shownMeters as [label, value] (label)}<Meter {label} {value} />{/each}
    </div>
  {/if}
  {#if shownRows.length}<KV rows={shownRows} />{/if}
  {#if missing.length}<p class="missing">Not reported: {missing.join(', ')}</p>{/if}
</Card>

<style>
  .engines {
    --accent: var(--accent-gpu);
    display: grid;
    gap: 0.4rem;
    margin-bottom: 1rem;
  }
  .missing {
    margin: 0.75rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
</style>

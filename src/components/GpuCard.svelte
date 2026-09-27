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
</script>

<Card title={[gpu.vendor, kind].filter(Boolean).join(' · ') || 'GPU'} subtitle={gpu.name}>
  <div class="engines">
    <Meter label="Overall" value={gpu.usage} />
    <Meter label="3D" value={e.graphics3d} />
    <Meter label="Video decode" value={e.decode} />
    <Meter label="Video encode" value={e.encode} />
    <Meter label="Compute" value={e.compute} />
    <Meter label="Copy" value={e.copy} />
    <Meter label="Tensor" value={e.tensor} note="N/A" />
    <Meter label="Ray tracing" value={e.raytrace} note="N/A" />
  </div>
  <KV
    rows={[
      ['Core clock', unit(gpu.mhzCore, 'MHz')],
      ['Memory clock', unit(gpu.mhzMem, 'MHz')],
      ['Temperature', fmtTemp(gpu.tempEdge)],
      ['Hotspot', fmtTemp(gpu.tempHotspot)],
      ['Memory temp', fmtTemp(gpu.tempMem)],
      ['Core voltage', unit(gpu.voltageV, 'V', 3)],
      ['VRAM', gpu.vramUsedMB != null ? `${fmtGB(gpu.vramUsedMB)} / ${fmtGB(gpu.vramTotalMB)} GB` : null],
      [
        'Power',
        gpu.powerW != null
          ? `${fmt(gpu.powerW, 1)} W${gpu.powerLimitW != null ? ` of ${fmt(gpu.powerLimitW)} W` : ''}`
          : null,
      ],
      ['Fan', [gpu.fanPct != null ? `${fmt(gpu.fanPct)}%` : null, unit(gpu.fanRpm, 'RPM')].filter(Boolean).join(' · ') || null],
      ['PCIe link', gpu.pcieGen != null ? `Gen ${gpu.pcieGen} ×${gpu.pcieWidth ?? '?'}` : null],
      [
        'Limiters',
        gpu.throttleReasons.length
          ? gpu.throttleReasons.join(', ')
          : gpu.throttleThermal != null
            ? 'None'
            : null,
      ],
    ]}
  />
</Card>

<style>
  .engines {
    --accent: var(--accent-gpu);
    display: grid;
    gap: 0.4rem;
    margin-bottom: 1rem;
  }
</style>

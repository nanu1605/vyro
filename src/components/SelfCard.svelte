<script lang="ts">
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import { fmt } from '../format';
  import type { AppSnapshot } from '../types';

  interface Props {
    app: AppSnapshot | null;
    uiFps: number;
  }
  let { app, uiFps }: Props = $props();

  // Spec §8 budget: steady CPU < 1.5 %, working set < 120 MB, first data < 2 s.
  const budget = (v: number | null | undefined, limit: number, text: string) =>
    v == null ? null : `${text} ${v < limit ? '✓' : '✗'}`;
</script>

<Card title="Vyro itself" subtitle={app?.ramMB != null ? `${fmt(app.ramMB)} MB · ${fmt(app.cpuPct, 1)}% CPU` : 'Footprint'}>
  <KV
    rows={[
      ['CPU', app?.cpuPct != null ? `${fmt(app.cpuPct, 2)}%` : null],
      ['Memory', app?.ramMB != null ? `${fmt(app.ramMB)} MB` : null],
      ['GPU', app?.gpuPct != null ? `${fmt(app.gpuPct, 1)}%` : null],
      ['Poll rate', app?.pollHz != null ? `${fmt(app.pollHz, 1)} Hz` : null],
      ['UI frame rate', `${uiFps} fps`],
      ['Active sensors', app?.activeSensors ?? null],
      ['Engine', app?.engine ?? null],
    ]}
  />
  <h4>Budget</h4>
  <KV
    rows={[
      ['Average CPU (< 1.5%)', budget(app?.cpuAvgPct, 1.5, `${fmt(app?.cpuAvgPct, 2)}%`)],
      ['Peak memory (< 120 MB)', budget(app?.ramPeakMB, 120, `${fmt(app?.ramPeakMB)} MB`)],
      ['Startup to first data (< 2 s)', budget(app?.startupMs != null ? app.startupMs / 1000 : null, 2, `${fmt(app?.startupMs != null ? app.startupMs / 1000 : null, 2)} s`)],
    ]}
  />
  {#if app?.parts.length}
    <h4>Processes</h4>
    <KV
      rows={app.parts.map((p) => [
        p.count > 1 ? `${p.name} ×${p.count}` : p.name,
        `${fmt(p.ramMB)} MB · ${fmt(p.cpuPct, 2)}%`,
      ])}
    />
  {/if}
  <p class="note">
    Includes the sensor service and the app's WebView2 processes. Averages and peaks run since start (reset them in
    Sensors); the budget assumes the window stays open. The UI only redraws while values change.
  </p>
</Card>

<style>
  h4 {
    margin: 0.9rem 0 0.4rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .note {
    margin: 0.75rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
</style>

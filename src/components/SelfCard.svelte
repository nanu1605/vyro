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
  <p class="note">Includes the app's WebView2 processes. The UI only redraws while values change.</p>
</Card>

<style>
  .note {
    margin: 0.75rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
</style>

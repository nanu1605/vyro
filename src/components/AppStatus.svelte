<script lang="ts">
  import type { AppSnapshot } from '../types';
  import { fmt } from '../format';

  interface Props {
    app: AppSnapshot | null;
    uiFps: number;
    source: string;
  }
  let { app, uiFps, source }: Props = $props();
</script>

<footer class="status" aria-label="Engine status">
  <span class="pill {app?.engine ?? 'starting'}">{app?.engine ?? 'starting'}</span>
  <span>source <b>{source}</b></span>
  <span>poll <b>{fmt(app?.pollHz, 1)}</b> Hz</span>
  <span>UI <b>{uiFps}</b> fps</span>
  <span>sensors <b>{app?.activeSensors ?? 0}</b></span>
  <span>app CPU <b>{fmt(app?.cpuPct, 1)}</b>%</span>
  <span>app RAM <b>{fmt(app?.ramMB)}</b> MB</span>
</footer>

<style>
  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1.1rem;
    padding: 0.6rem 0.2rem 0;
    color: var(--muted);
    font-size: 0.75rem;
  }
  b {
    color: var(--text);
    font-family: var(--font-num);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .pill {
    padding: 0.1rem 0.55rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 0.68rem;
  }
  .pill.running {
    border-color: color-mix(in srgb, var(--ok) 50%, transparent);
    color: var(--ok);
  }
  .pill.throttled {
    border-color: color-mix(in srgb, var(--warn) 50%, transparent);
    color: var(--warn);
  }
</style>

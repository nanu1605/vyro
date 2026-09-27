<script lang="ts">
  import type { BackendStatus } from '../types';

  interface Props {
    status: BackendStatus | null;
  }
  let { status }: Props = $props();

  const message = $derived.by(() => {
    if (!status) return null;
    if (status.mode === 'local') {
      return (
        `The Vyro sensor service is not running${status.detail ? ` (${status.detail})` : ''}, so CPU temperature ` +
        'and power, board fans and voltages, RAM temperatures, drive health and history while Vyro is closed ' +
        'are unavailable. Reinstall Vyro, or start “Vyro Sensor Service” in Services, then restart Vyro.'
      );
    }
    if (!status.connected) {
      return `Reconnecting to the Vyro sensor service…${status.detail ? ` (${status.detail})` : ''}`;
    }
    return null;
  });
</script>

{#if message}
  <div class="banner" role="status">{message}</div>
{/if}

<style>
  .banner {
    margin-bottom: 1rem;
    padding: 0.65rem 0.9rem;
    border: 1px solid var(--warn, #d9a441);
    border-radius: 10px;
    font-size: 0.85rem;
  }
</style>

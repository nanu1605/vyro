<script lang="ts">
  import type { PawnIoStatus } from '../types';

  interface Props {
    status: PawnIoStatus | null;
  }
  let { status }: Props = $props();

  const message = $derived.by(() => {
    switch (status?.state) {
      case 'notInstalled':
        return 'PawnIO is not installed. CPU temperature, CPU power and board fans need it.';
      case 'outdated':
        return `PawnIO ${status.version ?? ''} is too old for Vyro. Install the current version.`;
      case 'notRunning':
        return 'PawnIO is installed but its driver is not running. Restart Windows or reinstall PawnIO.';
      case 'noAccess':
        return 'Run Vyro as administrator to read CPU temperature and power through PawnIO.';
      case 'error':
        return `PawnIO could not be used${status.detail ? `: ${status.detail}` : '.'}`;
      default:
        return null;
    }
  });
  const install = $derived(status?.state === 'notInstalled' || status?.state === 'outdated');
</script>

{#if message}
  <div class="banner" role="status">
    <span>{message}</span>
    {#if install}
      <span class="url">Download: <code>https://pawnio.eu</code></span>
    {/if}
  </div>
{/if}

<style>
  .banner {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem 1rem;
    align-items: baseline;
    margin-bottom: 1rem;
    padding: 0.65rem 0.9rem;
    border: 1px solid var(--warn, #d9a441);
    border-radius: 10px;
    font-size: 0.85rem;
  }
  .url {
    color: var(--muted);
  }
  code {
    user-select: all;
  }
</style>

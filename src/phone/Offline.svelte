<script lang="ts">
  // A PC that does not answer (1.7): asleep or off, the relay out of reach, open elsewhere, too
  // old, its pairing expired or signed out. Loaded only then: most opens find the PC online.
  import ClockAlert from '@lucide/svelte/icons/clock-alert';
  import CloudOff from '@lucide/svelte/icons/cloud-off';
  import Download from '@lucide/svelte/icons/download';
  import LogOut from '@lucide/svelte/icons/log-out';
  import Moon from '@lucide/svelte/icons/moon';
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import type { LinkState } from './relay';
  import State from './ui/State.svelte';

  interface Props {
    state: LinkState;
    pc: string;
    /** Another paired PC can wake this one. */
    canWake: boolean;
    /** How many PCs this phone is paired with. */
    paired: number;
    waking: string | null;
    onwake: () => void;
    onuse: () => void;
    onforget: () => void;
  }
  let { state, pc, canWake, paired, waking, onwake, onuse, onforget }: Props = $props();
</script>

{#if state === 'asleep'}
  <State icon={Moon} title="{pc} is asleep or off">
    <p>Its readings come back here as soon as it wakes, with Vyro running and <b>Reach from anywhere</b> on.</p>
    {#if !canWake}
      <p>
        {paired > 1
          ? `To wake ${pc} from here, turn on Wake from phone on it (Settings → Remote) while it is on.`
          : `A phone can’t wake a PC by itself: another PC with Vyro on the same network sends the signal. Pair one with this phone, and turn on Wake from phone on ${pc}.`}
      </p>
    {/if}
    {#if waking}<p role="status">{waking}</p>{/if}
    {#snippet actions()}
      {#if canWake}<button type="button" onclick={onwake}>Wake {pc}</button>{/if}
    {/snippet}
  </State>
{:else if state === 'unreachable'}
  <State icon={CloudOff} title="Can’t reach the Vyro relay" tone="warn">
    <p>Check this phone’s connection. Trying again by itself…</p>
  </State>
{:else if state === 'elsewhere'}
  <State icon={Smartphone} title="Open in another window">
    <p>{pc} is open in another window of Vyro on this device.</p>
    {#snippet actions()}
      <button type="button" onclick={onuse}>Use it here</button>
    {/snippet}
  </State>
{:else if state === 'tooOld'}
  <State icon={Download} title="Update Vyro on {pc}" tone="warn">
    <p>This app needs a newer Vyro on the PC. Vyro updates from its tray menu → Check for updates.</p>
  </State>
{:else}
  <State icon={state === 'expired' ? ClockAlert : LogOut} title={state === 'expired' ? 'That pairing code has expired' : `Signed out of ${pc}`} tone="hot">
    <p>
      {state === 'expired'
        ? 'It lasts 10 minutes and works once. In Vyro on the PC, show a new one: Settings → Remote → Pair a phone.'
        : 'The PC signed this phone out, or its pairing ran out. Pair it again from Vyro: Settings → Remote → Pair a phone.'}
    </p>
    {#snippet actions()}
      <button type="button" onclick={onforget}>Remove {pc}</button>
    {/snippet}
  </State>
{/if}

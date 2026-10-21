<script lang="ts">
  // The home screen (1.7): the PCs this phone is paired with and how each is, then pairing one in
  // three steps (scanning first, pasting the link folded away), and installing the app.
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import Monitor from '@lucide/svelte/icons/monitor';
  import ScanQrCode from '@lucide/svelte/icons/scan-qr-code';
  import type { LinkState } from './relay';
  import type { PairedPc } from './pcs';

  interface Props {
    pcs: PairedPc[];
    states: Record<string, LinkState>;
    words: Record<LinkState, string>;
    /** Opened from the Home Screen (installed). */
    standalone: boolean;
    ios: boolean;
    /** Chrome can install the app here. */
    installable: boolean;
    onopen: (room: string) => void;
    onscan: () => void;
    onpaste: (text: string) => void;
    oninstall: () => void;
  }
  let { pcs, states, words, standalone, ios, installable, onopen, onscan, onpaste, oninstall }: Props = $props();

  let pasted = $state('');
</script>

{#if pcs.length}
  <section class="panel rise" aria-labelledby="pcs-title">
    <h1 id="pcs-title">Your PCs</h1>
    <ul class="pcs">
      {#each pcs as p, i (p.room)}
        {@const s = states[p.room] ?? 'connecting'}
        <li class="rise" style:--i={i + 1}>
          <button type="button" class="pc-row" onclick={() => onopen(p.room)}>
            <span class="pc-icon" aria-hidden="true"><Monitor size={22} /></span>
            <span class="pc-text">
              <span class="pc-name">{p.name}</span>
              <span class="pc-state {s}">{words[s]}</span>
            </span>
            <ChevronRight size={20} aria-hidden="true" />
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<section class="panel rise" style:--i={pcs.length ? 2 : 0} aria-labelledby="pair-title">
  <h1 id="pair-title">{pcs.length ? 'Pair another PC' : 'Pair with your PC'}</h1>
  <ol class="steps">
    <li>Open <b>Vyro</b> on the PC.</li>
    <li>Go to <b>Settings → Remote → From anywhere → Pair a phone</b>.</li>
    <li>Scan the code it shows {standalone ? 'here' : 'with this phone’s camera'}. It lasts 10 minutes.</li>
  </ol>
  <button type="button" class="scan" onclick={onscan}>
    <ScanQrCode size={20} aria-hidden="true" />
    <span>Scan the code</span>
  </button>
  <details class="paste">
    <summary>Or paste the pairing link</summary>
    <form
      onsubmit={(e) => {
        e.preventDefault();
        onpaste(pasted);
        pasted = '';
      }}
    >
      <label for="paste" class="sr-only">Pairing link</label>
      <div class="paste-row">
        <input id="paste" bind:value={pasted} autocomplete="off" spellcheck="false" inputmode="url" placeholder="https://…" />
        <button type="submit" class="ghost" disabled={!pasted.trim()}>Pair</button>
      </div>
    </form>
  </details>
</section>

{#if !standalone}
  <section class="panel rise" style:--i={3} aria-labelledby="install-help">
    <h2 id="install-help">Install the app</h2>
    {#if installable}
      <p>Vyro works like an app from your Home Screen, with no app store.</p>
      <div class="actions"><button type="button" onclick={oninstall}>Install Vyro</button></div>
    {:else if ios}
      <p>In Safari, tap <b>Share</b> → <b>Add to Home Screen</b>, then pair from the app there.</p>
    {:else}
      <p>In your browser’s menu, choose <b>Install app</b> or <b>Add to Home screen</b>.</p>
    {/if}
  </section>
{/if}

<p class="muted small about">
  Everything travels through the Vyro relay sealed with keys only this phone and your PC have; the relay cannot read it.
  Whether this phone may change a PC is decided on that PC.
</p>

<style>
  .pcs {
    display: grid;
    gap: 0.5rem;
    margin: 0.25rem 0 0;
    padding: 0;
    list-style: none;
  }
  .pc-row {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.75rem;
    padding: 0.75rem 0.8rem;
    border: 1px solid var(--line);
    background: color-mix(in srgb, var(--bg) 60%, transparent);
    color: var(--text);
    text-align: left;
  }
  .pc-row > :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .pc-icon {
    display: grid;
    flex: none;
    width: 2.6rem;
    height: 2.6rem;
    place-items: center;
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .pc-text {
    display: grid;
    flex: 1;
    gap: 0.1rem;
    min-width: 0;
  }
  .pc-name {
    overflow: hidden;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pc-state {
    color: var(--muted);
    font-size: 0.8rem;
    font-weight: 500;
  }
  .pc-state.online {
    color: var(--text);
  }
  .pc-state.online::before {
    content: '';
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-right: 0.35rem;
    border-radius: 50%;
    background: var(--ok);
  }
  .pc-state.connecting::before {
    content: '';
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    margin-right: 0.35rem;
    border-radius: 50%;
    background: var(--warn);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .steps {
    display: grid;
    gap: 0.55rem;
    margin: 0.25rem 0 1rem;
    padding: 0;
    list-style: none;
    counter-reset: step;
  }
  .steps li {
    position: relative;
    padding-left: 2.2rem;
    line-height: 1.45;
    counter-increment: step;
  }
  .steps li::before {
    content: counter(step);
    position: absolute;
    top: 0;
    left: 0;
    display: grid;
    width: 1.55rem;
    height: 1.55rem;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
    font-size: 0.8rem;
    font-weight: 700;
  }
  .scan {
    display: flex;
    width: 100%;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    min-height: 3rem;
  }
  .paste {
    margin-top: 0.9rem;
    font-size: 0.9rem;
  }
  .paste summary {
    color: var(--muted);
    cursor: pointer;
  }
  .paste-row {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.6rem;
  }
  .paste-row input {
    flex: 1;
    min-width: 0;
  }
  .about {
    margin: 0.5rem 0.25rem 0;
  }
</style>

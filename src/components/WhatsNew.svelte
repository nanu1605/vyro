<script lang="ts">
  // What's new (1.5): the notes of the versions since the last one shown, after an update, or
  // every version's when opened from Settings or the palette. Loaded only when it opens.
  import { onMount, type Component } from 'svelte';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import Cpu from '@lucide/svelte/icons/cpu';
  import Fan from '@lucide/svelte/icons/fan';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Mic from '@lucide/svelte/icons/mic';
  import MonitorSmartphone from '@lucide/svelte/icons/monitor-smartphone';
  import Power from '@lucide/svelte/icons/power';
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import Sunrise from '@lucide/svelte/icons/sunrise';
  import { compareVersions, NOTES, notesSince, shortVersion, type NoteIcon } from '../whatsnew';

  interface Props {
    /** This Vyro's version ('' when not known yet: the newest notes). */
    current: string;
    /** The version updated from; null: every version's notes (or an update from before 1.5). */
    from: string | null;
    /** Every version's notes, not only those since `from`. */
    all: boolean;
    onclose: () => void;
  }
  let { current, from, all, onclose }: Props = $props();

  const ICONS: Record<NoteIcon, Component> = {
    phone: Smartphone,
    end: CircleX,
    power: Power,
    wake: Sunrise,
    fan: Fan,
    gauge: Gauge,
    gpu: Cpu,
    screen: MonitorSmartphone,
    mic: Mic,
    sparkles: Sparkles,
  };

  const version = $derived(current || NOTES[0]?.version || '');
  const releases = $derived(all ? NOTES.filter((r) => compareVersions(r.version, version) <= 0) : notesSince(from, version));
  const first = $derived(releases[0]);
  const rest = $derived(releases.slice(1));

  let dialog = $state<HTMLDialogElement | null>(null);

  onMount(() => {
    dialog?.showModal();
  });
</script>

<dialog bind:this={dialog} class="whatsnew" aria-labelledby="whatsnew-title" {onclose}>
  <header>
    <span class="badge" aria-hidden="true"><Sparkles size={22} /></span>
    <div>
      <h2 id="whatsnew-title">What’s new in Vyro {shortVersion(first?.version ?? version)}</h2>
      {#if from && !all}<p class="sub">Updated from {shortVersion(from)}</p>{/if}
    </div>
  </header>

  {#if first}
    <ul class="items">
      {#each first.items as item, i (item.title)}
        {@const Icon = ICONS[item.icon]}
        <li style:--i={i}>
          <span class="icon" aria-hidden="true"><Icon size={18} /></span>
          <span class="words">
            <b>{item.title}</b>
            <span>{item.text}</span>
          </span>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="sub">This version has no notes.</p>
  {/if}

  {#each rest as r (r.version)}
    <details>
      <summary>Vyro {shortVersion(r.version)}</summary>
      <ul class="items small">
        {#each r.items as item (item.title)}
          {@const Icon = ICONS[item.icon]}
          <li>
            <span class="icon" aria-hidden="true"><Icon size={16} /></span>
            <span class="words">
              <b>{item.title}</b>
              <span>{item.text}</span>
            </span>
          </li>
        {/each}
      </ul>
    </details>
  {/each}

  <div class="actions">
    <!-- svelte-ignore a11y_autofocus -->
    <button type="button" class="primary" autofocus onclick={() => dialog?.close()}>Got it</button>
  </div>
</dialog>

<style>
  .whatsnew {
    width: min(36rem, calc(100vw - 2rem));
    max-height: calc(100vh - 3rem);
    padding: 1.3rem 1.4rem 1.1rem;
    overflow-y: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text);
    box-shadow: var(--shadow-lift);
  }
  .whatsnew[open] {
    animation: rise var(--dur-3) var(--ease-out);
  }
  .whatsnew::backdrop {
    background: rgb(0 0 0 / 55%);
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.85rem;
    margin-bottom: 1rem;
  }
  .badge {
    display: grid;
    flex: none;
    width: 2.6rem;
    height: 2.6rem;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent);
  }
  h2 {
    margin: 0;
    font-size: 1.15rem;
  }
  .sub {
    margin: 0.15rem 0 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .items {
    display: grid;
    gap: 0.75rem;
    margin: 0 0 1rem;
    padding: 0;
    list-style: none;
  }
  .items li {
    display: flex;
    gap: 0.75rem;
    align-items: flex-start;
    animation: item var(--dur-3) var(--ease-out) backwards;
    animation-delay: calc(var(--i, 0) * 50ms + 80ms);
  }
  .icon {
    display: grid;
    flex: none;
    width: 2rem;
    height: 2rem;
    place-items: center;
    border: 1px solid var(--line);
    border-radius: 10px;
    color: var(--accent);
  }
  .words {
    display: grid;
    gap: 0.15rem;
    min-width: 0;
    font-size: 0.86rem;
    line-height: 1.4;
  }
  .words span {
    color: var(--muted);
  }
  .items.small li {
    animation: none;
  }
  .items.small .icon {
    width: 1.75rem;
    height: 1.75rem;
  }
  details {
    margin: 0 0 0.75rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--line);
  }
  summary {
    margin-bottom: 0.6rem;
    color: var(--muted);
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .primary {
    padding: 0.5rem 1.2rem;
    border: 1px solid color-mix(in srgb, var(--accent) 60%, var(--line));
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
    color: var(--text);
    font: inherit;
    font-size: 0.86rem;
    font-weight: 600;
    cursor: pointer;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
  }
  @keyframes item {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>

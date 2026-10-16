<script lang="ts">
  // The page's header (P28): which view this is, the states that matter from anywhere (fan
  // control, the phone companion, an update), and the command palette.
  import Search from '@lucide/svelte/icons/search';
  import { view, type Tab } from '../../nav';
  import type { FanBrief, RemoteBrief } from '../../types';

  interface Props {
    tab: Tab;
    remote: RemoteBrief | null;
    fans: FanBrief | null;
    /** A newer version, when one is waiting. */
    update: string | null;
    onselect: (t: Tab) => void;
    onremote: () => void;
    onbios: () => void;
    onsearch: () => void;
  }
  let { tab, remote, fans, update, onselect, onremote, onbios, onsearch }: Props = $props();

  const v = $derived(view(tab));
  const isMac = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform);
</script>

<header class="top">
  <div class="title">
    {#key tab}
      <h2 class="name">{v.label}</h2>
    {/key}
    <span class="blurb">{v.blurb}</span>
  </div>
  <div class="chips">
    {#if update}
      <button type="button" class="chip update" onclick={() => onselect('settings')} title="Open Settings → Updates">
        <span class="led" aria-hidden="true"></span>
        Vyro {update} is available
      </button>
    {/if}
    {#if remote?.running}
      <button
        type="button"
        class="chip"
        onclick={onremote}
        title="Open Settings → Remote"
        aria-label="Remote companion on{remote.watching ? `, ${remote.watching} watching` : ''}. Open its settings."
      >
        <span class="led ring" class:live={remote.watching > 0} aria-hidden="true"></span>
        Remote{remote.watching ? ` · ${remote.watching} watching` : ''}
      </button>
    {/if}
    {#if fans?.active}
      <div class="chip fans" role="status">
        <button type="button" class="part" onclick={() => onselect('cooling')}>
          <span class="led" class:hot={fans.fullSpeed} aria-hidden="true"></span>
          {fans.fullSpeed ? 'Fans at full speed' : 'Fan control active'}
        </button>
        <button type="button" class="part bios" onclick={onbios} title="Hand every fan back to the BIOS">Return to BIOS</button>
      </div>
    {/if}
    <button type="button" class="search" onclick={onsearch} aria-label="Search" aria-keyshortcuts={isMac ? 'Meta+K' : 'Control+K'}>
      <Search size={15} aria-hidden="true" />
      <span>Search</span>
      <kbd>{isMac ? '⌘' : 'Ctrl'} K</kbd>
    </button>
  </div>
</header>

<style>
  .top {
    position: sticky;
    top: 0;
    z-index: 4;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem 1rem;
    min-height: var(--topbar-h);
    margin: 0 calc(-1 * var(--gutter)) 1rem;
    padding: 0.6rem var(--gutter);
    border-bottom: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
    background: color-mix(in srgb, var(--bg) 78%, transparent);
    backdrop-filter: blur(16px) saturate(1.15);
  }
  /* The page state (idle, gaming, warning, critical) as a line along the bar's foot. */
  .top::after {
    position: absolute;
    right: 0;
    bottom: -1px;
    left: 0;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--state) 20%, var(--state) 80%, transparent);
    opacity: 0.3;
    content: '';
    pointer-events: none;
  }
  :global(:root:not([data-state='idle'])) .top::after {
    opacity: 1;
  }
  /* Critical pulses four times, then stays lit. */
  :global(:root[data-state='critical']) .top::after {
    animation: state-pulse 1.2s ease-in-out 8 alternate;
  }
  @keyframes state-pulse {
    to {
      opacity: 0.3;
    }
  }
  .title {
    display: flex;
    flex: 1;
    align-items: baseline;
    gap: 0.75rem;
    min-width: 0;
  }
  .name {
    margin: 0;
    font-size: 1.15rem;
    font-weight: 650;
    letter-spacing: 0.01em;
    white-space: nowrap;
    animation: title-in var(--dur-3) var(--ease-out);
  }
  @keyframes title-in {
    from {
      opacity: 0;
      transform: translateX(-6px);
    }
  }
  .blurb {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.8rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    min-height: 2rem;
    padding: 0 0.75rem;
    border: 1px solid var(--accent-line);
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .chip.update {
    border-color: color-mix(in srgb, var(--accent-gpu) 50%, var(--line));
    background: color-mix(in srgb, var(--accent-gpu) 8%, transparent);
  }
  .chip.update .led {
    background: var(--accent-gpu);
  }
  .fans {
    gap: 0.2rem;
    padding: 0.15rem;
    cursor: default;
  }
  .part {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    min-height: 1.7rem;
    padding: 0 0.6rem;
    border: 0;
    border-radius: 999px;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .bios {
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
  }
  .led {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--accent);
  }
  .led.ring {
    border: 1.5px solid var(--accent);
    background: none;
  }
  .led.ring.live {
    background: var(--accent);
    animation: pulse 2s ease-in-out infinite;
  }
  .led.hot {
    background: var(--hot);
  }
  @keyframes pulse {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 25%, transparent);
    }
  }
  .search {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    min-height: 2rem;
    padding: 0 0.5rem 0 0.7rem;
    border: 1px solid var(--line);
    border-radius: 9px;
    background: color-mix(in srgb, var(--panel) 80%, transparent);
    color: var(--muted);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
    transition: border-color var(--dur-2);
  }
  .search:hover {
    border-color: var(--accent-line);
    color: var(--text);
  }
  kbd {
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--line);
    border-radius: 5px;
    background: var(--bg);
    font: inherit;
    font-size: 0.68rem;
  }
  @media (max-width: 640px) {
    .blurb,
    .search span,
    .search kbd {
      display: none;
    }
  }
</style>

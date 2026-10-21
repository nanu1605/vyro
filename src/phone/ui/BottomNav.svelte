<script lang="ts" generics="T extends string">
  // The tabs at the bottom (1.7), within the thumb's reach: an icon and a word each, the one open
  // marked by a pill that slides to it. A dot says something there wants a look.
  import type Icon from '@lucide/svelte/icons/activity';

  interface Tab {
    id: T;
    label: string;
    icon: typeof Icon;
    /** Something there wants a look: its words for screen readers. */
    dot?: string | null;
  }
  interface Props {
    /** What the tabs are of (the PC's name). */
    label: string;
    tabs: Tab[];
    current: T;
    onselect: (id: T) => void;
  }
  let { label, tabs, current, onselect }: Props = $props();

  const at = $derived(Math.max(0, tabs.findIndex((t) => t.id === current)));
</script>

<nav class="tabs" aria-label={label} style:--n={tabs.length} style:--at={at}>
  <span class="pill" aria-hidden="true"></span>
  {#each tabs as t (t.id)}
    <button
      type="button"
      class="tab"
      aria-current={current === t.id ? 'page' : undefined}
      aria-describedby={t.dot ? `dot-${t.id}` : undefined}
      onclick={() => onselect(t.id)}
    >
      <span class="icon">
        <t.icon size={22} aria-hidden="true" />
        {#if t.dot}<span class="dot" aria-hidden="true"></span>{/if}
      </span>
      <span class="word">{t.label}</span>
    </button>
    {#if t.dot}<span id="dot-{t.id}" class="sr-only">{t.dot}</span>{/if}
  {/each}
</nav>

<style>
  .tabs {
    position: fixed;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 30;
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    max-width: 40rem;
    margin: 0 auto;
    padding: 0.35rem max(0.4rem, env(safe-area-inset-right)) calc(0.35rem + env(safe-area-inset-bottom))
      max(0.4rem, env(safe-area-inset-left));
    border-top: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
    background: color-mix(in srgb, var(--bg) 86%, transparent);
    -webkit-backdrop-filter: blur(16px) saturate(150%);
    backdrop-filter: blur(16px) saturate(150%);
  }
  /* The pill under the open tab's icon, sliding from tab to tab. */
  .pill {
    position: absolute;
    top: 0.45rem;
    left: max(0.4rem, env(safe-area-inset-left));
    width: calc((100% - 2 * max(0.4rem, env(safe-area-inset-left))) / var(--n));
    height: 2rem;
    pointer-events: none;
    transform: translateX(calc(var(--at) * 100%));
    transition: transform var(--dur-3) var(--ease-spring);
  }
  .pill::before {
    content: '';
    display: block;
    width: 3.5rem;
    max-width: 90%;
    height: 100%;
    margin: 0 auto;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }
  .tab {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 0.15rem;
    min-width: 0;
    min-height: 3.4rem;
    padding: 0.2rem 0 0.1rem;
    border: 0;
    background: transparent;
    color: var(--muted);
    font-weight: 600;
  }
  .tab[aria-current='page'] {
    color: var(--text);
  }
  .icon {
    position: relative;
    display: grid;
    width: 3.5rem;
    max-width: 100%;
    height: 2rem;
    place-items: center;
  }
  .tab[aria-current='page'] .icon {
    color: var(--accent);
  }
  .word {
    max-width: 100%;
    overflow: hidden;
    font-size: 0.7rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    position: absolute;
    top: 0.2rem;
    right: calc(50% - 0.95rem);
    width: 0.55rem;
    height: 0.55rem;
    border: 2px solid var(--bg);
    border-radius: 50%;
    background: var(--warn);
  }
</style>

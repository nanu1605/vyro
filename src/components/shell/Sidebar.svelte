<script lang="ts">
  // The app's navigation (P28): the views in groups down the left side, in place of the tab
  // strip that scrolled sideways. Still the WAI-ARIA tabs pattern (vertical): arrow keys move
  // and wrap, Home/End jump to the ends, ids stay `tab-<id>`. It folds to a rail of icons,
  // on request (remembered) or by itself in a narrow window; the names stay for screen
  // readers and show as tooltips.
  import { onMount, tick } from 'svelte';
  import PanelLeftClose from '@lucide/svelte/icons/panel-left-close';
  import PanelLeftOpen from '@lucide/svelte/icons/panel-left-open';
  import Laptop from '@lucide/svelte/icons/laptop';
  import { GROUPS, VIEWS, type Tab } from '../../nav';

  export type Dot = { tone: 'warn' | 'fail' | 'new'; note: string };

  interface Props {
    tab: Tab;
    onselect: (t: Tab) => void;
    /** Status dots by view. */
    dots: Partial<Record<Tab, Dot>>;
    /** This PC is a laptop: My PC gets a laptop icon. */
    laptop?: boolean;
    /** Engine connection, for the line at the bottom. */
    status: { tone: 'ok' | 'warn' | 'off'; text: string };
  }
  let { tab, onselect, dots, laptop = false, status }: Props = $props();

  const KEY = 'vyro.nav.rail';
  function remembered(): boolean {
    try {
      return localStorage.getItem(KEY) === '1';
    } catch {
      return false;
    }
  }
  let folded = $state(remembered());
  let narrow = $state(false);
  const rail = $derived(folded || narrow);
  function toggle(): void {
    folded = !folded;
    try {
      localStorage.setItem(KEY, folded ? '1' : '0');
    } catch {
      /* not remembered */
    }
  }

  onMount(() => {
    const mq = window.matchMedia('(max-width: 1100px)');
    narrow = mq.matches;
    const on = (e: MediaQueryListEvent) => (narrow = e.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  });

  /** Tab list keys: Up/Down (and Left/Right) move and wrap, Home/End jump to the ends. */
  function onKey(e: KeyboardEvent): void {
    const i = VIEWS.findIndex((x) => x.id === tab);
    const n = VIEWS.length;
    const at: Record<string, number> = {
      ArrowDown: (i + 1) % n,
      ArrowRight: (i + 1) % n,
      ArrowUp: (i + n - 1) % n,
      ArrowLeft: (i + n - 1) % n,
      Home: 0,
      End: n - 1,
    };
    const to = at[e.key];
    if (to == null) return;
    e.preventDefault();
    const next = VIEWS[to];
    if (next) {
      onselect(next.id);
      document.getElementById(`tab-${next.id}`)?.focus();
    }
  }

  // The highlight slides to the selected view. It is measured again whenever the list changes
  // size (the fold's width animation, a window resize), so it ends on the view, not where the
  // view was when the fold began.
  let list = $state<HTMLDivElement | undefined>();
  let mark = $state({ y: 0, h: 0 });
  function measure(): void {
    const b = list?.querySelector<HTMLElement>(`#tab-${tab}`);
    if (!list || !b) return;
    const r = b.getBoundingClientRect();
    const y = r.top - list.getBoundingClientRect().top - list.clientTop + list.scrollTop;
    if (Math.abs(y - mark.y) > 0.1 || Math.abs(r.height - mark.h) > 0.1) mark = { y, h: r.height };
  }
  $effect(() => {
    void tab;
    void rail;
    void tick().then(measure);
  });
  onMount(() => {
    if (!list || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(list);
    return () => ro.disconnect();
  });

  const groups = GROUPS.map((g) => ({ ...g, views: VIEWS.filter((v) => v.group === g.id) }));
</script>

<nav class="sidebar" class:rail aria-label="Vyro">
  <div class="brand">
    <svg class="logo" viewBox="0 0 32 32" aria-hidden="true">
      <defs>
        <linearGradient id="vyro-mark" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="var(--accent)" />
          <stop offset="1" stop-color="var(--accent-gpu)" />
        </linearGradient>
      </defs>
      <rect x="1" y="1" width="30" height="30" rx="9" fill="url(#vyro-mark)" opacity="0.18" />
      <path d="M8 9.5 16 23l8-13.5" fill="none" stroke="url(#vyro-mark)" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
    <div class="name">
      <h1>VYRO</h1>
      <span class="sub">hardware monitor</span>
    </div>
  </div>

  <div class="tabs" role="tablist" aria-label="Views" aria-orientation="vertical" tabindex="-1" onkeydown={onKey} bind:this={list}>
    <span class="mark" aria-hidden="true" style:transform="translateY({mark.y}px)" style:height="{mark.h}px"></span>
    {#each groups as g (g.id)}
      <div class="group" aria-hidden="true">{g.label}</div>
      {#each g.views as v (v.id)}
        {@const Icon = v.id === 'mypc' && laptop ? Laptop : v.icon}
        {@const dot = dots[v.id]}
        <button
          id="tab-{v.id}"
          role="tab"
          type="button"
          aria-selected={tab === v.id}
          aria-controls="panel-{v.id}"
          tabindex={tab === v.id ? 0 : -1}
          title={rail ? v.label : undefined}
          onclick={() => onselect(v.id)}
        >
          <span class="icon"><Icon size={18} strokeWidth={1.9} aria-hidden="true" /></span>
          <span class="label">{v.label}</span>
          {#if dot}
            <span class="dot {dot.tone}" aria-hidden="true"></span>
            <span class="sr-only">({dot.note})</span>
          {/if}
        </button>
      {/each}
    {/each}
  </div>

  <div class="foot">
    <span class="status {status.tone}" title={status.text}>
      <span class="led" aria-hidden="true"></span>
      <span class="label">{status.text}</span>
    </span>
    {#if !narrow}
      <button type="button" class="fold" onclick={toggle} aria-label={folded ? 'Expand the sidebar' : 'Collapse the sidebar'} title={folded ? 'Expand' : 'Collapse'}>
        {#if folded}<PanelLeftOpen size={18} aria-hidden="true" />{:else}<PanelLeftClose size={18} aria-hidden="true" />{/if}
      </button>
    {/if}
  </div>
</nav>

<style>
  .sidebar {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    width: var(--sidebar-w);
    height: 100vh;
    height: 100dvh;
    padding: 1rem 0.75rem 0.75rem;
    border-right: 1px solid var(--line);
    background: color-mix(in srgb, var(--panel) 82%, transparent);
    transition: width var(--dur-3) var(--ease-out);
  }
  .rail {
    width: var(--sidebar-rail);
  }
  @media (max-width: 640px) {
    .sidebar {
      padding-inline: 0.45rem;
    }
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    padding: 0 0.35rem 1rem;
  }
  .logo {
    flex: none;
    width: 2rem;
    height: 2rem;
  }
  .name {
    min-width: 0;
    white-space: nowrap;
  }
  h1 {
    margin: 0;
    color: var(--text);
    font-size: 1rem;
    letter-spacing: 0.32em;
  }
  .sub {
    color: var(--muted);
    font-size: 0.72rem;
  }
  .rail .name,
  .rail .label,
  .rail .group {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .tabs {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-height: 0;
    margin: 0 -0.25rem;
    padding: 0 0.25rem;
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .group {
    /* One line at every width: the views below keep their place while the sidebar unfolds. */
    overflow: hidden;
    margin: 0.85rem 0 0.3rem 0.65rem;
    color: var(--muted);
    font-size: 0.66rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .group:first-of-type {
    margin-top: 0;
  }
  .mark {
    position: absolute;
    top: 0;
    right: 0.25rem;
    left: 0.25rem;
    border-radius: 10px;
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
    transition:
      transform var(--dur-3) var(--ease-out),
      height var(--dur-3) var(--ease-out);
    pointer-events: none;
  }
  button[role='tab'] {
    position: relative;
    display: flex;
    flex: none;
    align-items: center;
    gap: 0.7rem;
    min-height: 2.35rem;
    padding: 0 0.7rem;
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.86rem;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    transition:
      color var(--dur-2),
      background-color var(--dur-2);
  }
  button[role='tab']:focus-visible {
    outline-offset: -2px;
  }
  button[role='tab']:hover {
    background: color-mix(in srgb, var(--text) 5%, transparent);
    color: var(--text);
  }
  button[aria-selected='true'] {
    color: var(--text);
  }
  button[aria-selected='true']:hover {
    background: transparent;
  }
  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 1.25rem;
    transition: transform var(--dur-2) var(--ease-spring);
  }
  button[role='tab']:hover .icon {
    transform: scale(1.08);
  }
  button[aria-selected='true'] .icon {
    color: var(--accent);
  }
  .rail button[role='tab'] {
    justify-content: center;
    padding: 0;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    flex: none;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--warn);
  }
  .dot.fail {
    background: var(--hot);
  }
  .dot.new {
    background: var(--accent);
  }
  .rail .dot {
    position: absolute;
    top: 0.45rem;
    right: 0.6rem;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 0.35rem 0;
    border-top: 1px solid var(--line);
  }
  .rail .foot {
    flex-direction: column;
  }
  .status {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 0.5rem;
    min-width: 0;
    color: var(--muted);
    font-size: 0.74rem;
  }
  .rail .status {
    justify-content: center;
  }
  .led {
    flex: none;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--muted);
  }
  .ok .led {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent);
  }
  .warn .led {
    background: var(--warn);
  }
  .fold {
    display: grid;
    flex: none;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .fold:hover {
    border-color: var(--accent-line);
    color: var(--text);
  }
</style>

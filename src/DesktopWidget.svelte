<script lang="ts">
  // A desktop widget's window (P30), `index.html#widget=<id>`: transparent and frameless, sized
  // by the app to the widget (src-tauri/src/widgets.rs). Unlocked, it is dragged by its body;
  // right-click opens its menu.
  import { onMount } from 'svelte';
  import DesktopWidgetView from './components/DesktopWidgetView.svelte';
  import { dragWidget, getWidgets, onWidgetSensors, onWidgets, widgetAction } from './ipc';
  import { loadPrefs, prefs, profileSpec } from './prefs.svelte';
  import { applyLook } from './themes';
  import { widgetIdFromHash, type DesktopWidget, type Widgets } from './widgets';
  import type { SensorSnapshot } from './types';

  const id = widgetIdFromHash(location.hash);
  let w = $state.raw<DesktopWidget | null>(null);
  let snap = $state.raw<SensorSnapshot | null>(null);
  let menu = $state<{ x: number; y: number } | null>(null);
  let first = $state<HTMLButtonElement>();

  const pick = (all: Widgets) => (w = all.widgets.find((x) => x.id === id) ?? null);

  onMount(() => {
    void loadPrefs().catch(() => {});
    const stops: Array<() => void> = [];
    let disposed = false;
    const keep = (p: Promise<() => void>) =>
      void p.then((fn) => (disposed ? fn() : stops.push(fn))).catch(() => {});
    void getWidgets().then(pick).catch(() => {});
    keep(onWidgets(pick));
    keep(onWidgetSensors((s) => (snap = s)));
    return () => {
      disposed = true;
      stops.forEach((f) => f());
    };
  });

  // The settings file corrects the first look (theme, accent) once loaded; the profile in force
  // sets the motion, as in the main window.
  $effect(() => applyLook({ theme: prefs.theme, accent: prefs.accent, openOn: prefs.openOn }));
  $effect(() => {
    document.documentElement.dataset.motion = profileSpec().motion;
  });

  function down(e: MouseEvent): void {
    if (e.button !== 0 || !w || w.locked || menu) return;
    void dragWidget();
  }

  function open(e: MouseEvent): void {
    e.preventDefault();
    const W = 168;
    const H = 132;
    menu = { x: Math.max(4, Math.min(e.clientX, innerWidth - W - 4)), y: Math.max(4, Math.min(e.clientY, innerHeight - H - 4)) };
    queueMicrotask(() => first?.focus());
  }

  function act(action: 'lock' | 'unlock' | 'remove' | 'edit'): void {
    menu = null;
    if (w) void widgetAction(w.id, action);
  }

  function key(e: KeyboardEvent): void {
    if (!menu) return;
    if (e.key === 'Escape') menu = null;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const items = [...document.querySelectorAll<HTMLButtonElement>('.menu button')];
      const i = items.indexOf(document.activeElement as HTMLButtonElement);
      items[(i + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    }
  }
</script>

<svelte:window onkeydown={key} onblur={() => (menu = null)} />

<main>
  {#if w}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="frame" class:locked={w.locked} onmousedown={down} oncontextmenu={open}>
      <DesktopWidgetView {w} {snap} />
    </div>
    {#if menu}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="scrim" onmousedown={() => (menu = null)} oncontextmenu={(e) => e.preventDefault()}></div>
      <div class="menu" role="menu" aria-label="Widget" style:left="{menu.x}px" style:top="{menu.y}px">
        <button bind:this={first} role="menuitem" onclick={() => act(w?.locked ? 'unlock' : 'lock')}>
          {w.locked ? 'Unlock (move it)' : 'Lock in place'}
        </button>
        <button role="menuitem" onclick={() => act('edit')}>Edit…</button>
        <button role="menuitem" class="danger" onclick={() => act('remove')}>Remove</button>
      </div>
    {/if}
  {:else}
    <p class="sr-only">Widget not found</p>
  {/if}
</main>

<style>
  .frame {
    position: fixed;
    inset: 0;
    cursor: grab;
    user-select: none;
  }
  .frame.locked {
    cursor: default;
  }
  .scrim {
    position: fixed;
    inset: 0;
  }
  .menu {
    position: fixed;
    display: grid;
    min-width: 10.5rem;
    padding: 0.3rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    box-shadow: var(--shadow-lift);
    animation: pop var(--dur-1) var(--ease-out);
  }
  .menu button {
    padding: 0.45rem 0.6rem;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--accent-soft);
    outline: none;
  }
  .menu .danger {
    color: var(--hot);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
</style>

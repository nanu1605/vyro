<script lang="ts">
  // Desktop widgets (P30): add widgets from a gallery, pick their readings and look, and show
  // or hide them all. Each widget is a small window of its own (src-tauri/src/widgets.rs);
  // this page only edits widgets.json and shows live previews.
  import { onMount } from 'svelte';
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import Lock from '@lucide/svelte/icons/lock';
  import LockOpen from '@lucide/svelte/icons/lock-open';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import DesktopWidgetView from './DesktopWidgetView.svelte';
  import { readingDef } from '../dashboard';
  import { readingIds } from '../dashboard-edit';
  import { getWidgets, inTauri, onWidgets, setWidgets } from '../ipc';
  import {
    DESK_KINDS,
    EMPTY,
    MAX_WIDGETS,
    kindOf,
    newWidget,
    widgetSize,
    withSources,
    type DeskKind,
    type DeskLayer,
    type DeskStyle,
    type DesktopWidget,
    type Widgets,
  } from '../widgets';
  import type { SensorSnapshot } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
  }
  let { snap }: Props = $props();

  let cfg = $state.raw<Widgets>(EMPTY);
  let loaded = $state(false);
  let status = $state('');

  onMount(() => {
    getWidgets()
      .then((w) => (cfg = w))
      .catch((e: unknown) => (status = `Widgets not loaded: ${String(e)}`))
      .finally(() => (loaded = true));
    let stop: (() => void) | undefined;
    let disposed = false;
    void onWidgets((w) => (cfg = w)).then((fn) => (disposed ? fn() : (stop = fn)));
    return () => {
      disposed = true;
      stop?.();
    };
  });

  async function save(next: Widgets, done: string): Promise<void> {
    cfg = next;
    try {
      cfg = await setWidgets(next);
      status = done;
    } catch (e) {
      status = `Not saved: ${String(e)}`;
    }
  }

  const patched = (id: string, f: (w: DesktopWidget) => DesktopWidget): Widgets => ({
    ...cfg,
    widgets: cfg.widgets.map((w) => (w.id === id ? f(w) : w)),
  });
  const change = (id: string, patch: Partial<DesktopWidget>, done = 'Saved.') =>
    void save(patched(id, (w) => ({ ...w, ...patch })), done);
  /** While a slider moves: the preview follows, the widget is saved when it is let go. */
  const live = (id: string, patch: Partial<DesktopWidget>) => (cfg = patched(id, (w) => ({ ...w, ...patch })));

  const full = $derived(cfg.widgets.length >= MAX_WIDGETS);
  const allLocked = $derived(cfg.widgets.length > 0 && cfg.widgets.every((w) => w.locked));

  function add(kind: DeskKind): void {
    if (full) return;
    const w = newWidget(kind, cfg.widgets.map((x) => x.id));
    void save({ ...cfg, shown: true, widgets: [...cfg.widgets, w] }, `${kindOf(kind).name} added${inTauri ? ' to the desktop' : ''}.`).then(() =>
      setTimeout(() => document.getElementById(`desk-${w.id}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }), 60),
    );
  }

  function remove(w: DesktopWidget): void {
    void save({ ...cfg, widgets: cfg.widgets.filter((x) => x.id !== w.id) }, `${title(w)} removed.`);
  }

  function lockAll(on: boolean): void {
    void save({ ...cfg, widgets: cfg.widgets.map((w) => ({ ...w, locked: on })) }, on ? 'All widgets locked.' : 'Widgets unlocked: drag them where you want them.');
  }

  // Readings, grouped as in the Dashboard editor.
  const groups = $derived.by(() => {
    const g = new Map<string, Array<{ id: string; label: string }>>();
    for (const id of readingIds(snap)) {
      const r = readingDef(id, snap);
      if (r) g.set(r.group, [...(g.get(r.group) ?? []), { id, label: r.label }]);
    }
    return [...g].map(([group, items]) => ({ group, items }));
  });
  const label = (id: string) => readingDef(id, snap)?.label ?? id;
  /** "Gauge · CPU usage"; a stack or a clock shows several readings, so only its kind. */
  const title = (w: DesktopWidget) =>
    kindOf(w.kind).sources > 1 ? kindOf(w.kind).name : `${kindOf(w.kind).name} · ${w.sources[0] ? label(w.sources[0]) : 'no reading'}`;

  function setSource(w: DesktopWidget, i: number, id: string): void {
    const s = [...w.sources];
    if (i < s.length) s[i] = id;
    else s.push(id);
    void save(patched(w.id, (x) => withSources(x, s)), 'Saved.');
  }
  function dropSource(w: DesktopWidget, i: number): void {
    void save(patched(w.id, (x) => withSources(x, x.sources.filter((_, j) => j !== i))), 'Saved.');
  }

  const STYLES: Array<{ id: DeskStyle; name: string }> = [
    { id: 'app', name: 'App' },
    { id: 'glass', name: 'Glass' },
    { id: 'solid', name: 'Solid' },
  ];
  const LAYERS: Array<{ id: DeskLayer; name: string }> = [
    { id: 'desktop', name: 'Behind windows' },
    { id: 'top', name: 'On top' },
  ];

  /** A preview at the widget's normal size (its own scale is the window's). */
  const preview = (w: DesktopWidget) => ({ ...w, scale: 100 });
  const dims = (w: DesktopWidget) => widgetSize(preview(w));
  const sample = (kind: DeskKind): DesktopWidget => ({ ...newWidget(kind, []), id: `sample-${kind}` });

  /** Shrinks a preview that is wider than its stage (a phone), keeping its real layout. */
  function fit(stage: HTMLElement, width: number) {
    let w = width;
    const apply = () => {
      const room = stage.clientWidth - 32;
      (stage.firstElementChild as HTMLElement | null)?.style.setProperty('--fit', String(room > 0 ? Math.min(1, room / w) : 1));
    };
    const ro = new ResizeObserver(apply);
    ro.observe(stage);
    apply();
    return {
      update(next: number) {
        w = next;
        apply();
      },
      destroy: () => ro.disconnect(),
    };
  }
</script>

{#snippet readingSelect(w: DesktopWidget, i: number, value: string, name: string)}
  <select aria-label={name} {value} onchange={(e) => e.currentTarget.value && setSource(w, i, e.currentTarget.value)}>
    {#if !value}<option value="">Add a reading…</option>{/if}
    {#each groups as g (g.group)}
      <optgroup label={g.group}>
        {#each g.items as o (o.id)}
          <option value={o.id} disabled={o.id !== value && w.sources.includes(o.id)}>{o.label}</option>
        {/each}
      </optgroup>
    {/each}
  </select>
{/snippet}

<div class="page">
  <section class="panel intro" aria-labelledby="desk-head">
    <div class="intro-text">
      <h2 id="desk-head">Desktop widgets</h2>
      <p>
        Live readings on your desktop, each in a small window of its own. Drag an unlocked widget to move it;
        right-click it to lock, edit or remove it.
      </p>
      {#if !inTauri}
        <p class="note">In the browser preview, widgets stay on this page; the app puts them on your desktop.</p>
      {/if}
    </div>
    <div class="intro-actions">
      <label class="check">
        <input
          type="checkbox"
          class="switch"
          role="switch"
          checked={cfg.shown}
          disabled={!loaded}
          onchange={(e) => void save({ ...cfg, shown: e.currentTarget.checked }, e.currentTarget.checked ? 'Widgets shown.' : 'Widgets hidden.')}
        />
        <span>Show on the desktop<small>Also in the tray icon's menu.</small></span>
      </label>
      <span class="count">{cfg.widgets.length} of {MAX_WIDGETS}</span>
      {#if cfg.widgets.length}
        <button type="button" class="btn" onclick={() => lockAll(!allLocked)}>
          {#if allLocked}<LockOpen size={15} aria-hidden="true" /> Unlock all to arrange{:else}<Lock size={15} aria-hidden="true" /> Lock all{/if}
        </button>
      {/if}
    </div>
  </section>

  <section class="panel" aria-labelledby="gallery-head">
    <h2 id="gallery-head" class="head">Add a widget</h2>
    <ul class="gallery">
      {#each DESK_KINDS as k (k.id)}
        {@const s = sample(k.id)}
        {@const [bw, bh] = dims(s)}
        <li class="tile">
          <div class="stage" aria-hidden="true" use:fit={bw}>
            <div class="frame" style:width="{bw}px" style:height="{bh}px"><DesktopWidgetView w={s} {snap} /></div>
          </div>
          <div class="tile-foot">
            <span><b>{k.name}</b><small>{k.text}</small></span>
            <button type="button" class="btn primary" disabled={full || !loaded} onclick={() => add(k.id)} aria-label="Add a {k.name.toLowerCase()} widget">
              <Plus size={15} aria-hidden="true" /> Add
            </button>
          </div>
        </li>
      {/each}
    </ul>
    {#if full}<p class="note">That's {MAX_WIDGETS} widgets, the most there can be. Remove one to add another.</p>{/if}
  </section>

  <h2 class="section">On the desktop</h2>
  {#if loaded && cfg.widgets.length === 0}
    <p class="empty">No widgets yet. Add one from the gallery above.</p>
  {/if}
  <div class="mine">
    {#each cfg.widgets as w (w.id)}
      {@const [bw, bh] = dims(w)}
      {@const k = kindOf(w.kind)}
      <section class="panel widget" id="desk-{w.id}" aria-label={title(w)}>
        <div class="stage" aria-hidden="true" use:fit={bw}>
          <div class="frame" style:width="{bw}px" style:height="{bh}px"><DesktopWidgetView w={preview(w)} {snap} /></div>
        </div>
        <div class="form">
          <header>
            <h3>{title(w)}</h3>
            <button type="button" class="icon danger" onclick={() => remove(w)} aria-label="Remove {title(w)}" title="Remove">
              <Trash2 size={16} aria-hidden="true" />
            </button>
          </header>

          <fieldset>
            <legend>{k.sources > 1 ? `Readings (up to ${k.sources})` : 'Reading'}</legend>
            {#each w.sources as id, i (id)}
              <div class="source">
                {@render readingSelect(w, i, id, k.sources > 1 ? `Reading ${i + 1}` : 'Reading')}
                {#if w.sources.length > 1}
                  <button type="button" class="icon" onclick={() => dropSource(w, i)} aria-label="Remove {label(id)}" title="Remove">
                    <X size={15} aria-hidden="true" />
                  </button>
                {/if}
              </div>
            {/each}
            {#if w.sources.length < k.sources}
              <div class="source">{@render readingSelect(w, w.sources.length, '', 'Add a reading')}</div>
            {/if}
          </fieldset>

          <div class="ranges">
            <label>
              <span>Size <b>{w.scale} %</b></span>
              <input
                type="range"
                min="50"
                max="200"
                step="10"
                value={w.scale}
                oninput={(e) => live(w.id, { scale: Number(e.currentTarget.value) })}
                onchange={(e) => change(w.id, { scale: Number(e.currentTarget.value) })}
              />
            </label>
            <label>
              <span>Background <b>{w.opacity} %</b></span>
              <input
                type="range"
                min="0"
                max="100"
                step="5"
                value={w.opacity}
                oninput={(e) => live(w.id, { opacity: Number(e.currentTarget.value) })}
                onchange={(e) => change(w.id, { opacity: Number(e.currentTarget.value) })}
              />
            </label>
          </div>

          <div class="row">
            <span class="name" id="style-{w.id}">Style</span>
            <div class="segmented" role="group" aria-labelledby="style-{w.id}">
              {#each STYLES as s (s.id)}
                <button type="button" aria-pressed={w.style === s.id} onclick={() => change(w.id, { style: s.id })}>{s.name}</button>
              {/each}
            </div>
          </div>
          <div class="row">
            <span class="name" id="layer-{w.id}">Sits</span>
            <div class="segmented" role="group" aria-labelledby="layer-{w.id}">
              {#each LAYERS as l (l.id)}
                <button type="button" aria-pressed={w.layer === l.id} onclick={() => change(w.id, { layer: l.id })}>{l.name}</button>
              {/each}
            </div>
          </div>
          <label class="check">
            <input
              type="checkbox"
              class="switch"
              role="switch"
              checked={w.locked}
              onchange={(e) => change(w.id, { locked: e.currentTarget.checked }, e.currentTarget.checked ? 'Locked in place.' : 'Unlocked: drag it to move it.')}
            />
            <span>Locked in place<small>Unlocked, the widget moves when you drag it.</small></span>
          </label>
        </div>
      </section>
    {/each}
  </div>

  <p class="note foot">
    Windows' Show desktop (Win+D) can hide widgets that sit behind windows until you switch back to a window; "On
    top" keeps a widget over everything. Widgets read the same sensors as the app, at the profile's rate, also while the
    window is in the tray.
  </p>
  <p class="sr-only" role="status" aria-live="polite">{status}</p>
</div>

<style>
  .page {
    display: grid;
    gap: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1.05rem;
  }
  .head {
    margin-bottom: 0.8rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .section {
    margin: 0.4rem 0 -0.4rem;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .intro {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 1rem 2rem;
    background:
      radial-gradient(120% 140% at 0% 0%, var(--accent-soft), transparent 55%),
      linear-gradient(180deg, var(--card-top) 0%, var(--card) 40%);
  }
  .intro-text {
    flex: 1 1 22rem;
    min-width: 0;
  }
  .intro-text p {
    max-width: 46rem;
    margin: 0.35rem 0 0;
    color: var(--muted);
    font-size: 0.88rem;
  }
  .intro-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.8rem 1.2rem;
  }
  .count {
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .check small {
    display: block;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.38rem 0.75rem;
    border: 1px solid var(--line);
    background: var(--bg);
    color: var(--text);
    font-size: 0.82rem;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--accent-line);
  }
  .btn.primary {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }
  .btn:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    flex: none;
    padding: 0;
    border: 1px solid transparent;
    background: none;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    border-color: var(--line);
    color: var(--text);
  }
  .icon.danger:hover {
    color: var(--hot);
  }
  /* A bit of desktop behind each preview, so the see-through styles read as they will. */
  .stage {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    place-items: center;
    min-height: 15.5rem;
    padding: 1rem;
    overflow: hidden;
    border-radius: 12px;
    background:
      radial-gradient(90% 120% at 15% 10%, rgb(94 129 199 / 55%), transparent 60%),
      radial-gradient(80% 100% at 90% 90%, rgb(166 92 179 / 45%), transparent 60%),
      linear-gradient(140deg, #18233a, #251c35 55%, #12302f);
  }
  .frame {
    flex: none;
    transform: scale(var(--fit, 1));
    transition:
      width var(--dur-2) var(--ease-out),
      height var(--dur-2) var(--ease-out);
  }
  .gallery {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 19rem), 1fr));
    gap: 0.9rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .tile {
    display: grid;
    gap: 0.6rem;
    padding: 0.5rem;
    border: 1px solid var(--line);
    border-radius: 14px;
    background: var(--panel);
    transition: transform var(--dur-2) var(--ease-out);
  }
  .tile:hover {
    border-color: var(--accent-line);
    transform: translateY(-2px);
  }
  .tile-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem;
    padding: 0 0.3rem 0.2rem;
  }
  .tile-foot span {
    min-width: 0;
    font-size: 0.88rem;
  }
  .tile-foot small {
    display: block;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .mine {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 38rem), 1fr));
    gap: 1rem;
  }
  .widget {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.1fr);
    gap: 1.1rem;
    animation: rise var(--dur-3) var(--ease-out);
  }
  @media (max-width: 760px) {
    .widget {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .form {
    display: grid;
    align-content: start;
    gap: 0.8rem;
    min-width: 0;
  }
  .form header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h3 {
    margin: 0;
    overflow: hidden;
    font-size: 0.98rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  fieldset {
    display: grid;
    gap: 0.4rem;
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend,
  .name,
  .ranges span {
    padding: 0;
    margin-bottom: 0.3rem;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .source {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  select {
    flex: 1;
    min-width: 0;
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.85rem;
  }
  .ranges {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 10rem), 1fr));
    gap: 0.6rem 1rem;
  }
  .ranges label {
    display: grid;
  }
  .ranges span {
    display: flex;
    justify-content: space-between;
  }
  .ranges b {
    color: var(--text);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  input[type='range'] {
    width: 100%;
    margin: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem 0.8rem;
  }
  .row .name {
    margin: 0;
  }
  .note {
    margin: 0.6rem 0 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .foot {
    max-width: 60rem;
    margin: 0;
  }
  .empty {
    margin: 0;
    padding: 1.6rem;
    border: 1px dashed var(--line);
    border-radius: 14px;
    color: var(--muted);
    text-align: center;
  }
</style>

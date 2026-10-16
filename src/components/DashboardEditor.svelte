<script lang="ts">
  // Dashboard editor (P24): the shown layout's widgets with a bar each, a palette, drag to move,
  // drag a corner to resize. Every move is also a button, for the keyboard. Changes stay here
  // until "Done".
  import { untrack } from 'svelte';
  import DashWidget from './DashWidget.svelte';
  import { activeLayout, DETAIL_CARDS, isKind, MAX_ROWS, MAX_WIDGETS, NAME_MAX, narrowSpan, readingDef, replaceLayout } from '../dashboard';
  import { isPreset, PRESETS } from '../dashboard-presets';
  import {
    add,
    CARDS,
    deleteLayout,
    duplicate,
    gaugeSources,
    KINDS,
    move,
    newLayout,
    readingIds,
    remove,
    reset,
    resize,
    setSource,
    stepWidth,
    widgetTitle,
  } from '../dashboard-edit';
  import type { WidgetKind } from '../dashboard';
  import { getSensorTree } from '../ipc';
  import type { Dashboard, Layout, SensorSnapshot, SensorTree, Widget } from '../types';

  interface Props {
    dash: Dashboard;
    snap: SensorSnapshot | null;
    needs: string;
    uiFps: number;
    /** A compact profile is in force: the detail cards are marked as hidden for now. */
    compact: boolean;
    oncancel: () => void;
    onsave: (d: Dashboard) => void;
  }
  let { dash, snap, needs, uiFps, compact, oncancel, onsave }: Props = $props();

  let draft = $state.raw<Dashboard>(untrack(() => dash));
  const layout = $derived(activeLayout(draft));
  let palette = $state(false);
  let menu = $state<string | null>(null);
  let said = $state('');
  let tree = $state.raw<SensorTree | null>(null);

  function update(l: Layout, message?: string): void {
    draft = replaceLayout(draft, l);
    if (message) said = message;
  }

  const title = (w: Widget) => widgetTitle(w, snap);
  const size = (w: Widget) => `${w.w} of 12 columns, ${w.h} row${w.h === 1 ? '' : 's'}`;
  const index = (id: string) => layout.widgets.findIndex((w) => w.id === id);

  function placed(id: string, l: Layout): void {
    const i = l.widgets.findIndex((w) => w.id === id);
    update(l, `${title(l.widgets[i]!)} is now ${i + 1} of ${l.widgets.length}.`);
  }

  function sized(id: string, w: number, h: number): void {
    const l = resize(layout, id, w, h);
    const x = l.widgets.find((v) => v.id === id)!;
    update(l, `${title(x)}: ${size(x)}.`);
  }

  function addKind(kind: WidgetKind): void {
    update(add(layout, kind), `${KINDS[kind].label} added at the end.`);
  }

  // Sources a widget can show.
  $effect(() => {
    if (menu && layout.widgets.find((w) => w.id === menu)?.kind === 'history' && !tree)
      void getSensorTree(snap).then((t) => (tree = t)).catch(() => {});
  });
  function options(w: Widget): Array<{ group: string; items: Array<{ id: string; label: string }> }> {
    switch (w.kind) {
      case 'gauge':
        return [
          {
            group: '',
            items: gaugeSources(snap).map((id) => ({
              id,
              label: id === 'cpu' ? 'CPU' : `GPU ${Number(id.split('.')[1]) + 1}${snap?.gpu[Number(id.split('.')[1])] ? ` · ${snap.gpu[Number(id.split('.')[1])]!.name}` : ''}`,
            })),
          },
        ];
      case 'reading':
      case 'graph': {
        const groups = new Map<string, Array<{ id: string; label: string }>>();
        for (const id of readingIds(snap)) {
          const r = readingDef(id, snap)!;
          groups.set(r.group, [...(groups.get(r.group) ?? []), { id, label: r.label }]);
        }
        return [...groups].map(([group, items]) => ({ group, items }));
      }
      case 'history':
        return (tree?.groups ?? [])
          .map((g) => ({ group: g.group, items: g.sensors.filter((s) => s.history).map((s) => ({ id: s.id, label: s.name })) }))
          .filter((g) => g.items.length > 0);
      case 'card':
        return [{ group: '', items: Object.entries(CARDS).map(([id, label]) => ({ id, label })) }];
      default:
        return [];
    }
  }

  // Drag to move.
  let dragId = $state<string | null>(null);
  let overId = $state<string | null>(null);
  let overAfter = $state(false);

  function onDragOver(e: DragEvent, w: Widget): void {
    if (!dragId || dragId === w.id) return;
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    overAfter = e.clientX > r.left + r.width / 2;
    overId = w.id;
  }

  function onDrop(e: DragEvent, w: Widget): void {
    e.preventDefault();
    if (!dragId || dragId === w.id) return;
    const from = index(dragId);
    const target = index(w.id) + (overAfter ? 1 : 0);
    placed(dragId, move(layout, dragId, from < target ? target - 1 : target));
    dragId = overId = null;
  }

  // Drag a corner to resize, snapping to columns and rows.
  function startResize(e: PointerEvent, w: Widget): void {
    if (e.button !== 0) return;
    const handle = e.currentTarget as HTMLElement;
    const grid = handle.closest('.dash-grid') as HTMLElement;
    const style = getComputedStyle(grid);
    const cols = style.gridTemplateColumns.split(' ').length;
    const gap = parseFloat(style.columnGap) || 12;
    const colPx = (grid.clientWidth - gap * (cols - 1)) / cols + gap;
    const rowPx = 5 * parseFloat(getComputedStyle(document.documentElement).fontSize) + gap;
    const per = 12 / cols;
    const [x0, y0, w0, h0] = [e.clientX, e.clientY, w.w, w.h];
    handle.setPointerCapture(e.pointerId);
    const onMove = (ev: PointerEvent) => {
      const nw = w0 + Math.round((ev.clientX - x0) / colPx) * per;
      const nh = h0 + Math.round((ev.clientY - y0) / rowPx);
      const cur = layout.widgets.find((x) => x.id === w.id);
      if (cur && (cur.w !== nw || cur.h !== nh)) update(resize(layout, w.id, nw, nh));
    };
    const onUp = () => {
      handle.removeEventListener('pointermove', onMove);
      handle.removeEventListener('pointerup', onUp);
      handle.removeEventListener('pointercancel', onUp);
      const cur = layout.widgets.find((x) => x.id === w.id);
      if (cur) said = `${title(cur)}: ${size(cur)}.`;
    };
    handle.addEventListener('pointermove', onMove);
    handle.addEventListener('pointerup', onUp);
    handle.addEventListener('pointercancel', onUp);
  }

  function resizeKey(e: KeyboardEvent, w: Widget): void {
    const step: Record<string, [number, number]> = {
      ArrowRight: [stepWidth(w.w, 1), w.h],
      ArrowLeft: [stepWidth(w.w, -1), w.h],
      ArrowDown: [w.w, Math.min(MAX_ROWS, w.h + 1)],
      ArrowUp: [w.w, Math.max(1, w.h - 1)],
    };
    const to = step[e.key];
    if (!to) return;
    e.preventDefault();
    sized(w.id, to[0], to[1]);
  }

  const presetName = $derived(PRESETS[isPreset(layout.preset) ? layout.preset : 'default'].name);
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (menu = null)} />

<div class="editbar" role="toolbar" aria-label="Edit layout">
  <label class="name">
    <span>Name</span>
    <input
      value={layout.name}
      maxlength={NAME_MAX}
      oninput={(e) => update({ ...layout, name: e.currentTarget.value })}
      onblur={(e) => !e.currentTarget.value.trim() && update({ ...layout, name: 'Layout' })}
    />
  </label>
  <button type="button" aria-expanded={palette} aria-controls="dash-palette" onclick={() => (palette = !palette)}>Add widget</button>
  <button type="button" onclick={() => update(reset(layout), `Back to the ${presetName} preset.`)}>Reset to {presetName}</button>
  <button
    type="button"
    onclick={() => {
      // The edits go to the new layout; the original stays as it was before editing.
      const original = dash.layouts.find((l) => l.id === layout.id);
      const next = newLayout(draft, layout, `${layout.name} copy`.slice(0, NAME_MAX));
      if (next) {
        draft = original ? replaceLayout(next, original) : next;
        said = 'Your changes are now a new layout, which you are editing.';
      } else said = 'There are already 12 layouts.';
    }}
  >
    Save as new layout
  </button>
  <button
    type="button"
    disabled={draft.layouts.length <= 1}
    onclick={() => {
      const name = layout.name;
      draft = deleteLayout(draft, layout.id);
      said = `${name} will be deleted when you press Done. Now editing ${activeLayout(draft).name}.`;
    }}
  >
    Delete layout
  </button>
  <span class="spacer"></span>
  <button type="button" onclick={oncancel}>Cancel</button>
  <button type="button" class="primary" onclick={() => onsave(draft)}>Done</button>
</div>

{#if palette}
  <div id="dash-palette" class="palette" role="group" aria-label="Add a widget">
    {#each Object.entries(KINDS) as [kind, k] (kind)}
      <button type="button" disabled={layout.widgets.length >= MAX_WIDGETS} onclick={() => addKind(kind as WidgetKind)}>
        <b>{k.label}</b>
        <span>{k.text}</span>
      </button>
    {/each}
  </div>
{/if}

<p class="hint">
  Drag a widget to move it and its corner to resize it, or use its menu. {layout.widgets.length} of {MAX_WIDGETS} widgets.
</p>

<div class="dash-grid editing">
  {#each layout.widgets as w, i (w.id)}
    {@const t = title(w)}
    <div
      class="dash-cell"
      class:dragging={dragId === w.id}
      class:before={overId === w.id && !overAfter}
      class:after={overId === w.id && overAfter}
      style:--w={w.w}
      style:--wn={narrowSpan(w.w)}
      style:--h={w.h}
      role="group"
      aria-label="{t}, {i + 1} of {layout.widgets.length}"
      ondragover={(e) => onDragOver(e, w)}
      ondragleave={() => overId === w.id && (overId = null)}
      ondrop={(e) => onDrop(e, w)}
      ondragend={() => (dragId = overId = null)}
    >
      <!-- Dragged by its bar, so the corner and the menu stay clicks. -->
      <div
        class="dash-chrome wbar"
        role="presentation"
        draggable="true"
        ondragstart={(e) => {
          dragId = w.id;
          menu = null;
          if (e.dataTransfer) {
            e.dataTransfer.setData('text/plain', w.id);
            e.dataTransfer.effectAllowed = 'move';
            const cell = (e.currentTarget as HTMLElement).parentElement;
            if (cell) e.dataTransfer.setDragImage(cell, 20, 12);
          }
        }}
      >
        <span class="grip" aria-hidden="true"></span>
        <span class="wtitle">{t}</span>
        {#if compact && w.kind === 'card' && DETAIL_CARDS.has(w.source ?? '')}<span class="off">hidden by the profile</span>{/if}
        <span class="size" aria-hidden="true">{w.w}×{w.h}</span>
        <button type="button" class="more" aria-expanded={menu === w.id} aria-label="Options for {t}" onclick={() => (menu = menu === w.id ? null : w.id)}>
          ⋯
        </button>
      </div>
      {#if menu === w.id}
        {@const opts = isKind(w.kind) ? options(w) : []}
        <div class="dash-chrome menu" role="group" aria-label="Options for {t}">
          {#if opts.length || w.kind === 'history'}
            <label class="source">
              <span>Shows</span>
              <select value={w.source ?? ''} onchange={(e) => update(setSource(layout, w.id, e.currentTarget.value || null), `${title({ ...w, source: e.currentTarget.value })}.`)}>
                {#if w.kind === 'history' && !tree}<option value={w.source ?? ''}>Loading sensors…</option>{/if}
                {#each opts as g (g.group)}
                  {#if g.group}
                    <optgroup label={g.group}>
                      {#each g.items as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
                    </optgroup>
                  {:else}
                    {#each g.items as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
                  {/if}
                {/each}
              </select>
            </label>
          {/if}
          <div class="actions">
            <button type="button" disabled={i === 0} onclick={() => placed(w.id, move(layout, w.id, i - 1))}>Move earlier</button>
            <button type="button" disabled={i === layout.widgets.length - 1} onclick={() => placed(w.id, move(layout, w.id, i + 1))}>Move later</button>
            <button type="button" disabled={w.w <= 3} onclick={() => sized(w.id, stepWidth(w.w, -1), w.h)}>Narrower</button>
            <button type="button" disabled={w.w >= 12} onclick={() => sized(w.id, stepWidth(w.w, 1), w.h)}>Wider</button>
            <button type="button" disabled={w.h <= 1} onclick={() => sized(w.id, w.w, w.h - 1)}>Shorter</button>
            <button type="button" disabled={w.h >= MAX_ROWS} onclick={() => sized(w.id, w.w, w.h + 1)}>Taller</button>
            <button type="button" disabled={layout.widgets.length >= MAX_WIDGETS} onclick={() => update(duplicate(layout, w.id), `${t} duplicated.`)}>Duplicate</button>
            <button
              type="button"
              class="danger"
              onclick={() => {
                menu = null;
                update(remove(layout, w.id), `${t} removed.`);
              }}
            >
              Remove
            </button>
          </div>
        </div>
      {/if}
      <div class="preview" inert>
        <DashWidget widget={w} {snap} active={true} {needs} {uiFps} editing />
      </div>
      <button
        type="button"
        class="dash-chrome resize"
        aria-label="Resize {t}: arrow keys. Now {size(w)}"
        onpointerdown={(e) => startResize(e, w)}
        onkeydown={(e) => resizeKey(e, w)}
      ></button>
    </div>
  {/each}
</div>

<div class="sr-only" aria-live="polite">{said}</div>

<style>
  .editbar {
    position: sticky;
    top: 0;
    z-index: 3;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
    padding: 0.6rem 0.75rem;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--line));
    border-radius: 10px;
    background: var(--panel);
    box-shadow: var(--shadow);
    font-size: 0.8rem;
  }
  .spacer {
    flex: 1;
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
  }
  input,
  select,
  button {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--panel-edge);
    color: var(--text);
    font: inherit;
  }
  input {
    width: 12rem;
    max-width: 40vw;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .primary {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 22%, var(--panel-edge));
    font-weight: 600;
  }
  .danger {
    color: var(--hot);
  }
  .palette {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 13rem), 1fr));
    gap: 0.5rem;
    margin-bottom: 0.75rem;
  }
  .palette button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    padding: 0.55rem 0.7rem;
    text-align: left;
  }
  .palette span {
    color: var(--muted);
    font-size: 0.75rem;
  }
  .hint {
    margin: 0 0 0.6rem;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .editing .dash-cell {
    padding: 0.3rem;
    border: 1px dashed color-mix(in srgb, var(--accent) 50%, var(--line));
    border-radius: calc(var(--radius) + 4px);
  }
  .dash-cell.dragging {
    opacity: 0.45;
  }
  .dash-cell.before {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .dash-cell.after {
    box-shadow: inset -3px 0 0 var(--accent);
  }
  .wbar {
    cursor: grab;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-width: 0;
    margin-bottom: 0.3rem;
    font-size: 0.75rem;
  }
  .grip {
    flex: none;
    width: 0.6rem;
    height: 0.9rem;
    background-image: radial-gradient(circle, var(--muted) 1.2px, transparent 1.4px);
    background-size: 0.3rem 0.3rem;
  }
  .wtitle {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .off {
    color: var(--warn);
    white-space: nowrap;
  }
  .size {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .more {
    padding: 0 0.5rem;
    line-height: 1.3;
  }
  .menu {
    position: absolute;
    top: 2rem;
    right: 0.3rem;
    left: 0.3rem;
    z-index: 2;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.6rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    box-shadow: var(--shadow);
    font-size: 0.78rem;
    cursor: default;
  }
  .source select {
    flex: 1;
    min-width: 0;
  }
  .source {
    display: flex;
  }
  .actions {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 6.5rem), 1fr));
    gap: 0.35rem;
  }
  .preview {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }
  .preview > :global(*) {
    flex: 1 1 auto;
  }
  .resize {
    position: absolute;
    right: 0.15rem;
    bottom: 0.15rem;
    width: 1.1rem;
    height: 1.1rem;
    padding: 0;
    border: 0;
    border-radius: 3px;
    background:
      linear-gradient(135deg, transparent 45%, var(--muted) 45%, var(--muted) 55%, transparent 55%),
      linear-gradient(135deg, transparent 70%, var(--muted) 70%, var(--muted) 80%, transparent 80%);
    cursor: nwse-resize;
    touch-action: none;
  }
</style>

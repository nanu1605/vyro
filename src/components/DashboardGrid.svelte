<script lang="ts">
  // The Dashboard's widgets (P24): the shown layout, its picker and lock. The editor loads on
  // first "Edit layout".
  import { onMount } from 'svelte';
  import DashWidget from './DashWidget.svelte';
  import { activeLayout, DETAIL_CARDS, initial, narrowSpan, replaceLayout, shown, type Needs } from '../dashboard';
  import { getDashboard, setDashboard } from '../ipc';
  import { pack } from '../packing';
  import type { Dashboard, SensorSnapshot } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    /** The Dashboard tab shows. */
    active: boolean;
    needs: Needs;
    uiFps: number;
    /** A compact profile is in force: the detail cards stay hidden. */
    compact: boolean;
    profileName: string;
    onselect: (tab: 'gaming' | 'health') => void;
    /** Settings, at its Profile card ("Change profile"). */
    onprofile: () => void;
    /** Shows the live rail again (a wide window with the rail folded away). */
    onrail?: () => void;
  }
  let { snap, active, needs, uiFps, compact, profileName, onselect, onprofile, onrail }: Props = $props();

  let dash = $state.raw<Dashboard>(initial());
  let editing = $state(false);
  let error = $state<string | null>(null);

  onMount(() => {
    getDashboard()
      .then(async (d) => {
        if (d && d.layouts.length > 0) dash = d;
        // First run: Default shows at once; the other presets join the picker when loaded.
        else if (!editing) dash = (await import('../dashboard-presets')).seed();
      })
      .catch((e: unknown) => (error = String(e)));
  });

  const layout = $derived(activeLayout(dash));
  const visible = $derived(layout.widgets.filter((w) => shown(w, snap, compact)));
  const hiddenCards = $derived(compact ? layout.widgets.filter((w) => w.kind === 'card' && DETAIL_CARDS.has(w.source ?? '')).length : 0);

  /** Shows the change at once and keeps it if the app stores it. */
  async function save(next: Dashboard): Promise<boolean> {
    const before = dash;
    dash = next;
    try {
      dash = await setDashboard(next);
      error = null;
      return true;
    } catch (e) {
      dash = before;
      error = String(e);
      return false;
    }
  }

  async function done(next: Dashboard): Promise<void> {
    if (await save(next)) editing = false;
  }
</script>

<div class="bar">
  <label>
    <span>Layout</span>
    <select value={layout.id} disabled={editing} onchange={(e) => void save({ ...dash, active: e.currentTarget.value })}>
      {#each dash.layouts as l (l.id)}<option value={l.id}>{l.name}</option>{/each}
    </select>
  </label>
  <button
    type="button"
    disabled={editing || layout.locked}
    title={layout.locked ? 'Unlock the layout to edit it' : undefined}
    onclick={() => (editing = true)}
  >
    Edit layout
  </button>
  <button
    type="button"
    aria-pressed={layout.locked}
    disabled={editing}
    onclick={() => void save(replaceLayout(dash, { ...layout, locked: !layout.locked }))}
  >
    {layout.locked ? 'Locked' : 'Lock'}
  </button>
  {#if onrail}<button type="button" onclick={onrail}>Live panel</button>{/if}
  {#if error}<span class="error" role="alert">Could not save the layout: {error}</span>{/if}
</div>

{#if editing}
  {#await import('./DashboardEditor.svelte') then { default: DashboardEditor }}
    <DashboardEditor {dash} {snap} {needs} {uiFps} {compact} oncancel={() => (editing = false)} onsave={done} />
  {/await}
{:else}
  <div class="dash-grid" use:pack>
    {#each visible as w, i (w.id)}
      <div class="dash-cell" style:--w={w.w} style:--wn={narrowSpan(w.w)} style:--h={w.h} style:--span0={23 * w.h} style:--i={i}>
        <DashWidget widget={w} {snap} {active} {needs} {uiFps} {onselect} />
      </div>
    {/each}
  </div>
  {#if visible.length === 0}
    <p class="note">This layout is empty. <button type="button" class="link" onclick={() => (editing = true)}>Add widgets</button></p>
  {/if}
  {#if hiddenCards > 0}
    <p class="note">
      {profileName} profile: {hiddenCards} detail card{hiddenCards === 1 ? '' : 's'} hidden.
      <button type="button" class="link" onclick={onprofile}>Change profile</button>
    </p>
  {/if}
{/if}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 0.4rem 0.6rem;
    margin-bottom: 0.75rem;
    font-size: 0.8rem;
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
  }
  select,
  button {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--panel-edge);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  button:disabled,
  select:disabled {
    opacity: 0.6;
    cursor: default;
  }
  button[aria-pressed='true'] {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
  }
  .error {
    flex-basis: 100%;
    color: var(--hot);
    text-align: right;
  }
  .note {
    margin: 0.75rem 0 0;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
</style>

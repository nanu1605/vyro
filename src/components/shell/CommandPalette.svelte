<script lang="ts">
  // Command palette (P28, Ctrl+K): type to jump to a view, a settings section or a sensor's
  // history, switch the theme or accent, or run an action. A dialog with a combobox and a flat
  // list of options, each tagged with what it is.
  import CornerDownLeft from '@lucide/svelte/icons/corner-down-left';
  import Search from '@lucide/svelte/icons/search';
  import { getSensorTree } from '../../ipc';
  import { VIEWS, type Tab } from '../../nav';
  import { prefs, savePrefs } from '../../prefs.svelte';
  import { ACCENTS, THEMES } from '../../themes';
  import type { SensorSnapshot } from '../../types';

  interface Props {
    open: boolean;
    snap: SensorSnapshot | null;
    fansActive: boolean;
    onselect: (t: Tab) => void;
    /** A view and a part of it to scroll to. */
    onsection: (t: Tab, id: string) => void;
    onhistory: (sensor: string) => void;
    onbios: () => void;
  }
  let { open = $bindable(), snap, fansActive, onselect, onsection, onhistory, onbios }: Props = $props();

  interface Item {
    key: string;
    label: string;
    hint: string;
    kind: string;
    run: () => void;
  }

  const SECTIONS: Array<[Tab, string, string, string]> = [
    ['settings', 'appearance', 'Appearance', 'Theme, accent, open on'],
    ['settings', 'profile', 'Profile', 'Poll rate, notifications, animation'],
    ['settings', 'general', 'General', 'Tray, start with Windows'],
    ['settings', 'gaming', 'Gaming', 'Frame rates, game profiles'],
    ['settings', 'overlay', 'Overlay', 'In-game readings and graph'],
    ['settings', 'remote', 'Remote', 'Phone companion on the LAN'],
    ['settings', 'units', 'Units', '°C or °F'],
    ['settings', 'hidden-sensors', 'Hidden sensors', 'Sensors left out of views'],
    ['settings', 'service', 'Sensor service', 'Privileged sensors'],
    ['settings', 'updates', 'Updates', 'Check for a new version'],
    ['health', 'diagnostics', 'Diagnostics', 'One-click check and report'],
    ['health', 'alerts', 'Alerts', 'Rules, actions and the log'],
  ];

  let sensors = $state<Item[]>([]);
  let loaded = false;
  async function loadSensors(): Promise<void> {
    if (loaded) return;
    loaded = true;
    try {
      const tree = await getSensorTree(snap);
      sensors = tree.groups.flatMap((g) =>
        g.sensors.map((s) => ({
          key: `sensor:${s.id}`,
          label: s.name,
          hint: `${g.group} · open in History`,
          kind: 'Sensor',
          run: () => onhistory(s.id),
        })),
      );
    } catch {
      loaded = false;
    }
  }

  const base = $derived<Item[]>([
    ...VIEWS.map((v) => ({ key: `tab:${v.id}`, label: v.label, hint: v.blurb, kind: 'Go to', run: () => onselect(v.id) })),
    ...(fansActive ? [{ key: 'bios', label: 'Return fans to BIOS', hint: 'Hand every fan back', kind: 'Action', run: onbios }] : []),
    ...SECTIONS.map(([t, id, label, hint]) => ({ key: `sec:${id}`, label, hint, kind: t === 'settings' ? 'Settings' : 'Health', run: () => onsection(t, id) })),
    ...THEMES.map((t) => ({
      key: `theme:${t.id}`,
      label: `Theme: ${t.name}`,
      hint: prefs.theme === t.id ? 'in use' : t.description,
      kind: 'Look',
      run: () => void savePrefs({ theme: t.id }).catch(() => {}),
    })),
    ...[{ id: 'theme' as const, name: 'Theme’s own' }, ...ACCENTS].map((a) => ({
      key: `accent:${a.id}`,
      label: `Accent: ${a.name}`,
      hint: prefs.accent === a.id ? 'in use' : 'colour of highlights and gauges',
      kind: 'Look',
      run: () => void savePrefs({ accent: a.id }).catch(() => {}),
    })),
  ]);

  let query = $state('');
  let at = $state(0);
  const results = $derived.by(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const all = words.length ? [...base, ...sensors] : base.filter((i) => i.kind === 'Go to' || i.kind === 'Action');
    const hits = all.filter((i) => {
      const hay = `${i.label} ${i.hint} ${i.kind}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
    // Names that start with what was typed first.
    if (words[0]) hits.sort((a, b) => Number(!a.label.toLowerCase().startsWith(words[0]!)) - Number(!b.label.toLowerCase().startsWith(words[0]!)));
    return hits.slice(0, 60);
  });
  $effect(() => {
    void results;
    at = 0;
  });

  let dialog = $state<HTMLDialogElement | undefined>();
  let input = $state<HTMLInputElement | undefined>();
  let list = $state<HTMLDivElement | undefined>();
  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) {
      query = '';
      dialog.showModal();
      input?.focus();
      void loadSensors();
    } else if (!open && dialog.open) dialog.close();
  });

  function run(i: Item | undefined): void {
    if (!i) return;
    open = false;
    i.run();
  }

  function onKey(e: KeyboardEvent): void {
    const n = results.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!n) return;
      at = (at + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
      list?.querySelector(`#cmdk-${at}`)?.scrollIntoView({ block: 'nearest' });
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(results[at]);
    }
  }
</script>

<dialog class="palette" bind:this={dialog} onclose={() => (open = false)} aria-label="Search Vyro" onclick={(e) => e.target === dialog && (open = false)}>
  <div class="box">
    <div class="field">
      <Search size={17} aria-hidden="true" />
      <input
        bind:this={input}
        bind:value={query}
        onkeydown={onKey}
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls="cmdk-list"
        aria-autocomplete="list"
        aria-activedescendant={results.length ? `cmdk-${at}` : undefined}
        aria-label="Search views, settings, sensors and actions"
        placeholder="Search views, settings, sensors…"
        spellcheck="false"
        autocomplete="off"
      />
      <kbd>Esc</kbd>
    </div>
    <div class="list" id="cmdk-list" role="listbox" aria-label="Results" bind:this={list}>
      {#each results as r, i (r.key)}
        <!-- svelte-ignore a11y_click_events_have_key_events (the combobox handles the keys) -->
        <div id="cmdk-{i}" class="opt" class:on={i === at} role="option" tabindex="-1" aria-selected={i === at} onclick={() => run(r)} onmousemove={() => (at = i)}>
          <span class="text"><b>{r.label}</b><small>{r.hint}</small></span>
          <span class="kind">{r.kind}</span>
          {#if i === at}<CornerDownLeft size={14} aria-hidden="true" />{/if}
        </div>
      {:else}
        <p class="none">Nothing matches “{query}”.</p>
      {/each}
    </div>
  </div>
</dialog>

<style>
  .palette {
    width: min(40rem, calc(100vw - 2rem));
    max-height: min(32rem, 76vh);
    margin: 12vh auto auto;
    padding: 0;
    border: 1px solid var(--accent-line);
    border-radius: 16px;
    background: var(--panel);
    box-shadow:
      var(--shadow),
      0 0 0 1px rgb(0 0 0 / 20%),
      0 24px 60px -20px color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--text);
  }
  .palette::backdrop {
    background: rgb(0 0 0 / 45%);
    backdrop-filter: blur(3px);
  }
  .box {
    display: flex;
    flex-direction: column;
    max-height: inherit;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.8rem 1rem;
    border-bottom: 1px solid var(--line);
    color: var(--muted);
  }
  input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 1rem;
    outline: none;
  }
  .field:focus-within {
    color: var(--accent);
  }
  kbd {
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--line);
    border-radius: 5px;
    color: var(--muted);
    font: inherit;
    font-size: 0.68rem;
  }
  .list {
    overflow-y: auto;
    padding: 0.4rem;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 0.7rem;
    border-radius: 10px;
    cursor: pointer;
  }
  .opt.on {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  /* Muted text is too faint on the highlight: the chosen row uses full text colour. */
  .opt.on .text small,
  .opt.on .kind {
    color: var(--text);
  }
  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }
  .text b {
    font-size: 0.88rem;
    font-weight: 600;
  }
  .text small {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.74rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind {
    flex: none;
    padding: 0.05rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.68rem;
  }
  .none {
    margin: 0;
    padding: 1.2rem;
    color: var(--muted);
    text-align: center;
  }
</style>

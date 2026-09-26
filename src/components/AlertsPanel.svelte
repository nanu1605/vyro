<script lang="ts">
  import { untrack } from 'svelte';
  import { GROUP_LABEL } from '../format';
  import { getAlertLog, getAlertRules, getSensorTree, getSettings, setAlertRules, setSettings, testAlert, type AppSettings } from '../ipc';
  import type { AlertFired, AlertRule, Sensor, SensorSnapshot, SensorTree } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    active: boolean;
  }
  let { snap, active }: Props = $props();

  let rules = $state<AlertRule[]>([]);
  let tree = $state<SensorTree | null>(null);
  let log = $state<AlertFired[]>([]);
  let error = $state<string | null>(null);
  let saved = $state<string | null>(null);
  /** Rule being edited (a copy); `isNew` when it is not in `rules` yet. */
  let draft = $state<AlertRule | null>(null);
  let isNew = $state(false);
  let armedDelete = $state<string | null>(null);
  let settings = $state<AppSettings | null>(null);

  async function setTray(on: boolean): Promise<void> {
    if (!settings) return;
    const next = { ...settings, closeToTray: on };
    try {
      await setSettings(next);
      settings = next;
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  const sensors = $derived((tree?.groups ?? []).flatMap((g) => g.sensors));
  const byId = $derived(new Map(sensors.map((s) => [s.id, s])));

  async function reload(): Promise<void> {
    try {
      rules = await getAlertRules();
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    if (!active) return;
    untrack(() => {
      void reload();
      void getSettings()
        .then((x) => (settings = x))
        .catch(() => {});
      void getSensorTree(snap).then((t) => (tree = t));
    });
  });
  // Fired-alert log: once per tick while shown.
  $effect(() => {
    if (!active || !snap?.ts) return;
    void getAlertLog()
      .then((l) => (log = l))
      .catch(() => {});
  });

  async function commit(next: AlertRule[], what: string): Promise<boolean> {
    try {
      await setAlertRules(next);
      rules = await getAlertRules();
      error = null;
      saved = what;
      setTimeout(() => (saved = null), 2500);
      return true;
    } catch (e) {
      error = String(e);
      return false;
    }
  }

  function toggle(r: AlertRule): void {
    void commit(
      rules.map((x) => (x.id === r.id ? { ...x, enabled: !x.enabled } : x)),
      r.enabled ? `“${r.name}” off` : `“${r.name}” on`,
    );
  }

  function remove(r: AlertRule): void {
    if (armedDelete !== r.id) {
      armedDelete = r.id;
      return;
    }
    armedDelete = null;
    void commit(
      rules.filter((x) => x.id !== r.id),
      `“${r.name}” deleted`,
    );
  }

  function add(): void {
    const first = byId.get('cpu.temp.package') ?? sensors[0];
    draft = {
      id: `rule-${Date.now().toString(36)}`,
      name: '',
      enabled: true,
      kind: { type: 'threshold', sensor: first?.id ?? '', above: true, value: 90 },
      forSecs: 10,
      cooldownSecs: 600,
      sound: true,
    };
    isNew = true;
  }

  function edit(r: AlertRule): void {
    draft = structuredClone($state.snapshot(r));
    isNew = false;
  }

  const problem = $derived.by(() => {
    const d = draft;
    if (!d) return null;
    if (!d.name.trim()) return 'Give the rule a name.';
    if (d.name.length > 80) return 'Keep the name under 80 characters.';
    if (d.kind.type === 'threshold') {
      if (!d.kind.sensor) return 'Pick a sensor.';
      if (!Number.isFinite(d.kind.value)) return 'Enter a number for the limit.';
    }
    if (!(d.forSecs >= 0 && d.forSecs <= 3600)) return 'Hold time is 0 to 3600 seconds.';
    if (!(d.cooldownSecs >= 0 && d.cooldownSecs <= 86400)) return 'Quiet time is 0 to 1440 minutes.';
    return null;
  });

  async function save(): Promise<void> {
    const d = draft;
    if (!d || problem) return;
    const clean = { ...$state.snapshot(d), name: d.name.trim() };
    const next = isNew ? [...rules, clean] : rules.map((x) => (x.id === clean.id ? clean : x));
    if (await commit(next, `“${clean.name}” saved`)) draft = null;
  }

  function setKind(type: 'threshold' | 'health'): void {
    if (!draft || draft.kind.type === type) return;
    draft.kind =
      type === 'health'
        ? { type: 'health', problems: true }
        : { type: 'threshold', sensor: byId.has('cpu.temp.package') ? 'cpu.temp.package' : (sensors[0]?.id ?? ''), above: true, value: 90 };
    if (type === 'health') draft.forSecs = 0;
  }

  const unitOf = (id: string) => byId.get(id)?.unit ?? '';
  function describe(r: AlertRule): string {
    const quiet = r.cooldownSecs >= 60 ? `${Math.round(r.cooldownSecs / 60)} min` : `${r.cooldownSecs} s`;
    const tail = ` · quiet ${quiet} after firing${r.sound ? ' · sound' : ''}`;
    if (r.kind.type === 'health') {
      return (r.kind.problems ? 'System Health problems: stopped fans, failing drives, limits reached' : 'Any System Health warning or problem') + tail;
    }
    const s: Sensor | undefined = byId.get(r.kind.sensor);
    const unit = s?.unit ? (s.unit === '%' ? '%' : ` ${s.unit}`) : '';
    return `${s?.name ?? r.kind.sensor} ${r.kind.above ? 'above' : 'below'} ${r.kind.value}${unit}${r.forSecs ? ` for ${r.forSecs} s` : ''}${tail}${s ? '' : ' · sensor not present'}`;
  }
  const when = (t: number) =>
    new Date(t * 1000).toLocaleString('en-US', { weekday: 'short', hour: '2-digit', minute: '2-digit', hour12: false });
  const groups = $derived((tree?.groups ?? []).filter((g) => g.sensors.some((s) => s.unit !== '')));
</script>

{#snippet form(d: AlertRule)}
  <form
    class="editor"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <label class="full">
      <span>Name</span>
      <input type="text" bind:value={d.name} maxlength="80" placeholder="e.g. Pump stopped" />
    </label>
    <fieldset class="full kind">
      <legend>Watch</legend>
      <label><input type="radio" checked={d.kind.type === 'threshold'} onchange={() => setKind('threshold')} /> A sensor</label>
      <label><input type="radio" checked={d.kind.type === 'health'} onchange={() => setKind('health')} /> System Health</label>
    </fieldset>
    {#if d.kind.type === 'threshold'}
      <label class="full">
        <span>Sensor</span>
        <select bind:value={d.kind.sensor}>
          {#each groups as g (g.group)}
            <optgroup label={GROUP_LABEL[g.group] ?? g.group}>
              {#each g.sensors.filter((s) => s.unit !== '') as s (s.id)}
                <option value={s.id}>{s.name} ({s.unit})</option>
              {/each}
            </optgroup>
          {/each}
          {#if d.kind.sensor && !byId.has(d.kind.sensor)}
            <option value={d.kind.sensor}>{d.kind.sensor} (not present)</option>
          {/if}
        </select>
      </label>
      <label>
        <span>When it goes</span>
        <select bind:value={d.kind.above}>
          <option value={true}>above</option>
          <option value={false}>below</option>
        </select>
      </label>
      <label>
        <span>Limit ({unitOf(d.kind.sensor) || 'value'})</span>
        <input type="number" step="any" bind:value={d.kind.value} />
      </label>
      <label>
        <span>For at least (s)</span>
        <input type="number" min="0" max="3600" bind:value={d.forSecs} />
      </label>
    {:else}
      <label class="full">
        <span>Notify on</span>
        <select bind:value={d.kind.problems}>
          <option value={true}>Problems only (stopped fans, failing drives, limits reached)</option>
          <option value={false}>Warnings and problems</option>
        </select>
      </label>
    {/if}
    <label>
      <span>Then stay quiet (min)</span>
      <input
        type="number"
        min="0"
        max="1440"
        value={Math.round(d.cooldownSecs / 60)}
        oninput={(e) => (d.cooldownSecs = Math.round(Number((e.currentTarget as HTMLInputElement).value) * 60))}
      />
    </label>
    <label class="check"><input type="checkbox" bind:checked={d.sound} /> Play sound</label>
    <div class="full actions">
      {#if problem}<span class="err">{problem}</span>{/if}
      <button type="button" onclick={() => (draft = null)}>Cancel</button>
      <button type="submit" class="primary" disabled={!!problem}>{isNew ? 'Add rule' : 'Save'}</button>
    </div>
  </form>
{/snippet}

<section class="alerts">
  <div class="head">
    <h2>Alerts</h2>
    <div class="tools">
      {#if saved}<span class="saved" role="status">{saved}</span>{/if}
      <button type="button" onclick={() => void testAlert(true)}>Send test notification</button>
      <button type="button" class="primary" onclick={add} disabled={!!draft}>Add rule</button>
    </div>
  </div>
  <p class="note">Windows notifications while Vyro is running, including from the tray. Rules are saved on this PC.</p>
  {#if settings}
    <label class="tray">
      <input type="checkbox" checked={settings.closeToTray} onchange={(e) => void setTray((e.currentTarget as HTMLInputElement).checked)} />
      Keep running in the tray when the window is closed (alerts need Vyro running; quit from the tray icon)
    </label>
  {/if}
  {#if error}<p class="err">{error}</p>{/if}

  <ul class="rules">
    {#each rules as r (r.id)}
      <li class:off={!r.enabled}>
        <label class="switch" title={r.enabled ? 'Turn off' : 'Turn on'}>
          <input type="checkbox" checked={r.enabled} onchange={() => toggle(r)} aria-label="{r.name} enabled" />
          <span aria-hidden="true"></span>
        </label>
        <div class="desc">
          <b>{r.name}</b>
          <span>{describe(r)}</span>
        </div>
        <div class="row-tools">
          <button type="button" onclick={() => edit(r)} disabled={!!draft}>Edit</button>
          <button type="button" class:danger={armedDelete === r.id} onclick={() => remove(r)} onblur={() => (armedDelete = null)}>
            {armedDelete === r.id ? 'Delete?' : 'Delete'}
          </button>
        </div>
      </li>
      {#if draft && !isNew && draft.id === r.id}
        <li class="editing">{@render form(draft)}</li>
      {/if}
    {:else}
      <li class="empty">No rules. Add one to be notified about a temperature, a fan, power or System Health.</li>
    {/each}
    {#if draft && isNew}
      <li class="editing">{@render form(draft)}</li>
    {/if}
  </ul>

  <h3>Fired alerts</h3>
  {#if log.length}
    <ol class="log">
      {#each [...log].reverse() as a, i (i)}
        <li><time>{when(a.ts)}</time> <b>{a.title}</b> {a.message}</li>
      {/each}
    </ol>
  {:else}
    <p class="note">None since Vyro started.</p>
  {/if}
</section>

<style>
  .alerts {
    margin-top: 1.4rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  h3 {
    margin: 1.1rem 0 0.45rem;
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .tools,
  .row-tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }
  button {
    padding: 0.32rem 0.65rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    border-color: var(--muted);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.primary {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
  }
  button.danger {
    border-color: var(--hot);
    color: var(--hot);
  }
  .saved {
    color: var(--ok);
    font-size: 0.78rem;
  }
  .note {
    margin: 0.35rem 0 0.7rem;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .tray {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    margin: 0 0 0.8rem;
    font-size: 0.8rem;
  }
  .tray input {
    margin-top: 0.15rem;
  }
  .err {
    color: var(--hot);
    font-size: 0.8rem;
  }
  .rules {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .rules > li {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.55rem 0.8rem;
  }
  .rules > li + li {
    border-top: 1px solid var(--line);
  }
  .rules > li.off .desc {
    opacity: 0.55;
  }
  .rules > li.empty {
    color: var(--muted);
    font-size: 0.82rem;
  }
  .rules > li.editing {
    display: block;
    background: color-mix(in srgb, var(--panel-edge) 60%, var(--panel));
  }
  .desc {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    font-size: 0.82rem;
  }
  .desc span {
    color: var(--muted);
    font-size: 0.76rem;
  }
  .switch {
    position: relative;
    flex: none;
    width: 2.1rem;
    height: 1.2rem;
  }
  .switch input {
    position: absolute;
    inset: 0;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }
  .switch span {
    position: absolute;
    inset: 0;
    border-radius: 999px;
    background: var(--gauge-track);
    box-shadow: inset 0 0 0 1px var(--line);
    pointer-events: none;
    transition: background 0.15s;
  }
  .switch span::after {
    position: absolute;
    top: 0.15rem;
    left: 0.15rem;
    width: 0.9rem;
    height: 0.9rem;
    border-radius: 50%;
    background: var(--muted);
    content: '';
    transition: transform 0.15s;
  }
  .switch input:checked + span {
    background: color-mix(in srgb, var(--ok) 35%, var(--gauge-track));
  }
  .switch input:checked + span::after {
    background: var(--text);
    transform: translateX(0.9rem);
  }
  .switch input:focus-visible + span {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .editor {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 12rem), 1fr));
    gap: 0.6rem 0.8rem;
    font-size: 0.8rem;
  }
  .editor .full {
    grid-column: 1 / -1;
  }
  .editor label:not(.check):not(.kind label) {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .editor label > span {
    color: var(--muted);
  }
  .editor input[type='text'],
  .editor input[type='number'],
  .editor select {
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  fieldset {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    float: left;
    margin-right: 0.5rem;
    padding: 0;
    color: var(--muted);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    align-self: end;
    padding-bottom: 0.35rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 0.5rem;
  }
  .actions .err {
    margin-right: auto;
  }
  .log {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.8rem;
  }
  .log li {
    padding: 0.3rem 0;
  }
  .log li + li {
    border-top: 1px solid var(--line);
  }
  .log time {
    display: inline-block;
    min-width: 6.5rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  @media (max-width: 560px) {
    .rules > li:not(.editing) {
      flex-wrap: wrap;
    }
    .row-tools {
      width: 100%;
      justify-content: flex-end;
    }
  }
</style>

<script lang="ts">
  import { untrack } from 'svelte';
  import {
    actionsText,
    amountOf,
    CLEAR_SECS,
    conditionsOf,
    DEFAULT_ACTIONS,
    describeRule,
    FAN_BOOST,
    isLimit,
    isRate,
    kindFor,
    MAX_CONDITIONS,
    newTest,
    RATE_SECS,
    ruleProblem,
    shownAmount,
    storedAmount,
    TEST_LABEL,
    withAmount,
    type TestType,
  } from '../alerts';
  import { GROUP_LABEL } from '../format';
  import { getAlertLog, getAlertRules, getAlertUsual, getSensorTree, setAlertRules, testAlert } from '../ipc';
  import { displayUnit, fmtReading, prefs, profiles, tempText } from '../prefs.svelte';
  import type { AlertFired, AlertRule, Condition, Profile, SensorSnapshot, SensorTree } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    active: boolean;
  }
  let { snap, active }: Props = $props();

  let rules = $state<AlertRule[]>([]);
  let tree = $state.raw<SensorTree | null>(null);
  let log = $state.raw<AlertFired[]>([]);
  let error = $state<string | null>(null);
  let saved = $state<string | null>(null);
  /** Rule being edited (a copy, sensor rules as conditions); `isNew` when not in `rules` yet. */
  let draft = $state<AlertRule | null>(null);
  let isNew = $state(false);
  let armedDelete = $state<string | null>(null);
  /** Usual readings now, by sensor (`null`: not enough history yet). */
  let usual = $state<Record<string, number | null>>({});

  const sensors = $derived((tree?.groups ?? []).flatMap((g) => g.sensors));
  const byId = $derived(new Map(sensors.map((s) => [s.id, s])));
  const groups = $derived((tree?.groups ?? []).filter((g) => g.sensors.some((s) => s.unit !== '')));
  const holding = $derived(new Map((snap?.alertState?.active ?? []).map((a) => [a.ruleId, a])));
  const fansOn = $derived(snap?.fanControl.active ?? false);
  const unit = $derived(prefs.temperatureUnit);
  const nameOf = (p: Profile) => profiles.list.find((x) => x.id === p)?.name ?? p;
  const info = (id: string) => byId.get(id);
  const unitOf = (id: string) => byId.get(id)?.unit ?? '';

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
  // The usual reading of each sensor an "above usual" condition in the editor watches.
  $effect(() => {
    const d = draft;
    if (!d || d.kind.type !== 'conditions') return;
    for (const c of d.kind.conditions) {
      if (c.test.type !== 'aboveUsual' && c.test.type !== 'belowUsual') continue;
      const id = c.sensor;
      if (!id || untrack(() => id in usual)) continue;
      void getAlertUsual(id)
        .then((v) => (usual = { ...usual, [id]: v }))
        .catch(() => (usual = { ...usual, [id]: null }));
    }
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

  function firstSensor(): string {
    return byId.has('cpu.temp.package') ? 'cpu.temp.package' : (sensors.find((s) => s.unit !== '')?.id ?? '');
  }

  function newCondition(sensor = firstSensor()): Condition {
    return { sensor, test: newTest('above', unitOf(sensor)), hysteresis: 0 };
  }

  function add(): void {
    draft = {
      id: `rule-${Date.now().toString(36)}`,
      name: '',
      enabled: true,
      kind: { type: 'conditions', any: false, conditions: [newCondition()] },
      forSecs: 10,
      cooldownSecs: 600,
      sound: true,
      actions: { ...DEFAULT_ACTIONS },
    };
    isNew = true;
  }

  function edit(r: AlertRule): void {
    const copy = structuredClone($state.snapshot(r));
    if (copy.kind.type === 'threshold') copy.kind = { type: 'conditions', ...conditionsOf(copy.kind) };
    draft = copy;
    isNew = false;
  }

  const problem = $derived(draft ? ruleProblem(draft) : null);

  async function save(): Promise<void> {
    const d = draft;
    if (!d || problem) return;
    const clean = { ...$state.snapshot(d), name: d.name.trim() };
    if (clean.kind.type === 'conditions') clean.kind = kindFor(clean.kind.any, clean.kind.conditions);
    if (clean.kind.type === 'health') clean.actions = { ...clean.actions, tray: false, profile: null, fanBoost: null };
    const next = isNew ? [...rules, clean] : rules.map((x) => (x.id === clean.id ? clean : x));
    if (await commit(next, `“${clean.name}” saved`)) draft = null;
  }

  function setWatch(type: 'conditions' | 'health'): void {
    if (!draft || draft.kind.type === type) return;
    draft.kind = type === 'health' ? { type: 'health', problems: true } : { type: 'conditions', any: false, conditions: [newCondition()] };
    if (type === 'health') draft.forSecs = 0;
  }

  function setSensor(c: Condition, id: string): void {
    const was = unitOf(c.sensor);
    c.sensor = id;
    // Another kind of reading: amounts that made sense for the old one do not.
    if (unitOf(id) !== was) {
      c.test = newTest(c.test.type, unitOf(id), c.test);
      c.hysteresis = 0;
    }
  }

  const amountLabel = (type: TestType): string =>
    ({ above: 'Limit', below: 'Limit', rises: 'Rise', falls: 'Fall', aboveUsual: 'Above usual by', belowUsual: 'Below usual by' })[type];

  /** The unit shown next to an amount: the chosen temperature unit for temperatures. */
  const shownUnit = (id: string): string => displayUnit(unitOf(id));
  const round = (v: number) => Math.round(v * 10) / 10;

  const when = (t: number) =>
    new Date(t * 1000).toLocaleString('en-US', { weekday: 'short', hour: '2-digit', minute: '2-digit', hour12: false });
</script>

{#snippet condition(d: AlertRule, c: Condition, i: number, count: number)}
  {@const su = unitOf(c.sensor)}
  {@const limit = isLimit(c.test)}
  <li class="cond">
    <label class="sensor">
      <span>Sensor</span>
      <select value={c.sensor} onchange={(e) => setSensor(c, (e.currentTarget as HTMLSelectElement).value)}>
        {#each groups as g (g.group)}
          <optgroup label={GROUP_LABEL[g.group] ?? g.group}>
            {#each g.sensors.filter((s) => s.unit !== '') as s (s.id)}
              <option value={s.id}>{s.name} ({displayUnit(s.unit)})</option>
            {/each}
          </optgroup>
        {/each}
        {#if c.sensor && !byId.has(c.sensor)}
          <option value={c.sensor}>{c.sensor} (not present)</option>
        {/if}
      </select>
    </label>
    <label>
      <span>When it</span>
      <select value={c.test.type} onchange={(e) => (c.test = newTest((e.currentTarget as HTMLSelectElement).value as TestType, su, c.test))}>
        {#each Object.entries(TEST_LABEL) as [type, label] (type)}
          <option value={type}>{label}</option>
        {/each}
      </select>
    </label>
    <label>
      <span>{amountLabel(c.test.type)}{shownUnit(c.sensor) ? ` (${shownUnit(c.sensor)})` : ''}</span>
      <input
        type="number"
        step="any"
        min={limit ? undefined : 0}
        value={round(shownAmount(amountOf(c.test), limit, su, unit))}
        oninput={(e) => {
          const v = (e.currentTarget as HTMLInputElement).valueAsNumber;
          c.test = withAmount(c.test, storedAmount(v, limit, su, unit));
        }}
      />
    </label>
    {#if isRate(c.test)}
      <label>
        <span>Within (s)</span>
        <input
          type="number"
          min={RATE_SECS.min}
          max={RATE_SECS.max}
          value={c.test.secs}
          oninput={(e) => {
            if (isRate(c.test)) c.test = { ...c.test, secs: (e.currentTarget as HTMLInputElement).valueAsNumber };
          }}
        />
      </label>
    {/if}
    <label title="Once it holds, it clears only this far back past its limit, so a reading that hovers at the limit does not fire again and again.">
      <span>Clear margin{shownUnit(c.sensor) ? ` (${shownUnit(c.sensor)})` : ''}</span>
      <input
        type="number"
        step="any"
        min="0"
        value={round(shownAmount(c.hysteresis, false, su, unit))}
        oninput={(e) => (c.hysteresis = storedAmount((e.currentTarget as HTMLInputElement).valueAsNumber, false, su, unit))}
      />
    </label>
    {#if count > 1}
      <button type="button" class="remove" onclick={() => d.kind.type === 'conditions' && d.kind.conditions.splice(i, 1)}>
        Remove<span class="sr-only"> condition {i + 1}</span>
      </button>
    {/if}
    {#if c.test.type === 'aboveUsual' || c.test.type === 'belowUsual'}
      <p class="hint">
        {#if !(c.sensor in usual)}
          Looking up its usual reading…
        {:else if usual[c.sensor] == null}
          Needs about a day of history first; until then this condition never holds.
        {:else}
          Usually {fmtReading(usual[c.sensor], su)} now{su === '°C' && c.sensor.startsWith('cpu.') ? ' at this CPU load' : su === '°C' && c.sensor.startsWith('gpu.') ? ' at this GPU load' : ''}, from the past week.
        {/if}
      </p>
    {/if}
  </li>
{/snippet}

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
      <input type="text" bind:value={d.name} maxlength="80" placeholder="e.g. Hot under load" />
    </label>
    <fieldset class="full kind">
      <legend>Watch</legend>
      <label><input type="radio" checked={d.kind.type === 'conditions'} onchange={() => setWatch('conditions')} /> Sensors</label>
      <label><input type="radio" checked={d.kind.type === 'health'} onchange={() => setWatch('health')} /> System Health</label>
    </fieldset>
    {#if d.kind.type === 'conditions'}
      {@const kind = d.kind}
      <div class="full conds">
        {#if kind.conditions.length > 1}
          <label class="inline">
            <span>Fire when</span>
            <select bind:value={kind.any}>
              <option value={false}>all of these hold</option>
              <option value={true}>any of these holds</option>
            </select>
          </label>
        {/if}
        <ol class="cond-list" aria-label="Conditions">
          {#each kind.conditions as c, i (i)}
            {@render condition(d, c, i, kind.conditions.length)}
          {/each}
        </ol>
        {#if kind.conditions.length < MAX_CONDITIONS}
          <button type="button" onclick={() => kind.conditions.push(newCondition(kind.conditions.at(-1)?.sensor))}>Add condition</button>
        {/if}
      </div>
      <label>
        <span>For at least (s)</span>
        <input type="number" min="0" max="3600" bind:value={d.forSecs} />
      </label>
    {:else if d.kind.type === 'health'}
      {@const kind = d.kind}
      <label class="full">
        <span>Notify on</span>
        <select bind:value={kind.problems}>
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
    <fieldset class="full do">
      <legend>When it fires</legend>
      <label class="check"><input type="checkbox" bind:checked={d.actions.notify} /> Windows notification</label>
      <label class="check sub"><input type="checkbox" bind:checked={d.sound} disabled={!d.actions.notify} /> with sound</label>
      {#if d.kind.type !== 'health'}
        <label class="check"><input type="checkbox" bind:checked={d.actions.tray} /> Name it in the tray while it holds</label>
        <div class="action">
          <label class="check">
            <input
              type="checkbox"
              checked={d.actions.profile != null}
              onchange={(e) => (d.actions.profile = (e.currentTarget as HTMLInputElement).checked ? 'silent' : null)}
            />
            Switch profile while it holds
          </label>
          {#if d.actions.profile != null}
            <label class="inline">
              <span class="sr-only">Profile</span>
              <select bind:value={d.actions.profile}>
                {#each profiles.list as p (p.id)}
                  <option value={p.id}>{p.name}</option>
                {/each}
              </select>
            </label>
          {/if}
        </div>
        <div class="action">
          <label class="check">
            <input
              type="checkbox"
              checked={d.actions.fanBoost != null}
              onchange={(e) => (d.actions.fanBoost = (e.currentTarget as HTMLInputElement).checked ? 15 : null)}
            />
            Raise the fans Vyro drives while it holds
          </label>
          {#if d.actions.fanBoost != null}
            <label class="inline">
              <span>by (%)</span>
              <input type="number" min={FAN_BOOST.min} max={FAN_BOOST.max} bind:value={d.actions.fanBoost} />
            </label>
            <p class="hint">
              {#if fansOn}
                Only headers Vyro drives, never past 100 %; safety full speed still comes first.
              {:else}
                Fan control is off, so this changes nothing until you turn it on in Cooling.
              {/if}
            </p>
          {/if}
        </div>
      {/if}
      <label class="check"><input type="checkbox" bind:checked={d.actions.record} /> Keep a diagnostic record (readings and busiest programs)</label>
      {#if d.kind.type !== 'health'}
        <p class="hint full">Tray, profile and fan actions end {CLEAR_SECS} s after the conditions clear; a profile you pick meanwhile wins.</p>
      {/if}
    </fieldset>
    <div class="full actions">
      {#if problem}<span class="err" role="status">{problem}</span>{/if}
      <button type="button" onclick={() => (draft = null)}>Cancel</button>
      <button type="submit" class="primary" disabled={!!problem}>{isNew ? 'Add rule' : 'Save'}</button>
    </div>
  </form>
{/snippet}

<section class="alerts" id="alerts">
  <div class="head">
    <h2>Alerts</h2>
    <div class="tools">
      {#if saved}<span class="saved" role="status">{saved}</span>{/if}
      <button type="button" onclick={() => void testAlert(true)}>Send test notification</button>
      <button type="button" class="primary" onclick={add} disabled={!!draft}>Add rule</button>
    </div>
  </div>
  <p class="note">
    Windows notifications while Vyro is running, including from the tray (Settings: keep running in the tray, start
    with Windows). A rule can watch several sensors at once, and can also switch the profile, raise the fans or keep a
    record while it holds. Rules are saved on this PC.
  </p>
  {#if error}<p class="err">{error}</p>{/if}

  <ul class="rules">
    {#each rules as r (r.id)}
      {@const now = holding.get(r.id)}
      {@const does = actionsText(r.actions, nameOf)}
      <li class:off={!r.enabled}>
        <label class="switch" title={r.enabled ? 'Turn off' : 'Turn on'}>
          <input type="checkbox" checked={r.enabled} onchange={() => toggle(r)} aria-label="{r.name} enabled" />
          <span aria-hidden="true"></span>
        </label>
        <div class="desc">
          <b>{r.name}{#if now}<span class="badge" title="Its conditions hold now">Active</span>{/if}</b>
          <span>{tempText(describeRule(r, info, unit))}</span>
          {#if does.length}<span class="does">Then: {does.join(' · ')}</span>{/if}
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
        <li>
          <time>{when(a.ts)}</time> <b>{tempText(a.title)}</b> {tempText(a.message)}
          {#if a.also}
            <span class="muted">· not shown again: it repeats “{a.also}”</span>
          {:else if !a.notify}
            <span class="muted">· logged only</span>
          {/if}
          {#if a.actions.length}
            <span class="did">{a.actions.join(' · ')}</span>
          {/if}
          {#if a.diagnostic}
            <details>
              <summary>Diagnostic record</summary>
              <div class="diag">
                <ul aria-label="Readings">
                  {#each a.diagnostic.readings as x (x.id)}
                    <li><span>{x.name}</span> <b>{fmtReading(x.value, x.unit)}</b></li>
                  {/each}
                </ul>
                {#if a.diagnostic.programs.length}
                  <ul aria-label="Busiest programs">
                    {#each a.diagnostic.programs as p, j (j)}
                      <li>
                        <span>{p.name}</span>
                        <b>{p.cpuPct != null ? `${p.cpuPct.toFixed(1)}% CPU` : ''}{p.gpuPct != null && p.gpuPct >= 1 ? ` · ${p.gpuPct.toFixed(0)}% GPU` : ''}</b>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            </details>
          {/if}
        </li>
      {/each}
    </ol>
  {:else}
    <p class="note">None since Vyro started.</p>
  {/if}
</section>

<style>
  .alerts {
    margin-top: 1.4rem;
    /* Ctrl K's Alerts lands here, below the sticky header. */
    scroll-margin-top: calc(var(--topbar-h) + 1rem);
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
  /* Switched-off rules: the name fades, the description stays readable (contrast). */
  .rules > li.off .desc b {
    color: var(--muted);
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
  .editor label:not(.check):not(.kind label):not(.inline) {
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
  .desc .badge {
    margin-left: 0.45rem;
    padding: 0 0.4rem;
    border-radius: 4px;
    background: color-mix(in srgb, var(--warn) 22%, var(--bg));
    color: var(--text);
    font-size: 0.68rem;
    font-weight: 600;
    vertical-align: 0.08rem;
  }
  .desc .does {
    color: var(--text);
  }
  .conds {
    display: grid;
    gap: 0.5rem;
    justify-items: start;
  }
  .cond-list {
    display: grid;
    gap: 0.5rem;
    width: 100%;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .cond {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 9.5rem), 1fr));
    align-items: end;
    gap: 0.45rem 0.6rem;
    padding: 0.55rem 0.65rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--bg);
  }
  .cond > * {
    min-width: 0;
  }
  .cond input,
  .cond select {
    width: 100%;
    min-width: 0;
  }
  .cond .sensor {
    grid-column: span 2;
  }
  .cond .hint {
    grid-column: 1 / -1;
  }
  .remove {
    justify-self: start;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .editor label.inline {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }
  .editor label.inline > span {
    color: var(--muted);
  }
  .editor label.inline input[type='number'] {
    width: 5rem;
  }
  .do {
    flex-direction: column;
    gap: 0.35rem;
  }
  .do legend {
    float: none;
    margin-bottom: 0.2rem;
  }
  .do .check {
    align-self: start;
    padding-bottom: 0;
  }
  .do .sub {
    margin-left: 1.5rem;
  }
  .action {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem 0.8rem;
  }
  .action .hint {
    flex-basis: 100%;
    margin-left: 1.5rem;
  }
  .log .muted {
    color: var(--muted);
  }
  .log .did {
    display: block;
    margin-left: 6.8rem;
    color: var(--muted);
    font-size: 0.76rem;
  }
  details {
    margin: 0.25rem 0 0 6.8rem;
  }
  summary {
    color: var(--muted);
    font-size: 0.76rem;
    cursor: pointer;
  }
  .diag {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 2rem;
    margin-top: 0.3rem;
  }
  .diag ul {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.76rem;
  }
  .diag li {
    padding: 0.05rem 0;
  }
  .diag span {
    display: inline-block;
    min-width: 10rem;
    color: var(--muted);
  }
  .diag b {
    font-family: var(--font-num);
    font-weight: 500;
  }
  @media (max-width: 560px) {
    .rules > li:not(.editing) {
      flex-wrap: wrap;
    }
    .row-tools {
      width: 100%;
      justify-content: flex-end;
    }
    .cond .sensor {
      grid-column: 1 / -1;
    }
    .log .did,
    details {
      margin-left: 0;
    }
  }
</style>

<script lang="ts">
  // GPU tuning (1.6): the card's presets, the owner's own, the stability test and "Find my best".
  // The service changes the card and checks it; this view only asks.
  import { onMount, untrack } from 'svelte';
  import Card from './Card.svelte';
  import { fmt, fmtDateTime } from '../format';
  import { MAX_NAME, MAX_PRESETS, STOCK, clamp, describe, inForce, isStock, nameProblem, newId, runningText, same, testedText, withPreset, withoutPreset } from '../gputune';
  import {
    getGpuTestProgress,
    getGpuTune,
    getGpuTuneEvents,
    getProfiles,
    onGpuTest,
    restoreGpu,
    setGpuTune,
    startGpuTest,
    stopGpuTest,
  } from '../ipc';
  import type {
    BackendStatus,
    GpuTestProgress,
    Profile,
    ProfileSpec,
    SensorSnapshot,
    TuneConfig,
    TuneEvent,
    TunePresetView,
    TuneSettings,
    TuneStatus,
  } from '../types';

  interface Props {
    snap: SensorSnapshot | null;
    backend: BackendStatus | null;
    active: boolean;
  }
  let { snap, backend, active }: Props = $props();

  let status = $state.raw<TuneStatus | null>(null);
  let events = $state.raw<TuneEvent[]>([]);
  let profiles = $state.raw<ProfileSpec[]>([]);
  let error = $state<string | null>(null);
  let message = $state<string | null>(null);
  let busy = $state(false);
  let confirm = $state<HTMLDialogElement | null>(null);
  let progress = $state.raw<GpuTestProgress | null>(null);
  /** The custom settings being edited, and the owner's preset they came from. */
  let draft = $state<TuneSettings>({ ...STOCK });
  let draftName = $state('');
  let editing = $state<string | null>(null);

  const FAMILY: Record<string, string> = {
    turing: 'Turing (RTX 20 / GTX 16)',
    ampere: 'Ampere (RTX 30)',
    ada: 'Ada Lovelace (RTX 40)',
    blackwell: 'Blackwell (RTX 50)',
    rdna2: 'RDNA 2 (RX 6000)',
    rdna3: 'RDNA 3 (RX 7000)',
    rdna4: 'RDNA 4 (RX 9000)',
    alchemist: 'Alchemist (Arc A)',
    battlemage: 'Battlemage (Arc B)',
  };
  /** Whose driver interface tunes the card (2.0), as the anti-cheat note names it. */
  const INTERFACE: Record<string, string> = {
    nvidia: "NVIDIA's own driver interface, as NVIDIA's tools do",
    amd: "AMD's own driver interface (ADLX), as AMD Software does",
    intel: "Intel's own driver interface, as Intel's tools do",
  };

  function refresh(): void {
    getGpuTune()
      .then((s) => {
        status = s;
        error = null;
      })
      .catch((e: unknown) => (error = String(e).replace(/^Error: /, '')));
  }
  function loadEvents(): void {
    getGpuTuneEvents()
      .then((e) => (events = [...e].reverse().slice(0, 30)))
      .catch(() => {});
  }
  const ts = $derived(snap?.ts ?? 0);
  $effect(() => {
    if (!active) return;
    void ts;
    untrack(refresh);
  });
  const eventKey = $derived(status ? `${status.applied}|${status.held}|${status.waiting}|${status.config.enabled}|${status.config.tested.length}` : '');
  $effect(() => {
    if (!active) return;
    void eventKey;
    untrack(loadEvents);
  });
  $effect(() => {
    if (!active || profiles.length) return;
    untrack(() =>
      getProfiles()
        .then((p) => (profiles = p))
        .catch(() => {}),
    );
  });
  onMount(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    void getGpuTestProgress().then((p) => (progress = p));
    void onGpuTest(
      (p) => (progress = p),
      (d) => {
        progress = null;
        message = d.message;
        refresh();
        loadEvents();
      },
    ).then((fn) => {
      if (gone) fn();
      else stop = fn;
    });
    return () => {
      gone = true;
      stop?.();
    };
  });

  const writable = $derived(status?.capability.state === 'writable');
  const on = $derived(status?.config.enabled ?? false);
  const limits = $derived(status?.limits ?? null);
  const own = $derived(status?.config.presets ?? []);
  const force = $derived(status ? inForce(status) : null);
  const testing = $derived(progress != null);
  const profileName = (p: Profile) => profiles.find((x) => x.id === p)?.name ?? p;
  const missing = $derived.by(() => {
    if (!limits) return [];
    const m: string[] = [];
    if (!limits.core) m.push('core offset');
    if (!limits.mem) m.push('memory offset');
    if (!limits.power) m.push('power limit');
    return m;
  });

  async function run(f: () => Promise<void>, done?: string): Promise<void> {
    busy = true;
    error = null;
    message = null;
    try {
      await f();
      if (done) message = done;
      refresh();
      loadEvents();
    } catch (e) {
      error = String(e).replace(/^Error: /, '');
    } finally {
      busy = false;
    }
  }

  const save = (cfg: TuneConfig, apply: boolean, done?: string) => run(() => setGpuTune(cfg, apply), done);

  function turnOn(): void {
    confirm?.close();
    if (!status) return;
    void save({ ...status.config, enabled: true }, true, 'GPU tuning is on. Stock is applied until you choose a preset.');
  }
  function turnOff(): void {
    if (!status) return;
    void save({ ...status.config, enabled: false }, true, 'GPU tuning is off: the card runs stock.');
  }
  /** Makes `p` the preset in force (for every profile without its own) and applies it. */
  function apply(p: TunePresetView): void {
    if (!status) return;
    void save({ ...status.config, preset: p.id }, true, `${p.name} applied.`);
  }
  function test(s: TuneSettings): void {
    void run(() => startGpuTest('test', s));
  }
  function findBest(): void {
    void run(() => startGpuTest('best', null));
  }
  function stopTest(): void {
    void run(() => stopGpuTest());
  }
  function toStock(): void {
    void run(() => restoreGpu(), 'The card runs stock until you apply a preset.');
  }

  // Custom settings.
  function edit(p: TunePresetView): void {
    draft = { ...p.settings };
    draftName = p.kind === 'custom' ? p.name : `${p.name} (mine)`;
    editing = p.kind === 'custom' ? p.id : null;
    document.getElementById('gpu-custom')?.scrollIntoView({ block: 'start', behavior: 'smooth' });
  }
  const nameIssue = $derived(status ? nameProblem(draftName, own, editing) : null);
  const full = $derived(editing == null && own.length >= MAX_PRESETS);
  function keep(applyIt: boolean): void {
    if (!status || !limits || nameIssue || full) return;
    const id = editing ?? newId([...status.presets, ...own]);
    const preset = { id, name: draftName.trim(), settings: clamp(draft, limits), kind: 'custom' as const, note: null };
    let cfg = withPreset(status.config, preset);
    if (applyIt) cfg = { ...cfg, preset: id };
    editing = id;
    void save(cfg, applyIt, applyIt ? `${preset.name} saved and applied.` : `${preset.name} saved.`);
  }
  function remove(p: TunePresetView): void {
    if (!status) return;
    if (editing === p.id) editing = null;
    void save(withoutPreset(status.config, p.id), force?.id === p.id, `${p.name} deleted.`);
  }
  function setProfile(p: Profile, id: string): void {
    if (!status) return;
    const profiles = { ...status.config.profiles };
    if (id === '') delete profiles[p];
    else profiles[p] = id;
    void save({ ...status.config, profiles }, true);
  }
  function setApplyAtStart(v: boolean): void {
    if (!status) return;
    void save({ ...status.config, applyAtStart: v }, false);
  }
  const w = (pct: number) => (limits?.powerDefaultW != null ? ` (${fmt((limits.powerDefaultW * pct) / 100, 0)} W)` : '');
</script>

<section class="panel-wrap">
  {#if !status}
    <p class="panel empty" role="status">{error ?? 'Reading the GPU…'}</p>
  {:else if !writable}
    <div class="panel empty" role="status">
      <h2>GPU tuning is not available</h2>
      <p>{status.capability.state !== 'writable' ? status.capability.reason : ''}.</p>
      {#if backend?.mode === 'local'}
        <p class="note">GPU tuning runs in the Vyro sensor service, which is not running.</p>
      {:else}
        <p class="note">
          Vyro tunes NVIDIA RTX 20 to RTX 50, AMD Radeon RX 6000 to RX 9000 and Intel Arc A- and B-series cards, desktop and
          laptop, each through its maker's own driver interface. Graphics built into a processor are not tuned.
        </p>
      {/if}
    </div>
  {:else if limits}
    <div class="head">
      <div>
        <h2>GPU tuning</h2>
        <p class="sub">
          {status.gpu?.name}{status.gpu?.family ? ` · ${FAMILY[status.gpu.family]}` : ''}{status.gpu?.laptop ? ' · laptop' : ''}{status.gpu?.driver
            ? ` · ${status.gpu.vendor === 'amd' ? 'AMD Software' : 'driver'} ${status.gpu.driver}`
            : ''}
        </p>
      </div>
      <div class="actions">
        {#if on}
          <button type="button" disabled={busy} onclick={turnOff}>Turn off</button>
          <button type="button" class="primary" disabled={busy || isStock(status.current ?? STOCK)} onclick={toStock}>Return to stock</button>
        {:else}
          <button type="button" class="primary" disabled={busy} onclick={() => confirm?.showModal()}>Turn on GPU tuning…</button>
        {/if}
      </div>
    </div>

    {#if error}<p class="alert" role="alert">{error}</p>{/if}
    {#if message}<p class="note ok" role="status">{message}</p>{/if}
    {#if status.held}
      <div class="alert" role="alert">
        <span>{status.held}</span>
        {#if force}<button type="button" disabled={busy} onclick={() => force && apply(force)}>Apply {force.name} again</button>{/if}
      </div>
    {:else if status.waiting && on}
      <div class="pending" role="status">
        <span>{status.waiting}</span>
        {#if force && !isStock(force.settings)}
          <button type="button" class="primary" disabled={busy} onclick={() => force && apply(force)}>Apply {force.name}</button>
        {/if}
      </div>
    {/if}

    {#if !on}
      <div class="panel intro">
        <p>
          Change the card's core and memory clocks and its power limit, with presets made for this card's family, your own
          settings, and a stability test. Off until you turn it on; the card runs its own settings.
        </p>
        {#if missing.length}
          <p class="note">This card's driver does not allow changing its {missing.join(' or ')}.</p>
        {/if}
      </div>
    {:else}
      {#if progress}
        <div class="panel test" role="status" aria-live="polite">
          <div class="test-head">
            <b>{progress.kind === 'best' ? 'Find my best' : 'Stability test'}</b>
            <span>{progress.step}</span>
            <button type="button" onclick={stopTest}>Stop</button>
          </div>
          <progress max="1" value={progress.done} aria-label="Test progress">{Math.round(progress.done * 100)} %</progress>
          <p class="note">
            The card runs {describe(progress.settings)} while Vyro checks every result. Leave the PC alone; if the screen goes
            black for a moment, the driver reset and the settings failed.
          </p>
        </div>
      {/if}

      <div class="grid">
        <Card title="Now">
          <p class="state"><b>{runningText(status)}</b></p>
          <dl class="kv">
            <dt>Core offset</dt>
            <dd>{status.current ? `${status.current.coreMhz > 0 ? '+' : ''}${status.current.coreMhz} MHz` : '—'}</dd>
            <dt>Memory offset</dt>
            <dd>{status.current ? `${status.current.memMhz > 0 ? '+' : ''}${status.current.memMhz} MHz` : '—'}</dd>
            <dt>Power limit</dt>
            <dd>{status.current ? `${status.current.powerPct} %` : '—'}{status.clocks.powerLimitW != null ? ` · ${fmt(status.clocks.powerLimitW, 0)} W` : ''}</dd>
            <dt>Highest core clock</dt>
            <dd>{status.clocks.coreMaxMhz != null ? `${fmt(status.clocks.coreMaxMhz, 0)} MHz` : '—'}</dd>
            <dt>Memory clock</dt>
            <dd>{status.clocks.memMaxMhz != null ? `${fmt(status.clocks.memMaxMhz, 0)} MHz` : '—'}</dd>
          </dl>
          {#if missing.length}
            <p class="note">This card's driver does not allow changing its {missing.join(' or ')}.</p>
          {/if}
        </Card>

        <Card title="Presets" subtitle="Suggested for this card's family, scaled to what its driver allows" wide>
          <ul class="presets">
            {#each status.presets as p (p.id)}
              {@const tested = testedText(p.testedAt)}
              <li class:current={status.applied === p.id || (p.id === 'stock' && status.applied == null && !status.trial)}>
                <div class="what">
                  <b>{p.name}</b>
                  {#if force?.id === p.id}<span class="tag on">chosen</span>{/if}
                  {#if tested && p.id !== 'stock'}<span class="tag ok">{tested}</span>{/if}
                  <span class="vals">{describe(p.settings)}{p.settings.powerPct !== 100 ? w(p.settings.powerPct) : ''}</span>
                  {#if p.note}<span class="pnote">{p.note}</span>{/if}
                </div>
                <div class="row-actions">
                  <button type="button" class="primary" disabled={busy || testing} onclick={() => apply(p)}>
                    Apply<span class="sr-only"> {p.name}</span>
                  </button>
                  {#if p.id !== 'stock'}
                    <button type="button" disabled={busy || testing} onclick={() => test(p.settings)}>
                      Test<span class="sr-only"> {p.name}</span>
                    </button>
                    <button type="button" disabled={busy} onclick={() => edit(p)}>
                      {p.kind === 'custom' ? 'Edit' : 'Copy'}<span class="sr-only"> {p.name}</span>
                    </button>
                  {/if}
                  {#if p.kind === 'custom'}
                    <button type="button" class="danger" disabled={busy} onclick={() => remove(p)}>
                      Delete<span class="sr-only"> {p.name}</span>
                    </button>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
          <p class="note">
            “Test” runs the stability test for about 5 minutes: full load with every result checked. Settings that pass are
            marked tested, and only tested settings are applied when Vyro starts.
          </p>
        </Card>

        <Card title="Custom" subtitle={editing ? `Editing ${draftName}` : 'Your own settings'} id="gpu-custom">
          {#if limits.core}
            <label class="slider">
              <span>Core offset <b>{draft.coreMhz > 0 ? '+' : ''}{draft.coreMhz} MHz</b></span>
              <input type="range" min={limits.core.min} max={limits.core.max} step="5" bind:value={draft.coreMhz} />
              <small>{limits.core.min} to +{limits.core.max} MHz allowed by the driver</small>
            </label>
          {/if}
          {#if limits.mem}
            <label class="slider">
              <span>Memory offset <b>{draft.memMhz > 0 ? '+' : ''}{draft.memMhz} MHz</b></span>
              <input type="range" min={limits.mem.min} max={limits.mem.max} step="25" bind:value={draft.memMhz} />
              <small>{limits.mem.min} to +{limits.mem.max} MHz allowed by the driver</small>
            </label>
          {/if}
          {#if limits.power}
            <label class="slider">
              <span>Power limit <b>{draft.powerPct} %{w(draft.powerPct)}</b></span>
              <input type="range" min={limits.power.min} max={limits.power.max} step="1" bind:value={draft.powerPct} />
              <small>{limits.power.min} to {limits.power.max} % allowed by the driver</small>
            </label>
          {/if}
          <label class="name">
            <span>Name</span>
            <input type="text" maxlength={MAX_NAME} bind:value={draftName} placeholder="My settings" />
          </label>
          {#if draftName && nameIssue}<p class="note warn-text">{nameIssue}</p>{/if}
          {#if full}<p class="note warn-text">You have {MAX_PRESETS} presets: delete one to save another.</p>{/if}
          <div class="actions">
            <button type="button" disabled={busy || !!nameIssue || full || testing} onclick={() => keep(false)}>Save</button>
            <button type="button" class="primary" disabled={busy || !!nameIssue || full || testing} onclick={() => keep(true)}>Save and apply</button>
            <button type="button" disabled={busy || testing || isStock(draft)} onclick={() => test(clamp(draft, limits))}>Test</button>
            <button type="button" class="ghost" disabled={busy} onclick={() => ((draft = { ...STOCK }), (draftName = ''), (editing = null))}>Clear</button>
          </div>
          {#if !same(clamp(draft, limits), draft)}
            <p class="note">Values outside the driver's range are brought within it.</p>
          {/if}
        </Card>

        <Card title="Find my best">
          <p class="small">
            Raises the core clock step by step while the stability test passes, then the memory clock while it stays fast,
            keeps a margin below the last step that passed, and tests the result for 5 minutes. About 20 to 40 minutes, at the
            highest power limit. The result is saved as “Best for this card”.
          </p>
          <div class="actions">
            <button type="button" class="primary" disabled={busy || testing} onclick={findBest}>Find my best</button>
          </div>
          <p class="note">It does not start while a game runs, a benchmark runs or Health shows a warning.</p>
        </Card>

        <Card title="Options">
          <label class="check">
            <input type="checkbox" checked={status.config.applyAtStart} disabled={busy} onchange={(e) => setApplyAtStart(e.currentTarget.checked)} />
            <span>Apply at start <small>(only settings that passed the stability test)</small></span>
          </label>
          <h3>Per profile</h3>
          <div class="profiles">
            {#each profiles as pr (pr.id)}
              <label>
                <span>{pr.name}</span>
                <select value={status.config.profiles[pr.id] ?? ''} disabled={busy} onchange={(e) => setProfile(pr.id, e.currentTarget.value)}>
                  <option value="">As chosen above</option>
                  {#each status.presets as p (p.id)}
                    <option value={p.id}>{p.name}</option>
                  {/each}
                </select>
              </label>
            {/each}
          </div>
          <p class="note">In use now: the {profileName(status.profile)} profile.</p>
        </Card>

        <Card title="Safety">
          <ul class="safety">
            <li>The card goes back to stock at 90 °C, or 105 °C at its hotspot, until you apply a preset again.</li>
            <li>After a crash or a display driver reset while tuned, it starts at stock and “Apply at start” is turned off.</li>
            <li>Stock before sleep, when the Vyro service stops and when Vyro is uninstalled.</li>
            <li>Vyro reads the values back every 5 seconds; if another program keeps changing them, Vyro stops tuning.</li>
            <li>From a phone, only Stock and tested presets can be applied.</li>
          </ul>
        </Card>

        <Card title="Recent events" subtitle={events.length ? null : 'Nothing yet'}>
          {#if events.length}
            <ul class="events">
              {#each events as ev, i (i)}
                <li class:bad={ev.kind === 'fault' || ev.kind === 'hot' || ev.kind === 'crash' || ev.kind === 'failed'}>
                  <time datetime={new Date(ev.ts * 1000).toISOString()}>{fmtDateTime(ev.ts)}</time>
                  <span>{ev.message}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="note">Presets applied, tests, guards and profile changes show here.</p>
          {/if}
        </Card>
      </div>
    {/if}

    <dialog bind:this={confirm} aria-labelledby="gpu-confirm-title">
      <h2 id="gpu-confirm-title">Turn on GPU tuning?</h2>
      <p>Vyro will change the {status.gpu?.name}'s clocks and power limit when you apply a preset.</p>
      <ul>
        <li>Higher clocks can crash games or the PC, or show glitches. Test settings before you rely on them.</li>
        {#if status.gpu?.vendor === 'intel'}
          <li>
            Intel asks every tuning tool to say this: raising clocks or power can shorten the card's life, and turning tuning on
            accepts Intel's overclocking warranty waiver for this card.
          </li>
        {:else}
          <li>Raising clocks may affect your card's warranty, as with any tuning tool.</li>
        {/if}
        <li>
          Some games' anti-cheat checks tuning tools. Vyro uses only {INTERFACE[status.gpu?.vendor ?? 'nvidia']}; see the
          anti-cheat notes in Vyro's documentation.
        </li>
        <li>After a crash or a driver reset Vyro starts the card at stock. Closing the window does not end tuning; use “Return to stock”.</li>
        <li>
          Close MSI Afterburner, AMD Software's own tuning and other tuning tools first: two programs setting the same clocks
          fight.
        </li>
      </ul>
      <div class="actions">
        <button type="button" onclick={() => confirm?.close()}>Cancel</button>
        <button type="button" class="primary" onclick={turnOn}>Turn on</button>
      </div>
    </dialog>
  {/if}
</section>

<style>
  .empty h2,
  .head h2 {
    margin: 0 0 0.35rem;
    font-size: 1rem;
  }
  .empty p {
    max-width: 44rem;
    margin: 0 0 0.4rem;
    font-size: 0.88rem;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.75rem;
    margin-bottom: 0.9rem;
  }
  .sub {
    margin: 0;
    color: var(--muted);
    font-size: 0.82rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  button {
    padding: 0.35rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .primary {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
    background: color-mix(in srgb, var(--accent) 16%, var(--bg));
  }
  .ghost {
    background: none;
  }
  input[type='text'],
  select {
    padding: 0.3rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
  }
  .alert,
  .pending {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
    margin: 0 0 0.9rem;
    padding: 0.55rem 0.85rem;
    border-radius: 10px;
    font-size: 0.85rem;
  }
  .alert {
    border: 1px solid color-mix(in srgb, var(--hot) 55%, var(--line));
    background: color-mix(in srgb, var(--hot) 10%, var(--panel));
  }
  .pending {
    border: 1px solid color-mix(in srgb, var(--warn) 45%, var(--line));
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
  }
  .intro p {
    max-width: 48rem;
    margin: 0 0 0.4rem;
    font-size: 0.88rem;
  }
  .test {
    margin-bottom: 0.9rem;
  }
  .test-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.9rem;
    margin-bottom: 0.5rem;
    font-size: 0.88rem;
  }
  .test-head button {
    margin-left: auto;
  }
  progress {
    width: 100%;
    height: 0.6rem;
    accent-color: var(--accent);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 19rem), 1fr));
    gap: 0.75rem;
  }
  .state {
    margin: 0 0 0.6rem;
    font-size: 0.86rem;
  }
  .kv {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.3rem 1rem;
    margin: 0;
    font-size: 0.84rem;
  }
  .kv dt {
    color: var(--muted);
  }
  .kv dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .presets {
    display: grid;
    gap: 0.4rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .presets li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
    padding: 0.55rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 10px;
  }
  .presets li.current {
    border-color: var(--accent-line);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .what {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.2rem 0.5rem;
    min-width: 0;
    font-size: 0.86rem;
  }
  .vals {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .pnote {
    flex-basis: 100%;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .row-actions button {
    padding: 0.3rem 0.7rem;
    font-size: 0.8rem;
  }
  .tag {
    padding: 0.05rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.7rem;
  }
  .tag.on {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
    color: var(--accent);
  }
  .tag.ok {
    border-color: color-mix(in srgb, var(--ok) 60%, var(--line));
    color: var(--text);
  }
  .slider,
  .name {
    display: grid;
    gap: 0.3rem;
    margin-bottom: 0.7rem;
    font-size: 0.84rem;
  }
  .slider small {
    color: var(--muted);
  }
  .slider input {
    width: 100%;
    accent-color: var(--accent);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.86rem;
  }
  .check small {
    color: var(--muted);
  }
  h3 {
    margin: 0.9rem 0 0.4rem;
    color: var(--muted);
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .profiles {
    display: grid;
    gap: 0.35rem;
  }
  .profiles label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    font-size: 0.84rem;
  }
  .note {
    margin: 0.4rem 0 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .note.ok {
    margin: 0 0 0.9rem;
    color: var(--text);
  }
  .warn-text {
    color: var(--warn);
  }
  .small {
    margin: 0 0 0.6rem;
    font-size: 0.84rem;
  }
  .safety {
    margin: 0;
    padding-left: 1.1rem;
    font-size: 0.82rem;
  }
  .safety li + li {
    margin-top: 0.3rem;
  }
  .events {
    display: grid;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.8rem;
  }
  .events li {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.6rem;
  }
  .events time {
    color: var(--muted);
    white-space: nowrap;
  }
  .events li.bad span {
    color: var(--hot);
  }
  .danger {
    border-color: color-mix(in srgb, var(--hot) 45%, var(--line));
  }
  dialog {
    max-width: 34rem;
    padding: 1.2rem 1.4rem;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text);
  }
  dialog::backdrop {
    background: rgb(0 0 0 / 55%);
  }
  dialog h2 {
    margin: 0 0 0.6rem;
    font-size: 1.05rem;
  }
  dialog p,
  dialog li {
    font-size: 0.86rem;
  }
  dialog ul {
    margin: 0 0 1rem;
    padding-left: 1.2rem;
  }
  dialog li + li {
    margin-top: 0.3rem;
  }
  dialog .actions {
    justify-content: flex-end;
  }
</style>

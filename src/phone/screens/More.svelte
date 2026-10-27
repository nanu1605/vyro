<script lang="ts">
  // Everything else Vyro does, from the phone (1.5): shut down, restart, sleep or lock the PC,
  // its profile and fans, GPU tuning (1.6), a benchmark, diagnostics, updates, waking another PC,
  // and (1.7) its devices. A tile for each says how it is; a tap opens it. What changes the PC
  // shows only to a phone the owner allowed to control it.
  import Battery from '@lucide/svelte/icons/battery-low';
  import ChevronLeft from '@lucide/svelte/icons/chevron-left';
  import Cpu from '@lucide/svelte/icons/cpu';
  import Download from '@lucide/svelte/icons/download';
  import Fan from '@lucide/svelte/icons/fan';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Power from '@lucide/svelte/icons/power';
  import Sliders from '@lucide/svelte/icons/sliders-horizontal';
  import Stethoscope from '@lucide/svelte/icons/stethoscope';
  import Sunrise from '@lucide/svelte/icons/sunrise';
  import { fmt, fmtDateTime } from '../../format';
  import { runningText } from '../../gputune';
  import type {
    BenchProgress,
    DiagReport,
    DiagSummary,
    Effective,
    FanStatus,
    PowerAction,
    PowerState,
    Preflight,
    ProfileSpec,
    RunSummary,
    TuneStatus,
  } from '../../types';
  import Sheet from '../ui/Sheet.svelte';
  import { toast } from '../ui/toast.svelte';
  import { GONE } from '../relay';
  import { errText, every, type ScreenProps } from './ask';
  import Devices from './Devices.svelte';

  interface Props extends ScreenProps {
    /** Other paired PCs that told this phone how to wake them. */
    sleepers: Array<{ name: string; mac: string }>;
    /** A device's battery is low: what the PC says of it. */
    low: string | null;
  }
  let { ask, can, pc, sleepers, low }: Props = $props();

  interface UpdateStatus {
    configured: boolean;
    current: string;
    checking: boolean;
    available: { version: string } | null;
    error: string | null;
    installing: boolean;
  }
  type Section = 'devices' | 'power' | 'gpu' | 'profile' | 'fans' | 'bench' | 'diag' | 'updates' | 'wake';

  let power = $state.raw<PowerState | null>(null);
  let profiles = $state.raw<{ list: ProfileSpec[]; state: Effective } | null>(null);
  let fans = $state.raw<FanStatus | null>(null);
  let bench = $state.raw<{ progress: BenchProgress | null; preflight: Preflight } | null>(null);
  let runs = $state.raw<RunSummary[]>([]);
  let diags = $state.raw<DiagSummary[]>([]);
  let updates = $state.raw<UpdateStatus | null>(null);
  let gpu = $state.raw<TuneStatus | null>(null);
  let busy = $state(false);
  let section = $state<Section | null>(null);
  /** The power action being confirmed. */
  let confirming = $state<PowerAction | null>(null);
  let soon = $state(true);
  let force = $state(false);
  /** Installing an update is being confirmed. */
  let confirmInstall = $state(false);

  /** Takes what the PC answers; when one answer fails (a slow link), what it said last stays,
   * so a tile does not go and take its open section with it. */
  const take = <T,>(p: Promise<T>, set: (v: T) => void) => p.then(set, () => {});

  function load(): Promise<unknown> {
    return Promise.all([
      take(ask<PowerState>('power.state'), (v) => (power = v)),
      take(ask<typeof profiles>('profiles'), (v) => (profiles = v)),
      take(ask<FanStatus>('fans'), (v) => (fans = v)),
      take(ask<typeof bench>('bench.progress'), (v) => (bench = v)),
      take(ask<RunSummary[]>('bench.runs'), (v) => (runs = v ?? [])),
      take(ask<DiagSummary[]>('diag.runs'), (v) => (diags = v ?? [])),
      take(ask<UpdateStatus>('updates'), (v) => (updates = v)),
      take(ask<TuneStatus>('gpu.tune'), (v) => (gpu = v)),
    ]);
  }
  $effect(() => every(5000, load));

  async function run(what: string, method: string, params: object = {}): Promise<void> {
    busy = true;
    try {
      const r = await ask<unknown>(method, params);
      if (method === 'diag.start') {
        const d = r as DiagReport;
        toast(`Diagnostics done: ${d.checks.filter((c) => c.status === 'attention').length} to look at.`);
      } else toast(what);
      void load();
    } catch (e) {
      toast(errText(e), 'hot');
    } finally {
      busy = false;
    }
  }

  const POWER: Array<{ id: PowerAction; label: string; show: (s: PowerState | null) => boolean }> = [
    { id: 'shutDown', label: 'Shut down', show: () => true },
    { id: 'restart', label: 'Restart', show: () => true },
    { id: 'sleep', label: 'Sleep', show: (s) => s?.canSleep !== false },
    { id: 'hibernate', label: 'Hibernate', show: (s) => s?.canHibernate === true },
    { id: 'lock', label: 'Lock', show: () => true },
    { id: 'signOut', label: 'Sign out', show: () => true },
  ];
  const delayed = (a: PowerAction | null) => a === 'shutDown' || a === 'restart';
  const label = (a: PowerAction) => POWER.find((p) => p.id === a)?.label ?? a;
  const AFTER: Partial<Record<PowerAction, string>> = {
    restart: 'Vyro answers again once someone signs in on the PC.',
    signOut: 'Vyro answers again once someone signs in on the PC.',
    shutDown: 'Wake it from here only through another Vyro PC on its network.',
    sleep: 'Wake it from here only through another Vyro PC on its network.',
    hibernate: 'Wake it from here only through another Vyro PC on its network.',
  };

  /** Vyro closes on the PC as the installer starts, before it answers: the PC leaving is the
   * installer starting, and only another answer says why it did not. */
  async function install(): Promise<void> {
    confirmInstall = false;
    busy = true;
    toast(`Installing: once someone at ${pc} allows it, Vyro starts again there.`);
    try {
      await ask('updates.install');
    } catch (e) {
      if (errText(e) !== GONE) toast(errText(e), 'hot');
    } finally {
      busy = false;
    }
  }

  async function doPower(a: PowerAction): Promise<void> {
    confirming = null;
    await run(`${label(a)}: asked.`, 'power', { action: a, wait: delayed(a) ? (soon ? 30 : 0) : 0, force });
  }

  const gpuOn = $derived(gpu != null && gpu.capability.state === 'writable' && gpu.config.enabled);
  const profileName = $derived(profiles ? (profiles.list.find((p) => p.id === profiles?.state.profile)?.name ?? profiles.state.profile) : null);

  /** The tiles, each with how its part is now. */
  const tiles = $derived(
    [
      { id: 'devices' as const, label: 'Devices', icon: Battery, status: low ? low.replace(/^low battery: /, 'Low: ') : 'Batteries and connections', tone: low ? 'warn' : '', show: can('devices') },
      { id: 'power' as const, label: 'Power', icon: Power, status: power?.pending ? `${label(power.pending.action)} pending` : 'Shut down, restart, sleep, lock', tone: power?.pending ? 'warn' : '', show: can('power') },
      { id: 'gpu' as const, label: 'GPU tuning', icon: Cpu, status: gpu ? runningText(gpu) : '', tone: '', show: gpuOn },
      { id: 'profile' as const, label: 'Profile', icon: Sliders, status: profileName ? `${profileName}${profiles?.state.game ? ` for ${profiles.state.game}` : ''}` : '', tone: '', show: profiles != null },
      { id: 'fans' as const, label: 'Fans', icon: Fan, status: fans ? (fans.active ? `Vyro, ${fans.profile}` : 'The BIOS') : '', tone: '', show: fans != null },
      { id: 'bench' as const, label: 'Benchmark', icon: Gauge, status: bench?.progress ? `${fmt(bench.progress.done * 100)} %` : runs[0] ? `Last ${fmtDateTime(runs[0].ts)}` : 'Quick or full', tone: bench?.progress ? 'ok' : '', show: bench != null },
      { id: 'diag' as const, label: 'Diagnostics', icon: Stethoscope, status: diags[0] ? `${diags[0].attention} to look at` : 'Check this PC', tone: diags[0]?.attention ? 'warn' : '', show: true },
      { id: 'updates' as const, label: 'Updates', icon: Download, status: updates ? (updates.available ? `${updates.available.version} available` : `Vyro ${updates.current}`) : '', tone: updates?.available ? 'ok' : '', show: updates != null },
      { id: 'wake' as const, label: 'Wake another PC', icon: Sunrise, status: sleepers.map((s) => s.name).join(', '), tone: '', show: sleepers.length > 0 && can('wake') },
    ].filter((t) => t.show),
  );
  const open = $derived(tiles.find((t) => t.id === section) ?? null);
</script>

{#if !open}
  <h1 class="title">More on {pc}</h1>
  <ul class="tiles">
    {#each tiles as t, i (t.id)}
      <li class="rise" style:--i={i}>
        <button type="button" class="tile" onclick={() => (section = t.id)}>
          <span class="icon {t.tone}" aria-hidden="true"><t.icon size={20} /></span>
          <span class="name">{t.label}</span>
          {#if t.status}<span class="status {t.tone}">{t.status}</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
{:else}
  <div class="section">
    <button type="button" class="back" onclick={() => (section = null)}>
      <ChevronLeft size={18} aria-hidden="true" /> More
    </button>

    {#if open.id === 'devices'}
      <Devices {ask} {can} {pc} />
    {:else if open.id === 'power'}
      <section class="panel" aria-labelledby="power-title">
        <h1 id="power-title">Power</h1>
        {#if power?.pending}
          <p class="note warn" role="status">
            {label(power.pending.action)} at {fmtDateTime(power.pending.at)}, asked from {power.pending.by}.
          </p>
          <div class="actions">
            <button type="button" disabled={busy} onclick={() => void run('Cancelled.', 'power.cancel')}>Cancel it</button>
          </div>
        {:else}
          <div class="grid">
            {#each POWER.filter((p) => p.show(power)) as p (p.id)}
              <button type="button" class="ghost" disabled={busy} onclick={() => ((confirming = p.id), (soon = true), (force = false))}>
                {p.label}…
              </button>
            {/each}
          </div>
        {/if}
      </section>
    {:else if open.id === 'wake'}
      <section class="panel" aria-labelledby="wake-title">
        <h1 id="wake-title">Wake another PC</h1>
        <p class="muted small">
          {pc} sends the wake signal on its network. It works when the other PC is wired to the same network and set up for
          it (Settings → Remote on that PC).
        </p>
        <div class="actions">
          {#each sleepers as s (s.mac)}
            <button type="button" class="ghost" disabled={busy} onclick={() => void run(`Wake signal sent to ${s.name}.`, 'wake', { mac: s.mac, pc: s.name })}>
              Wake {s.name}
            </button>
          {/each}
        </div>
      </section>
    {:else if open.id === 'gpu' && gpu}
      {@const usable = gpu.presets.filter((p) => p.testedAt != null)}
      <section class="panel" aria-labelledby="gpu-title">
        <h1 id="gpu-title">GPU tuning</h1>
        <p>{runningText(gpu)}{gpu.held ? `. ${gpu.held}` : ''}</p>
        {#if can('gpu.preset')}
          <div class="actions">
            {#each usable as p (p.id)}
              <button
                type="button"
                class="ghost"
                aria-pressed={gpu.applied === p.id || (p.id === 'stock' && gpu.applied == null)}
                disabled={busy || gpu.trial != null}
                onclick={() => void run(`${p.name} applied.`, 'gpu.preset', { id: p.id, name: p.name })}>{p.name}</button
              >
            {/each}
          </div>
          <p class="muted small">Stock and presets that passed the stability test. Your own values and tests are set at the PC.</p>
        {/if}
      </section>
    {:else if open.id === 'profile' && profiles}
      <section class="panel" aria-labelledby="profile-title">
        <h1 id="profile-title">Profile</h1>
        <p>{profileName} in force{profiles.state.game ? ` (for ${profiles.state.game})` : ''}.</p>
        {#if can('profile.pick')}
          <div class="actions">
            {#each profiles.list as p (p.id)}
              <button
                type="button"
                class="ghost"
                aria-pressed={p.id === profiles.state.base}
                disabled={busy}
                onclick={() => void run(`${p.name} profile chosen.`, 'profile.pick', { profile: p.id })}>{p.name}</button
              >
            {/each}
          </div>
        {/if}
      </section>
    {:else if open.id === 'fans' && fans}
      <section class="panel" aria-labelledby="fans-title">
        <h1 id="fans-title">Fans</h1>
        <p>
          {fans.active ? `Vyro drives the fans (${fans.profile} settings).` : 'The BIOS drives the fans.'}
          {fans.overrideReason ?? ''}
        </p>
        {#if fans.active && can('fans.bios')}
          <div class="actions">
            <button type="button" class="ghost" disabled={busy} onclick={() => void run('Fans back to the BIOS.', 'fans.bios')}>Return to BIOS</button>
          </div>
        {/if}
        <p class="muted small">Fan control is turned on and set up at the PC.</p>
      </section>
    {:else if open.id === 'bench' && bench}
      <section class="panel" aria-labelledby="bench-title">
        <h1 id="bench-title">Benchmark</h1>
        {#if bench.progress}
          <p role="status">{bench.progress.step} · {fmt(bench.progress.done * 100)} %</p>
          <div class="bar" aria-hidden="true"><i style:width="{bench.progress.done * 100}%"></i></div>
          {#if can('bench.stop')}
            <div class="actions"><button type="button" class="ghost" disabled={busy} onclick={() => void run('Stopped.', 'bench.stop')}>Stop</button></div>
          {/if}
        {:else if bench.preflight.blockers.length}
          <p class="small">{bench.preflight.blockers[0]}</p>
        {:else if can('bench.start')}
          <div class="actions">
            <button type="button" class="ghost" disabled={busy} onclick={() => void run('Quick benchmark started.', 'bench.start', { mode: 'quick' })}>
              Quick (about {Math.round(bench.preflight.quickSecs / 60)} min)
            </button>
            <button type="button" class="ghost" disabled={busy} onclick={() => void run('Full benchmark started.', 'bench.start', { mode: 'full' })}>
              Full (about {Math.round(bench.preflight.fullSecs / 60)} min)
            </button>
          </div>
        {/if}
        {#if runs.length}
          <ul class="rows runs">
            {#each runs.slice(0, 3) as r (r.id)}
              <li class="small">{fmtDateTime(r.ts)} · {r.mode} · {r.status}{r.errors ? ` · ${r.errors} errors` : ''}</li>
            {/each}
          </ul>
        {/if}
      </section>
    {:else if open.id === 'diag'}
      <section class="panel" aria-labelledby="diag-title">
        <h1 id="diag-title">Diagnostics</h1>
        {#if diags[0]}
          <p class="small">Last run {fmtDateTime(diags[0].ts)}: {diags[0].attention} to look at.</p>
        {:else}
          <p class="muted small">Checks drives, memory, temperatures, drivers and Windows' own reports.</p>
        {/if}
        {#if can('diag.start')}
          <div class="actions"><button type="button" class="ghost" disabled={busy} onclick={() => void run('Diagnostics done.', 'diag.start')}>Run diagnostics</button></div>
        {/if}
      </section>
    {:else if open.id === 'updates' && updates}
      <section class="panel" aria-labelledby="updates-title">
        <h1 id="updates-title">Updates</h1>
        <p>
          Vyro {updates.current} on the PC.{updates.available ? ` ${updates.available.version} is available.` : ''}
          {updates.error ?? ''}
        </p>
        {#if updates.configured}
          <div class="actions">
            {#if can('updates.check')}
              <button type="button" class="ghost" disabled={busy || updates.checking} onclick={() => void run('Checked.', 'updates.check')}>Check now</button>
            {/if}
            {#if updates.available && can('updates.install')}
              <button type="button" disabled={busy || updates.installing} onclick={() => (confirmInstall = true)}>
                Install {updates.available.version}
              </button>
            {/if}
          </div>
        {/if}
      </section>
    {/if}
  </div>
{/if}

<Sheet open={confirming != null} title={confirming ? `${label(confirming)} ${pc}?` : ''} onclose={() => (confirming = null)}>
  {#if confirming}
    {#if delayed(confirming)}
      <label class="check"><input type="checkbox" bind:checked={soon} /> In 30 seconds (it can be cancelled until then)</label>
      <label class="check"><input type="checkbox" bind:checked={force} /> Close apps even with unsaved work</label>
    {/if}
    {#if bench?.progress}<p class="small warn-text">A benchmark is running.</p>{/if}
    {#if profiles?.state.game}<p class="small warn-text">A game is running: {profiles.state.game}.</p>{/if}
    {#if AFTER[confirming]}<p class="muted small">{AFTER[confirming]}</p>{/if}
    <div class="actions sheet-actions">
      <button type="button" disabled={busy} onclick={() => confirming && void doPower(confirming)}>{label(confirming)}</button>
      <button type="button" class="ghost" onclick={() => (confirming = null)}>Cancel</button>
    </div>
  {/if}
</Sheet>

<Sheet open={confirmInstall} title="Install Vyro {updates?.available?.version ?? ''} on {pc}?" onclose={() => (confirmInstall = false)}>
  <p class="muted small">
    Windows asks someone at {pc} to allow it. Vyro closes there as the installer starts, so this phone cannot reach {pc} until
    it is allowed and Vyro has started again.
  </p>
  <div class="actions sheet-actions">
    <button type="button" disabled={busy} onclick={() => void install()}>Install</button>
    <button type="button" class="ghost" onclick={() => (confirmInstall = false)}>Cancel</button>
  </div>
</Sheet>

<style>
  .title {
    margin: 0.25rem 0 0.75rem;
  }
  .section {
    animation: in var(--dur-3) var(--ease-out);
  }
  @keyframes in {
    from {
      opacity: 0;
      transform: translateX(18px);
    }
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.6rem;
    margin: 0 0 0.75rem;
    padding: 0;
    list-style: none;
  }
  .tile {
    display: grid;
    align-content: start;
    gap: 0.2rem;
    width: 100%;
    height: 100%;
    min-height: 6.5rem;
    padding: 0.85rem;
    border: 1px solid var(--line);
    border-radius: var(--r-card);
    background: linear-gradient(180deg, var(--card-top) 0%, var(--card) 40%);
    box-shadow: var(--shadow-card);
    color: var(--text);
    text-align: left;
  }
  .icon {
    display: grid;
    width: 2.25rem;
    height: 2.25rem;
    margin-bottom: 0.35rem;
    place-items: center;
    border-radius: 11px;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }
  .icon.warn {
    background: color-mix(in srgb, var(--warn) 18%, transparent);
    color: var(--warn);
  }
  .name {
    font-weight: 700;
  }
  .status {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.78rem;
    font-weight: 500;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .status.warn {
    color: var(--text);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 0.15rem;
    margin: 0 0 0.6rem -0.4rem;
    padding: 0.2rem 0.6rem 0.2rem 0.3rem;
    border: 0;
    background: transparent;
    color: var(--accent);
    font-weight: 600;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0.5rem;
    margin-top: 0.5rem;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    min-height: 2.75rem;
  }
  .check input {
    width: 1.25rem;
    min-height: 0;
    height: 1.25rem;
    flex: none;
  }
  .sheet-actions button {
    flex: 1;
  }
  .warn-text {
    color: var(--warn);
  }
  .bar {
    height: 6px;
    margin: 0.25rem 0 0.5rem;
    overflow: hidden;
    border-radius: 3px;
    background: var(--gauge-track);
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width var(--dur-3) var(--ease-out);
  }
  .runs {
    margin-top: 0.75rem;
  }
  .runs li {
    padding: 0.4rem 0;
  }
</style>

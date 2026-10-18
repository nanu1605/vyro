<script lang="ts">
  // Everything else Vyro does, from the phone (1.5): shut down, restart, sleep or lock the PC,
  // its profile and fans, a benchmark, diagnostics, updates, and waking another PC. What
  // changes the PC shows only to a phone the owner allowed to control it.
  import { fmt, fmtDateTime } from '../../format';
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
  } from '../../types';
  import { errText, every, type ScreenProps } from './ask';

  interface Props extends ScreenProps {
    /** Other paired PCs that told this phone how to wake them. */
    sleepers: Array<{ name: string; mac: string }>;
  }
  let { ask, can, pc, sleepers }: Props = $props();

  interface UpdateStatus {
    configured: boolean;
    current: string;
    checking: boolean;
    available: { version: string } | null;
    error: string | null;
    installing: boolean;
  }

  let power = $state.raw<PowerState | null>(null);
  let profiles = $state.raw<{ list: ProfileSpec[]; state: Effective } | null>(null);
  let fans = $state.raw<FanStatus | null>(null);
  let bench = $state.raw<{ progress: BenchProgress | null; preflight: Preflight } | null>(null);
  let runs = $state.raw<RunSummary[]>([]);
  let diags = $state.raw<DiagSummary[]>([]);
  let updates = $state.raw<UpdateStatus | null>(null);
  let message = $state<string | null>(null);
  let busy = $state(false);
  /** The power action being confirmed. */
  let confirming = $state<PowerAction | null>(null);
  let soon = $state(true);
  let force = $state(false);

  const quiet = <T,>(p: Promise<T>) => p.catch(() => null);

  function load(): void {
    void quiet(ask<PowerState>('power.state')).then((v) => (power = v));
    void quiet(ask<typeof profiles>('profiles')).then((v) => (profiles = v));
    void quiet(ask<FanStatus>('fans')).then((v) => (fans = v));
    void quiet(ask<typeof bench>('bench.progress')).then((v) => (bench = v));
    void quiet(ask<RunSummary[]>('bench.runs')).then((v) => (runs = v ?? []));
    void quiet(ask<DiagSummary[]>('diag.runs')).then((v) => (diags = v ?? []));
    void quiet(ask<UpdateStatus>('updates')).then((v) => (updates = v));
  }
  $effect(() => every(5000, load));

  async function run(what: string, method: string, params: object = {}): Promise<void> {
    busy = true;
    message = null;
    try {
      const r = await ask<unknown>(method, params);
      message = what;
      if (method === 'diag.start') {
        const d = r as DiagReport;
        message = `Diagnostics done: ${d.checks.filter((c) => c.status === 'attention').length} to look at.`;
      }
      load();
    } catch (e) {
      message = errText(e);
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

  async function doPower(a: PowerAction): Promise<void> {
    confirming = null;
    await run(`${label(a)}: asked.`, 'power', { action: a, wait: delayed(a) ? (soon ? 30 : 0) : 0, force });
  }
</script>

<h1 class="title">More on {pc}</h1>
{#if message}<p class="note" role="status">{message}</p>{/if}

{#if can('power')}
  <section class="panel" aria-labelledby="power-title">
    <h2 id="power-title">Power</h2>
    {#if power?.pending}
      <p class="note warn" role="status">
        {label(power.pending.action)} at {fmtDateTime(power.pending.at)}, asked from {power.pending.by}.
      </p>
      <div class="actions">
        <button type="button" disabled={busy} onclick={() => void run('Cancelled.', 'power.cancel')}>Cancel it</button>
      </div>
    {:else if confirming}
      <div class="sheet" role="group" aria-labelledby="confirm-title">
        <p id="confirm-title"><b>{label(confirming)} {pc}?</b></p>
        {#if delayed(confirming)}
          <label><input type="checkbox" bind:checked={soon} /> In 30 seconds (it can be cancelled until then)</label>
          <label><input type="checkbox" bind:checked={force} /> Close apps even with unsaved work</label>
        {/if}
        {#if bench?.progress}<p class="small warn-text">A benchmark is running.</p>{/if}
        {#if profiles?.state.game}<p class="small warn-text">A game is running: {profiles.state.game}.</p>{/if}
        {#if AFTER[confirming]}<p class="muted small">{AFTER[confirming]}</p>{/if}
        <div class="actions">
          <button type="button" disabled={busy} onclick={() => confirming && void doPower(confirming)}>{label(confirming)}</button>
          <button type="button" class="ghost" onclick={() => (confirming = null)}>Cancel</button>
        </div>
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
{/if}

{#if sleepers.length && can('wake')}
  <section class="panel" aria-labelledby="wake-title">
    <h2 id="wake-title">Wake another PC</h2>
    <p class="muted small">{pc} sends the wake signal on its network. It works when the other PC is wired to the same network and set up for it (Settings → Remote on that PC).</p>
    <div class="actions">
      {#each sleepers as s (s.mac)}
        <button type="button" class="ghost" disabled={busy} onclick={() => void run(`Wake signal sent to ${s.name}.`, 'wake', { mac: s.mac, pc: s.name })}>
          Wake {s.name}
        </button>
      {/each}
    </div>
  </section>
{/if}

{#if profiles}
  <section class="panel" aria-labelledby="profile-title">
    <h2 id="profile-title">Profile</h2>
    <p>
      {profiles.list.find((p) => p.id === profiles?.state.profile)?.name ?? profiles.state.profile} in force{profiles.state.game
        ? ` (for ${profiles.state.game})`
        : ''}.
    </p>
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
{/if}

{#if fans}
  <section class="panel" aria-labelledby="fans-title">
    <h2 id="fans-title">Fans</h2>
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
{/if}

{#if bench}
  <section class="panel" aria-labelledby="bench-title">
    <h2 id="bench-title">Benchmark</h2>
    {#if bench.progress}
      <p role="status">{bench.progress.step} · {fmt(bench.progress.done * 100)} %</p>
      {#if can('bench.stop')}
        <div class="actions"><button type="button" class="ghost" disabled={busy} onclick={() => void run('Stopped.', 'bench.stop')}>Stop</button></div>
      {/if}
    {:else}
      {#if bench.preflight.blockers.length}
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
    {/if}
    {#each runs.slice(0, 3) as r (r.id)}
      <p class="small">{fmtDateTime(r.ts)} · {r.mode} · {r.status}{r.errors ? ` · ${r.errors} errors` : ''}</p>
    {/each}
  </section>
{/if}

<section class="panel" aria-labelledby="diag-title">
  <h2 id="diag-title">Diagnostics</h2>
  {#if diags[0]}
    <p class="small">Last run {fmtDateTime(diags[0].ts)}: {diags[0].attention} to look at.</p>
  {/if}
  {#if can('diag.start')}
    <div class="actions"><button type="button" class="ghost" disabled={busy} onclick={() => void run('Diagnostics done.', 'diag.start')}>Run diagnostics</button></div>
  {/if}
</section>

{#if updates}
  <section class="panel" aria-labelledby="updates-title">
    <h2 id="updates-title">Updates</h2>
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
          <button type="button" disabled={busy || updates.installing} onclick={() => void run('Installing: Vyro restarts on the PC.', 'updates.install')}>
            Install {updates.available.version}
          </button>
        {/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .title {
    margin-bottom: 0.75rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
    gap: 0.5rem;
    margin-top: 0.5rem;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }
  .actions button,
  .grid button {
    padding: 0.5rem 0.8rem;
  }
  button[aria-pressed='true'] {
    border-color: var(--accent);
    font-weight: 700;
  }
  .sheet {
    display: grid;
    gap: 0.5rem;
    padding: 0.75rem;
    border: 1px solid var(--line);
    border-radius: 10px;
  }
  .sheet p {
    margin: 0;
  }
  .sheet label {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .sheet input {
    width: auto;
  }
  .warn-text {
    color: var(--warn);
  }
  section h2 {
    margin-bottom: 0.4rem;
  }
</style>

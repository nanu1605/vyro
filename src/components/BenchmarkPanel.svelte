<script lang="ts">
  import {
    deleteBenchmark,
    getBenchmarkPreflight,
    getBenchmarkProgress,
    getBenchmarkRun,
    getBenchmarkRuns,
    getHardwareChanges,
    onBenchDone,
    onBenchProgress,
    startBenchmark,
    stopBenchmark,
  } from '../ipc';
  import {
    AREAS,
    MODES,
    baselineFor,
    compare,
    fmtChange,
    fmtDuration,
    fmtMetric,
    fmtSpread,
    fmtValue,
    headline,
    setupChanges,
    setupRows,
    stabilityLabel,
    summaryText,
    verdictText,
  } from '../bench';
  import { describe } from '../changes';
  import { scrollable } from '../scrollable';
  import { fmtTemp, tempText } from '../prefs.svelte';
  import type { BenchMode, BenchProgress, BenchRun, HardwareChange, Preflight, RunSummary, SustainLoop, Watch } from '../types';

  interface Props {
    active: boolean;
  }
  let { active }: Props = $props();

  let mode = $state<BenchMode>('quick');
  let pre = $state.raw<Preflight | null>(null);
  let progress = $state.raw<BenchProgress | null>(null);
  let runs = $state.raw<RunSummary[]>([]);
  let selected = $state<number | null>(null);
  let run = $state.raw<BenchRun | null>(null);
  /** "auto" (the newest earlier completed run) or a run id. */
  let baseChoice = $state('auto');
  let base = $state.raw<BenchRun | null>(null);
  let changes = $state.raw<HardwareChange[]>([]);
  let error = $state<string | null>(null);
  let starting = $state(false);
  let confirmDelete = $state<number | null>(null);

  async function loadRuns(pick?: number): Promise<void> {
    try {
      runs = await getBenchmarkRuns();
      if (pick != null) selected = pick;
      else if (selected == null || !runs.some((r) => r.id === selected)) selected = runs[0]?.id ?? null;
    } catch (e) {
      error = String(e);
    }
  }

  function refresh(): void {
    getBenchmarkPreflight()
      .then((p) => (pre = p))
      .catch((e: unknown) => (error = String(e)));
  }

  $effect(() => {
    if (!active) return;
    refresh();
    void loadRuns();
    void getBenchmarkProgress().then((p) => p && (progress = p));
    // The start checks follow the PC (a game starting, temperatures) while nothing runs.
    const t = setInterval(() => {
      if (!progress) refresh();
    }, 5000);
    const offs = [
      onBenchProgress((p) => (progress = p)),
      onBenchDone((r) => {
        progress = null;
        baseChoice = 'auto';
        void loadRuns(r.id || undefined);
        refresh();
      }),
    ];
    return () => {
      clearInterval(t);
      for (const o of offs) void o.then((off) => off());
    };
  });

  $effect(() => {
    const id = selected;
    if (id == null) {
      run = null;
      return;
    }
    let gone = false;
    void getBenchmarkRun(id).then((r) => {
      if (!gone) run = r;
    });
    return () => {
      gone = true;
    };
  });

  const autoBase = $derived(run ? baselineFor(run, runs) : null);
  const baseSummary = $derived(baseChoice === 'auto' ? autoBase : (runs.find((r) => String(r.id) === baseChoice) ?? null));
  // Sustained compares with the last Full run when the baseline was a Quick one.
  const sustainBase = $derived(
    run && baseChoice === 'auto' && baseSummary && baseSummary.mode !== 'full' ? baselineFor(run, runs, 'full') : baseSummary,
  );
  $effect(() => {
    const id = baseSummary?.id;
    if (id == null) {
      base = null;
      return;
    }
    let gone = false;
    void getBenchmarkRun(id).then((r) => {
      if (!gone) base = r;
    });
    return () => {
      gone = true;
    };
  });
  // Hardware changes logged between the two runs (P19).
  $effect(() => {
    const from = base?.ts;
    const to = run?.ts;
    if (from == null || to == null) {
      changes = [];
      return;
    }
    let gone = false;
    void getHardwareChanges(200)
      .then((all) => {
        if (!gone) changes = all.filter((c) => c.ts > Math.min(from, to) && c.ts <= Math.max(from, to) && c.kind !== 'tracking');
      })
      .catch(() => (changes = []));
    return () => {
      gone = true;
    };
  });

  const rows = $derived(run && base ? compare(run, base, sustainBase) : []);
  const setupDiff = $derived(run && base ? setupChanges(run.setup, base.setup) : []);
  const errors = $derived(run ? run.errors.calc + run.errors.storage + (run.errors.hardware ?? 0) + run.errors.gpuResets : 0);
  const others = $derived(runs.filter((r) => r.id !== run?.id));

  async function start(): Promise<void> {
    starting = true;
    error = null;
    try {
      await startBenchmark(mode);
      progress = (await getBenchmarkProgress()) ?? {
        mode,
        area: null,
        step: 'Settling',
        done: 0,
        elapsed: 0,
        planned: mode === 'quick' ? (pre?.quickSecs ?? 0) : (pre?.fullSecs ?? 0),
        finished: [],
      };
    } catch (e) {
      error = String(e);
      refresh();
    } finally {
      starting = false;
    }
  }

  async function remove(id: number): Promise<void> {
    if (confirmDelete !== id) {
      confirmDelete = id;
      return;
    }
    confirmDelete = null;
    try {
      await deleteBenchmark(id);
      if (selected === id) selected = null;
      if (baseChoice === String(id)) baseChoice = 'auto';
      await loadRuns();
    } catch (e) {
      error = String(e);
    }
  }

  function when(ts: number): string {
    const d = new Date(ts * 1000);
    const today = new Date().toDateString() === d.toDateString();
    return d.toLocaleString(
      undefined,
      today ? { hour: '2-digit', minute: '2-digit' } : { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' },
    );
  }

  const W = (v: number | null, unit: string) => (v == null ? null : `${fmtValue(v, unit)} ${unit}`);

  /** "CPU 81 °C · 92 W peak · 5,180 MHz · GPU 66 °C · 186 W · throttled 0 s". */
  function watchText(w: Watch): string {
    const cpu = [fmtTemp(w.cpuTempMax), W(w.cpuPowerMax, 'W'), W(w.cpuMhzAvg, 'MHz')].filter(Boolean);
    const gpu = [fmtTemp(w.gpuTempMax), W(w.gpuPowerMax, 'W'), W(w.gpuMhzAvg, 'MHz')].filter(Boolean);
    const parts = [];
    if (cpu.length) parts.push(`CPU ${cpu.join(', ')}`);
    if (gpu.length) parts.push(`GPU ${gpu.join(', ')}`);
    if (w.throttledSecs != null) parts.push(`throttled ${fmtDuration(w.throttledSecs)}`);
    if (w.fullSpeedSecs > 0) parts.push(`fans at full speed ${fmtDuration(w.fullSpeedSecs)}`);
    return parts.join(' · ');
  }

  function loopBars(loops: SustainLoop[], pick: (l: SustainLoop) => number | null): number[] {
    const v = loops.map(pick).filter((x): x is number => x != null);
    const hi = Math.max(...v, 0);
    return hi > 0 ? v.map((x) => x / hi) : [];
  }

  const MODE_IDS: BenchMode[] = ['quick', 'full'];
  const rowFor = (key: string) => rows.find((r) => r.key === key);
  const statusOf = (r: RunSummary) => (r.status === 'completed' ? (r.busy ? 'Busy PC' : 'Completed') : 'Stopped');
</script>

<section class="bench" aria-labelledby="bench-title">
  <header class="intro">
    <h2 id="bench-title">Benchmark</h2>
    <p class="fine">
      Runs Vyro's own workloads and compares the result with this PC's earlier runs. Each area runs three passes; a
      difference counts only beyond their spread. The numbers are not comparable with other tools or other PCs.
    </p>
  </header>

  {#if progress}
    <div class="card running" role="region" aria-labelledby="run-title">
      <div class="row">
        <div>
          <h3 id="run-title">
            {MODES[progress.mode].label} run · {progress.area ? AREAS[progress.area].label : progress.step || 'Finishing'}
          </h3>
          <p class="fine" aria-live="polite">
            {progress.step || 'Working out the results'} · {fmtDuration(progress.elapsed)} of about {fmtDuration(progress.planned)}
          </p>
        </div>
        <button type="button" class="danger" onclick={() => void stopBenchmark()}>Stop</button>
      </div>
      <div
        class="bar"
        role="progressbar"
        aria-label="Benchmark progress"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(progress.done * 100)}
      >
        <span style="width: {progress.done * 100}%"></span>
      </div>
      <p class="fine">
        The CPU and GPU run at full load. Closing the Vyro window or quitting stops the run; so does any temperature
        reaching Health's problem limit.
      </p>
      {#if progress.finished.length}
        <ul class="done">
          {#each progress.finished as a (a.area)}
            <li>
              <b>{AREAS[a.area].label}</b>
              <span>{a.missing ? tempText(a.missing) : a.metrics.filter((m) => m.better !== 'shown').map(fmtMetric).join(' · ')}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <div class="card start">
      <fieldset class="modes">
        <legend>Mode</legend>
        {#each MODE_IDS as m (m)}
          <label class="mode" class:on={mode === m}>
            <input type="radio" name="bench-mode" value={m} bind:group={mode} />
            <span class="what">
              <b>{MODES[m].label}</b>
              {#if pre}<span class="time">about {fmtDuration(m === 'quick' ? pre.quickSecs : pre.fullSecs)}</span>{/if}
              <small>{MODES[m].what}</small>
            </span>
          </label>
        {/each}
      </fieldset>
      <p class="fine">
        Uses {pre?.gpu ?? 'no Direct3D 11 GPU'} and {pre?.drive ?? 'the drive with the temporary folder'} (about 3 GiB
        written). Loads the CPU and GPU fully; the Benchmark profile is in force meanwhile.
      </p>
      {#each pre?.blockers ?? [] as b (b)}<p class="blocker" role="alert">{tempText(b)}</p>{/each}
      {#each pre?.notes ?? [] as n (n)}<p class="note">{n}</p>{/each}
      <div class="row actions">
        <button type="button" class="primary" disabled={!pre || pre.blockers.length > 0 || starting} onclick={start}>
          Start {MODES[mode].label.toLowerCase()} run
        </button>
      </div>
    </div>
  {/if}
  {#if error}<p class="blocker" role="alert">{error}</p>{/if}

  {#if run}
    <section class="card result" aria-labelledby="result-title">
      <div class="row">
        <div>
          <h3 id="result-title">{MODES[run.mode].label} run · {when(run.ts)}</h3>
          <p class="fine">
            {run.status === 'completed' ? 'Completed' : 'Stopped'} after {fmtDuration(run.secs)}{#if run.reason}: {tempText(
                run.reason,
              )}{/if} · method {run.method}
          </p>
        </div>
        <div class="chips">
          {#if run.busy}<span class="chip warn">Busy PC</span>{/if}
          {#if errors}<span class="chip fail">{errors} error{errors === 1 ? '' : 's'}</span>{/if}
        </div>
      </div>

      <div class="compare">
        <label>
          <span>Compared with</span>
          <select bind:value={baseChoice}>
            <option value="auto">{autoBase ? `The last completed run (${when(autoBase.ts)})` : 'No earlier completed run'}</option>
            {#each others as r (r.id)}
              <option value={String(r.id)}>{when(r.ts)} · {MODES[r.mode].label}{r.status === 'stopped' ? ' (stopped)' : ''}</option>
            {/each}
          </select>
        </label>
        <p class="summary">
          {#if base}{summaryText(rows)}{:else if baseSummary}…{:else}The first run here: later runs compare with it.{/if}
        </p>
      </div>
      {#if setupDiff.length || changes.length}
        <div class="between">
          <h4>What changed in between</h4>
          <ul>
            {#each setupDiff as c (c.label)}<li>{c.label}: {c.from} → {c.to}</li>{/each}
            {#each changes as c (c.id)}
              {@const d = describe(c)}
              <li>{d.title}{d.device ? `: ${d.device}` : ''}{d.from && d.to ? ` (${d.from} → ${d.to})` : ''}</li>
            {/each}
          </ul>
        </div>
      {/if}
      {#each run.notes as n (n)}<p class="note">{tempText(n)}</p>{/each}

      <div class="areas">
        {#each run.areas as a (a.area)}
          <article class="area" class:missing={a.missing != null}>
            <header>
              <h4>{AREAS[a.area].label}</h4>
              <small>{a.detail ?? AREAS[a.area].hint}</small>
            </header>
            {#if a.missing}
              <p class="fine">{tempText(a.missing)}</p>
            {:else}
              <div class="table-wrap" use:scrollable role="region" aria-label="{AREAS[a.area].label} results">
              <table>
                <tbody>
                  {#each a.metrics as m (m.key)}
                    {@const row = rowFor(m.key)}
                    <tr>
                      <th scope="row">{m.label}</th>
                      <td class="num">
                        {fmtMetric(m)}
                        {#if m.key === 'sustain.stability'}<span class="tag">{stabilityLabel(m.value)}</span>{/if}
                      </td>
                      <td class="spread">{fmtSpread(m.spread) ?? ''}</td>
                      <td class="cmp">
                        {#if row}<span class="verdict {row.verdict}">{verdictText(row)} {fmtChange(row.change)}</span>{/if}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
              </div>
              {#if a.area === 'sustained' && a.loops.length}
                {@const cpu = loopBars(a.loops, (l) => l.cpu)}
                {@const gpu = loopBars(a.loops, (l) => l.gpu)}
                <div class="loops">
                  {#each [{ name: 'CPU', bars: cpu }, { name: 'GPU', bars: gpu }] as s (s.name)}
                    {#if s.bars.length}
                      <div class="loop-row">
                        <span>{s.name} per loop</span>
                        <svg
                          viewBox="0 0 100 24"
                          preserveAspectRatio="none"
                          role="img"
                          aria-label="{s.name} work in each of {s.bars.length} loops, from {Math.round(
                            Math.min(...s.bars) * 100,
                          )}% to 100% of the best"
                        >
                          {#each s.bars as h, i (i)}
                            <rect x={i * (100 / s.bars.length) + 1} width={100 / s.bars.length - 2} y={24 - h * 24} height={h * 24} />
                          {/each}
                        </svg>
                      </div>
                    {/if}
                  {/each}
                </div>
              {/if}
            {/if}
            {#if a.watch}<p class="watch">{watchText(a.watch)}</p>{/if}
          </article>
        {/each}
      </div>

      <details>
        <summary>Setup</summary>
        <dl class="setup">
          {#each setupRows(run.setup) as r (r.label)}<dt>{r.label}</dt><dd>{r.value}</dd>{/each}
        </dl>
      </details>
    </section>
  {/if}

  <section class="card list" aria-labelledby="runs-title">
    <h3 id="runs-title">Runs on this PC</h3>
    {#if !runs.length}
      <p class="fine">No runs yet. The first one becomes the baseline later runs compare with.</p>
    {:else}
      <div class="table-wrap" use:scrollable role="region" aria-label="Runs, table">
      <table class="runs">
        <thead>
          <tr>
            <th scope="col">When</th>
            <th scope="col" class="wide">Mode</th>
            <th scope="col" class="num">CPU multi</th>
            <th scope="col" class="num wide">Graphics</th>
            <th scope="col">Result</th>
            <th scope="col"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody>
          {#each runs as r (r.id)}
            {@const cpu = headline(r, 'cpu.multi')}
            {@const fps = headline(r, 'gfx.fps')}
            <tr class:sel={r.id === selected}>
              <td>
                <button type="button" class="link" aria-pressed={r.id === selected} onclick={() => (selected = r.id)}>{when(r.ts)}</button>
              </td>
              <td class="wide">{MODES[r.mode].label}</td>
              <td class="num">{cpu == null ? '—' : fmtValue(cpu, 'pts')}</td>
              <td class="num wide">{fps == null ? '—' : `${fmtValue(fps, 'fps')} fps`}</td>
              <td>
                <span class="state {r.status} " class:busy={r.busy}>{statusOf(r)}</span>{#if r.errors}<span class="chip fail small"
                    >{r.errors} err</span
                  >{/if}
              </td>
              <td class="act">
                <button
                  type="button"
                  class="ghost"
                  class:danger={confirmDelete === r.id}
                  aria-label={confirmDelete === r.id ? `Confirm deleting the run of ${when(r.ts)}` : `Delete the run of ${when(r.ts)}`}
                  onclick={() => remove(r.id)}
                  onblur={() => confirmDelete === r.id && (confirmDelete = null)}>{confirmDelete === r.id ? 'Confirm' : 'Delete'}</button
                >
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      </div>
    {/if}
  </section>
</section>

<style>
  .table-wrap {
    max-width: 100%;
    overflow-x: auto;
  }
  .bench {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  h3 {
    margin: 0;
    font-size: 0.95rem;
  }
  h4 {
    margin: 0;
    font-size: 0.85rem;
  }
  .fine {
    margin: 0.3rem 0 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .card {
    padding: 0.85rem 1rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }
  .row > div {
    min-width: 0;
  }
  .actions {
    margin-top: 0.7rem;
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
  button.ghost {
    padding: 0.15rem 0.45rem;
    font-size: 0.74rem;
  }
  button.link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .modes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 14rem), 1fr));
    gap: 0.6rem;
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: 0.4rem;
    color: var(--muted);
    font-size: 0.74rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .mode {
    display: flex;
    gap: 0.55rem;
    align-items: flex-start;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    cursor: pointer;
  }
  .mode.on {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .mode input {
    margin-top: 0.2rem;
  }
  .what {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.5rem;
    align-items: baseline;
    min-width: 0;
  }
  .what small {
    flex-basis: 100%;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .time {
    color: var(--muted);
    font-size: 0.78rem;
  }
  .blocker {
    margin: 0.5rem 0 0;
    padding: 0.4rem 0.6rem;
    border-left: 3px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 9%, transparent);
    font-size: 0.8rem;
  }
  .note {
    margin: 0.5rem 0 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .running {
    border-left: 3px solid var(--accent);
  }
  .bar {
    height: 0.5rem;
    margin-top: 0.7rem;
    border-radius: 999px;
    background: var(--gauge-track);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
    transition: width 0.25s linear;
  }
  .done {
    display: grid;
    gap: 0.2rem;
    margin: 0.6rem 0 0;
    padding: 0;
    list-style: none;
    font-size: 0.8rem;
  }
  .done li {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.6rem;
  }
  .done span {
    color: var(--muted);
    font-family: var(--font-num);
  }
  .chips {
    display: flex;
    gap: 0.4rem;
  }
  .chip {
    padding: 0.05rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    font-size: 0.74rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .chip.warn {
    border-color: var(--warn);
  }
  .chip.fail {
    border-color: var(--hot);
  }
  .chip.small {
    margin-left: 0.35rem;
    padding: 0 0.35rem;
    font-size: 0.68rem;
  }
  .compare {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
    margin-top: 0.7rem;
  }
  .compare label {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    align-items: center;
    min-width: 0;
    font-size: 0.8rem;
  }
  select {
    max-width: 100%;
    padding: 0.25rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.8rem;
  }
  .summary {
    margin: 0;
    font-size: 0.85rem;
    font-weight: 600;
  }
  .between {
    margin-top: 0.6rem;
    font-size: 0.8rem;
  }
  .between ul {
    margin: 0.3rem 0 0;
    padding-left: 1.1rem;
  }
  .areas {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 22rem), 1fr));
    gap: 0.7rem;
    margin-top: 0.8rem;
  }
  .area {
    min-width: 0;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .area header {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.6rem;
    align-items: baseline;
    margin-bottom: 0.35rem;
  }
  .area header small {
    min-width: 0;
    color: var(--muted);
    font-size: 0.72rem;
    overflow-wrap: anywhere;
  }
  .area.missing h4 {
    color: var(--muted);
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.8rem;
  }
  th,
  td {
    padding: 0.2rem 0.3rem 0.2rem 0;
    text-align: left;
    vertical-align: baseline;
  }
  .area th {
    color: var(--muted);
    font-weight: 400;
  }
  .num {
    font-family: var(--font-num);
    text-align: right;
    white-space: nowrap;
  }
  .spread {
    color: var(--muted);
    font-size: 0.72rem;
    white-space: nowrap;
  }
  .cmp {
    text-align: right;
  }
  .verdict {
    font-size: 0.72rem;
    white-space: nowrap;
  }
  .verdict.same {
    color: var(--muted);
  }
  .verdict.faster {
    color: var(--ok);
  }
  .verdict.slower {
    color: var(--warn);
  }
  .tag {
    margin-left: 0.35rem;
    color: var(--muted);
    font-family: var(--font-ui);
    font-size: 0.72rem;
  }
  .watch {
    margin: 0.4rem 0 0;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .loops {
    display: grid;
    gap: 0.3rem;
    margin-top: 0.4rem;
  }
  .loop-row {
    display: grid;
    grid-template-columns: 6.5rem 1fr;
    gap: 0.5rem;
    align-items: center;
    font-size: 0.72rem;
    color: var(--muted);
  }
  .loop-row svg {
    width: 100%;
    height: 1.5rem;
  }
  .loop-row rect {
    fill: var(--accent);
    opacity: 0.8;
  }
  details {
    margin-top: 0.8rem;
  }
  summary {
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
  }
  .setup {
    display: grid;
    grid-template-columns: minmax(8rem, max-content) 1fr;
    gap: 0.2rem 1rem;
    margin: 0.5rem 0 0;
    font-size: 0.78rem;
  }
  .setup dt {
    color: var(--muted);
  }
  .setup dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .runs {
    margin-top: 0.5rem;
  }
  .runs th + th,
  .runs td + td {
    padding-left: 0.8rem;
  }
  .runs thead th {
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    border-bottom: 1px solid var(--line);
  }
  .runs td {
    border-bottom: 1px solid color-mix(in srgb, var(--line) 50%, transparent);
  }
  .runs tr.sel td {
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }
  .act {
    text-align: right;
  }
  .state.stopped,
  .state.busy {
    color: var(--warn);
  }
  @media (max-width: 560px) {
    .wide {
      display: none;
    }
    .setup {
      grid-template-columns: 1fr;
    }
    .setup dd {
      margin-bottom: 0.3rem;
    }
  }
</style>

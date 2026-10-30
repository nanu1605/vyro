<script lang="ts">
  import { deleteDiagnostic, exportDiagnostics, getDiagnosticRun, getDiagnosticRuns, revealReport, runDiagnostics } from '../ipc';
  import { CHECKS, FORMATS, STATUS, count, fmtCitation, ordered, sourceText, stamp, type ExportFormat } from '../diag';
  import { prefs, tempText } from '../prefs.svelte';
  import type { DiagReport, DiagSummary } from '../types';

  interface Props {
    active: boolean;
    /** Opens a sensor in History. */
    onhistory?: (id: string, at: number | null) => void;
  }
  let { active, onhistory }: Props = $props();

  let runs = $state.raw<DiagSummary[]>([]);
  let report = $state.raw<DiagReport | null>(null);
  let running = $state(false);
  let error = $state<string | null>(null);
  let serials = $state(false);
  let exporting = $state<ExportFormat | null>(null);
  let saved = $state<string | null>(null);
  let loaded = false;

  async function load(pick?: number): Promise<void> {
    try {
      runs = await getDiagnosticRuns();
      const id = pick ?? report?.id ?? runs[0]?.id;
      report = id != null ? await getDiagnosticRun(id) : null;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    if (!active || loaded) return;
    loaded = true;
    void load();
  });

  async function run(): Promise<void> {
    running = true;
    error = null;
    saved = null;
    try {
      const r = await runDiagnostics();
      report = r;
      runs = await getDiagnosticRuns();
    } catch (e) {
      error = String(e);
    } finally {
      running = false;
    }
  }

  async function pick(id: string): Promise<void> {
    saved = null;
    report = await getDiagnosticRun(Number(id));
  }

  async function remove(): Promise<void> {
    if (!report) return;
    const id = report.id;
    report = null;
    saved = null;
    await deleteDiagnostic(id);
    await load(runs.find((r) => r.id !== id)?.id);
  }

  async function exportAs(format: ExportFormat): Promise<void> {
    if (!report) return;
    exporting = format;
    error = null;
    try {
      saved = await exportDiagnostics(report.id, format, serials, prefs.temperatureUnit, when(report.ts), stamp(report.ts));
    } catch (e) {
      error = String(e);
    } finally {
      exporting = null;
    }
  }

  function when(ts: number): string {
    return new Date(ts * 1000).toLocaleString(undefined, {
      day: 'numeric',
      month: 'short',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  const unit = $derived(prefs.temperatureUnit);
  const attention = $derived(report ? count(report, 'attention') : 0);
</script>

<section class="diag" id="diagnostics" aria-labelledby="diag-title">
  <div class="head">
    <div>
      <h2 id="diag-title">Diagnostics</h2>
      <p class="fine">
        A check-up from what Vyro already records: the past week, the Health log, drive health and the event log. No load,
        about a second.
      </p>
    </div>
    <button type="button" class="primary" disabled={running} onclick={run}>{running ? 'Running…' : 'Run diagnostics'}</button>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}

  {#if report}
    <div class="meta">
      <p class="counts" aria-live="polite">
        <b class:warn={attention > 0}>{attention} attention</b> · {count(report, 'pass')} pass · {count(report, 'info')} info · {count(
          report,
          'unavailable',
        )} unavailable
      </p>
      <label class="which">
        <span class="sr-only">Report</span>
        <select value={String(report.id)} onchange={(e) => pick(e.currentTarget.value)}>
          {#each runs as r (r.id)}
            <option value={String(r.id)}>{when(r.ts)}{r.attention ? ` · ${r.attention} attention` : ''}</option>
          {/each}
        </select>
      </label>
    </div>

    <ul class="checks">
      {#each ordered(report) as c (c.id)}
        {@const st = STATUS[c.status]}
        <li class="check tone-{st.tone}">
          <details open={c.status === 'attention'}>
            <summary>
              <span class="pill">{st.label}</span>
              <b>{CHECKS[c.id]}</b>
              <span class="sum">{tempText(c.summary)}</span>
            </summary>
            {#if c.advice}
              <p class="advice"><b>{c.status === 'unavailable' ? 'Why' : 'What to do'}:</b> {tempText(c.advice)}</p>
            {/if}
            {#if c.citations.length}
              <table>
                <thead><tr><th scope="col">Reading</th><th scope="col">Value</th><th scope="col">From</th></tr></thead>
                <tbody>
                  {#each c.citations as x, i (i)}
                    <tr>
                      <td>
                        {x.label}
                        {#if x.metric && onhistory}
                          <button
                            type="button"
                            class="link"
                            onclick={() => onhistory(x.metric!, x.ago != null ? report!.ts - x.ago : null)}
                            aria-label="Open {x.label} in History">History</button
                          >
                        {/if}
                      </td>
                      <td class="v">{fmtCitation(x, unit)}</td>
                      <td class="src">{sourceText(x)}</td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            {/if}
          </details>
        </li>
      {/each}
    </ul>

    <div class="export">
      <span class="label" id="export-label">Export</span>
      <div class="buttons" role="group" aria-labelledby="export-label">
        {#each FORMATS as f (f.id)}
          <button type="button" disabled={exporting != null} onclick={() => exportAs(f.id)}>
            {exporting === f.id ? 'Saving…' : f.label}
          </button>
        {/each}
      </div>
      <label class="serials"><input type="checkbox" bind:checked={serials} /> Include serial numbers</label>
      <button type="button" class="ghost" onclick={remove}>Delete this report</button>
    </div>
    {#if saved}
      <p class="saved" role="status">
        Saved to <code>{saved}</code>
        <button type="button" class="link" onclick={() => revealReport(saved!)}>Show in folder</button>
      </p>
    {/if}
    <p class="fine">
      {report.engine} · Vyro {report.appVersion} · checks v{report.version}. Serial numbers are never kept with a report;
      an export adds them only when ticked.
    </p>
  {:else if !running}
    <p class="fine">No report yet.</p>
  {/if}
</section>

<style>
  .diag {
    margin-top: 1.2rem;
    padding: 0.85rem 1rem;
    /* Ctrl K's Diagnostics lands here, below the sticky header. */
    scroll-margin-top: calc(var(--topbar-h) + 1rem);
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }
  .head > div {
    min-width: 0;
    flex: 1 1 18rem;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
  }
  .fine {
    margin: 0.3rem 0 0;
    color: var(--muted);
    font-size: 0.78rem;
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
  button.ghost {
    margin-left: auto;
    font-size: 0.74rem;
  }
  button.link {
    margin-left: 0.4rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: 0.72rem;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .error {
    margin: 0.5rem 0 0;
    padding: 0.4rem 0.6rem;
    border-left: 3px solid var(--hot);
    font-size: 0.8rem;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem 1rem;
    margin-top: 0.7rem;
  }
  .counts {
    margin: 0;
    font-size: 0.85rem;
  }
  .counts b.warn {
    color: var(--warn);
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
  .checks {
    display: grid;
    gap: 0.35rem;
    margin: 0.6rem 0 0;
    padding: 0;
    list-style: none;
  }
  .check {
    border: 1px solid var(--line);
    border-left: 3px solid var(--tone);
    border-radius: 7px;
  }
  .tone-ok {
    --tone: var(--ok);
  }
  .tone-warn {
    --tone: var(--warn);
  }
  .tone-info {
    --tone: var(--accent);
  }
  .tone-muted {
    --tone: var(--muted);
  }
  summary {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.2rem 0.6rem;
    padding: 0.45rem 0.6rem;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .pill {
    padding: 0 0.45rem;
    border: 1px solid var(--tone);
    border-radius: 999px;
    font-size: 0.7rem;
    font-weight: 700;
  }
  .sum {
    flex: 1 1 16rem;
    min-width: 0;
    color: var(--muted);
  }
  .advice {
    margin: 0 0.6rem 0.4rem;
    font-size: 0.8rem;
  }
  table {
    width: calc(100% - 1.2rem);
    margin: 0 0.6rem 0.5rem;
    border-collapse: collapse;
    font-size: 0.78rem;
  }
  th,
  td {
    padding: 0.18rem 0.5rem 0.18rem 0;
    text-align: left;
    vertical-align: baseline;
    border-top: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
  }
  th {
    color: var(--muted);
    font-weight: 600;
    font-size: 0.72rem;
  }
  td {
    overflow-wrap: anywhere;
  }
  .v {
    font-family: var(--font-num);
    font-weight: 600;
  }
  .src {
    color: var(--muted);
  }
  .export {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.8rem;
    margin-top: 0.8rem;
    font-size: 0.8rem;
  }
  .label {
    color: var(--muted);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .serials {
    display: inline-flex;
    gap: 0.35rem;
    align-items: center;
  }
  .saved {
    margin: 0.5rem 0 0;
    font-size: 0.78rem;
    overflow-wrap: anywhere;
  }
  code {
    font-size: 0.74rem;
  }
</style>

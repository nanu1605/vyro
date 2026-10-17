<script lang="ts">
  // A PC's readings as the phone pages show them (P26, 1.4): alerts and Health first, then a
  // tile per part, then the board, fans and drives, then recent alerts. Shared by the local
  // companion (remote.html) and the phone app (phone.html).
  import { fmt, fmtDateTime } from '../format';
  import type { RemoteView, TempUnit } from '../types';
  import { driveRows, fanRows, notices, tempRows, tiles, type Level, type Row } from './companion';

  interface Props {
    view: RemoteView;
    unit: TempUnit;
    /** The PC's name. */
    pc: string;
    /** Milliseconds, for "2 min ago". */
    now: number;
  }
  let { view, unit, pc, now }: Props = $props();

  const notes = $derived(notices(view));
  const cards = $derived(tiles(view, unit));
  const lists = $derived(
    [
      { id: 'temps', title: 'Temperatures', rows: tempRows(view, unit) },
      { id: 'fans', title: 'Fans', rows: fanRows(view) },
      { id: 'drives', title: 'Drives', rows: driveRows(view, unit) },
    ].filter((l) => l.rows.length),
  );
  const WORD: Record<Exclude<Level, null>, string> = { ok: 'OK', warn: 'Warning', hot: 'Problem' };

  const ago = (ts: number) => {
    const s = Math.max(0, Math.round(now / 1000 - ts));
    return s < 60 ? `${s} s ago` : s < 3600 ? `${Math.floor(s / 60)} min ago` : fmtDateTime(ts);
  };
  const rowLevel = (r: Row) => (r.level === 'warn' || r.level === 'hot' ? r.level : null);
</script>

  <h1 class="sr-only">Readings from {pc || 'this PC'}</h1>
  {#if notes.length}
    <section class="notices" aria-label="Alerts and health">
      <ul>
        {#each notes as n, i (i)}
          <li class={n.level}><span class="sr-only">{n.level === 'hot' ? 'Problem:' : 'Warning:'}</span>{n.text}</li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if view.game}
    <section class="panel game" aria-labelledby="game-title">
      <h2 id="game-title">{view.game.app}</h2>
      <p>
        <span class="num">{fmt(view.game.fps)}</span> FPS
        <span class="muted">· 1% low {fmt(view.game.low1Fps)} · {fmt(view.game.frameMs, 1)} ms</span>
      </p>
    </section>
  {/if}

  <section class="tiles" aria-label="Readings">
    {#each cards as t (t.id)}
      <article class="panel tile {t.level ?? ''}" aria-labelledby="t-{t.id}">
        <h2 id="t-{t.id}">
          {t.title}
          {#if t.level === 'warn' || t.level === 'hot'}<span class="tag {t.level}">{WORD[t.level]}</span>{/if}
        </h2>
        {#if t.sub}<p class="sub">{t.sub}</p>{/if}
        <p class="big"><span class="num">{t.value}</span> <span class="unit">{t.unit}</span></p>
        {#if t.bar != null}
          <div class="bar" aria-hidden="true"><i style:width="{Math.max(0, Math.min(100, t.bar))}%"></i></div>
        {/if}
        <dl>
          {#each t.lines as [k, v] (k)}
            <div><dt>{k}</dt><dd>{v}</dd></div>
          {/each}
        </dl>
      </article>
    {/each}
  </section>

  {#each lists as l (l.id)}
    <section class="panel list" aria-labelledby="l-{l.id}">
      <h2 id="l-{l.id}">{l.title}</h2>
      <ul>
        {#each l.rows as r, i (i)}
          <li>
            <span class="name">{r.name}</span>
            <span class="value {rowLevel(r) ?? ''}">
              {r.value}
              {#if rowLevel(r)}<span class="tag {rowLevel(r)}">{WORD[rowLevel(r)!]}</span>{/if}
            </span>
            {#if r.note}<span class="note-text">{r.note}</span>{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#if view.recent.length}
    <section class="panel list" aria-labelledby="l-recent">
      <h2 id="l-recent">Recent alerts</h2>
      <ul>
        {#each view.recent as a, i (i)}
          <li class="recent">
            <span class="name">{a.title}</span>
            <span class="note-text">{ago(a.ts)}</span>
            {#if a.message}<span class="msg">{a.message}</span>{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

<style>
  .notices ul {
    display: grid;
    gap: 0.4rem;
    margin: 0 0 0.75rem;
    padding: 0;
    list-style: none;
  }
  .notices li {
    padding: 0.55rem 0.75rem;
    border-left: 3px solid var(--warn);
    border-radius: 6px;
    background: color-mix(in srgb, var(--warn) 14%, var(--panel));
  }
  .notices li.hot {
    border-left-color: var(--hot);
    background: color-mix(in srgb, var(--hot) 16%, var(--panel));
  }
  .game p {
    margin: 0.3rem 0 0;
  }
  .game .num {
    font-size: 1.6rem;
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }
  .tile {
    margin: 0;
    min-width: 0;
  }
  .tile.warn {
    border-color: color-mix(in srgb, var(--warn) 60%, var(--line));
  }
  .tile.hot {
    border-color: color-mix(in srgb, var(--hot) 70%, var(--line));
  }
  .sub {
    overflow: hidden;
    margin: 0.15rem 0 0;
    color: var(--muted);
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .big {
    margin: 0.35rem 0 0.25rem;
  }
  .num {
    font-family: var(--font-num);
    font-size: 2rem;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .unit {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .bar {
    height: 4px;
    margin-bottom: 0.5rem;
    border-radius: 2px;
    background: var(--gauge-track);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  dl {
    display: grid;
    gap: 0.2rem;
    margin: 0;
    font-size: 0.82rem;
  }
  dl div {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
  }
  dt {
    color: var(--muted);
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .tag {
    padding: 0.05rem 0.4rem;
    border-radius: 999px;
    color: var(--bg);
    font-size: 0.68rem;
    font-weight: 700;
    letter-spacing: 0;
    text-transform: none;
  }
  .tag.warn {
    background: var(--warn);
  }
  .tag.hot {
    background: var(--hot);
  }
  .list ul {
    margin: 0.5rem 0 0;
    padding: 0;
    list-style: none;
  }
  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.25rem 0.75rem;
    padding: 0.45rem 0;
    border-top: 1px solid var(--line);
    font-size: 0.9rem;
  }
  .list li:first-child {
    border-top: 0;
  }
  .name {
    flex: 1 1 10rem;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .value {
    font-variant-numeric: tabular-nums;
  }
  .note-text {
    color: var(--muted);
    font-size: 0.8rem;
  }
  .recent .msg {
    flex-basis: 100%;
    color: var(--muted);
    font-size: 0.82rem;
  }
</style>

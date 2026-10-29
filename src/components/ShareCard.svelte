<script lang="ts">
  import { onMount } from 'svelte';
  import { DEFAULT_TITLE, POWERED_BY, SIZES, STYLES, TITLE_MAX, cardModel, fileName, font, layout, paint, type CardPalette, type CardSize, type CardStyle } from '../card';
  import { stamp } from '../diag';
  import { getDrives, getPerformanceScore, getSystemInfo, revealCard, saveShareCard } from '../ipc';
  import { prefs } from '../prefs.svelte';
  import { theme, tokens } from '../themes';
  import type { DriveInfo, HardwareId, OsInfo, PerformanceScore } from '../types';

  interface Props {
    hw: HardwareId;
    onclose: () => void;
  }
  let { hw, onclose }: Props = $props();

  let size = $state<CardSize>('wide');
  let style = $state<CardStyle>('full');
  let title = $state(DEFAULT_TITLE);
  let withScore = $state(true);
  let os = $state.raw<OsInfo | null>(null);
  let drives = $state.raw<DriveInfo[]>([]);
  let score = $state.raw<PerformanceScore | null>(null);
  let canvas: HTMLCanvasElement;
  let busy = $state(false);
  let saved = $state<string | null>(null);
  let status = $state<string | null>(null);

  onMount(() => {
    void getSystemInfo()
      .then((s) => (os = s.os))
      .catch(() => {});
    void getDrives()
      .then((d) => (drives = d))
      .catch(() => {});
    void getPerformanceScore()
      .then((r) => (score = r.latest))
      .catch(() => {});
  });

  const hasScore = $derived(score?.score != null);
  const model = $derived(cardModel(hw, os, drives, score, { title, style, withScore }));
  const palette = $derived.by((): CardPalette => {
    const t = theme(prefs.theme);
    const v = tokens(prefs.theme, prefs.accent);
    return {
      bg: v['--bg']!,
      card: v['--card']!,
      cardTop: v['--card-top']!,
      line: v['--line']!,
      text: v['--text']!,
      muted: v['--muted']!,
      accent: v['--accent-cpu']!,
      second: v['--accent-gpu']!,
      ring: [v['--accent-cpu-fill']!, v['--accent-gpu-fill']!],
      track: v['--gauge-track']!,
      glow: t.palette.glow,
      light: t.light,
    };
  });
  const dims = $derived(SIZES[size]);
  /** What the picture says, for screen readers. */
  const described = $derived(
    [model.title, ...model.lines.map((l) => `${l.label}: ${l.value}`), model.score ? `Performance Score ${model.score.value}, ${model.score.band}` : null, POWERED_BY]
      .filter(Boolean)
      .join('. '),
  );

  $effect(() => {
    const ctx = canvas?.getContext('2d');
    if (!ctx) return;
    canvas.width = dims.w;
    canvas.height = dims.h;
    const measure = (text: string, px: number, weight: number) => {
      ctx.font = font(px, weight);
      return ctx.measureText(text).width;
    };
    paint(ctx, layout(model, size, style, measure), size, palette, style === 'transparent');
  });

  function png(): Promise<Blob> {
    return new Promise((resolve, reject) => canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('could not draw the card'))), 'image/png'));
  }

  async function save(): Promise<void> {
    busy = true;
    status = null;
    saved = null;
    try {
      const blob = await png();
      const name = fileName(stamp(Math.floor(Date.now() / 1000)), size, style);
      saved = await saveShareCard(new Uint8Array(await blob.arrayBuffer()), name);
    } catch (e) {
      status = String(e);
    } finally {
      busy = false;
    }
  }

  async function copy(): Promise<void> {
    busy = true;
    status = null;
    saved = null;
    try {
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': png() })]);
      status = 'Copied to the clipboard.';
    } catch (e) {
      status = `Could not copy: ${String(e)}`;
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel share" aria-labelledby="share-title">
  <div class="head">
    <h2 id="share-title">Share card</h2>
    <button type="button" class="ghost" onclick={onclose}>Close</button>
  </div>
  <div class="layout">
    <div class="options">
      <label class="name">
        Name on the card
        <input type="text" maxlength={TITLE_MAX} bind:value={title} placeholder={DEFAULT_TITLE} />
      </label>
      <fieldset>
        <legend>Size</legend>
        {#each Object.entries(SIZES) as [id, s] (id)}
          <label class="check">
            <input type="radio" name="card-size" value={id} bind:group={size} />
            <span>{s.label}<small>{s.w} × {s.h}</small></span>
          </label>
        {/each}
      </fieldset>
      <fieldset>
        <legend>Style</legend>
        {#each Object.entries(STYLES) as [id, s] (id)}
          <label class="check">
            <input type="radio" name="card-style" value={id} bind:group={style} />
            <span>{s.label}<small>{s.hint}</small></span>
          </label>
        {/each}
      </fieldset>
      <label class="check">
        <input type="checkbox" bind:checked={withScore} disabled={!hasScore} />
        <span
          >Performance Score<small>{hasScore ? 'The score now, with its band.' : 'No score yet: it needs about a day of use.'}</small></span
        >
      </label>
      <div class="actions">
        <button type="button" class="primary" disabled={busy} onclick={save}>Save PNG</button>
        <button type="button" disabled={busy} onclick={copy}>Copy</button>
      </div>
      {#if saved}
        <p class="status" role="status">
          Saved to <code>{saved}</code>
          <button type="button" class="link" onclick={() => revealCard(saved!)}>Show in folder</button>
        </p>
      {:else if status}
        <p class="status" role="status">{status}</p>
      {/if}
      <p class="note">
        Uses the current theme and accent. No serial numbers and no computer name: only the parts, Windows and the
        name you type.
      </p>
    </div>
    <div class="frame" class:clear={style === 'transparent'} style:max-width="min(100%, {(62 * dims.w) / dims.h}vh)" role="img" aria-label={described}>
      <canvas bind:this={canvas} aria-hidden="true"></canvas>
    </div>
  </div>
</section>

<style>
  .share {
    margin-bottom: 1rem;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.75rem;
  }
  h2 {
    margin: 0;
    color: var(--muted);
    font-size: 0.72rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(14rem, 18rem) minmax(0, 1fr);
    gap: 1rem 1.5rem;
    align-items: start;
  }
  @media (max-width: 760px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .name {
    display: grid;
    gap: 0.3rem;
    margin-bottom: 0.7rem;
    font-size: 0.85rem;
  }
  input[type='text'] {
    padding: 0.35rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  fieldset {
    margin: 0 0 0.5rem;
    padding: 0;
    border: 0;
  }
  legend {
    margin-bottom: 0.35rem;
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    margin-bottom: 0.45rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .check input {
    margin-top: 0.2rem;
  }
  .check small {
    display: block;
    color: var(--muted);
    font-size: 0.75rem;
  }
  .actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.6rem;
  }
  button {
    padding: 0.35rem 0.8rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
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
    font-size: 0.76rem;
  }
  button.link {
    margin-left: 0.4rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .status {
    margin: 0.6rem 0 0;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }
  code {
    font-size: 0.75rem;
  }
  .note {
    margin: 0.7rem 0 0;
    color: var(--muted);
    font-size: 0.76rem;
  }
  .frame {
    width: 100%;
    justify-self: center;
    border: 1px solid var(--line);
    border-radius: 8px;
    overflow: hidden;
    line-height: 0;
  }
  /* A checkerboard shows what is see-through. */
  .frame.clear {
    background: repeating-conic-gradient(var(--panel-edge) 0 25%, var(--bg) 0 50%) 0 0 / 24px 24px;
  }
  canvas {
    width: 100%;
    height: auto;
  }
</style>

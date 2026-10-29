<script lang="ts">
  import Card from './Card.svelte';
  import CardGrid, { type CardItem } from './CardGrid.svelte';
  import KV from './KV.svelte';
  import LoadState from './LoadState.svelte';
  import Pill from './Pill.svelte';
  import { fmt, NA } from '../format';
  import { getDisplays, onDisplaysChanged } from '../ipc';
  import type { DisplayInfo } from '../types';

  let displays = $state.raw<DisplayInfo[] | null>(null);
  let error = $state<string | null>(null);
  let selected = $state<string | null>(null);
  let revealed = $state<Record<string, boolean>>({});
  let loading = $state(false);

  function load(): void {
    loading = true;
    getDisplays()
      .then((v) => {
        displays = v;
        error = null;
        if (!v.some((d) => d.id === selected)) selected = v[0]?.id ?? null;
      })
      .catch((e: unknown) => (error = String(e)))
      .finally(() => (loading = false));
  }

  $effect(() => {
    load();
    let stop: (() => void) | undefined;
    let disposed = false;
    void onDisplaysChanged(load).then((fn) => {
      if (disposed) fn();
      else stop = fn;
    });
    return () => {
      disposed = true;
      stop?.();
    };
  });

  // Arrangement: desktop rectangles scaled into a fixed viewBox.
  const layout = $derived.by(() => {
    const rects = (displays ?? []).filter((d) => d.rect != null);
    if (rects.length === 0) return null;
    const xs = rects.flatMap((d) => [d.rect![0], d.rect![0] + d.rect![2]]);
    const ys = rects.flatMap((d) => [d.rect![1], d.rect![1] + d.rect![3]]);
    const minX = Math.min(...xs);
    const minY = Math.min(...ys);
    const w = Math.max(...xs) - minX;
    const h = Math.max(...ys) - minY;
    return { minX, minY, w, h, pad: Math.max(w, h) * 0.03, rects };
  });

  function res(w: number | null, h: number | null): string | null {
    return w != null && h != null ? `${w} × ${h}` : null;
  }

  function hz(v: number | null): string | null {
    return v == null ? null : `${fmt(v, Number.isInteger(v) ? 0 : 2)} Hz`;
  }

  function nits(v: number | null, digits = 0): string | null {
    return v == null ? null : `${fmt(v, digits)} nits`;
  }

  function vrrRange(d: DisplayInfo): string | null {
    const { minHz, maxHz } = d.vrr;
    return minHz != null && maxHz != null ? `${fmt(minHz)}–${fmt(maxHz)} Hz` : null;
  }

  function subtitle(d: DisplayInfo): string {
    return [
      d.manufacturer,
      d.diagonalIn != null ? `${fmt(d.diagonalIn, 1)}″` : null,
      d.primary ? 'Primary' : 'Secondary',
    ]
      .filter(Boolean)
      .join(' · ');
  }

  function hexLines(hex: string): string {
    const bytes = hex.match(/../g) ?? [];
    const lines: string[] = [];
    for (let i = 0; i < bytes.length; i += 16) lines.push(bytes.slice(i, i + 16).join(' '));
    return lines.join('\n');
  }

  function select(id: string): void {
    selected = id;
    document.getElementById(`disp:${id}`)?.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  }
  // The arrangement across the top, then one card per display, in Vyro's order or the owner's (2.0.2).
  const cards = $derived<CardItem[]>([
    { id: 'disp-arrangement', title: 'Arrangement', w: 'full' },
    ...(displays ?? []).map((d, i) => ({ id: `disp:${d.id}`, title: `Display ${i + 1}: ${d.name}` })),
  ]);
</script>

{#if error || !displays || displays.length === 0}
  <LoadState what="displays" {error} loading={!displays || loading} empty="No active displays found." retry={load} />
{:else}
  {@const shown = displays}
  <CardGrid view="displays" items={cards} min="20rem">
    {#snippet card(id)}
      {#if id === 'disp-arrangement'}
        <Card title="Arrangement" id="disp-arrangement" subtitle="{shown.length} display{shown.length === 1 ? '' : 's'}">
          <div class="toolbar">
            <button type="button" onclick={load} disabled={loading}>{loading ? 'Refreshing…' : 'Refresh'}</button>
            <span class="hint">Updates automatically when monitors change.</span>
          </div>
          {#if layout}
            <svg
              class="arrangement"
              viewBox="{layout.minX - layout.pad} {layout.minY - layout.pad} {layout.w + 2 * layout.pad} {layout.h +
                2 * layout.pad}"
              role="group"
              aria-label="Desktop arrangement"
            >
              {#each layout.rects as d, i (d.id)}
                {@const [x, y, w, h] = d.rect!}
                {@const fs = Math.min(w, h) * 0.09}
                <g
                  class="screen"
                  class:sel={selected === d.id}
                  role="button"
                  tabindex="0"
                  aria-label="{d.name}, {res(d.width, d.height)}"
                  onclick={() => select(d.id)}
                  onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && select(d.id)}
                >
                  <rect x={x + w * 0.01} y={y + h * 0.01} width={w * 0.98} height={h * 0.98} rx={Math.min(w, h) * 0.03} />
                  <text x={x + w / 2} y={y + h / 2 - fs * 0.4} font-size={fs * 1.6} class="num">{i + 1}</text>
                  <text x={x + w / 2} y={y + h / 2 + fs * 1.1} font-size={fs * 0.7}>{d.name}</text>
                  <text x={x + w / 2} y={y + h / 2 + fs * 2} font-size={fs * 0.6} class="dim">
                    {res(d.width, d.height)} · {hz(d.refreshHz) ?? NA}
                  </text>
                </g>
              {/each}
            </svg>
          {/if}
        </Card>
      {:else}
        {@const i = shown.findIndex((x) => `disp:${x.id}` === id)}
        {@const d = shown[i]}
        {#if d}
          <Card title="Display {i + 1}{d.internal ? ' · built-in' : ''}" {id} subtitle={d.name} class={selected === d.id ? 'picked' : undefined}>
            <p class="meta">{subtitle(d)}</p>
            <div class="pills">
              <span>HDR</span>
              <Pill value={d.hdr.enabled} on="On" off={d.hdr.supported ? 'Off' : 'Not supported'} unknown={d.hdr.supported ? 'Supported' : 'Unknown'} />
              <span>VRR</span>
              <Pill
                value={d.vrr.supported === false ? false : d.vrr.enabled}
                on="On"
                off={d.vrr.supported === false ? 'Not supported' : 'Off'}
                unknown={d.vrr.supported ? 'Supported' : 'Unknown'}
              />
            </div>

            <h3>Signal</h3>
            <KV
              rows={[
                ['Resolution', res(d.width, d.height)],
                ['Native', res(d.nativeWidth, d.nativeHeight)],
                ['Refresh', hz(d.refreshHz)],
                ['Max refresh', hz(d.maxRefreshHz)],
                ['Connection', d.connection],
                ['Color depth', d.bitsPerColor != null ? `${d.bitsPerColor}-bit` : null],
                ['Encoding', d.colorEncoding],
                ['Color space', d.colorSpace],
                ['Scaling', d.scalePct != null ? `${d.scalePct}% (${d.dpi} DPI)` : null],
                ['Orientation', d.orientation != null ? `${d.orientation}°` : null],
                ['Brightness', d.brightnessPct != null ? `${d.brightnessPct}%` : null],
                ['GPU', d.adapter],
              ]}
            />

            <h3>HDR</h3>
            <KV
              rows={[
                ['SDR white', nits(d.hdr.sdrWhiteNits)],
                ['Peak', nits(d.hdr.maxNits)],
                ['Full frame', nits(d.hdr.maxFullFrameNits)],
                ['Black level', nits(d.hdr.minNits, 4)],
                ['Transfer', d.hdr.eotfs.join(', ') || null],
                ['Wide gamut', d.hdr.gamuts.join(', ') || null],
              ]}
            />

            <h3>Variable refresh</h3>
            <KV
              rows={[
                ['Technology', d.vrr.technology],
                ['Range', vrrRange(d)],
                ['Varying now', d.vrr.active == null ? null : d.vrr.active ? 'Yes (game or video)' : 'No (desktop idle)'],
                ['Source', d.vrr.source],
              ]}
            />

            <h3>Panel</h3>
            <KV
              rows={[
                ['Model', d.model],
                ['Manufacturer', d.manufacturerId != null && d.manufacturer !== d.manufacturerId ? `${d.manufacturer} (${d.manufacturerId})` : d.manufacturer],
                ['Product code', d.productCode],
                ['Made', d.manufactured],
                ['Size', d.widthMM != null ? `${d.widthMM} × ${d.heightMM} mm` : null],
                ['Diagonal', d.diagonalIn != null ? `${fmt(d.diagonalIn, 1)}″` : null],
                ['Windows name', d.gdiName],
              ]}
            />
            <div class="serial">
              <span>Serial</span>
              {#if d.serial}
                <code>{revealed[d.id] ? d.serial : '•'.repeat(Math.min(d.serial.length, 12))}</code>
                <button type="button" onclick={() => (revealed[d.id] = !revealed[d.id])} aria-pressed={!!revealed[d.id]}>
                  {revealed[d.id] ? 'Hide' : 'Show'}
                </button>
              {:else}
                <code class="na">{NA}</code>
              {/if}
            </div>

            {#if d.edid}
              <details>
                <summary>EDID {d.edid.version}{d.edid.checksumOk ? '' : ' · checksum error'}</summary>
                <KV
                  rows={[
                    ['Interface', d.edid.interface],
                    ['Bits / color', d.edid.bitsPerColor],
                    ['Gamma', d.edid.gamma != null ? fmt(d.edid.gamma, 2) : null],
                    ['Extensions', d.edid.extensions.join(', ') || 'None'],
                    ['Range limits', d.edid.rangeHz ? `${d.edid.rangeHz[0]}–${d.edid.rangeHz[1]} Hz` : null],
                  ]}
                />
                <pre aria-label="EDID bytes (serial removed)">{hexLines(d.edid.hex)}</pre>
              </details>
            {/if}
          </Card>
        {/if}
      {/if}
    {/snippet}
  </CardGrid>
{/if}

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
    margin-bottom: 0.75rem;
  }
  .hint {
    color: var(--muted);
    font-size: 0.75rem;
  }
  button {
    padding: 0.25rem 0.7rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--panel-edge);
    color: var(--text);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .arrangement {
    display: block;
    width: 100%;
    max-height: 16rem;
  }
  .screen {
    cursor: pointer;
    outline: none;
  }
  .screen rect {
    fill: color-mix(in srgb, var(--accent-cpu) 10%, var(--panel));
    stroke: var(--line);
    stroke-width: 1.5px;
    vector-effect: non-scaling-stroke;
  }
  .screen:hover rect,
  .screen:focus-visible rect {
    stroke: var(--muted);
    stroke-width: 2px;
  }
  .screen.sel rect {
    fill: color-mix(in srgb, var(--accent-cpu) 24%, var(--panel));
    stroke: var(--accent-cpu);
    stroke-width: 2px;
  }
  text {
    fill: var(--text);
    text-anchor: middle;
    dominant-baseline: middle;
    font-family: inherit;
  }
  text.num {
    font-family: var(--font-num);
    font-weight: 700;
  }
  text.dim {
    fill: var(--muted);
  }
  :global(.card.picked) {
    border-color: color-mix(in srgb, var(--accent-cpu) 50%, var(--line));
  }
  .meta {
    margin: -0.5rem 0 0.6rem;
    color: var(--muted);
    font-size: 0.8rem;
  }
  .pills {
    display: grid;
    grid-template-columns: auto auto auto auto;
    justify-content: start;
    align-items: center;
    gap: 0.4rem 0.5rem;
    font-size: 0.78rem;
    color: var(--muted);
  }
  h3 {
    margin: 1rem 0 0.45rem;
    color: var(--muted);
    font-size: 0.7rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .serial {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin-top: 0.4rem;
    font-size: 0.85rem;
  }
  .serial span {
    min-width: 7rem;
    color: var(--muted);
  }
  code {
    font-family: var(--font-num);
  }
  .na {
    color: var(--muted);
  }
  details {
    margin-top: 1rem;
    font-size: 0.85rem;
  }
  summary {
    margin-bottom: 0.5rem;
    color: var(--muted);
    cursor: pointer;
  }
  pre {
    margin: 0.75rem 0 0;
    padding: 0.6rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    font-family: var(--font-num);
    font-size: 0.68rem;
    line-height: 1.45;
    /* A narrow card wraps the lines rather than scrolling sideways (2.0.2). */
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>

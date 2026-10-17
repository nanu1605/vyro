<script lang="ts">
  // The accent's colour wheel (1.3): hue round the wheel, saturation out from the middle and
  // brightness on a slider, or a #rrggbb typed in. Dragging previews on the whole page without
  // saving; letting go (or Enter) saves, as the other sliders do, so the settings file is not
  // written on every move.
  import { contrast, fromHsv, isHex, toHsv, type Hsv } from '../color';
  import { customPair, nearWarning, type Theme } from '../themes';

  interface Props {
    /** The colour in use, `#rrggbb`. */
    value: string;
    /** The theme it is shown on: its text shade is worked out for it. */
    theme: Theme;
    /** While dragging: the page shows this colour, unsaved. */
    onpreview: (hex: string) => void;
    /** Let go, Enter or a recent colour: save it. */
    oncommit: (hex: string) => void;
  }
  let { value, theme, onpreview, oncommit }: Props = $props();

  const RECENT_KEY = 'vyro.accent.recent';
  const SIZE = 168;

  let hsv = $state<Hsv>(toHsv('#4cc9f0'));
  let typed = $state('');
  let canvas = $state<HTMLCanvasElement>();
  let dragging = false;

  // Follow the saved colour when it changes from elsewhere (the command palette, another window).
  $effect(() => {
    if (!dragging && isHex(value)) {
      hsv = toHsv(value);
      typed = value;
    }
  });

  const hex = $derived(fromHsv(hsv));
  const text = $derived(customPair(theme, hex).text[0]);
  const warning = $derived(nearWarning(hex));

  function readRecent(): string[] {
    try {
      const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]') as unknown;
      return Array.isArray(v) ? v.filter((c): c is string => typeof c === 'string' && isHex(c)).slice(0, 6) : [];
    } catch {
      return [];
    }
  }
  let recent = $state(readRecent());

  function commit(c: string): void {
    recent = [c, ...recent.filter((x) => x !== c)].slice(0, 6);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
    } catch {
      /* the list is a convenience */
    }
    typed = c;
    oncommit(c);
  }

  // The wheel, redrawn when the brightness changes.
  $effect(() => {
    const el = canvas;
    if (!el) return;
    const v = hsv.v;
    const dpr = window.devicePixelRatio || 1;
    const n = Math.round(SIZE * dpr);
    el.width = n;
    el.height = n;
    const ctx = el.getContext('2d');
    if (!ctx) return;
    const img = ctx.createImageData(n, n);
    const r = n / 2;
    for (let y = 0; y < n; y++) {
      for (let x = 0; x < n; x++) {
        const dx = x + 0.5 - r;
        const dy = y + 0.5 - r;
        const d = Math.hypot(dx, dy) / r;
        if (d > 1) continue;
        const h = ((Math.atan2(-dy, dx) * 180) / Math.PI + 360) % 360;
        const c = fromHsv({ h, s: d, v });
        const i = (y * n + x) * 4;
        img.data[i] = parseInt(c.slice(1, 3), 16);
        img.data[i + 1] = parseInt(c.slice(3, 5), 16);
        img.data[i + 2] = parseInt(c.slice(5, 7), 16);
        // A soft edge.
        img.data[i + 3] = Math.round(255 * Math.min(1, (1 - d) * r));
      }
    }
    ctx.putImageData(img, 0, 0);
  });

  const thumb = $derived({
    x: 50 + 50 * hsv.s * Math.cos((hsv.h * Math.PI) / 180),
    y: 50 - 50 * hsv.s * Math.sin((hsv.h * Math.PI) / 180),
  });

  function fromPointer(e: PointerEvent, el: HTMLElement): void {
    const b = el.getBoundingClientRect();
    const dx = e.clientX - (b.left + b.width / 2);
    const dy = e.clientY - (b.top + b.height / 2);
    const s = Math.min(1, Math.hypot(dx, dy) / (b.width / 2));
    const h = ((Math.atan2(-dy, dx) * 180) / Math.PI + 360) % 360;
    hsv = { ...hsv, h, s };
    onpreview(fromHsv(hsv));
  }

  function down(e: PointerEvent): void {
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    dragging = true;
    fromPointer(e, el);
  }
  function move(e: PointerEvent): void {
    if (dragging) fromPointer(e, e.currentTarget as HTMLElement);
  }
  function up(): void {
    if (!dragging) return;
    dragging = false;
    commit(hex);
  }
  function cancel(): void {
    dragging = false;
    onpreview(value);
    hsv = toHsv(value);
  }

  function key(e: KeyboardEvent): void {
    const step = e.shiftKey ? 10 : 2;
    const next = { ...hsv };
    if (e.key === 'ArrowLeft') next.h = (next.h - step + 360) % 360;
    else if (e.key === 'ArrowRight') next.h = (next.h + step) % 360;
    else if (e.key === 'ArrowUp') next.s = Math.min(1, next.s + step / 100);
    else if (e.key === 'ArrowDown') next.s = Math.max(0, next.s - step / 100);
    else return;
    e.preventDefault();
    hsv = next;
    onpreview(fromHsv(next));
  }
  function keyup(e: KeyboardEvent): void {
    if (e.key.startsWith('Arrow')) commit(hex);
  }

  function typedIn(): void {
    const t = typed.trim().toLowerCase();
    const c = /^#?[0-9a-f]{6}$/.test(t) ? (t.startsWith('#') ? t : `#${t}`) : null;
    if (c) {
      hsv = toHsv(c);
      commit(c);
    }
  }
  const typedOk = $derived(/^#?[0-9a-fA-F]{6}$/.test(typed.trim()));
</script>

<div class="picker">
  <div
    class="wheel"
    style:width="{SIZE}px"
    style:height="{SIZE}px"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={cancel}
    role="presentation"
  >
    <canvas bind:this={canvas} aria-hidden="true" style:width="{SIZE}px" style:height="{SIZE}px"></canvas>
    <span
      class="thumb"
      role="slider"
      tabindex="0"
      aria-label="Accent colour: hue and saturation"
      aria-valuemin={0}
      aria-valuemax={359}
      aria-valuenow={Math.round(hsv.h)}
      aria-valuetext="Hue {Math.round(hsv.h)} degrees, saturation {Math.round(hsv.s * 100)} percent"
      style:left="{thumb.x}%"
      style:top="{thumb.y}%"
      style:background={hex}
      onkeydown={key}
      onkeyup={keyup}
    ></span>
  </div>
  <div class="controls">
    <label class="field">
      <span>Brightness</span>
      <input
        type="range"
        min="5"
        max="100"
        value={Math.round(hsv.v * 100)}
        oninput={(e) => {
          hsv = { ...hsv, v: Number(e.currentTarget.value) / 100 };
          onpreview(hex);
        }}
        onchange={() => commit(hex)}
      />
    </label>
    <label class="field">
      <span>Colour</span>
      <input
        class="hex"
        type="text"
        maxlength="7"
        spellcheck="false"
        autocomplete="off"
        aria-invalid={!typedOk}
        bind:value={typed}
        onchange={typedIn}
        onkeydown={(e) => e.key === 'Enter' && typedIn()}
      />
    </label>
    {#if recent.length > 0}
      <div class="recent" role="group" aria-label="Recent colours">
        {#each recent as c (c)}
          <button type="button" style:background={c} aria-label="Use {c}" title={c} onclick={() => ((hsv = toHsv(c)), commit(c))}
          ></button>
        {/each}
      </div>
    {/if}
    <p class="shade">
      <span class="sample" aria-hidden="true" style:background={hex}></span>
      {#if text !== hex}
        Text in the accent uses <b style:color={text}>{text}</b>, a shade that stays readable on {theme.name}; as
        picked it has {contrast(hex, theme.palette.bg).toFixed(1)}:1 against the page.
      {:else}
        {hex} reads well on {theme.name} as it is.
      {/if}
    </p>
    {#if warning}
      <p class="hint">Close to the warning colours: alerts may be harder to tell apart.</p>
    {/if}
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 1rem 1.25rem;
    margin-top: 0.6rem;
  }
  .wheel {
    position: relative;
    flex: none;
    border-radius: 50%;
    touch-action: none;
    cursor: crosshair;
  }
  canvas {
    display: block;
    border-radius: 50%;
  }
  .thumb {
    position: absolute;
    width: 1.1rem;
    height: 1.1rem;
    border: 2px solid #fff;
    border-radius: 50%;
    box-shadow:
      0 0 0 1px rgb(0 0 0 / 55%),
      0 2px 6px rgb(0 0 0 / 40%);
    transform: translate(-50%, -50%);
    cursor: grab;
  }
  .controls {
    display: flex;
    flex: 1 1 14rem;
    flex-direction: column;
    gap: 0.6rem;
    min-width: 0;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.85rem;
  }
  .field span {
    min-width: 5.5rem;
    color: var(--muted);
  }
  .field input[type='range'] {
    flex: 1;
    min-width: 0;
  }
  .hex {
    width: 6.5rem;
    padding: 0.25rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 6px;
    background: var(--panel);
    color: var(--text);
    font: inherit;
    font-variant-numeric: tabular-nums;
  }
  .hex[aria-invalid='true'] {
    border-color: var(--hot);
  }
  .recent {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .recent button {
    width: 1.5rem;
    height: 1.5rem;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: 50%;
    cursor: pointer;
  }
  .shade,
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .shade b {
    font-weight: 600;
  }
  .sample {
    display: inline-block;
    width: 0.8rem;
    height: 0.8rem;
    margin-right: 0.35rem;
    border-radius: 3px;
    vertical-align: -0.1rem;
    box-shadow: inset 0 0 0 1px rgb(0 0 0 / 25%);
  }
  .hint {
    color: var(--warn);
  }
</style>

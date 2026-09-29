<script lang="ts">
  import { add, move, path, remove, T_MAX, T_MIN, toPoint, x, y, type Box } from '../curve';
  import { fmtTemp } from '../prefs.svelte';
  import type { CurvePoint } from '../types';

  interface Props {
    points: CurvePoint[];
    /** The header's floor: speeds below it are shaded (the engine never goes lower). */
    minPct: number;
    /** Current source temperature and target speed, drawn as a live marker. */
    live?: { temp: number; pct: number } | null;
    label: string;
    onchange: (points: CurvePoint[]) => void;
  }
  let { points, minPct, live = null, label, onchange }: Props = $props();

  const box: Box = { width: 440, height: 230, left: 34, right: 18, top: 10, bottom: 24 };
  const TEMP_TICKS = [20, 30, 40, 50, 60, 70, 80, 90, 100];
  const PCT_TICKS = [0, 25, 50, 75, 100];

  let svg = $state<SVGSVGElement | null>(null);
  let dragging = $state<number | null>(null);
  let focused = $state<number | null>(null);

  /** Pointer position in chart units (the SVG scales with its container). */
  function at(e: PointerEvent | MouseEvent): CurvePoint | null {
    const r = svg?.getBoundingClientRect();
    if (!r || !r.width) return null;
    return toPoint(box, ((e.clientX - r.left) / r.width) * box.width, ((e.clientY - r.top) / r.height) * box.height);
  }

  function down(e: PointerEvent, i: number): void {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = i;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }

  function drag(e: PointerEvent): void {
    if (dragging == null) return;
    const p = at(e);
    if (p) onchange(move(points, dragging, p));
  }

  function up(): void {
    dragging = null;
  }

  function addAt(e: MouseEvent): void {
    const p = at(e);
    if (p) onchange(add(points, p.tempC));
  }

  function drop(e: MouseEvent, i: number): void {
    e.preventDefault();
    onchange(remove(points, i));
  }

  function key(e: KeyboardEvent, i: number): void {
    const p = points[i];
    if (!p) return;
    const step = e.shiftKey ? 5 : 1;
    const by: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, step],
      ArrowDown: [0, -step],
    };
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      onchange(remove(points, i));
      return;
    }
    const d = by[e.key];
    if (!d) return;
    e.preventDefault();
    onchange(move(points, i, { tempC: p.tempC + d[0], pct: p.pct + d[1] }));
  }

  const line = $derived(path(box, points));
  const area = $derived(
    `${line}L${x(box, T_MAX)},${y(box, 0)}L${x(box, T_MIN)},${y(box, 0)}Z`,
  );
  const tempLabel = (t: number) => fmtTemp(t) ?? `${t} °C`;
</script>

<figure class="editor">
  <svg
    bind:this={svg}
    viewBox="0 0 {box.width} {box.height}"
    role="group"
    aria-label="{label}: fan speed by temperature. Double-click to add a point; drag, or focus a point and use the arrow keys, to move it; right-click or Delete removes it."
    ondblclick={addAt}
    onpointermove={drag}
    onpointerup={up}
    onpointercancel={up}
  >
    <rect
      class="floor"
      x={box.left}
      y={y(box, minPct)}
      width={box.width - box.left - box.right}
      height={y(box, 0) - y(box, minPct)}
    />
    {#each PCT_TICKS as p (p)}
      <line class="grid" x1={box.left} x2={box.width - box.right} y1={y(box, p)} y2={y(box, p)} />
      <text class="axis" x={box.left - 6} y={y(box, p) + 3.5} text-anchor="end">{p}%</text>
    {/each}
    {#each TEMP_TICKS as t (t)}
      <line class="grid" x1={x(box, t)} x2={x(box, t)} y1={box.top} y2={y(box, 0)} />
      <text class="axis" x={x(box, t)} y={box.height - 7} text-anchor="middle">{tempLabel(t)}</text>
    {/each}
    <path class="area" d={area} />
    <path class="line" d={line} />
    {#if live}
      <line class="live-guide" x1={x(box, live.temp)} x2={x(box, live.temp)} y1={box.top} y2={y(box, 0)} />
      <circle class="live" cx={x(box, live.temp)} cy={y(box, live.pct)} r="5" />
    {/if}
    {#each points as p, i (i)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <g
        class="point"
        class:active={dragging === i || focused === i}
        role="button"
        tabindex="0"
        aria-label="Point {i + 1}: {tempLabel(p.tempC)}, {p.pct} %"
        onpointerdown={(e) => down(e, i)}
        oncontextmenu={(e) => drop(e, i)}
        onkeydown={(e) => key(e, i)}
        onfocus={() => (focused = i)}
        onblur={() => (focused = null)}
      >
        <circle class="hit" cx={x(box, p.tempC)} cy={y(box, p.pct)} r="12" />
        <circle class="dot" cx={x(box, p.tempC)} cy={y(box, p.pct)} r="5.5" />
        {#if dragging === i || focused === i}
          <text class="tip" x={x(box, p.tempC)} y={y(box, p.pct) - 11} text-anchor="middle">
            {tempLabel(p.tempC)} · {p.pct}%
          </text>
        {/if}
      </g>
    {/each}
  </svg>
  <figcaption>
    Double-click to add a point · drag to move · right-click to remove. Shaded: below this header’s {minPct}% minimum.
  </figcaption>
</figure>

<style>
  .editor {
    max-width: 40rem;
    margin: 0;
  }
  svg {
    display: block;
    width: 100%;
    height: auto;
    touch-action: none;
    user-select: none;
  }
  .floor {
    fill: color-mix(in srgb, var(--warn) 9%, transparent);
  }
  .grid {
    stroke: var(--line);
    stroke-width: 1;
  }
  .axis {
    fill: var(--muted);
    font-size: 10px;
  }
  .area {
    fill: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .line {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linejoin: round;
  }
  .live-guide {
    stroke: var(--accent-gpu);
    stroke-dasharray: 3 3;
    stroke-width: 1;
  }
  .live {
    fill: var(--accent-gpu);
    stroke: var(--bg);
    stroke-width: 2;
  }
  .point {
    cursor: grab;
    outline: none;
  }
  .point:focus-visible .dot {
    stroke: var(--text);
    stroke-width: 2.5;
  }
  .hit {
    fill: transparent;
  }
  .dot {
    fill: var(--bg);
    stroke: var(--accent);
    stroke-width: 2;
  }
  .active .dot {
    fill: var(--accent);
  }
  .tip {
    fill: var(--text);
    font-size: 11px;
    font-weight: 600;
    paint-order: stroke;
    stroke: var(--bg);
    stroke-width: 3px;
  }
  figcaption {
    margin-top: 0.3rem;
    color: var(--muted);
    font-size: 0.74rem;
  }
</style>

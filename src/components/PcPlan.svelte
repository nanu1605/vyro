<script lang="ts">
  // My PC as a flat plan (P23): the parts seen from the open side, for machines without WebGL
  // or when 2-D is chosen. Same parts and colours as the 3-D view.
  import { CASE, type Part } from '../pcmodel';

  interface Props {
    parts: Part[];
    /** Fill per part id: a colour, 'dim', or nothing for its own look. */
    tint: Map<string, string | 'dim'>;
    labels: Map<string, string>;
    selected: string | null;
    onpick: (id: string) => void;
  }
  let { parts, tint, labels, selected, onpick }: Props = $props();

  const OWN: Partial<Record<Part['kind'], string>> = {
    board: 'var(--panel-edge)',
    cpu: '#c9ccd2',
    cooler: '#8a94a6',
    aio: '#1d2128',
    radiator: '#59616e',
    vrm: '#59616e',
    chipset: '#59616e',
    ram: '#2a3446',
    slot: 'var(--line)',
    gpu: '#2b313b',
    m2: '#1a1f27',
    drive: '#5b6573',
    fan: '#20262f',
    psu: '#14181f',
  };
  const fill = (p: Part) => {
    const t = tint.get(p.id);
    return t === 'dim' ? '#3a4250' : (t ?? OWN[p.kind] ?? '#2b313b');
  };
  // Back to front by depth, so the cooler sits over the CPU and the card over the board.
  const ordered = $derived([...parts].sort((a, b) => a.pos[2] + a.size[2] / 2 - (b.pos[2] + b.size[2] / 2)));
  const y = (v: number) => CASE[1] - v;
</script>

<svg class="plan" viewBox="-10 -10 {CASE[0] + 20} {CASE[1] + 20}" role="group" aria-label="My PC, seen from the open side">
  <rect class="case" x="0" y="0" width={CASE[0]} height={CASE[1]} rx="6" />
  <rect class="shroud" x="0" y={y(100)} width={CASE[0]} height="100" />
  {#each ordered as p (p.id)}
    {@const [cx, cy] = p.pos}
    {@const [w, h] = p.size}
    {#if p.kind === 'fan' && p.fan?.axis === 'z'}
      <circle class="fan" cx={cx} cy={y(cy)} r={Math.min(w, h) / 2 - 2} fill={fill(p)} />
    {:else}
      <rect
        class="part"
        class:sel={selected === p.id}
        x={cx - w / 2}
        y={y(cy) - h / 2}
        width={w}
        height={h}
        rx={p.kind === 'board' ? 4 : 2}
        fill={fill(p)}
      />
    {/if}
  {/each}
  {#each parts.filter((p) => p.labelled && p.group != null) as p (p.id)}
    {@const [ax, ay] = p.anchor}
    <g
      class="label"
      class:sel={selected === p.id}
      role="button"
      tabindex="0"
      aria-label={labels.get(p.id) ?? p.label}
      onclick={() => onpick(p.id)}
      onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), onpick(p.id))}
    >
      <text x={ax} y={y(ay) - 6}>{labels.get(p.id) ?? p.label}</text>
    </g>
  {/each}
</svg>

<style>
  .plan {
    display: block;
    width: 100%;
    max-height: 34rem;
  }
  .case {
    fill: none;
    stroke: var(--line);
    stroke-width: 2;
  }
  .shroud {
    fill: color-mix(in srgb, var(--panel) 70%, transparent);
  }
  .part {
    stroke: color-mix(in srgb, var(--text) 18%, transparent);
    stroke-width: 0.8;
  }
  .part.sel {
    stroke: var(--accent);
    stroke-width: 3;
  }
  .fan {
    stroke: var(--muted);
    stroke-width: 1.5;
  }
  .label {
    cursor: pointer;
    outline: none;
  }
  .label text {
    fill: var(--text);
    font-family: var(--font-ui);
    font-size: 13px;
    font-weight: 600;
    paint-order: stroke;
    stroke: var(--bg);
    stroke-width: 4px;
    text-anchor: middle;
  }
  .label:focus-visible text,
  .label.sel text {
    fill: var(--accent);
  }
</style>

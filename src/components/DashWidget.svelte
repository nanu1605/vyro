<script lang="ts">
  // One Dashboard widget (P24), drawn from the snapshot the Dashboard already has. Gauges,
  // readings and cards are here; the other kinds load on first use (DashExtra).
  import GaugePanel from './GaugePanel.svelte';
  import StatTile from './StatTile.svelte';
  import CoresCard from './CoresCard.svelte';
  import CpuSensorsCard from './CpuSensorsCard.svelte';
  import GpuCard from './GpuCard.svelte';
  import MemoryCard from './MemoryCard.svelte';
  import PowerCard from './PowerCard.svelte';
  import BoardCard from './BoardCard.svelte';
  import SelfCard from './SelfCard.svelte';
  import { gameShown, isKind, readingDef, readingView, type Needs } from '../dashboard';
  import { fmt } from '../format';
  import { prefs } from '../prefs.svelte';
  import type { SensorSnapshot, Widget } from '../types';

  interface Props {
    widget: Widget;
    snap: SensorSnapshot | null;
    /** The Dashboard shows: History and Events read only then. */
    active: boolean;
    /** Why a driver reading is missing ("needs PawnIO", "not reported"), the CPU's and the board's. */
    needs: Needs;
    uiFps: number;
    /** In the editor: placeholders instead of nothing, no links. */
    editing?: boolean;
    onselect?: (tab: 'gaming' | 'health') => void;
  }
  let { widget, snap, active, needs, uiFps, editing = false, onselect }: Props = $props();

  const reading = $derived(widget.kind === 'reading' ? readingDef(widget.source ?? '', snap) : null);
  const view = (r: NonNullable<typeof reading>) => readingView(r, snap, needs, prefs.temperatureUnit);

  const gpuIndex = $derived(widget.source?.startsWith('gpu.') ? Number(widget.source.split('.')[1]) : null);
  const gpu = $derived(gpuIndex == null ? null : (snap?.gpu[gpuIndex] ?? null));
  const many = $derived((snap?.gpu.length ?? 0) > 1);
</script>

{#if !isKind(widget.kind)}
  <div class="panel box muted">This widget comes from a newer version of Vyro.</div>
{:else if widget.kind === 'gauge'}
  {#if gpuIndex == null}
    <GaugePanel
      label="CPU"
      ts={snap?.ts ?? 0}
      usage={snap?.cpu.usageTotal ?? null}
      mhz={snap?.cpu.mhzCurrent ?? null}
      detail={snap?.cpu.mhzBoost != null ? `base ${fmt(snap.cpu.mhzBase)} · boost ${fmt(snap.cpu.mhzBoost)} MHz` : null}
    />
  {:else}
    <div class="gpu">
      <GaugePanel
        label={many || gpuIndex > 0 ? `GPU ${gpuIndex + 1}` : 'GPU'}
        ts={snap?.ts ?? 0}
        usage={gpu?.usage ?? null}
        mhz={gpu?.mhzCore ?? null}
        detail={gpu?.name ?? 'No GPU detected'}
      />
    </div>
  {/if}
{:else if widget.kind === 'reading'}
  {#if reading}
    {@const v = view(reading)}
    {#if reading.game && gameShown(snap) && !editing}
      <button type="button" class="tile-link" onclick={() => onselect?.('gaming')} aria-label="{reading.label}: {snap?.game?.app}, open Gaming">
        <StatTile label={reading.label} value={v.value} unit={v.unit} digits={reading.digits} sub={v.sub} warn={v.warn} hot={v.hot} />
      </button>
    {:else}
      <StatTile
        label={reading.label}
        value={v.value}
        unit={v.unit}
        digits={reading.digits}
        sub={editing && reading.game && !gameShown(snap) ? 'shown while a game runs' : v.sub}
        warn={v.warn}
        hot={v.hot}
      />
    {/if}
  {:else}
    <div class="panel box muted">Unknown reading “{widget.source}”.</div>
  {/if}
{:else if widget.kind === 'card'}
  {#if widget.source === 'cores'}
    <CoresCard cpu={snap?.cpu ?? null} />
  {:else if widget.source === 'cpuSensors'}
    <CpuSensorsCard cpu={snap?.cpu ?? null} needs={needs.cpu} />
  {:else if widget.source === 'gpus'}
    <div class="stack">
      {#each snap?.gpu ?? [] as g, i (i)}<GpuCard gpu={g} />{:else}<div class="panel box muted">No GPU detected.</div>{/each}
    </div>
  {:else if widget.source === 'memory'}
    <MemoryCard mem={snap?.memory ?? null} />
  {:else if widget.source === 'power'}
    <PowerCard power={snap?.power ?? null} gpus={snap?.gpu ?? []} needs={needs.cpu} />
  {:else if widget.source === 'board'}
    <BoardCard board={snap?.board ?? null} fans={snap?.fans ?? []} voltages={snap?.voltages ?? []} pending={needs.board} />
  {:else if widget.source === 'self'}
    <SelfCard app={snap?.app ?? null} {uiFps} />
  {:else}
    <div class="panel box muted">Unknown card “{widget.source}”.</div>
  {/if}
{:else}
  <!-- Graph, History, Game, Health and Events load with the first one shown. -->
  {#await import('./DashExtra.svelte') then { default: DashExtra }}
    <DashExtra {widget} {snap} {active} {needs} {editing} {onselect} />
  {/await}
{/if}

<style>
  .box {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    min-width: 0;
  }
  .muted {
    color: var(--muted);
  }
  .gpu {
    --accent: var(--accent-gpu);
    --accent-fill: var(--accent-gpu-fill);
    display: contents;
  }
  .tile-link {
    display: grid;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .tile-link:hover :global(.tile) {
    border-color: var(--accent-gpu);
  }
  /* Side by side when the card is wide enough for two (1.2). */
  .stack {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 22rem), 1fr));
    align-items: start;
    gap: 0.75rem;
  }
</style>

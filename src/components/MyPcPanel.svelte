<script lang="ts">
  // My PC (P23): this PC as a stylized model with live temperatures, the GPU on its own, and the
  // thermal map. 3-D (Three.js, loaded with this tab) or a flat plan. The WebGL context exists
  // only while the tab shows.
  import { onMount, untrack } from 'svelte';
  import Card from './Card.svelte';
  import KV from './KV.svelte';
  import LoadState from './LoadState.svelte';
  import PcPlan from './PcPlan.svelte';
  import {
    cardKey,
    COLOUR_CHOICES,
    detectCard,
    FAN_CHOICES,
    LENGTH_CHOICES,
    POWER_CHOICES,
    POWER_LABEL,
    SLOT_CHOICES,
    validShape,
    type GpuShape,
  } from '../gpuspecs';
  import { buildLaptop, buildPc, fanSpeedText, formOf, heat, heatColor, inMode, MODES, partTemp, spinRate, type Part, type ThermalMode } from '../pcmodel';
  import { fmt } from '../format';
  import { DEFAULT_MY_PC, getDisplays, getDrives, getHardwareId } from '../ipc';
  import { fmtTemp, prefs, profileSpec, savePrefs } from '../prefs.svelte';
  import { theme, tokens } from '../themes';
  import type { DisplayInfo, DriveInfo, HardwareId, SensorSnapshot } from '../types';
  import type { PcScene } from '../scene3d';

  interface Props {
    snap: SensorSnapshot | null;
    /** The tab shows: the 3-D view exists only then. */
    active: boolean;
    onselect: (tab: 'hardware' | 'cooling' | 'storage' | 'displays') => void;
  }
  let { snap, active, onselect }: Props = $props();

  type View = 'pc' | 'gpu' | 'thermal';
  const VIEWS: Array<{ id: View; label: string }> = [
    { id: 'pc', label: 'PC' },
    { id: 'gpu', label: 'GPU' },
    { id: 'thermal', label: 'Thermal map' },
  ];
  function remembered(): View {
    try {
      const v = localStorage.getItem('vyro.mypc.view');
      return v === 'gpu' || v === 'thermal' ? v : 'pc';
    } catch {
      return 'pc';
    }
  }
  let picked = $state<View>(remembered());
  let mode = $state<ThermalMode>('full');
  let selected = $state<string | null>(null);
  let gpuPick = $state(0);
  function setView(v: View): void {
    picked = v;
    selected = null;
    try {
      localStorage.setItem('vyro.mypc.view', v);
    } catch {
      /* not remembered */
    }
  }

  let hw = $state.raw<HardwareId | null>(null);
  let drives = $state.raw<DriveInfo[]>([]);
  let displays = $state.raw<DisplayInfo[]>([]);
  let error = $state<string | null>(null);
  onMount(() => {
    getHardwareId()
      .then((h) => (hw = h))
      .catch((e: unknown) => (error = String(e)));
    getDrives()
      .then((d) => (drives = d))
      .catch(() => {});
    getDisplays()
      .then((d) => (displays = d))
      .catch(() => {});
  });

  // The owner's card shapes, checked.
  const shapes = $derived(
    Object.fromEntries(
      Object.entries(prefs.myPc?.gpuShapes ?? {}).flatMap(([k, v]) => {
        const s = validShape(v);
        return s ? [[k, s]] : [];
      }),
    ) as Record<string, GpuShape>,
  );

  // Tower or laptop (P29): what the firmware says, or the owner's pick.
  const form = $derived(formOf(hw, prefs.myPc?.form ?? 'auto'));
  const laptop = $derived(form === 'laptop');
  const views = $derived(laptop ? VIEWS.filter((v) => v.id !== 'gpu') : VIEWS);
  // A laptop has no card to show on its own.
  const view = $derived<View>(laptop && picked === 'gpu' ? 'pc' : picked);
  // Board fan headers seen turning this session (only ever grows, so the case fans keep their
  // places): a header that never turns has no fan on it, or its fans run from a hub.
  let spunFans = $state<ReadonlySet<number>>(new Set());
  $effect(() => {
    const now = (snap?.fans ?? []).flatMap((f, i) => ((f.rpm ?? 0) > 0 && !spunFans.has(i) ? [i] : []));
    if (now.length) spunFans = new Set([...spunFans, ...now]);
  });
  const parts = $derived(hw ? (laptop ? buildLaptop : buildPc)({ hw, snap, drives, shapes, cardKey, spunFans }) : []);
  const gpus = $derived(parts.filter((p) => p.kind === 'gpu'));
  const gpuPart = $derived(gpus[Math.min(gpuPick, gpus.length - 1)] ?? null);
  const shown = $derived(view === 'gpu' ? (gpuPart ? [gpuPart] : []) : parts);
  // Rebuild meshes only when the shapes change, not on every reading.
  const structure = $derived(JSON.stringify(shown.map((p) => [p.id, p.pos, p.size, p.gpu?.shape])));

  // Readings.
  const temps = $derived(new Map(parts.map((p) => [p.id, partTemp(p, snap)])));
  const labels = $derived(
    new Map(
      parts.map((p) => {
        const t = temps.get(p.id);
        const name = p.label.length > 20 ? `${p.label.slice(0, 18).trimEnd()}…` : p.label;
        return [p.id, t ? `${name} ${fmtTemp(t.temp)}` : name];
      }),
    ),
  );
  const rgb = (c: [number, number, number]) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
  const tint = $derived.by(() => {
    const m = new Map<string, string | 'dim'>();
    if (view !== 'thermal') return m;
    for (const p of parts) {
      const t = temps.get(p.id);
      m.set(p.id, inMode(p, mode) && t ? rgb(heatColor(heat(t.temp, t.limits))) : 'dim');
    }
    return m;
  });
  const spin = $derived(
    new Map(parts.flatMap((p) => (p.fan && snap ? [[p.id, spinRate(p.fan.rpm(snap))] as [string, number]] : []))),
  );
  const motion = $derived(profileSpec().motion);
  const maxFps = $derived(motion === 'off' ? 0 : motion === 'reduced' ? 30 : 60);
  /** Warm parts glow (not under the thermal map, which colours them already). */
  const heats = $derived(new Map([...temps].flatMap(([id, t]) => (t ? [[id, heat(t.temp, t.limits)] as [string, number]] : []))));

  // The scene's switches (remembered).
  function flag(key: string, fallback: boolean): boolean {
    try {
      const v = localStorage.getItem(key);
      return v == null ? fallback : v === '1';
    } catch {
      return fallback;
    }
  }
  function remember(key: string, on: boolean): void {
    try {
      localStorage.setItem(key, on ? '1' : '0');
    } catch {
      /* not remembered */
    }
  }
  let airflow = $state(flag('vyro.mypc.airflow', true));
  let xray = $state(flag('vyro.mypc.xray', true));
  let explode = $state(false);
  $effect(() => remember('vyro.mypc.airflow', airflow));
  $effect(() => remember('vyro.mypc.xray', xray));
  async function setForm(f: 'auto' | 'desktop' | 'laptop'): Promise<void> {
    try {
      await savePrefs({ myPc: { ...DEFAULT_MY_PC, ...prefs.myPc, form: f } });
      selected = null;
    } catch (e) {
      error = String(e);
    }
  }

  // Hover: the part under the pointer, named next to it.
  let hover = $state<{ id: string; x: number; y: number } | null>(null);

  // 3-D: one scene while the tab shows; gone (context freed) otherwise.
  const webgl = typeof document !== 'undefined' && (() => {
    try {
      const c = document.createElement('canvas');
      return !!(c.getContext('webgl2') ?? c.getContext('webgl'));
    } catch {
      return false;
    }
  })();
  let failed = $state(false);
  const threeD = $derived((prefs.myPc?.threeD ?? true) && webgl && !failed);
  let canvas = $state<HTMLCanvasElement | undefined>();
  let scene = $state.raw<PcScene | null>(null);
  let spots = $state.raw(new Map<string, { x: number; y: number; front: boolean }>());

  function placeLabels(s: PcScene): void {
    const next = s.project();
    let changed = next.size !== spots.size;
    for (const [id, p] of next) {
      const o = spots.get(id);
      if (!o || Math.abs(o.x - p.x) > 0.5 || Math.abs(o.y - p.y) > 0.5 || o.front !== p.front) changed = true;
    }
    if (changed) spots = next;
    if (canvas) canvas.dataset.frames = String(s.frames());
  }

  // A fresh canvas per scene: a canvas whose context was freed cannot have another.
  let wrap = $state<HTMLDivElement | undefined>();
  $effect(() => {
    if (!active || !threeD || !wrap || !hw) return;
    const kind = view === 'gpu' ? 'gpu' : laptop ? 'laptop' : 'pc';
    // From the settings, not the page's computed style: a theme or accent change rebuilds the
    // scene whatever order the effects run in.
    const t = theme(prefs.theme);
    const v = tokens(t.id, prefs.accent);
    const pal = { line: v['--line']!, board: v['--panel-edge']!, accent: v['--accent-cpu']!, bg: v['--panel']!, light: t.light };
    const el = document.createElement('canvas');
    el.setAttribute('role', 'img');
    el.setAttribute(
      'aria-label',
      kind === 'gpu'
        ? 'Your graphics card in 3-D. Drag to turn it.'
        : `This ${kind === 'laptop' ? 'laptop' : 'PC'} in 3-D. Drag to turn it, scroll to zoom, double-click a part to look closer.`,
    );
    wrap.prepend(el);
    canvas = el;
    let gone = false;
    let made: PcScene | null = null;
    void import('../scene3d').then(({ createScene }) => {
      if (gone) return;
      made = createScene(el, kind, pal, (id) => (selected = id));
      if (!made) {
        failed = true;
        return;
      }
      made.onFrame(() => made && placeLabels(made));
      made.onHover((id, x, y) => (hover = id ? { id, x, y } : null));
      scene = made;
    });
    return () => {
      gone = true;
      made?.dispose();
      el.remove();
      canvas = undefined;
      scene = null;
      spots = new Map();
      hover = null;
    };
  });
  $effect(() => {
    void structure;
    const s = scene;
    if (s) untrack(() => s.setParts(shown));
  });
  $effect(() => scene?.setTint(tint));
  $effect(() => scene?.setSpin(spin));
  $effect(() => scene?.setMaxFps(maxFps));
  $effect(() => scene?.setHeat(heats));
  $effect(() => scene?.setOptions({ quality: motion === 'full' ? 'high' : 'low', airflow, explode, xray }));
  // The laptop's screen shows the readings.
  $effect(() => {
    const s = scene;
    if (!s || !laptop || !snap) return;
    const g = snap.gpu.find((x) => x.integrated === false) ?? snap.gpu[0];
    s.setScreen({
      rows: [
        ['CPU', fmtTemp(snap.cpu.tempPackage) ?? `${fmt(snap.cpu.usageTotal)} %`],
        ['GPU', fmtTemp(g?.tempEdge) ?? `${fmt(g?.usage)} %`],
        ['CPU use', `${fmt(snap.cpu.usageTotal)} %`],
        ['Memory', snap.memory.usedMB != null ? `${fmt(snap.memory.usedMB / 1024, 1)} GB` : '—'],
      ],
      load: snap.cpu.usageTotal,
    });
  });

  // Inspect.
  const part = $derived(parts.find((p) => p.id === selected) ?? null);
  function details(p: Part): Array<[string, string | number | null]> {
    const s = snap;
    const t = temps.get(p.id);
    const temp: Array<[string, string | null]> = t
      ? [
          ['Temperature', fmtTemp(t.temp)],
          ['Warning at', fmtTemp(t.limits[0])],
          ['Problem at', fmtTemp(t.limits[1])],
        ]
      : [['Temperature', 'not reported']];
    switch (p.kind) {
      case 'cpu':
      case 'cooler':
      case 'aio':
        return [
          ['Processor', hw?.cpu.brand ?? null],
          ...temp,
          ['Usage', s?.cpu.usageTotal != null ? `${fmt(s.cpu.usageTotal)} %` : null],
          ['Clock', s?.cpu.mhzCurrent != null ? `${fmt(s.cpu.mhzCurrent)} MHz` : null],
          ['Power', s?.cpu.powerPkgW != null ? `${fmt(s.cpu.powerPkgW, 1)} W` : null],
          ['Cooling', p.kind === 'aio' ? 'Liquid (pump reported)' : 'Air cooler'],
        ];
      case 'die':
        if (p.group === 'gpu') {
          const g = p.gpuIndex != null ? s?.gpu[p.gpuIndex] : undefined;
          return [
            ['Graphics', g?.name ?? null],
            ...temp,
            ['Hotspot', fmtTemp(g?.tempHotspot)],
            ['Usage', g?.usage != null ? `${fmt(g.usage)} %` : null],
            ['Power', g?.powerW != null ? `${fmt(g.powerW)} W` : null],
          ];
        }
        return [
          ['Processor', hw?.cpu.brand ?? null],
          ...temp,
          ['Usage', s?.cpu.usageTotal != null ? `${fmt(s.cpu.usageTotal)} %` : null],
          ['Clock', s?.cpu.mhzCurrent != null ? `${fmt(s.cpu.mhzCurrent)} MHz` : null],
          ['Power', s?.cpu.powerPkgW != null ? `${fmt(s.cpu.powerPkgW, 1)} W` : null],
        ];
      case 'battery':
        return [['Battery', 'see Devices for its charge']];
      case 'heatpipe':
        return [['Heat pipe', 'carries heat from the chips to the fins']];
      case 'gpu': {
        const g = s?.gpu[p.gpu!.index];
        return [
          ['Card', g?.name ?? null],
          ['Looks like', p.gpu!.card.design],
          ...temp,
          ['Hotspot', fmtTemp(g?.tempHotspot)],
          ['Usage', g?.usage != null ? `${fmt(g.usage)} %` : null],
          ['Power', g?.powerW != null ? `${fmt(g.powerW)} W` : null],
          ['Fans', fanSpeedText(g?.fanRpm ?? null)],
        ];
      }
      case 'ram':
      case 'sodimm': {
        const d = hw?.memory.dimms[p.dimm ?? -1];
        return [
          ['Slot', d?.slot ?? null],
          ['Module', d ? `${(d.sizeMB / 1024).toFixed(0)} GB ${d.memoryType ?? ''} ${d.configuredSpeedMts ?? ''} MT/s`.trim() : null],
          ['Maker', d?.manufacturer ?? null],
          ...temp,
        ];
      }
      case 'm2':
      case 'drive': {
        const d = s?.storage.find((x) => x.id === p.id);
        return [['Drive', p.label], ['Volumes', d?.volumes || null], ['Health', d?.health ?? null], ...temp];
      }
      case 'vrm':
      case 'chipset':
      case 'board':
        return [
          ['Board', hw?.board.product ?? null],
          ['System', fmtTemp(s?.board.tempSystem)],
          ['VRM', fmtTemp(s?.board.tempVrmMos ?? s?.board.tempVrm)],
          ['Chipset', fmtTemp(s?.board.tempChipset ?? s?.board.tempPch)],
        ];
      case 'fan':
      case 'blower': {
        const rpm = p.fan && s ? p.fan.rpm(s) : null;
        return [
          ['Fan', p.label],
          ['Speed', fanSpeedText(rpm)],
        ];
      }
      default:
        return [[p.label, 'no readings']];
    }
  }

  // "Does this look like your card?"
  let draft = $state<GpuShape | null>(null);
  const detected = $derived(gpuPart ? detectCard(hw!.gpus.find((g) => cardKey(g) === gpuPart.gpu!.key) ?? hw!.gpus[0]!) : null);
  $effect(() => {
    void gpuPart?.gpu?.key;
    draft = gpuPart ? { ...gpuPart.gpu!.shape } : null;
  });
  const mine = $derived(gpuPart ? gpuPart.gpu!.key in shapes : false);
  async function keep(shape: GpuShape | null): Promise<void> {
    if (!gpuPart) return;
    const next = { ...(prefs.myPc?.gpuShapes ?? {}) };
    if (shape) next[gpuPart.gpu!.key] = shape;
    else delete next[gpuPart.gpu!.key];
    try {
      await savePrefs({ myPc: { ...DEFAULT_MY_PC, ...prefs.myPc, gpuShapes: next } });
      error = null;
    } catch (e) {
      error = String(e);
    }
  }
  async function set3d(on: boolean): Promise<void> {
    try {
      await savePrefs({ myPc: { ...DEFAULT_MY_PC, ...prefs.myPc, threeD: on } });
    } catch (e) {
      error = String(e);
    }
  }

  const tempKey = (p: Part) => temps.get(p.id);
  const legend = [0, 0.3, 0.5, 1].map((h) => rgb(heatColor(h)));
</script>

{#if error && !hw}
  <LoadState what="this PC's parts" {error} />
{:else if !hw}
  <LoadState what="this PC's parts" error={null} loading />
{:else}
  <div class="bar">
    <div class="views" role="radiogroup" aria-label="View">
      {#each views as v (v.id)}
        <button type="button" role="radio" aria-checked={view === v.id} onclick={() => setView(v.id)}>{v.label}</button>
      {/each}
    </div>
    {#if threeD && view !== 'gpu'}
      <div class="tools" role="group" aria-label="3-D view">
        <button type="button" aria-pressed={explode} onclick={() => (explode = !explode)} title="Lift the parts apart">Explode</button>
        <button type="button" aria-pressed={airflow} onclick={() => (airflow = !airflow)} title="Air from the intakes to the exhausts">Airflow</button>
        {#if laptop}
          <button type="button" aria-pressed={xray} onclick={() => (xray = !xray)} title="See through the keyboard deck">See inside</button>
        {/if}
        <button type="button" onclick={() => scene?.focus(null)} title="Back to the whole view">Reset view</button>
      </div>
    {/if}
    {#if view === 'thermal'}
      <label>
        <span>Show</span>
        <select bind:value={mode}>
          {#each MODES as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
        </select>
      </label>
    {/if}
    {#if view === 'gpu' && gpus.length > 1}
      <label>
        <span>Card</span>
        <select bind:value={gpuPick}>
          {#each gpus as g, i (g.id)}<option value={i}>{g.label} · {snap?.gpu[g.gpu!.index]?.name ?? ''}</option>{/each}
        </select>
      </label>
    {/if}
    <span class="spacer"></span>
    <label>
      <span>Model</span>
      <select value={prefs.myPc?.form ?? 'auto'} onchange={(e) => void setForm(e.currentTarget.value as 'auto' | 'desktop' | 'laptop')}>
        <option value="auto">Automatic ({hw?.board.formFactor === 'laptop' ? 'laptop' : 'desktop'})</option>
        <option value="desktop">Desktop</option>
        <option value="laptop">Laptop</option>
      </select>
    </label>
    <label class="toggle">
      <input type="checkbox" checked={threeD} disabled={!webgl || failed} onchange={(e) => void set3d(e.currentTarget.checked)} />
      <span>3-D</span>
    </label>
    {#if !webgl || failed}<span class="note">3-D is not available on this PC's graphics; showing the plan.</span>{/if}
  </div>

  <div class="layout">
    <div class="stage panel">
      {#if view === 'gpu' && !gpuPart}
        <p class="empty">No graphics card: this PC uses the processor's own graphics.</p>
      {:else if threeD}
        <div class="canvas-wrap" bind:this={wrap}>
          {#if view !== 'gpu'}
            {#each parts.filter((p) => p.labelled && p.group != null) as p (p.id)}
              {@const at = spots.get(p.id)}
              {#if at && at.front}
                <button
                  type="button"
                  class="tag"
                  class:sel={selected === p.id}
                  style:left="{at.x}px"
                  style:top="{at.y}px"
                  onclick={() => (selected = p.id)}
                >
                  {#if view === 'thermal' && tint.get(p.id) && tint.get(p.id) !== 'dim'}<span class="swatch" style:background={tint.get(p.id)}></span>{/if}
                  {labels.get(p.id)}
                </button>
              {/if}
            {/each}
          {/if}
          {#if hover && !(parts.find((p) => p.id === hover?.id)?.labelled)}
            <span class="tip" style:left="{hover.x}px" style:top="{hover.y}px" aria-hidden="true">{labels.get(hover.id) ?? hover.id}</span>
          {/if}
        </div>
      {:else if view === 'gpu' && gpuPart}
        <svg class="card2d" viewBox="-10 -20 {gpuPart.gpu!.shape.lengthMm + 20} 170" role="img" aria-label="Your graphics card, {gpuPart.gpu!.shape.fans} fans, {gpuPart.gpu!.shape.lengthMm} mm long">
          <rect x="0" y="0" width={gpuPart.gpu!.shape.lengthMm} height="126" rx="8" class="shroud {gpuPart.gpu!.shape.colour}" />
          {#each Array.from({ length: gpuPart.gpu!.shape.fans }, (_, i) => i) as i (i)}
            {@const n = gpuPart.gpu!.shape.fans}
            {@const r = Math.min(48, gpuPart.gpu!.shape.lengthMm / n / 2 - 5)}
            <circle cx={(i + 0.5) * (gpuPart.gpu!.shape.lengthMm / n)} cy="63" r={r} class="fan2d" />
          {/each}
        </svg>
      {:else}
        <PcPlan parts={view === 'gpu' ? shown : parts} {tint} {labels} {selected} {laptop} onpick={(id) => (selected = id)} />
      {/if}
      {#if view === 'thermal'}
        <div class="legend" aria-hidden="true">
          <span>Cool</span>
          <span class="ramp" style:background="linear-gradient(90deg, {legend[0]} 0%, {legend[1]} 30%, {legend[2]} 50%, {legend[3]} 100%)"></span>
          <span>Limit</span>
        </div>
      {/if}
    </div>

    <aside class="side">
      {#if view === 'gpu' && gpuPart && draft}
        <Card title="Your graphics card" subtitle={snap?.gpu[gpuPart.gpu!.index]?.name ?? gpuPart.label}>
          <KV
            rows={[
              ['Looks like', mine ? 'Your description' : (detected?.design ?? null)],
              ['Temperature', fmtTemp(snap?.gpu[gpuPart.gpu!.index]?.tempEdge)],
              ['Fans', fanSpeedText(snap?.gpu[gpuPart.gpu!.index]?.fanRpm ?? null)],
            ]}
          />
          <fieldset class="confirm">
            <legend>Does this look like your card?</legend>
            <label><span>Fans</span>
              <select bind:value={draft.fans}>{#each FAN_CHOICES as f (f)}<option value={f}>{f}</option>{/each}</select>
            </label>
            <label><span>Length</span>
              <select bind:value={draft.lengthMm}>
                {#each [...new Set([...LENGTH_CHOICES, draft.lengthMm])].sort((a, b) => a - b) as l (l)}<option value={l}>{l} mm</option>{/each}
              </select>
            </label>
            <label><span>Thickness</span>
              <select bind:value={draft.slots}>{#each SLOT_CHOICES as s (s)}<option value={s}>{s} slots</option>{/each}</select>
            </label>
            <label><span>Power</span>
              <select bind:value={draft.power}>{#each POWER_CHOICES as p (p)}<option value={p}>{POWER_LABEL[p]}</option>{/each}</select>
            </label>
            <label><span>Colour</span>
              <select bind:value={draft.colour}>{#each COLOUR_CHOICES as c (c)}<option value={c}>{c[0]!.toUpperCase() + c.slice(1)}</option>{/each}</select>
            </label>
            <label class="check"><input type="checkbox" bind:checked={draft.flow} /> <span>One fan on each side (flow-through)</span></label>
            <div class="buttons">
              <button type="button" class="primary" onclick={() => draft && void keep({ ...draft })}>Save</button>
              <button type="button" disabled={!mine} onclick={() => void keep(null)}>Back to detected</button>
            </div>
            <p class="note">The model is our own simple drawing; it is matched by the card's id, not by a photo.</p>
          </fieldset>
        </Card>
      {:else if part}
        <Card title={part.kind === 'fan' || part.kind === 'blower' ? 'Fan' : part.label} subtitle={tempKey(part) ? fmtTemp(tempKey(part)!.temp) : null}>
          <KV rows={details(part)} />
          {#if part.tab}
            {@const tab = part.tab}
            <button type="button" class="link" onclick={() => onselect(tab)}>Open {tab[0]!.toUpperCase() + tab.slice(1)} →</button>
          {/if}
        </Card>
      {:else}
        <Card title={view === 'thermal' ? 'Thermal map' : laptop ? 'This laptop' : 'This PC'} subtitle={(laptop ? hw.board.systemProduct : null) ?? hw.board.product ?? null}>
          <p class="note">
            {view === 'thermal'
              ? 'Each part is coloured by how close it is to its limits: blue cool, green normal, amber at the warning limit, red at the problem limit. Grey parts report no temperature.'
              : threeD
                ? `Click a part or its label for its details; double-click to look closer. Drag to turn the ${laptop ? 'laptop' : 'PC'}, scroll to zoom.`
                : 'Click a label for its details.'}
          </p>
          <ul class="temps">
            {#each parts.filter((p) => p.labelled && tempKey(p)) as p (p.id)}
              {@const t = tempKey(p)!}
              <li>
                <button type="button" class="row" onclick={() => (selected = p.id)}>
                  <span class="swatch" style:background={rgb(heatColor(heat(t.temp, t.limits)))}></span>
                  <span class="name">{p.label}</span>
                  <span class="val">{fmtTemp(t.temp)}</span>
                </button>
              </li>
            {/each}
          </ul>
          {#if displays.length && view === 'pc'}
            <h3>Displays</h3>
            <ul class="temps">
              {#each displays as d (d.id)}
                <li>
                  <button type="button" class="row" onclick={() => onselect('displays')}>
                    <span class="name">{d.name}</span>
                    <span class="val">{d.width && d.height ? `${d.width} × ${d.height}` : ''}{d.refreshHz ? ` · ${fmt(d.refreshHz)} Hz` : ''}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </Card>
      {/if}
      {#if error && hw}<p class="error" role="alert">{error}</p>{/if}
    </aside>
  </div>
{/if}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.8rem;
    margin-bottom: 0.75rem;
    font-size: 0.8rem;
  }
  .views {
    display: inline-flex;
    gap: 2px;
    padding: 3px;
    border: 1px solid var(--line);
    border-radius: 11px;
    background: var(--panel);
  }
  .views button {
    padding: 0.32rem 0.85rem;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .views button:hover {
    color: var(--text);
  }
  .views button[aria-checked='true'] {
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent-line);
    color: var(--text);
  }
  .spacer {
    flex: 1;
  }
  .tools {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }
  .tools button {
    padding: 0.32rem 0.75rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel);
    color: var(--muted);
    font: inherit;
    cursor: pointer;
  }
  .tools button:hover {
    color: var(--text);
  }
  .tools button[aria-pressed='true'] {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--text);
  }
  .tip {
    position: absolute;
    z-index: 3;
    padding: 0.2rem 0.5rem;
    border: 1px solid var(--accent-line);
    border-radius: 6px;
    background: color-mix(in srgb, var(--panel) 92%, transparent);
    color: var(--text);
    font-size: 0.75rem;
    white-space: nowrap;
    transform: translate(12px, -130%);
    pointer-events: none;
  }
  label {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
  }
  select,
  button {
    padding: 0.25rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    background: var(--panel-edge);
    color: var(--text);
    font: inherit;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: 0.78rem;
  }
  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 20rem;
    gap: 1rem;
    align-items: start;
  }
  @media (max-width: 900px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .stage {
    position: relative;
    min-height: 22rem;
    padding: 0.5rem;
  }
  /* As tall as the window allows below the bar (1.2: a short, wide view cut the case off); the
     scene frames the whole model for whatever shape this is. */
  .canvas-wrap {
    position: relative;
    container-type: inline-size;
    height: clamp(22rem, calc(100dvh - 14rem), 64rem);
    width: 100%;
    overflow: hidden;
  }
  @media (max-width: 900px) {
    .canvas-wrap {
      height: auto;
      aspect-ratio: 4 / 3;
    }
  }
  .canvas-wrap :global(canvas) {
    display: block;
    width: 100%;
    height: 100%;
    cursor: grab;
    touch-action: none;
  }
  .tag {
    position: absolute;
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.1rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: color-mix(in srgb, var(--panel) 88%, transparent);
    font-size: 0.72rem;
    font-weight: 600;
    white-space: nowrap;
    transform: translate(-50%, -120%);
  }
  .tag.sel {
    border-color: var(--accent);
  }
  /* Too many labels for a narrow view: the list beside (or below) has the same readings. */
  @container (max-width: 520px) {
    .tag {
      display: none;
    }
  }
  .swatch {
    display: inline-block;
    flex: none;
    width: 0.65rem;
    height: 0.65rem;
    border-radius: 3px;
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0.5rem 0.25rem 0;
    color: var(--muted);
    font-size: 0.72rem;
  }
  .ramp {
    flex: 0 1 14rem;
    height: 0.5rem;
    border-radius: 4px;
  }
  .empty {
    margin: 2rem 1rem;
    color: var(--muted);
  }
  .card2d {
    display: block;
    width: 100%;
    max-height: 22rem;
  }
  .shroud {
    stroke: var(--line);
    stroke-width: 2;
  }
  .shroud.black {
    fill: #1d2128;
  }
  .shroud.gunmetal {
    fill: #4a4f57;
  }
  .shroud.silver {
    fill: #b8bec7;
  }
  .shroud.white {
    fill: #e8eaee;
  }
  .fan2d {
    fill: #2b313b;
    stroke: #151a21;
    stroke-width: 4;
  }
  .side {
    display: grid;
    gap: 1rem;
  }
  .temps {
    margin: 0.75rem 0 0;
    padding: 0;
    list-style: none;
  }
  h3 {
    margin: 1rem 0 0;
    color: var(--muted);
    font-size: 0.72rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.35rem 0;
    border: 0;
    border-top: 1px solid var(--line);
    border-radius: 0;
    background: none;
    text-align: left;
  }
  .row .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row .val {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .confirm {
    display: grid;
    gap: 0.45rem;
    min-width: 0;
    margin: 0.75rem 0 0;
    padding: 0.75rem;
    border: 1px solid var(--line);
    border-radius: 10px;
  }
  .confirm legend {
    padding: 0 0.3rem;
    font-weight: 600;
  }
  .confirm label {
    justify-content: space-between;
  }
  .confirm select {
    min-width: 0;
    max-width: 11rem;
  }
  .confirm label.check {
    justify-content: flex-start;
  }
  .buttons {
    display: flex;
    gap: 0.5rem;
  }
  .primary {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 22%, var(--panel-edge));
    font-weight: 600;
  }
  .link {
    margin-top: 0.75rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
  }
  .error {
    margin: 0;
    color: var(--hot);
    font-size: 0.8rem;
  }
</style>

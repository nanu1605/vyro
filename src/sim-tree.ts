// Browser-only stand-in for the engine's sensor registry: derives a sensor tree from the
// simulated snapshot and tracks min / max / avg the way the Rust registry does.

import type { Group, HistoryRange, HistorySeries, Sensor, SensorSnapshot, SensorTree, Source, Unit } from './types';

const GROUP_ORDER: Group[] = ['cpu', 'gpu', 'memory', 'storage', 'board', 'fans', 'voltages', 'network', 'power', 'system', 'app'];

interface Stats {
  min: number | null;
  max: number | null;
  sum: number;
  count: number;
}

const stats = new Map<string, Stats>();

export function resetSimStats(): void {
  stats.clear();
}

function sensor(id: string, group: Group, name: string, unit: Unit, value: number | null | undefined, history = true, source: Source = 'sim'): Sensor {
  const v = value ?? null;
  const st = stats.get(id) ?? { min: null, max: null, sum: 0, count: 0 };
  if (v != null) {
    st.min = st.min == null ? v : Math.min(st.min, v);
    st.max = st.max == null ? v : Math.max(st.max, v);
    st.sum += v;
    st.count += 1;
  }
  stats.set(id, st);
  return {
    id, group, name, unit, value: v, min: st.min, max: st.max,
    avg: st.count ? st.sum / st.count : null, source, available: v != null, history,
  };
}

export function simTree(s: SensorSnapshot): SensorTree {
  const c = s.cpu;
  const list: Sensor[] = [
    sensor('cpu.usage.total', 'cpu', 'CPU Total', '%', c.usageTotal),
    sensor('cpu.clock.current', 'cpu', 'CPU Clock', 'MHz', c.mhzCurrent),
    sensor('cpu.temp.package', 'cpu', 'CPU (Tdie)', '°C', c.tempPackage),
    sensor('cpu.power.package', 'cpu', 'CPU Package Power', 'W', c.powerPkgW),
    sensor('cpu.power.cores', 'cpu', 'CPU Cores Power', 'W', c.powerCoreW),
    sensor('cpu.vid', 'cpu', 'CPU Core VID', 'V', c.vidCore),
    sensor('cpu.clock.effective', 'cpu', 'CPU Effective Clock', 'MHz', c.mhzEffective),
    ...c.tempCcd.map((t, i) => sensor(`cpu.temp.ccd${i}`, 'cpu', `CPU CCD${i + 1}`, '°C', t)),
    ...c.usagePerCore.map((u, i) => sensor(`cpu.core${i}.usage`, 'cpu', `CPU #${i} Usage`, '%', u, false)),
    ...s.gpu.flatMap((g, i) => [
      sensor(`gpu.${i}.usage`, 'gpu', `GPU #${i} Usage`, '%', g.usage),
      sensor(`gpu.${i}.clock.core`, 'gpu', `GPU #${i} Core Clock`, 'MHz', g.mhzCore),
      sensor(`gpu.${i}.temp.edge`, 'gpu', `GPU #${i} Temperature`, '°C', g.tempEdge),
      sensor(`gpu.${i}.temp.hotspot`, 'gpu', `GPU #${i} Hotspot`, '°C', g.tempHotspot),
      sensor(`gpu.${i}.vram.used`, 'gpu', `GPU #${i} VRAM Used`, 'MB', g.vramUsedMB),
      sensor(`gpu.${i}.power`, 'gpu', `GPU #${i} Power`, 'W', g.powerW),
      sensor(`gpu.${i}.fan`, 'gpu', `GPU #${i} Fan`, '%', g.fanPct),
    ]),
    sensor('memory.used', 'memory', 'Memory Used', 'MB', s.memory.usedMB, true, 'os'),
    sensor('memory.available', 'memory', 'Memory Available', 'MB', s.memory.availableMB, true, 'os'),
    ...s.memory.dimms.map((d) => sensor(`memory.${d.slot.toLowerCase()}`, 'memory', `${d.slot} temperature`, '°C', d.tempC)),
    ...s.storage.map((d, i) => sensor(`storage.disk${i}.temp`, 'storage', `${d.model} Temperature`, '°C', d.tempC, true, 'smart')),
    sensor('board.temp1', 'board', 'System', '°C', s.board.tempSystem),
    sensor('board.temp2', 'board', 'VRM MOS', '°C', s.board.tempVrmMos),
    sensor('board.temp3', 'board', 'Chipset', '°C', s.board.tempChipset),
    ...s.fans.map((f, i) => sensor(`fan.${i}`, 'fans', f.name, 'RPM', f.rpm)),
    ...s.voltages.map((v, i) => sensor(`voltage.vin${i}`, 'voltages', v.name, 'V', v.value)),
    ...s.network.flatMap((n) => [
      sensor(`network.${n.id}.down`, 'network', `${n.adapter} Download`, 'Mbps', n.downMbps, true, 'os'),
      sensor(`network.${n.id}.up`, 'network', `${n.adapter} Upload`, 'Mbps', n.upMbps, true, 'os'),
    ]),
    sensor('network.ping', 'network', 'Ping', 'ms', s.ping.rttMs, true, 'os'),
    sensor('power.gpu', 'power', 'GPU Power (all)', 'W', s.power.gpuW, true, 'app'),
  ];
  const groups = GROUP_ORDER.map((group) => ({ group, sensors: list.filter((x) => x.group === group) })).filter((g) => g.sensors.length);
  return { groups };
}

/** Same bucket width as the engine's `HistoryRange::step_secs`. */
const RANGE: Record<HistoryRange, { secs: number; res: number }> = {
  '5m': { secs: 300, res: 1 },
  '30m': { secs: 1800, res: 1 },
  '1h': { secs: 3600, res: 10 },
  '6h': { secs: 21600, res: 10 },
  '24h': { secs: 86400, res: 10 },
  '7d': { secs: 604800, res: 60 },
};

export function stepSecs(range: HistoryRange): number {
  const { secs, res } = RANGE[range];
  const step = Math.max(res, Math.ceil(secs / 600));
  return Math.ceil(step / res) * res;
}

/** Typical level and swing per metric, for the browser preview only. */
function profile(id: string): [number, number] {
  if (id.endsWith('.usage') || id === 'cpu.usage.total') return [25, 18];
  if (id.includes('dimm') || id.startsWith('memory.dimm')) return [39, 2];
  if (id.includes('.temp')) return [id.startsWith('storage') ? 38 : 48, 8];
  if (id.includes('clock')) return [id.startsWith('gpu') ? 1400 : 3800, 900];
  if (id.includes('power')) return [45, 25];
  if (id.includes('vram')) return [3000, 900];
  if (id === 'memory.used') return [14000, 1500];
  if (id.startsWith('fan.')) return [1400, 250];
  if (id === 'cpu.vid') return [1.15, 0.12];
  if (id.startsWith('voltage.')) return [1.2, 0.02];
  if (id === 'network.ping') return [11, 2];
  if (id.startsWith('network.')) return [3, 3];
  return [10, 5];
}

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return (h >>> 0) / 2 ** 32;
}

/** A plausible, deterministic series so the History view can be previewed without the engine. */
export function simHistory(metric: string, range: HistoryRange, now = Math.floor(Date.now() / 1000)): HistorySeries {
  const step = stepSecs(range);
  const [base, swing] = profile(metric);
  const seed = hash(metric) * 100;
  const end = Math.floor(now / step) * step;
  const start = end - RANGE[range].secs;
  const points = [];
  for (let ts = start + step; ts <= end; ts += step) {
    // App "closed" for a stretch each night, so long ranges show a gap.
    const hour = new Date(ts * 1000).getHours();
    if (RANGE[range].secs >= 86400 && hour >= 2 && hour < 7) continue;
    const t = ts / 60;
    const load = 0.5 + 0.35 * Math.sin(t / 37 + seed) + 0.15 * Math.sin(t / 5.3 + seed * 2);
    const avg = Math.max(0, base + swing * (load - 0.5) * 2);
    const spread = swing * 0.15 * (step > 1 ? 1 : 0);
    points.push({ ts, avg, min: Math.max(0, avg - spread), max: avg + spread });
  }
  return { metric, range, stepSecs: step, points };
}

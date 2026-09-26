// Browser-only stand-in for the engine's sensor registry: derives a sensor tree from the
// simulated snapshot and tracks min / max / avg the way the Rust registry does.

import type { Group, Sensor, SensorSnapshot, SensorTree, Source, Unit } from './types';

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

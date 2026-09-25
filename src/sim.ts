// Browser-only stand-in for the Rust engine, so the UI runs under plain `vite dev` and in
// headless browser tests. The desktop app always uses the real pipeline (the Rust SimProvider
// on non-Windows hosts).

import type { SensorSnapshot } from './types';

/** Every field present and unavailable, mirroring what the Rust side serializes. */
export function emptySnapshot(ts: number): SensorSnapshot {
  return {
    ts,
    cpu: {
      usageTotal: null, usagePerCore: [], mhzCurrent: null, mhzPerCore: [], mhzEffective: null,
      multiplier: null, bclkMhz: null, mhzBase: null, mhzBoost: null, tempPackage: null,
      tempPerCore: [], powerPkgW: null, powerCoreW: null, powerSocW: null, voltagePkg: null,
      voltageCore: null, throttleThermal: null, throttlePowerLimit: null, throttleCurrentLimit: null,
    },
    gpu: [],
    memory: {
      totalMB: null, usedMB: null, availableMB: null, cachedMB: null, committedMB: null,
      commitLimitMB: null, pageFileUsedMB: null, compressedMB: null, pageFaultsPerSec: null,
      channels: null, dimms: [],
    },
    storage: [],
    board: {
      tempChipset: null, tempVrm: null, tempVrmMos: null, tempPch: null, tempSocket: null,
      tempPcie: null, tempM2: null,
    },
    fans: [],
    voltages: [],
    network: [],
    power: { estTotalW: null, cpuW: null, gpuW: null, ramW: null, storageW: null },
    processes: [],
    app: {
      cpuPct: null, ramMB: null, gpuPct: null, pollHz: null, uiFps: null, activeSensors: 0,
      engine: 'starting',
    },
  };
}

interface SimState {
  cpu: number;
  cpuTarget: number;
  gpu: number;
  gpuTarget: number;
  cpuTemp: number;
  gpuTemp: number;
  mem: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const pickLoad = (rand: () => number) => (rand() < 0.25 ? 55 + rand() * 45 : 3 + rand() * 22);

export function initialSimState(): SimState {
  return { cpu: 10, cpuTarget: 10, gpu: 5, gpuTarget: 5, cpuTemp: 40, gpuTemp: 36, mem: 12000 };
}

/** Advances the simulation one tick and returns the resulting snapshot. */
export function nextSnapshot(st: SimState, ts: number, pollHz: number, rand: () => number = Math.random): SensorSnapshot {
  if (rand() < 0.08) st.cpuTarget = pickLoad(rand);
  if (rand() < 0.08) st.gpuTarget = pickLoad(rand);
  st.cpu = clamp(st.cpu + (st.cpuTarget - st.cpu) * 0.35 + (rand() - 0.5) * 6, 0.5, 100);
  st.gpu = clamp(st.gpu + (st.gpuTarget - st.gpu) * 0.3 + (rand() - 0.5) * 5, 0, 100);
  st.cpuTemp += (36 + st.cpu * 0.52 - st.cpuTemp) * 0.15;
  st.gpuTemp += (33 + st.gpu * 0.42 - st.gpuTemp) * 0.12;
  st.mem = clamp(st.mem + (rand() - 0.5) * 120, 4000, 31000);

  const s = emptySnapshot(ts);
  const cpuW = 11 + st.cpu * 1.35;
  const gpuW = 16 + st.gpu * 1.9;
  const mhz = 2400 + (st.cpu / 100) * 3000 + (rand() - 0.5) * 120;
  s.cpu.usageTotal = st.cpu;
  s.cpu.mhzCurrent = mhz;
  s.cpu.mhzBase = 3600;
  s.cpu.mhzBoost = 5400;
  s.cpu.tempPackage = st.cpuTemp;
  s.cpu.powerPkgW = cpuW;
  s.gpu.push({
    name: 'Simulated GPU (browser)',
    vendor: 'Simulated',
    integrated: false,
    usage: st.gpu,
    engine: { graphics3d: st.gpu, decode: 0, encode: 0, compute: 0, copy: 0, tensor: null, raytrace: null },
    mhzCore: 210 + (st.gpu / 100) * 2500,
    mhzMem: st.gpu > 8 ? 10501 : 405,
    tempEdge: st.gpuTemp,
    tempHotspot: st.gpuTemp + 8 + st.gpu * 0.1,
    tempMem: null,
    vramUsedMB: 1100 + st.gpu * 75,
    vramTotalMB: 12288,
    powerW: gpuW,
    powerLimitW: 200,
    voltageV: null,
    fanPct: st.gpuTemp < 50 ? 0 : clamp((st.gpuTemp - 50) * 3 + 30, 0, 100),
    fanRpm: null,
    pcieGen: 4,
    pcieWidth: 16,
    throttleThermal: false,
    throttlePowerLimit: false,
    throttleReasons: [],
  });
  s.memory.totalMB = 32768;
  s.memory.usedMB = st.mem;
  s.memory.availableMB = 32768 - st.mem;
  s.fans.push({
    name: 'CPU_FAN', rpm: 450 + Math.max(0, st.cpuTemp - 30) * 22, pct: null, pwm: null,
    minRpm: 300, maxRpm: 2000, kind: 'fan', status: 'ok',
  });
  s.power = { estTotalW: cpuW + gpuW + 45, cpuW, gpuW, ramW: null, storageW: null };
  s.app.pollHz = pollHz;
  s.app.activeSensors = 14;
  s.app.engine = 'running';
  return s;
}

/** Emits a simulated snapshot immediately and then every `intervalMs`. Returns a stop function. */
export function startSim(cb: (s: SensorSnapshot) => void, intervalMs = 1000): () => void {
  const st = initialSimState();
  const tick = () => cb(nextSnapshot(st, Math.floor(Date.now() / 1000), 1000 / intervalMs));
  tick();
  const id = setInterval(tick, intervalMs);
  return () => clearInterval(id);
}

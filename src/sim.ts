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
      commitLimitMB: null, pageFileUsedMB: null, pageFileTotalMB: null, standbyMB: null, modifiedMB: null,
      freeMB: null, pagedPoolMB: null, nonPagedPoolMB: null, compressedMB: null, pageFaultsPerSec: null,
      hardFaultsPerSec: null, channels: null, dimms: [],
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

const SIM_PROCS: Array<[string, number, number, number, number]> = [
  ['game.exe', 9120, 0.55, 5200, 0.9],
  ['chrome.exe', 4412, 0.15, 1350, 0.04],
  ['chrome.exe', 4588, 0.05, 620, 0],
  ['Discord.exe', 7004, 0.04, 410, 0.02],
  ['MsMpEng.exe', 3120, 0.03, 290, 0],
  ['dwm.exe', 1308, 0.02, 95, 0.03],
  ['explorer.exe', 6020, 0.01, 180, 0],
  ['vyro.exe', 8800, 0.004, 42, 0.001],
];

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
    engine: { graphics3d: st.gpu, decode: 2, encode: st.gpu > 50 ? 3 : 0, compute: 0, copy: st.gpu * 0.05, tensor: null, raytrace: null },
    mhzCore: 210 + (st.gpu / 100) * 2500,
    mhzMem: st.gpu > 8 ? 10501 : 405,
    tempEdge: st.gpuTemp,
    tempHotspot: st.gpuTemp + 8 + st.gpu * 0.1,
    tempMem: st.gpuTemp + 6,
    vramUsedMB: 1100 + st.gpu * 75,
    vramTotalMB: 12288,
    powerW: gpuW,
    powerLimitW: 200,
    voltageV: 0.7 + st.gpu * 0.0028,
    fanPct: st.gpuTemp < 50 ? 0 : clamp((st.gpuTemp - 50) * 3 + 30, 0, 100),
    fanRpm: st.gpuTemp < 50 ? 0 : 900 + (st.gpuTemp - 50) * 40,
    pcieGen: 4,
    pcieWidth: 16,
    throttleThermal: false,
    throttlePowerLimit: false,
    throttleReasons: [],
  });
  for (let i = 0; i < 16; i++) {
    const bias = Math.sin(i * 2.3);
    s.cpu.usagePerCore.push(clamp(st.cpu + bias * 12 + (rand() - 0.5) * 16, 0, 100));
    s.cpu.mhzPerCore.push(mhz + bias * 150);
  }
  s.processes = SIM_PROCS.map(([name, pid, c, ram, g]) => ({
    pid,
    name,
    cpuPct: Math.min(100, st.cpu * c * (0.8 + rand() * 0.4)),
    ramMB: ram * (0.97 + rand() * 0.06),
    gpuPct: Math.min(100, st.gpu * g),
  }));
  s.memory.totalMB = 32768;
  s.memory.usedMB = st.mem;
  s.memory.availableMB = 32768 - st.mem;
  Object.assign(s.memory, {
    committedMB: st.mem + 3100,
    commitLimitMB: 37888,
    cachedMB: 9800,
    standbyMB: 9100,
    modifiedMB: 700,
    freeMB: Math.max(0, 32768 - st.mem - 9100),
    pagedPoolMB: 1100,
    nonPagedPoolMB: 610,
    pageFileUsedMB: 850,
    pageFileTotalMB: 5120,
    compressedMB: 380,
    pageFaultsPerSec: 900 + rand() * 1500,
    hardFaultsPerSec: rand() < 0.2 ? rand() * 60 : 0,
  });
  const io = (busy: number) => (rand() < busy ? rand() * 400 : rand() * 2);
  const rd = io(0.2);
  const wr = io(0.15);
  s.storage = [
    {
      id: 'disk0', model: 'Samsung SSD 980 PRO 1TB', volumes: 'C:', health: 'ok', type: 'NVMe',
      tempC: 41 + (rd + wr) * 0.02, tempControllerC: null, tempNandC: null, healthPct: 98,
      capacityMB: 953870, usedMB: 440820, freeMB: 512430, readMBs: rd, writeMBs: wr,
      readIops: rd * 60, writeIops: wr * 40, activityPct: Math.min(100, (rd + wr) / 4),
      totalReadGB: 18540, totalWriteGB: 15294, powerOnHours: 3456, powerCycles: 812,
      nvmePctUsed: 2, nvmeAvailableSparePct: 100, nvmeCriticalWarning: 0,
    },
    {
      id: 'disk1', model: 'ST2000DM008-2UB102', volumes: 'D:', health: 'warn', type: 'SATA',
      tempC: 34, tempControllerC: null, tempNandC: null, healthPct: null,
      capacityMB: 1907729, usedMB: 885832, freeMB: 1021880, readMBs: rd * 0.1, writeMBs: 0,
      readIops: rd, writeIops: 0, activityPct: Math.min(100, rd / 2), totalReadGB: null,
      totalWriteGB: null, powerOnHours: 13210, powerCycles: 1530, nvmePctUsed: null,
      nvmeAvailableSparePct: null, nvmeCriticalWarning: null,
    },
  ];
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

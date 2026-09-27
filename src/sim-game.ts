// Browser preview of gaming capture: a game near 144 FPS (lower under heavy GPU load) with an
// occasional hitch, shaped like the engine's `GameSnapshot`.

import type { GameSession, GameSnapshot } from './types';

const STARTED = Math.floor(Date.now() / 1000) - 1260;

export function simGame(ts: number, gpuLoad: number, rand: () => number): GameSnapshot {
  const base = 1000 / (150 - gpuLoad * 0.35);
  const recentMs = Array.from({ length: 240 }, (_, i) =>
    (i + ts) % 97 === 0 ? base * 3.2 : base * (0.94 + rand() * 0.12),
  );
  const last = recentMs.slice(-Math.round(1000 / base));
  const frameMs = last.reduce((a, b) => a + b, 0) / last.length;
  const sorted = [...recentMs].sort((a, b) => a - b);
  const at = (p: number) => sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)]!;
  return {
    state: 'capturing',
    detail: null,
    app: 'Cyberpunk2077.exe',
    pid: 9120,
    runtime: 'DXGI',
    presentMode: 'Hardware: Independent Flip',
    fps: 1000 / frameMs,
    frameMs,
    low1Fps: 1000 / at(99),
    low01Fps: 1000 / at(99.9),
    cpuBusyMs: frameMs * 0.52,
    gpuBusyMs: frameMs * 0.94,
    latencyMs: frameMs + 9.5,
    displayedFps: 1000 / frameMs,
    bound: 'gpu',
    started: STARTED,
    frames: (ts - STARTED) * 138,
    sessionFps: 138.4,
    sessionLow1Fps: 96.2,
    recentMs,
  };
}

const now = Math.floor(Date.now() / 1000);
export const SAMPLE_SESSIONS: GameSession[] = [
  {
    app: 'Cyberpunk2077.exe', runtime: 'DXGI', started: now - 90_000, secs: 5_420, frames: 612_000,
    avgFps: 112.9, low1Fps: 78.4, low01Fps: 61.0, maxFrameMs: 48.2, bound: 'gpu',
    cpuTempMax: 78.5, gpuTempMax: 71.0, cpuUsageAvg: 34.0, gpuUsageAvg: 97.1,
  },
  {
    app: 'cs2.exe', runtime: 'Vulkan', started: now - 260_000, secs: 2_890, frames: 1_050_000,
    avgFps: 363.3, low1Fps: 190.1, low01Fps: 131.6, maxFrameMs: 22.9, bound: 'cpu',
    cpuTempMax: 83.0, gpuTempMax: 58.5, cpuUsageAvg: 41.0, gpuUsageAvg: 62.4,
  },
  {
    app: 'Hades2.exe', runtime: 'DXGI', started: now - 400_000, secs: 3_600, frames: 432_000,
    avgFps: 120.0, low1Fps: 116.3, low01Fps: 104.2, maxFrameMs: 12.5, bound: 'capped',
    cpuTempMax: 61.0, gpuTempMax: 49.0, cpuUsageAvg: 9.0, gpuUsageAvg: 28.0,
  },
];

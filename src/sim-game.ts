// Browser preview of gaming capture: a game near 144 FPS (lower under heavy GPU load) with an
// occasional hitch, shaped like the engine's `GameSnapshot`.

import type { GameSession, GameSnapshot } from './types';

const STARTED = Math.floor(Date.now() / 1000) - 1260;

export function simGame(ts: number, gpuLoad: number, rand: () => number): GameSnapshot {
  const base = 1000 / (150 - gpuLoad * 0.35);
  // 100 ms buckets on a fixed clock: each keeps its value as the graph scrolls.
  const end = Math.floor(Date.now() / 100);
  const timeline = Array.from({ length: 150 }, (_, i) => {
    const k = end - 150 + i;
    const wobble = Math.sin(k * 0.21) * 0.25 + Math.sin(k * 0.047) * 0.35;
    const avgMs = base + wobble;
    const hitch = k % 53 === 0;
    return { avgMs: hitch ? avgMs * 1.35 : avgMs, maxMs: hitch ? base * 3.2 : avgMs + 0.6 + Math.abs(Math.sin(k)) * 0.8 };
  });
  const recent = timeline.slice(-10);
  const frameMs = recent.reduce((a, p) => a + p.avgMs, 0) / recent.length;
  const maxes = timeline.map((p) => p.maxMs).sort((a, b) => a - b);
  void rand;
  return {
    state: 'capturing',
    detail: null,
    app: 'Cyberpunk2077.exe',
    pid: 9120,
    runtime: 'DXGI',
    presentMode: 'Hardware: Independent Flip',
    fps: 1000 / frameMs,
    frameMs,
    low1Fps: 1000 / maxes[Math.floor(maxes.length * 0.9)]!,
    low01Fps: 1000 / maxes[maxes.length - 1]!,
    cpuBusyMs: frameMs * 0.52,
    gpuBusyMs: frameMs * 0.94,
    latencyMs: frameMs + 9.5,
    displayedFps: 1000 / frameMs,
    bound: 'gpu',
    started: STARTED,
    frames: (ts - STARTED) * 138,
    sessionFps: 138.4,
    sessionLow1Fps: 96.2,
    timeline,
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

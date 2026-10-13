// Browser preview of session replay: a timeline for each simulated session that has one, shaped
// like the service's `GameTimeline` (already downsampled to at most 1500 samples), with a
// frame-rate dip, stutters, limits, an alt-tab, a profile change and the overlay coming on.

import { SAMPLE_SESSIONS } from './sim-game';
import type { GameEvent, GameSample, GameTimeline, Marker } from './types';

const r1 = (v: number) => Math.round(v * 10) / 10;

export function simTimeline(id: number): GameTimeline | null {
  const session = SAMPLE_SESSIONS.find((s) => s.id === id);
  if (!session?.hasTimeline) return null;
  const total = session.secs * 1000;
  const stepMs = Math.max(1000, Math.ceil(total / 1500));
  const base = session.avgFps ?? 100;
  const gpuHeavy = session.bound === 'gpu';
  const at = (share: number) => Math.round((total * share) / stepMs) * stepMs;
  const left = at(0.5);
  const back = at(0.52);
  const dip = at(0.25);
  const stutters = [at(0.15), at(0.38), at(0.77)];
  const silent = at(0.6);

  const samples: GameSample[] = [];
  for (let t = 0; t <= total; t += stepMs) {
    if (t > left && t < back) continue; // switched away: no frames
    const k = t / 1000;
    const wobble = Math.sin(k / 47) * 0.06 + Math.sin(k / 61) * 0.04;
    const quiet = t >= silent ? 0.9 : 1; // the Silent profile costs some frames
    const fps = (t >= dip && t < dip + 40_000 ? base * 0.48 : base * (1 + wobble)) * quiet;
    const frameMs = 1000 / fps;
    const hitch = stutters.some((s) => Math.abs(s - t) < stepMs / 2);
    const warm = 1 - Math.exp(-k / 900);
    samples.push({
      t,
      fps: r1(fps),
      fpsMin: r1(fps * (hitch ? 0.35 : 0.82)),
      frameMs: r1(frameMs * 100) / 100,
      frameMaxMs: r1(hitch ? 48 + (k % 7) : frameMs * 1.6),
      cpuUsage: r1((session.cpuUsageAvg ?? 30) + Math.sin(k / 130) * 6),
      gpuUsage: r1(Math.min(99, (session.gpuUsageAvg ?? 80) + Math.sin(k / 170) * 2)),
      cpuTemp: r1(52 + ((session.cpuTempMax ?? 75) - 52) * warm + Math.sin(k / 230)),
      gpuTemp: r1(45 + ((session.gpuTempMax ?? 70) - 45) * warm + Math.sin(k / 310)),
      cpuPower: r1((gpuHeavy ? 78 : 110) * quiet + Math.sin(k / 110) * 6),
      gpuPower: r1((gpuHeavy ? 196 : 140) * quiet + Math.sin(k / 190) * 4),
    });
  }

  const ev = (t: number, marker: Marker): GameEvent => ({ t, marker });
  const events: GameEvent[] = [
    ev(at(0.02), { kind: 'overlay', shown: true }),
    ev(stutters[0]!, { kind: 'stutter', ms: 49, usual: r1(1000 / base) }),
    ev(dip, { kind: 'fpsDrop', fps: r1(base * 0.48), usual: r1(base) }),
    ...(gpuHeavy ? [ev(at(0.31), { kind: 'powerLimit', part: 'gpu' })] : []),
    ev(stutters[1]!, { kind: 'stutter', ms: 53, usual: r1(1000 / base) }),
    ev(left, { kind: 'left' }),
    ev(back, { kind: 'back' }),
    ev(silent, { kind: 'profile', profile: 'silent', auto: false }),
    ev(at(0.7), { kind: 'thermal', part: 'cpu' }),
    ev(stutters[2]!, { kind: 'stutter', ms: 50, usual: r1(1000 / base) }),
  ];
  return { session, stepMs, samples, events };
}

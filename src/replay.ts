// Game session replay (P15): samples as chart points, what each marker means, and which
// recorded sensors give the context of a moment.

import { fmt } from './format';
import type { ChartDef } from './presets';
import type { GameSample, GameTimeline, HistoryPoint, Marker, Part, Profile } from './types';

export type MarkerGroup = 'frames' | 'limits' | 'you';

export const GROUPS: Array<{ id: MarkerGroup; label: string; color: string }> = [
  { id: 'frames', label: 'Frame rate', color: 'var(--hot)' },
  { id: 'limits', label: 'Limits', color: 'var(--warn)' },
  { id: 'you', label: 'You', color: 'var(--accent)' },
];

/** Seconds of sensor history either side of a moment. */
export const CONTEXT_SECS = 60;

/** Unix seconds of a moment `t` ms into the session. */
export const tsOf = (tl: GameTimeline, t: number): number => tl.session.started + t / 1000;

/** The session's span on the time axis, unix seconds. */
export function span(tl: GameTimeline): [number, number] {
  const last = tl.samples[tl.samples.length - 1];
  const end = Math.max(tl.session.ended ?? 0, last ? tsOf(tl, last.t) : 0, tl.session.started + tl.session.secs);
  return [tl.session.started, Math.max(end, tl.session.started + 1)];
}

type Pick = (s: GameSample) => number | null;

/** Chart points: `avg` with a band from `min` to `max` (each defaulting to `avg`); samples
 * without the reading are left out, so the line breaks there. */
export function points(tl: GameTimeline, avg: Pick, min: Pick = avg, max: Pick = avg): HistoryPoint[] {
  const out: HistoryPoint[] = [];
  for (const s of tl.samples) {
    const a = avg(s);
    if (a == null) continue;
    out.push({ ts: tsOf(tl, s.t), avg: a, min: Math.min(a, min(s) ?? a), max: Math.max(a, max(s) ?? a) });
  }
  return out;
}

/** The sample nearest to unix second `ts`. */
export function sampleAt(tl: GameTimeline, ts: number): GameSample | null {
  const t = (ts - tl.session.started) * 1000;
  let best: GameSample | null = null;
  for (const s of tl.samples) {
    if (!best || Math.abs(s.t - t) < Math.abs(best.t - t)) best = s;
  }
  return best;
}

/** "12:34" or "1:02:03" into the session. */
export function clock(ms: number): string {
  const secs = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = String(secs % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

export interface MarkerInfo {
  group: MarkerGroup;
  title: string;
  text: string;
}

const PART: Record<Part, string> = { cpu: 'CPU', gpu: 'GPU' };

/** What a marker means, in words. */
export function markerInfo(m: Marker, profileName: (p: Profile) => string): MarkerInfo {
  switch (m.kind) {
    case 'fpsDrop':
      return { group: 'frames', title: 'Frame rate drop', text: `Fell to ${fmt(m.fps)} FPS, from a usual ${fmt(m.usual)} FPS.` };
    case 'stutter':
      return { group: 'frames', title: 'Stutter', text: `A frame took ${fmt(m.ms)} ms; frames usually took ${fmt(m.usual, 1)} ms.` };
    case 'thermal':
      return {
        group: 'limits',
        title: `${PART[m.part]} temperature limit`,
        text: `The ${m.part === 'cpu' ? 'processor' : 'graphics card'} slowed down to stay within its temperature limit.`,
      };
    case 'powerLimit':
      return {
        group: 'limits',
        title: `${PART[m.part]} power limit`,
        text:
          m.part === 'gpu'
            ? 'The graphics card held its clocks at its power limit, as most do under full load.'
            : 'The processor held its clocks at its power limit.',
      };
    case 'throttle':
      return { group: 'limits', title: `${PART[m.part]} clock limit`, text: `Clocks held back: ${m.reason}.` };
    case 'left':
      return { group: 'you', title: 'Switched away', text: 'Another window came to the front.' };
    case 'back':
      return { group: 'you', title: 'Back in the game', text: 'The game came back to the front.' };
    case 'profile':
      return {
        group: 'you',
        title: `${profileName(m.profile)} profile`,
        text: m.auto ? 'Chosen for the game automatically.' : 'Picked by you.',
      };
    case 'overlay':
      return { group: 'you', title: m.shown ? 'Overlay shown' : 'Overlay hidden', text: m.shown ? 'The overlay came on.' : 'The overlay went off.' };
  }
}

/** History charts for the context of a moment, in this order. */
const CONTEXT = ['usage', 'clocks', 'power', 'temps', 'memory', 'disks', 'network'];
/** The engine answers at most this many readings per request. */
export const MAX_CONTEXT_METRICS = 24;

/** The History tab's charts that give a moment its context, at most 4 readings each and
 * [`MAX_CONTEXT_METRICS`] in all. */
export function contextCharts(charts: ChartDef[]): ChartDef[] {
  let left = MAX_CONTEXT_METRICS;
  const out: ChartDef[] = [];
  for (const key of CONTEXT) {
    const c = charts.find((x) => x.key === key);
    if (!c || left <= 0) continue;
    const metrics = c.metrics.slice(0, Math.min(4, left));
    left -= metrics.length;
    out.push({ ...c, metrics });
  }
  return out;
}

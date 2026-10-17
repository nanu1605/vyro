import { describe, expect, it } from 'vitest';
import { inBox, laptopAirflow, towerAirflow, type Streamline } from './airflow';
import { cardKey } from './gpuspecs';
import { buildLaptop, buildPc, CASE, type Part, type Vec3 } from './pcmodel';
import { sampleHardware, sampleLaptop } from './sample';
import { initialSimState, nextSnapshot } from './sim';

const snap = nextSnapshot(initialSimState(), 1_700_000_000, 1, () => 0.5);
const aio = buildPc({ hw: sampleHardware, snap, cardKey });
const tower = buildPc({ hw: sampleHardware, snap: { ...snap, fans: snap.fans.filter((f) => f.kind !== 'pump') }, cardKey });
const [W, H] = CASE;

const box = (p: Part) => ({
  min: [0, 1, 2].map((a) => p.pos[a]! - p.size[a]! / 2) as Vec3,
  max: [0, 1, 2].map((a) => p.pos[a]! + p.size[a]! / 2) as Vec3,
});
const end = (l: Streamline) => l.points[l.points.length - 1]!;
/** Out of the back by the rear fan, or out of the top through the radiator. */
const left = (p: Vec3) => p[0] < 4 || p[1] > H - 4;

describe('airflow', () => {
  for (const [name, parts] of [
    ['with an AIO and a top radiator', aio],
    ['with a tower cooler', tower],
  ] as const) {
    describe(name, () => {
      const lines = towerAirflow(parts);

      it('starts at the front fans', () => {
        expect(lines.length).toBeGreaterThanOrEqual(12);
        const intakes = parts.filter((p) => p.kind === 'fan' && p.pos[0] > W / 2);
        for (const l of lines) {
          const s = l.points[0]!;
          expect(intakes.some((f) => Math.abs(s[1] - f.pos[1]) < 60 && Math.abs(s[2] - f.pos[2]) < 60 && s[0] > 400)).toBe(true);
        }
      });

      it('goes round the parts, never through them', () => {
        const solid = parts.filter((p) => p.kind === 'gpu' || p.kind === 'aio' || p.kind === 'ram').map(box);
        for (const l of lines)
          for (const p of l.points) {
            for (const b of solid) expect(inBox(b, p, 1), `${p} in a part`).toBe(false);
            // Above the PSU shroud, and inside the case unless on the way out.
            expect(p[1]).toBeGreaterThan(100);
            if (!left(p)) {
              expect(p[0]).toBeLessThan(W);
              expect(p[2]).toBeGreaterThan(0);
              expect(p[2]).toBeLessThan(CASE[2]);
            }
          }
      });

      it('mostly leaves by an exhaust', () => {
        const out = lines.filter((l) => left(end(l))).length;
        expect(out / lines.length).toBeGreaterThanOrEqual(0.85);
      });

      it('warms on the way, never cools', () => {
        for (const l of lines)
          for (let i = 1; i < l.heat.length; i++) {
            expect(l.heat[i]![0]).toBeGreaterThanOrEqual(l.heat[i - 1]![0]);
            expect(l.heat[i]![1]).toBeGreaterThanOrEqual(l.heat[i - 1]![1]);
          }
        // Some air passes the CPU's cooler (or radiator), some the graphics card.
        expect(lines.some((l) => l.heat.at(-1)![0] > 0.4)).toBe(true);
        expect(lines.some((l) => l.heat.at(-1)![1] > 0.4)).toBe(true);
      });

      it('is the same every time', () => {
        expect(JSON.stringify(towerAirflow(parts))).toBe(JSON.stringify(lines));
      });
    });
  }

  it('leaves through the radiator and the rear fan both, with an AIO', () => {
    const ends = towerAirflow(aio).map(end);
    expect(ends.some((p) => p[1] > H - 4)).toBe(true);
    expect(ends.some((p) => p[0] < 4)).toBe(true);
  });

  it('has nothing to draw without fans', () => {
    expect(towerAirflow(aio.filter((p) => p.kind !== 'fan'))).toEqual([]);
  });

  it('runs through each of a laptop’s blowers and out of the back', () => {
    const parts = buildLaptop({ hw: sampleLaptop, snap, cardKey });
    const lines = laptopAirflow(parts);
    expect(lines.length).toBe(parts.filter((p) => p.kind === 'blower').length * 5);
    for (const l of lines) {
      expect(end(l)[2]).toBeLessThan(0);
      expect(l.along.at(-1)).toBeGreaterThan(100);
    }
    // The CPU's blower warms with the CPU, the GPU's with the GPU.
    expect(lines.filter((l) => l.heat.at(-1)![0] > 0).length).toBe(5);
  });
});

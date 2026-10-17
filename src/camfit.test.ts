import { describe, expect, it } from 'vitest';
import { Box3, PerspectiveCamera, Vector3 } from 'three';
import { corners, fitDistance } from './camfit';

// The tower's case, as My PC draws it (mm): 460 wide and tall, 220 deep.
const CASE = new Box3(new Vector3(0, -17, 0), new Vector3(460, 465, 220));

/** Where each corner lands on screen (normalised, -1 to 1) for a camera fitted this way. */
function fitted(box: Box3, dir: Vector3, aspect: number, margin = 0.9) {
  const target = box.getCenter(new Vector3());
  const camera = new PerspectiveCamera(32, aspect, 5, 6000);
  const d = fitDistance(corners(box), target, dir, camera.up, camera.fov, aspect, margin);
  camera.position.copy(target).addScaledVector(dir.clone().normalize(), d);
  camera.lookAt(target);
  camera.updateMatrixWorld();
  camera.updateProjectionMatrix();
  return corners(box).map((c) => c.project(camera));
}

describe('framing the 3-D view', () => {
  const dirs = [new Vector3(0.57, 0.28, 3.8), new Vector3(0, 0, 1), new Vector3(1, 0.4, 0.2), new Vector3(-0.3, 0.9, 0.4)];

  it('fits every corner of the case, for narrow, square, wide and very wide views', () => {
    for (const dir of dirs)
      for (const aspect of [0.6, 1, 1.78, 3.1]) {
        const ndc = fitted(CASE, dir, aspect);
        for (const p of ndc) {
          expect(Math.abs(p.x)).toBeLessThanOrEqual(0.9 + 1e-6);
          expect(Math.abs(p.y)).toBeLessThanOrEqual(0.9 + 1e-6);
          expect(p.z).toBeLessThan(1);
        }
      }
  });

  it('is tight: some corner reaches the margin', () => {
    for (const dir of dirs)
      for (const aspect of [0.6, 1, 1.78, 3.1]) {
        const ndc = fitted(CASE, dir, aspect);
        const most = Math.max(...ndc.map((p) => Math.max(Math.abs(p.x), Math.abs(p.y))));
        expect(most).toBeGreaterThan(0.88);
      }
  });

  it('stands further back for a wide, short view than for a square one (the case is tall)', () => {
    const t = CASE.getCenter(new Vector3());
    const up = new Vector3(0, 1, 0);
    const dir = new Vector3(0, 0, 1);
    const square = fitDistance(corners(CASE), t, dir, up, 32, 1);
    const wide = fitDistance(corners(CASE), t, dir, up, 32, 3.1);
    // Height decides both, so the same; a narrow view needs more.
    expect(wide).toBeCloseTo(square, 6);
    expect(fitDistance(corners(CASE), t, dir, up, 32, 0.6)).toBeGreaterThan(square);
  });

  it('copes with looking straight down', () => {
    const d = fitDistance(corners(CASE), CASE.getCenter(new Vector3()), new Vector3(0, 1, 0), new Vector3(0, 1, 0), 32, 1.5);
    expect(Number.isFinite(d)).toBe(true);
    expect(d).toBeGreaterThan(0);
  });
});

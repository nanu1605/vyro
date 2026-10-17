// Framing the 3-D view (1.2): how far back the camera stands so that the whole model fits the
// canvas, whatever its shape. Pure (three's vector maths only), so it is unit-tested.

import { Box3, Vector3 } from 'three';

/** A box's eight corners. */
export function corners(box: Box3): Vector3[] {
  const out: Vector3[] = [];
  for (const x of [box.min.x, box.max.x]) for (const y of [box.min.y, box.max.y]) for (const z of [box.min.z, box.max.z]) out.push(new Vector3(x, y, z));
  return out;
}

/**
 * The distance from `target`, along `dir` (from the target towards the camera), at which a camera
 * looking at `target` with a vertical field of view of `vfov` degrees and the given aspect sees
 * every point, each inside `margin` of the half view (0.9: a tenth of the way in from the edge).
 */
export function fitDistance(points: readonly Vector3[], target: Vector3, dir: Vector3, up: Vector3, vfov: number, aspect: number, margin = 0.9): number {
  const forward = dir.clone().normalize().negate();
  let right = new Vector3().crossVectors(forward, up);
  // Looking straight up or down: any sideways axis will do.
  if (right.lengthSq() < 1e-9) right = new Vector3(1, 0, 0);
  right.normalize();
  const camUp = new Vector3().crossVectors(right, forward).normalize();
  const ty = Math.tan((vfov * Math.PI) / 360) * margin;
  const tx = ty * aspect;
  let d = 0;
  const v = new Vector3();
  for (const p of points) {
    v.copy(p).sub(target);
    // Depth past the target, and the sideways and upward offsets from the line of sight.
    const z = v.dot(forward);
    d = Math.max(d, Math.abs(v.dot(right)) / tx - z, Math.abs(v.dot(camUp)) / ty - z);
  }
  return d;
}

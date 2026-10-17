// The airflow in My PC's 3-D view (1.2, after the first field check asked for it to look like a
// wind tunnel's smoke lines): streamlines traced through a simple flow field built from the parts.
// Air comes in at the front fans, slides round the graphics card, the RAM and the PSU shroud, and
// leaves by the rear fan and through the radiator. Pure and deterministic, so it is unit-tested;
// the scene draws the lines (`scene3d.ts`) and colours them by the live temperatures.

import { CASE, type Part, type Vec3 } from './pcmodel';

/** One smoke line: its points, the distance along it to each (mm), and how much of the CPU's and
 * the GPU's heat the air has taken up by then (0 to 1 each). */
export interface Streamline {
  points: Vec3[];
  along: number[];
  heat: Array<[number, number]>;
}

interface Fan {
  /** Centre, the way the air goes through it, and its radius. */
  c: Vec3;
  n: Vec3;
  r: number;
  /** An exhaust's share of the pull. */
  w?: number;
}

interface Box {
  min: Vec3;
  max: Vec3;
}

const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a: Vec3, k: number): Vec3 => [a[0] * k, a[1] * k, a[2] * k];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const len = (a: Vec3) => Math.sqrt(dot(a, a));
const unit = (a: Vec3): Vec3 => {
  const l = len(a);
  return l > 1e-9 ? mul(a, 1 / l) : [0, 0, 0];
};

const boxOf = (p: Part, grow = 0): Box => ({
  min: [p.pos[0] - p.size[0] / 2 - grow, p.pos[1] - p.size[1] / 2 - grow, p.pos[2] - p.size[2] / 2 - grow],
  max: [p.pos[0] + p.size[0] / 2 + grow, p.pos[1] + p.size[1] / 2 + grow, p.pos[2] + p.size[2] / 2 + grow],
});

/** How far a point is outside a box (0 inside). */
function outside(b: Box, p: Vec3): number {
  let s = 0;
  for (let a = 0; a < 3; a++) {
    const d = Math.max(b.min[a]! - p[a]!, 0, p[a]! - b.max[a]!);
    s += d * d;
  }
  return Math.sqrt(s);
}

export const inBox = (b: Box, p: Vec3, slack = 0) => [0, 1, 2].every((a) => p[a]! > b.min[a]! + slack && p[a]! < b.max[a]! - slack);

/** The face of a box nearest a point near it, as its outward normal. */
function face(b: Box, p: Vec3): Vec3 {
  let best = Infinity;
  let n: Vec3 = [0, 1, 0];
  for (let a = 0; a < 3; a++) {
    for (const [edge, sign] of [
      [b.min[a]!, -1],
      [b.max[a]!, 1],
    ] as const) {
      const d = Math.abs(p[a]! - edge);
      if (d < best) {
        best = d;
        n = [0, 0, 0];
        n[a] = sign;
      }
    }
  }
  return n;
}

/** Without the part of `v` going into a surface with outward normal `n`, weighted by `w`. */
function slide(v: Vec3, n: Vec3, w: number): Vec3 {
  const into = dot(v, n);
  return into < 0 ? sub(v, mul(n, into * w)) : v;
}

interface Field {
  intakes: Fan[];
  exhausts: Fan[];
  /** Fans inside the case that push air along (a tower cooler's). */
  boosters: Fan[];
  obstacles: Box[];
  /** The inside of the case or base, and where air leaves it. */
  room: Box;
  /** Where the CPU's and the GPU's heat goes into the air. */
  cpuHeat: Box[];
  gpuHeat: Box[];
}

/** The way the air goes at `p` (not normalised). */
function velocity(f: Field, p: Vec3): Vec3 {
  let v: Vec3 = [0, 0, 0];
  for (const fan of f.intakes) {
    const d = sub(p, fan.c);
    const a = dot(d, fan.n);
    const r = len(sub(d, mul(fan.n, a)));
    // A jet straight in that widens and weakens, and a source spreading out from the fan.
    if (a > -15) {
      const spread = fan.r * (1 + Math.max(0, a) / 220);
      v = add(v, mul(fan.n, 1.6 * Math.exp(-(r * r) / (spread * spread)) * Math.exp(-Math.max(0, a) / 280)));
    }
    const dd = dot(d, d) + fan.r * fan.r;
    v = add(v, mul(d, 12000 / (dd * Math.sqrt(dd))));
  }
  for (const fan of f.exhausts) {
    // Drawn towards a point a little past it, so that the air goes out rather than round it.
    const d = sub(add(fan.c, mul(fan.n, 70)), p);
    const dd = dot(d, d) + fan.r * fan.r;
    v = add(v, mul(d, (70000 * (fan.w ?? 1)) / (dd * Math.sqrt(dd))));
    // Close to it, drawn straight out through it.
    const a = dot(sub(p, fan.c), fan.n);
    const r = len(sub(sub(p, fan.c), mul(fan.n, a)));
    if (a > -90 && r < fan.r * 1.3) v = add(v, mul(fan.n, 1.4 * Math.exp(-(r * r) / (fan.r * fan.r)) * Math.exp(-Math.max(0, -a) / 70)));
  }
  for (const fan of f.boosters) {
    const d = sub(p, fan.c);
    const a = dot(d, fan.n);
    const r = len(sub(d, mul(fan.n, a)));
    if (a > -60 && a < 160) v = add(v, mul(fan.n, 1.2 * Math.exp(-(r * r) / (fan.r * fan.r)) * Math.exp(-Math.abs(a) / 110)));
  }
  return v;
}

/** Whether a point has left the room through an exhaust. */
function leaving(f: Field, p: Vec3): Fan | null {
  for (const fan of f.exhausts) {
    const d = sub(p, fan.c);
    const a = dot(d, fan.n);
    if (a > -20 && len(sub(d, mul(fan.n, a))) < fan.r * 1.15) return fan;
  }
  return null;
}

/** The step's direction at `p`: the field's, kept out of the parts and inside the walls but for
 * the exhausts. */
function heading(f: Field, p: Vec3): Vec3 {
  let v = unit(velocity(f, p));
  if (len(v) === 0) return v;
  // Lines bend round a part as they near it, and never go in.
  const near = 14;
  for (const b of f.obstacles) {
    const out = outside(b, p);
    if (out < near) v = slide(v, face(b, p), out <= 0.5 ? 1 : 1 - out / near);
  }
  if (!leaving(f, p)) {
    const { min, max } = f.room;
    for (let a = 0; a < 3; a++) {
      const lo = p[a]! - min[a]!;
      const hi = max[a]! - p[a]!;
      const nLo: Vec3 = [0, 0, 0];
      nLo[a] = 1;
      const nHi: Vec3 = [0, 0, 0];
      nHi[a] = -1;
      if (lo < near) v = slide(v, nLo, lo <= 0.5 ? 1 : 1 - lo / near);
      if (hi < near) v = slide(v, nHi, hi <= 0.5 ? 1 : 1 - hi / near);
    }
  }
  return unit(v);
}

const STEP = 3;
const MAX = 900;

/** Follows the air from `seed` until it leaves by an exhaust, stalls, or has gone `MAX` mm. */
function trace(f: Field, seed: Vec3): Streamline {
  const points: Vec3[] = [seed];
  const along = [0];
  const heat: Array<[number, number]> = [[0, 0]];
  let p = seed;
  let s = 0;
  let cpu = 0;
  let gpu = 0;
  let out = 0;
  while (s < MAX) {
    // Midpoint (RK2) step.
    const k1 = heading(f, p);
    if (len(k1) === 0) break;
    const k2 = heading(f, add(p, mul(k1, STEP / 2)));
    const d = len(k2) > 0 ? k2 : k1;
    let q = add(p, mul(d, STEP));
    // Never into a part: stay where the step would have gone in.
    if (f.obstacles.some((b) => inBox(b, q))) {
      q = add(p, mul(slide(d, face(f.obstacles.find((b) => inBox(b, q))!, p), 1), STEP));
      if (f.obstacles.some((b) => inBox(b, q))) break;
    }
    p = q;
    s += STEP;
    // Heat taken up near the CPU's cooler (or radiator) and the GPU.
    const take = (zones: Box[]) => zones.reduce((m, b) => Math.max(m, Math.exp(-((outside(b, p) / 18) ** 2))), 0);
    cpu += take(f.cpuHeat) * STEP;
    gpu += take(f.gpuHeat) * STEP;
    points.push(p);
    along.push(s);
    heat.push([1 - Math.exp(-cpu / 140), 1 - Math.exp(-gpu / 160)]);
    // A little past the exhaust, the line ends.
    const gone = leaving(f, p);
    if (gone && dot(sub(p, gone.c), gone.n) > 40) break;
    if (!gone && !inBox(f.room, p, -2)) out += STEP;
    if (out > 30) break;
  }
  return { points, along, heat };
}

/** Seeds over a fan's face: two rings, just past it. */
function seeds(fan: Fan, inner: number, outer: number): Vec3[] {
  const helper: Vec3 = Math.abs(fan.n[1]) > 0.9 ? [1, 0, 0] : [0, 1, 0];
  const u = unit([fan.n[1] * helper[2] - fan.n[2] * helper[1], fan.n[2] * helper[0] - fan.n[0] * helper[2], fan.n[0] * helper[1] - fan.n[1] * helper[0]]);
  const w: Vec3 = [fan.n[1] * u[2] - fan.n[2] * u[1], fan.n[2] * u[0] - fan.n[0] * u[2], fan.n[0] * u[1] - fan.n[1] * u[0]];
  const at = add(fan.c, mul(fan.n, 16));
  const out: Vec3[] = [];
  for (const [rr, count, turn] of [
    [0.38, inner, 0],
    [0.78, outer, 0.5],
  ] as const) {
    for (let i = 0; i < count; i++) {
      const t = ((i + turn) / count) * Math.PI * 2;
      out.push(add(at, add(mul(u, Math.cos(t) * fan.r * rr), mul(w, Math.sin(t) * fan.r * rr))));
    }
  }
  return out;
}

/** The tower's airflow from its parts (`buildPc`). */
export function towerAirflow(parts: readonly Part[]): Streamline[] {
  const [W, H, D] = CASE;
  const fans = parts.filter((p) => p.kind === 'fan');
  const face = (p: Part) => Math.min(...p.size) === p.size[0] ? 0 : 1;
  const radius = (p: Part) => Math.max(...p.size) / 2 - 4;
  const intakes: Fan[] = fans.filter((p) => !p.id.startsWith('radfan') && p.id !== 'cooler.fan' && p.pos[0] > W / 2).map((p) => ({ c: p.pos, n: [-1, 0, 0], r: radius(p) }));
  const exhausts: Fan[] = [
    ...fans.filter((p) => !p.id.startsWith('radfan') && p.id !== 'cooler.fan' && p.pos[0] <= W / 2).map((p): Fan => ({ c: [0, p.pos[1], p.pos[2]], n: [-1, 0, 0], r: radius(p) })),
    // The radiator's fans share one pull between them.
    ...fans.filter((p) => p.id.startsWith('radfan')).map((p, _, all): Fan => ({ c: [p.pos[0], H, p.pos[2]], n: [0, 1, 0], r: radius(p), w: 0.8 / all.length })),
  ];
  if (!intakes.length || !exhausts.length) return [];
  const boosters: Fan[] = fans.filter((p) => p.id === 'cooler.fan').map((p) => ({ c: p.pos, n: face(p) === 0 ? [-1, 0, 0] : [0, 1, 0], r: radius(p) }));
  const ram = parts.filter((p) => p.kind === 'ram');
  const obstacles: Box[] = [
    ...parts.filter((p) => p.kind === 'gpu' || p.kind === 'aio').map((p) => boxOf(p, 2)),
    // The sticks as one block: air goes over or round them, not between.
    ...(ram.length
      ? [
          {
            min: [Math.min(...ram.map((p) => boxOf(p).min[0])) - 2, Math.min(...ram.map((p) => boxOf(p).min[1])) - 2, 0] as Vec3,
            max: [Math.max(...ram.map((p) => boxOf(p).max[0])) + 2, Math.max(...ram.map((p) => boxOf(p).max[1])) + 2, Math.max(...ram.map((p) => boxOf(p).max[2])) + 2] as Vec3,
          },
        ]
      : []),
    // The PSU shroud.
    { min: [-50, -50, -50], max: [W + 50, 104, D + 50] },
  ];
  const cooler = parts.find((p) => p.kind === 'cooler');
  const radiator = parts.find((p) => p.kind === 'radiator');
  const field: Field = {
    intakes,
    exhausts,
    boosters,
    obstacles,
    room: { min: [4, 104, 6], max: [W - 4, H - 4, D - 4] },
    cpuHeat: [cooler, radiator].flatMap((p) => (p ? [boxOf(p)] : [])),
    gpuHeat: parts.filter((p) => p.kind === 'gpu').map((p) => boxOf(p)),
  };
  // Lines that stall in a corner short of the way out are left out, unless long enough to read.
  return intakes
    .flatMap((fan) => seeds({ ...fan, c: [fan.c[0] - 14, fan.c[1], fan.c[2]] }, 5, 9).map((s) => trace(field, s)))
    .filter((l) => l.along[l.along.length - 1]! >= 150);
}

/** The laptop's airflow: in under the base, through each blower, out of the back vents. */
export function laptopAirflow(parts: readonly Part[]): Streamline[] {
  const out: Streamline[] = [];
  for (const b of parts.filter((p) => p.kind === 'blower')) {
    const cpuSide = b.id === 'fan.cpu';
    for (const [dx, dy] of [
      [-18, 0],
      [-9, 1.5],
      [0, 0],
      [9, 1.5],
      [18, 0],
    ] as const) {
      const x = b.pos[0] + dx;
      // Under the base, up into the blower, then along the fins and out of the back.
      const path: Vec3[] = [
        [x, -30, 80 - Math.abs(dx)],
        [x, -6, 58],
        [x, 8 + dy, 44],
        [x * 0.85 + b.pos[0] * 0.15, 11 + dy, 22],
        [x, 11 + dy, 4],
        [x, 13 + dy, -40],
      ];
      out.push(polyline(path, (t) => (t < 0.45 ? 0 : Math.min(1, (t - 0.45) / 0.3)), cpuSide));
    }
  }
  return out;
}

/** A smooth line through `path`, sampled every 4 mm; `warm(t)` is the heat taken up at a share
 * `t` of the way along, all of it from the CPU's side or the GPU's. */
function polyline(path: Vec3[], warm: (t: number) => number, cpuSide: boolean): Streamline {
  const points: Vec3[] = [];
  const along: number[] = [];
  const heat: Array<[number, number]> = [];
  // Catmull-Rom through the points.
  const at = (i: number) => path[Math.max(0, Math.min(path.length - 1, i))]!;
  const raw: Vec3[] = [];
  for (let i = 0; i < path.length - 1; i++)
    for (let k = 0; k < 16; k++) {
      const t = k / 16;
      const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)];
      const c = (a: number, b: number, cc: number, d: number) =>
        0.5 * (2 * b + (-a + cc) * t + (2 * a - 5 * b + 4 * cc - d) * t * t + (-a + 3 * b - 3 * cc + d) * t * t * t);
      raw.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1]), c(p0[2], p1[2], p2[2], p3[2])]);
    }
  raw.push(path[path.length - 1]!);
  let s = 0;
  for (const [i, p] of raw.entries()) {
    if (i > 0) s += len(sub(p, raw[i - 1]!));
    points.push(p);
    along.push(s);
  }
  for (const a of along) {
    const w = warm(a / s);
    heat.push(cpuSide ? [w, 0] : [0, w]);
  }
  return { points, along, heat };
}

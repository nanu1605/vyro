// My PC in 3-D (P23, P29): the parts from `pcmodel.ts` drawn with Three.js, as a tower with a
// glass side or as a laptop seen through its keyboard deck. Loaded with the My PC tab and gone
// with it: `dispose` stops the loop and frees the WebGL context.
//
// What moves: fans at their measured speed (slowly when none is reported), RGB that drifts
// around the accent, air from the intakes to the exhausts, a camera that flies in and turns
// slowly while nobody drags it, parts that lift apart in the exploded view. All of it at most
// `maxFps` a second, and only while something moves; with the profile's animation off nothing
// moves and a frame is drawn only when a reading or the view changes.

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/examples/jsm/environments/RoomEnvironment.js';
import { RoundedBoxGeometry } from 'three/examples/jsm/geometries/RoundedBoxGeometry.js';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';
import { mergeGeometries } from 'three/examples/jsm/utils/BufferGeometryUtils.js';
import { corners, fitDistance } from './camfit';
import type { GpuShape } from './gpuspecs';
import { laptopAirflow, towerAirflow } from './airflow';
import { CASE, heatColor, LAPTOP, LID, LID_OPEN, type Part } from './pcmodel';

export type SceneView = 'pc' | 'gpu' | 'laptop';

export interface ScenePalette {
  /** Case edges and empty slots. */
  line: string;
  /** The board. */
  board: string;
  accent: string;
  /** The card behind the canvas (the scene's background when bloom is on). */
  bg: string;
  light: boolean;
}

export interface SceneOptions {
  /** Bloom, the fly-in, slow turning and the RGB drift (the profile's full animation). */
  quality: 'high' | 'low';
  /** Air from the intakes to the exhausts. */
  airflow: boolean;
  /** Parts lifted apart. */
  explode: boolean;
  /** Laptop: see through the keyboard deck. */
  xray: boolean;
}

/** What the laptop's screen shows. */
export interface ScreenInfo {
  rows: Array<[string, string]>;
  /** CPU use, 0–100, for the little graph. */
  load: number | null;
}

export interface PcScene {
  /** The parts to draw (for the GPU view, the card to show). */
  setParts(parts: Part[]): void;
  /** Per part: a colour (thermal map), 'dim' (not in the thermal mode) or nothing (own look). */
  setTint(tint: Map<string, string | 'dim'>): void;
  /** Fan turns per second, by part id. */
  setSpin(spin: Map<string, number>): void;
  /** How hot each part is, 0–1 (`pcmodel.heat`): warm parts glow a little. */
  setHeat(heat: Map<string, number>): void;
  /** At most this many frames a second while something moves; 0: nothing moves. */
  setMaxFps(fps: number): void;
  setOptions(o: Partial<SceneOptions>): void;
  setScreen(info: ScreenInfo): void;
  /** The part under the pointer, with the pointer's place in the canvas. */
  onHover(cb: (id: string | null, x: number, y: number) => void): void;
  /** Fly to a part, or back to the whole view. */
  focus(id: string | null): void;
  /** Label anchors on screen, CSS px from the canvas's top left. */
  project(): Map<string, { x: number; y: number; front: boolean }>;
  /** Called after each frame (labels follow the view). */
  onFrame(cb: () => void): void;
  /** Draw once (a reading changed). */
  invalidate(): void;
  /** Frames drawn so far (for tests). */
  frames(): number;
  dispose(): void;
}

const CARD_COLOURS: Record<GpuShape['colour'], number> = {
  black: 0x1d2128,
  gunmetal: 0x4a4f57,
  silver: 0xb8bec7,
  white: 0xe8eaee,
};
const METAL = 0x9aa3b2;
const DARK = 0x15191f;
const FAN = 0x262b33;
const DIM = 0x3a4250;
const COPPER = 0xc8763e;
const GOLD = 0xd4af37;
const CARD_HEIGHT = 126;

/** Whether this machine can draw WebGL at all (else the 2-D plan is used). */
export function webglAvailable(): boolean {
  try {
    const c = document.createElement('canvas');
    return !!(c.getContext('webgl2') ?? c.getContext('webgl'));
  } catch {
    return false;
  }
}

function mat(color: number, opts: Partial<THREE.MeshStandardMaterialParameters> = {}): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color, roughness: 0.55, metalness: 0.25, ...opts });
}

function boxMesh(size: [number, number, number], m: THREE.Material, at: [number, number, number] = [0, 0, 0]): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(...size), m);
  mesh.position.set(...at);
  return mesh;
}

function rounded(size: [number, number, number], radius: number, m: THREE.Material, at: [number, number, number] = [0, 0, 0]): THREE.Mesh {
  const mesh = new THREE.Mesh(new RoundedBoxGeometry(size[0], size[1], size[2], 3, radius), m);
  mesh.position.set(...at);
  return mesh;
}

function tube(points: THREE.Vector3[], radius: number, m: THREE.Material, segments = 48): THREE.Mesh {
  const curve = new THREE.CatmullRomCurve3(points, false, 'centripetal');
  return new THREE.Mesh(new THREE.TubeGeometry(curve, segments, radius, 10, false), m);
}

// ---------------------------------------------------------------------------------------------
// Textures drawn on canvases (no image files)
// ---------------------------------------------------------------------------------------------

function canvasTexture(w: number, h: number, draw: (ctx: CanvasRenderingContext2D) => void): THREE.CanvasTexture {
  const c = document.createElement('canvas');
  c.width = w;
  c.height = h;
  const ctx = c.getContext('2d')!;
  draw(ctx);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}

/** A circuit board: traces, pads and a little silkscreen. Seeded, so it looks the same each time. */
function boardTexture(base: string): THREE.CanvasTexture {
  let seed = 7;
  const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  return canvasTexture(512, 640, (ctx) => {
    ctx.fillStyle = base;
    ctx.fillRect(0, 0, 512, 640);
    ctx.strokeStyle = 'rgba(255,255,255,0.07)';
    ctx.lineWidth = 2;
    for (let i = 0; i < 90; i++) {
      let x = rnd() * 512;
      let y = rnd() * 640;
      ctx.beginPath();
      ctx.moveTo(x, y);
      for (let k = 0; k < 4; k++) {
        if (rnd() > 0.5) x += (rnd() - 0.5) * 160;
        else y += (rnd() - 0.5) * 160;
        ctx.lineTo(x, y);
      }
      ctx.stroke();
    }
    ctx.fillStyle = 'rgba(255,255,255,0.12)';
    for (let i = 0; i < 160; i++) ctx.fillRect(rnd() * 512, rnd() * 640, 3, 3);
    ctx.fillStyle = 'rgba(255,255,255,0.22)';
    ctx.font = '600 15px sans-serif';
    ctx.fillText('PCIE_X16', 60, 360);
    ctx.fillText('M.2_1', 220, 270);
    ctx.fillText('CPU_FAN', 300, 40);
    ctx.font = '700 22px sans-serif';
    ctx.fillText('VYRO', 380, 600);
  });
}

/** A sleeved cable's weave, repeated along the tube. */
function sleeveTexture(): THREE.CanvasTexture {
  const t = canvasTexture(64, 16, (ctx) => {
    ctx.fillStyle = '#1b1f26';
    ctx.fillRect(0, 0, 64, 16);
    ctx.strokeStyle = '#2e3440';
    ctx.lineWidth = 3;
    for (let x = -16; x < 80; x += 8) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x + 16, 16);
      ctx.stroke();
    }
  });
  t.wrapS = THREE.RepeatWrapping;
  t.wrapT = THREE.RepeatWrapping;
  t.repeat.set(30, 1);
  return t;
}

/** A soft shadow under the case (cheaper than real shadows, and kinder to it). */
function shadowTexture(): THREE.CanvasTexture {
  return canvasTexture(256, 256, (ctx) => {
    const g = ctx.createRadialGradient(128, 128, 10, 128, 128, 128);
    g.addColorStop(0, 'rgba(0,0,0,0.55)');
    g.addColorStop(1, 'rgba(0,0,0,0)');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, 256, 256);
  });
}

// ---------------------------------------------------------------------------------------------
// Fans
// ---------------------------------------------------------------------------------------------

interface Spinner {
  g: THREE.Object3D;
  axis: 'x' | 'y' | 'z';
  /** Blades fade into a blurred disc at speed. */
  blade?: THREE.MeshStandardMaterial;
  disc?: THREE.MeshBasicMaterial;
}

/** One curved, pitched blade between the hub and the tip, facing +z. */
function bladeGeometry(rh: number, rt: number, count: number): THREE.BufferGeometry {
  const span = ((Math.PI * 2) / count) * 0.72;
  const sweep = 0.55;
  const s = new THREE.Shape();
  const n = 8;
  const lead = (t: number) => sweep * Math.pow(t, 1.3);
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const r = rh + (rt - rh) * t;
    const a = lead(t);
    if (i === 0) s.moveTo(r * Math.cos(a), r * Math.sin(a));
    else s.lineTo(r * Math.cos(a), r * Math.sin(a));
  }
  for (let i = n; i >= 0; i--) {
    const t = i / n;
    const r = rh + (rt - rh) * t;
    const a = lead(t) - span * (0.75 + 0.25 * t);
    s.lineTo(r * Math.cos(a), r * Math.sin(a));
  }
  const g = new THREE.ExtrudeGeometry(s, { depth: 1.2, bevelEnabled: false, curveSegments: 2 });
  g.translate(0, 0, -0.6);
  // Pitch the blade about its middle line.
  const mid = sweep * 0.5 - span * 0.85 * 0.5;
  g.rotateZ(-mid);
  g.rotateX(0.42);
  g.rotateZ(mid);
  return g;
}

interface FanOpts {
  /** A square frame (case and radiator fans) or just a ring (graphics cards). */
  frame?: 'square' | 'ring';
  count?: number;
  /** An RGB ring. */
  rgb?: THREE.MeshStandardMaterial | null;
  frameColor?: number;
}

/** A fan facing +z: frame, hub and blades (the blades turn). */
function fan(diameter: number, o: FanOpts = {}): { g: THREE.Group; spin: Spinner } {
  const g = new THREE.Group();
  const r = diameter / 2;
  const count = o.count ?? 7;
  const frameMat = mat(o.frameColor ?? DARK, { roughness: 0.7 });
  if (o.frame === 'square') {
    const t = 5;
    const d = 24;
    g.add(boxMesh([diameter, t, d], frameMat, [0, r - t / 2, 0]), boxMesh([diameter, t, d], frameMat, [0, -r + t / 2, 0]));
    g.add(boxMesh([t, diameter, d], frameMat, [r - t / 2, 0, 0]), boxMesh([t, diameter, d], frameMat, [-r + t / 2, 0, 0]));
    // Struts at the back holding the motor.
    for (let k = 0; k < 4; k++) {
      const arm = new THREE.Group();
      arm.add(boxMesh([r * 0.8, 2.5, 2], frameMat, [r * 0.55, 0, -10]));
      arm.rotation.z = Math.PI / 4 + (k * Math.PI) / 2;
      g.add(arm);
    }
  }
  g.add(new THREE.Mesh(new THREE.TorusGeometry(r - (o.frame === 'square' ? 4 : 2), 1.6, 8, 48), frameMat));
  if (o.rgb) {
    const light = new THREE.Mesh(new THREE.TorusGeometry(r - 5, 0.9, 6, 48), o.rgb);
    light.position.z = 11.5;
    g.add(light);
  }
  const hub = new THREE.Mesh(new THREE.CylinderGeometry(r * 0.3, r * 0.3, 9, 28), mat(FAN, { roughness: 0.4 }));
  hub.rotation.x = Math.PI / 2;
  const cap = new THREE.Mesh(new THREE.CircleGeometry(r * 0.22, 28), mat(0x3a414c, { metalness: 0.6, roughness: 0.3 }));
  cap.position.z = 4.6;
  const blades = new THREE.Group();
  blades.name = 'blades';
  const bladeMat = mat(FAN, { side: THREE.DoubleSide, roughness: 0.45, transparent: true });
  // All the blades in one mesh: one draw call per fan.
  const one = bladeGeometry(r * 0.3, r * 0.9, count);
  const all = mergeGeometries(Array.from({ length: count }, (_, i) => one.clone().rotateZ((i / count) * Math.PI * 2)));
  one.dispose();
  blades.add(new THREE.Mesh(all ?? new THREE.BufferGeometry(), bladeMat));
  const disc = new THREE.MeshBasicMaterial({ color: 0x2a3038, transparent: true, opacity: 0, depthWrite: false });
  const discMesh = new THREE.Mesh(new THREE.RingGeometry(r * 0.3, r * 0.9, 48), disc);
  discMesh.position.z = 0.8;
  g.add(hub, cap, blades, discMesh);
  return { g, spin: { g: blades, axis: 'z', blade: bladeMat, disc } };
}

/** A laptop's blower: a ring of thin blades about the y axis in an open housing. */
function blower(diameter: number, height: number): { g: THREE.Group; spin: Spinner } {
  const g = new THREE.Group();
  const r = diameter / 2;
  const housing = mat(0x23272e, { roughness: 0.6, side: THREE.DoubleSide });
  const wall = new THREE.Mesh(new THREE.CylinderGeometry(r, r, height, 40, 1, true), housing);
  const floor = new THREE.Mesh(new THREE.CircleGeometry(r, 40), housing);
  floor.rotation.x = -Math.PI / 2;
  floor.position.y = -height / 2;
  // The outlet towards the fins at the back.
  const outlet = boxMesh([r * 1.3, height, r * 0.7], housing, [0, 0, -r * 0.75]);
  const blades = new THREE.Group();
  blades.name = 'blades';
  const bladeMat = mat(0x3b424d, { roughness: 0.4, transparent: true });
  const ring = Array.from({ length: 34 }, (_, i) =>
    new THREE.BoxGeometry(r * 0.34, height * 0.75, 0.7)
      .rotateY(0.55)
      .translate(r * 0.66, 0, 0)
      .rotateY((i / 34) * Math.PI * 2),
  );
  blades.add(new THREE.Mesh(mergeGeometries(ring) ?? new THREE.BufferGeometry(), bladeMat));
  for (const b of ring) b.dispose();
  blades.add(new THREE.Mesh(new THREE.CylinderGeometry(r * 0.42, r * 0.42, height * 0.8, 32), mat(0x2c323b, { metalness: 0.4 })));
  const disc = new THREE.MeshBasicMaterial({ color: 0x343a44, transparent: true, opacity: 0, depthWrite: false, side: THREE.DoubleSide });
  const discMesh = new THREE.Mesh(new THREE.RingGeometry(r * 0.42, r * 0.95, 40), disc);
  discMesh.rotation.x = -Math.PI / 2;
  discMesh.position.y = height * 0.38;
  g.add(wall, floor, outlet, blades, discMesh);
  return { g, spin: { g: blades, axis: 'y', blade: bladeMat, disc } };
}

/** Fins: thin plates across a box, for heatsinks and radiators (one mesh). */
function fins(size: [number, number, number], along: 0 | 1 | 2, color = METAL): THREE.Group {
  const g = new THREE.Group();
  const m = mat(color, { metalness: 0.8, roughness: 0.32 });
  const n = Math.max(4, Math.floor(size[along] / 4));
  const plate: [number, number, number] = [...size];
  plate[along] = 0.8;
  const plates = Array.from({ length: n }, (_, i) => {
    const at: [number, number, number] = [0, 0, 0];
    at[along] = -size[along] / 2 + (i + 0.5) * (size[along] / n);
    return new THREE.BoxGeometry(...plate).translate(...at);
  });
  g.add(new THREE.Mesh(mergeGeometries(plates) ?? new THREE.BufferGeometry(), m));
  for (const p of plates) p.dispose();
  return g;
}

// ---------------------------------------------------------------------------------------------
// Graphics card
// ---------------------------------------------------------------------------------------------

/** A graphics card in its own frame: length along x, height along y, fans facing +z. */
export function card(shape: GpuShape, rgb: THREE.MeshStandardMaterial): { g: THREE.Group; spins: Spinner[]; body: THREE.MeshStandardMaterial } {
  const g = new THREE.Group();
  const len = shape.lengthMm;
  const thick = shape.slots * 20.32;
  const body = mat(CARD_COLOURS[shape.colour], { metalness: 0.45, roughness: 0.42 });
  g.add(rounded([len, CARD_HEIGHT, thick - 6], 4, body, [0, 0, 1]));
  // Angled cuts across the shroud, lit by the accent.
  for (const x of [-0.18, 0.22]) {
    const cut = boxMesh([len * 0.16, 2.2, 1.2], rgb, [len * x, CARD_HEIGHT * 0.36, thick / 2 - 2]);
    cut.rotation.z = -0.35;
    g.add(cut);
  }
  // Backplate, board and the gold contacts.
  g.add(boxMesh([len - 4, CARD_HEIGHT - 10, 2], mat(0x2a2f37, { metalness: 0.7, roughness: 0.35 }), [0, 0, -thick / 2 + 1]));
  g.add(boxMesh([len - 10, 112, 1.6], mat(0x0f2a1a), [0, -3, -thick / 2 + 3.5]));
  g.add(boxMesh([86, 7, 1.6], mat(GOLD, { metalness: 1, roughness: 0.25 }), [-len / 2 + 70, -CARD_HEIGHT / 2 - 3, -thick / 2 + 3.5]));
  // Bracket and the light strip along the top.
  g.add(boxMesh([2, 120, thick], mat(METAL, { metalness: 0.85, roughness: 0.3 }), [-len / 2 - 1, -2, 0]));
  g.add(boxMesh([len * 0.62, 2.2, 3], rgb, [len * 0.05, CARD_HEIGHT / 2 + 0.6, thick / 2 - 6]));
  // Heat pipes curling out of the fin stack at the top edge.
  const pipeMat = mat(COPPER, { metalness: 1, roughness: 0.3 });
  for (let i = 0; i < 3; i++) {
    const x = -len * 0.15 + i * 16;
    g.add(
      tube(
        [new THREE.Vector3(x, CARD_HEIGHT / 2 - 10, -thick / 2 + 12), new THREE.Vector3(x + 6, CARD_HEIGHT / 2 + 3, 0), new THREE.Vector3(x + 2, CARD_HEIGHT / 2 - 8, thick / 2 - 10)],
        2.2,
        pipeMat,
        16,
      ),
    );
  }
  // Power plugs on the top edge, towards the far end.
  const plugs = shape.power === 'none' ? 0 : shape.power === '3x8' ? 3 : shape.power === '2x8' || shape.power === '8+6' ? 2 : 1;
  const plugW = shape.power === '16' ? 20 : 22;
  for (let i = 0; i < plugs; i++)
    g.add(boxMesh([plugW, 8, 9], mat(0x111111), [len / 2 - 40 - i * (plugW + 4), CARD_HEIGHT / 2 + 4, -thick / 2 + 12]));
  // Fans.
  const d = Math.min(96, (shape.flow ? len / 2 : len / shape.fans) - 10);
  const spins: Spinner[] = [];
  const add = (x: number, z: number, back = false) => {
    const f = fan(d, { frame: 'ring', count: 9, rgb, frameColor: 0x1b1f25 });
    f.g.position.set(x, 0, z);
    if (back) f.g.rotation.y = Math.PI;
    g.add(f.g);
    spins.push(f.spin);
  };
  if (shape.flow) {
    add(-len / 2 + d / 2 + 18, thick / 2 - 1.5);
    add(len / 2 - d / 2 - 18, -thick / 2 - 1, true);
  } else for (let i = 0; i < shape.fans; i++) add(-len / 2 + (i + 0.5) * (len / shape.fans), thick / 2 - 1.5);
  return { g, spins, body };
}

// ---------------------------------------------------------------------------------------------
// Parts
// ---------------------------------------------------------------------------------------------

interface Built {
  root: THREE.Object3D;
  /** Where it sits, and where it goes in the exploded view. */
  base: THREE.Vector3;
  explode: THREE.Vector3;
  /** Materials coloured by the thermal map, with their own colour. */
  tinted: Array<{ m: THREE.MeshStandardMaterial; base: number }>;
  spins: Spinner[];
  anchor: THREE.Vector3;
}

interface Kit {
  pal: ScenePalette;
  /** An RGB material, which drifts around the accent. */
  rgb: () => THREE.MeshStandardMaterial;
  boardTex: THREE.Texture;
  laptop: boolean;
}

/** One part as meshes, placed in the case or the laptop. */
function build(p: Part, k: Kit): Built {
  const root = new THREE.Group();
  const tinted: Built['tinted'] = [];
  const spins: Spinner[] = [];
  const own = (color: number, opts?: Partial<THREE.MeshStandardMaterialParameters>) => {
    const m = mat(color, opts);
    tinted.push({ m, base: color });
    return m;
  };
  const metal = (stack: THREE.Group) => {
    for (const plate of stack.children as THREE.Mesh[]) {
      const m = plate.material as THREE.MeshStandardMaterial;
      if (!tinted.some((t) => t.m === m)) tinted.push({ m, base: METAL });
    }
  };
  const [sx, sy, sz] = p.size;
  let base = new THREE.Vector3(...p.pos);
  switch (p.kind) {
    case 'board': {
      // A little lighter than the panels, so it reads as a board; traces drawn on it.
      const c = new THREE.Color(k.pal.board).lerp(new THREE.Color(k.pal.light ? 0x6b7a90 : 0x2c3646), 0.5);
      root.add(boxMesh([sx, sy, sz], own(c.getHex(), { map: k.boardTex, roughness: 0.75, metalness: 0.1 })));
      if (!k.laptop) {
        // PCIe slots (the top one armoured), the rear I/O cover and the power plugs.
        const slot = mat(0x101317, { roughness: 0.6 });
        const armour = mat(0xb9c0ca, { metalness: 0.9, roughness: 0.25 });
        for (const [i, y] of [270, 209, 148].entries()) root.add(boxMesh([90, 5, 5], i === 0 ? armour : slot, [-sx / 2 + 70, y - p.pos[1], sz / 2 + 2.5]));
        root.add(rounded([30, 110, 26], 3, mat(0x1a1e25, { metalness: 0.5, roughness: 0.4 }), [-sx / 2 + 16, 380 - p.pos[1], 14]));
        root.add(boxMesh([2, 90, 2], k.rgb(), [-sx / 2 + 31.5, 380 - p.pos[1], 22]));
        root.add(boxMesh([10, 52, 12], mat(0x16191e), [sx / 2 - 7, 300 - p.pos[1], 7.5]));
        root.add(boxMesh([18, 9, 10], mat(0x16191e), [-sx / 2 + 26, sy / 2 - 9, 6.5]));
      }
      break;
    }
    case 'cooler': {
      root.add(boxMesh([sx, sy, 4], own(0x2a2f37), [0, 0, -sz / 2 + 2]));
      const stack = fins([sx, sy, sz - 14], 2);
      stack.position.z = 1;
      root.add(stack);
      metal(stack);
      // Copper heat pipes rising from the base through the fins.
      const pipe = mat(COPPER, { metalness: 1, roughness: 0.3 });
      for (const x of [-12, -4, 4, 12])
        for (const y of [-sy * 0.3, sy * 0.3])
          root.add(tube([new THREE.Vector3(x, y * 0.2, -sz / 2 + 3), new THREE.Vector3(x, y, -sz / 2 + 30), new THREE.Vector3(x, y, sz / 2 - 6)], 2.2, pipe, 12));
      // The top cap, lit.
      root.add(rounded([sx + 2, sy + 2, 6], 2.5, mat(0x1b1f26, { metalness: 0.6, roughness: 0.35 }), [0, 0, sz / 2 - 3]));
      root.add(boxMesh([sx * 0.8, 2, 1], k.rgb(), [0, sy / 2 - 6, sz / 2 + 0.3]));
      break;
    }
    case 'radiator': {
      root.add(boxMesh([sx, sy, 3], own(0x2a2f37), [0, 0, -sz / 2 + 1.5]));
      const stack = fins([sx, sy, sz - 6], 0);
      stack.position.z = 2;
      root.add(stack);
      metal(stack);
      break;
    }
    case 'aio': {
      root.add(rounded([sx, sy, sz], 8, own(0x1b1f26, { metalness: 0.5, roughness: 0.35 })));
      const ring = new THREE.Mesh(new THREE.TorusGeometry(sx * 0.33, 2.2, 10, 48), k.rgb());
      ring.position.z = sz / 2 + 0.5;
      const rotor = new THREE.Group();
      const rotorMat = mat(0x3a414c, { metalness: 0.7, roughness: 0.25 });
      rotor.add(boxMesh([sx * 0.5, 3, 1], rotorMat), boxMesh([3, sx * 0.5, 1], rotorMat));
      rotor.position.z = sz / 2 + 1;
      root.add(ring, rotor);
      spins.push({ g: rotor, axis: 'z' });
      // Hoses up to the radiator at the top of the case.
      const hose = mat(0x16191e, { roughness: 0.8 });
      for (const dx of [-12, 12])
        root.add(tube([new THREE.Vector3(dx, sy / 2, 0), new THREE.Vector3(dx + 10, sy / 2 + 40, 40), new THREE.Vector3(dx + 60, 448 - p.pos[1] - 20, 110 - p.pos[2])], 4.2, hose, 32));
      break;
    }
    case 'vrm':
    case 'chipset': {
      root.add(rounded([sx, sy, sz], 2, own(0x59616e, { metalness: 0.75, roughness: 0.32 })));
      if (p.kind === 'chipset') root.add(boxMesh([sx * 0.5, 2, 0.6], k.rgb(), [0, 0, sz / 2 + 0.3]));
      else {
        const f = fins([sx - 2, sy - 2, 6], sx > sy ? 0 : 1, 0x7d8696);
        f.position.z = sz / 2 + 3;
        root.add(f);
      }
      break;
    }
    case 'cpu':
      root.add(boxMesh([sx, sy, sz], own(0xc9ccd2, { metalness: 0.85, roughness: 0.25 })));
      break;
    case 'ram':
      root.add(rounded([sx, sy, sz - 4], 1.5, own(0x2a3446, { metalness: 0.55, roughness: 0.35 })));
      root.add(boxMesh([sx + 0.4, sy - 6, 4], k.rgb(), [0, 0, sz / 2 - 2]));
      break;
    case 'slot':
      root.add(boxMesh([sx, sy, sz], mat(new THREE.Color(k.pal.line).getHex())));
      break;
    case 'm2':
      root.add(boxMesh([sx, sy, sz], own(0x1a1f27)));
      // A finned heatsink on a tower; a label on a laptop.
      if (k.laptop) root.add(boxMesh([sx * 0.7, 0.5, sz * 0.6], mat(0x3a4250), [0, sy / 2 + 0.3, 0]));
      else root.add(rounded([sx * 0.9, sy * 0.85, 3], 1, mat(0x4b5260, { metalness: 0.8, roughness: 0.3 }), [0, 0, sz / 2 + 1.5]));
      break;
    case 'drive':
      root.add(rounded([sx, sy, sz], 2, own(sy > 10 ? 0x9aa3ae : 0x20252d, { metalness: sy > 10 ? 0.7 : 0.25, roughness: 0.35 })));
      break;
    case 'psu':
      root.add(rounded([sx, sy, sz], 3, mat(0x14181f, { metalness: 0.5, roughness: 0.5 })));
      root.add(boxMesh([sx * 0.6, 1, 0.5], k.rgb(), [0, sy * 0.25, sz / 2 + 0.3]));
      break;
    case 'fan': {
      // Its diameter is the size across its face (the middle of the three).
      const f = fan([sx, sy, sz].sort((a, b) => a - b)[1]!, { frame: 'square', count: 9, rgb: k.rgb() });
      // Face the fan along its axis.
      if (p.fan?.axis === 'x') f.g.rotation.y = Math.PI / 2;
      if (p.fan?.axis === 'y') f.g.rotation.x = Math.PI / 2;
      root.add(f.g);
      spins.push(f.spin);
      break;
    }
    case 'gpu': {
      const c = card(p.gpu!.shape, k.rgb());
      // In the case the card stands out from the board with its fans facing down.
      c.g.rotation.x = Math.PI / 2;
      root.add(c.g);
      tinted.push({ m: c.body, base: c.body.color.getHex() });
      spins.push(...c.spins);
      break;
    }
    // Laptop.
    case 'die':
      root.add(boxMesh([sx + 10, 1, sz + 10], mat(0x1f6b45, { roughness: 0.6 }), [0, -1, 0]));
      root.add(boxMesh([sx, sy, sz], own(0x2b2f36, { metalness: 0.9, roughness: 0.2 })));
      break;
    case 'heatpipe':
      root.add(tube(p.path!.map((v) => new THREE.Vector3(v[0] - p.pos[0], v[1] - p.pos[1], v[2] - p.pos[2])), 2.4, own(COPPER, { metalness: 1, roughness: 0.28 })));
      break;
    case 'blower': {
      const b = blower(sx, sy);
      root.add(b.g);
      spins.push(b.spin);
      break;
    }
    case 'sodimm':
      root.add(boxMesh([sx, sy * 0.4, sz], own(0x1f5c3a, { roughness: 0.6 })));
      for (let i = 0; i < 4; i++) root.add(boxMesh([10, sy * 0.6, 9], mat(0x14171c), [-sx / 2 + 10 + i * 16, sy * 0.4, 0]));
      break;
    case 'battery':
      root.add(rounded([sx, sy, sz], 2.5, mat(0x23272e, { roughness: 0.7 })));
      root.add(boxMesh([sx * 0.25, 0.4, sz * 0.4], mat(0x3a4250), [sx * 0.25, sy / 2 + 0.2, 0]));
      break;
    case 'deck':
      // Drawn by the laptop's scene, which changes it with x-ray.
      break;
    case 'lid':
      // Hinged at the back edge of the deck (the scene adds the screen).
      base = new THREE.Vector3(LAPTOP[0] / 2, LAPTOP[1], 0);
      break;
  }
  root.position.copy(base);
  root.traverse((o) => (o.userData.part = p.id));
  return { root, base, explode: new THREE.Vector3(...(p.explode ?? [0, 0, 0])), tinted, spins, anchor: new THREE.Vector3(...p.anchor) };
}

// ---------------------------------------------------------------------------------------------
// The scene
// ---------------------------------------------------------------------------------------------

export function createScene(canvas: HTMLCanvasElement, view: SceneView, pal: ScenePalette, onpick: (id: string | null) => void): PcScene | null {
  let renderer: THREE.WebGLRenderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: 'high-performance' });
  } catch {
    return null;
  }
  const opts: SceneOptions = { quality: 'high', airflow: true, explode: false, xray: true };
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(32, 1, 5, 6000);
  const laptop = view === 'laptop';

  // Light: a room reflected in the metal and glass, a key light, a cool fill, an accent rim.
  const pmrem = new THREE.PMREMGenerator(renderer);
  const room = new RoomEnvironment();
  const env = pmrem.fromScene(room, 0.04).texture;
  scene.environment = env;
  scene.environmentIntensity = pal.light ? 0.85 : 0.6;
  scene.add(new THREE.HemisphereLight(0xffffff, pal.light ? 0x9aa3ae : 0x283040, pal.light ? 1.4 : 1.0));
  const key = new THREE.DirectionalLight(0xffffff, 2.2);
  key.position.set(500, 900, 1100);
  const fill = new THREE.DirectionalLight(0xbfd4ff, 0.7);
  fill.position.set(-500, 200, 600);
  const accent = new THREE.Color(pal.accent);
  const rim = new THREE.DirectionalLight(accent, 1.2);
  rim.position.set(-600, 500, -700);
  scene.add(key, fill, rim);

  const controls = new OrbitControls(camera, canvas);
  controls.enablePan = false;
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.minDistance = view === 'gpu' ? 200 : laptop ? 260 : 450;
  const farthest = view === 'gpu' ? 900 : laptop ? 1400 : 1800;
  controls.maxDistance = farthest;
  controls.autoRotateSpeed = 0.35;

  const textures: THREE.Texture[] = [];
  const keep = <T extends THREE.Texture>(t: T): T => (textures.push(t), t);
  const boardTex = keep(boardTexture(new THREE.Color(pal.board).lerp(new THREE.Color(pal.light ? 0x6b7a90 : 0x2c3646), 0.5).getStyle()));

  // RGB: emissive materials that drift a little around the accent's hue. The case's stay; the
  // parts' are made again with the parts.
  const caseRgb: THREE.MeshStandardMaterial[] = [];
  const partRgb: THREE.MeshStandardMaterial[] = [];
  const hsl = { h: 0, s: 0, l: 0 };
  accent.getHSL(hsl);
  const makeRgb = (list: THREE.MeshStandardMaterial[]) => () => {
    const m = new THREE.MeshStandardMaterial({ color: accent.clone(), emissive: accent.clone(), emissiveIntensity: 1.6, roughness: 0.4 });
    list.push(m);
    return m;
  };
  const kit: Kit = { pal, rgb: makeRgb(partRgb), boardTex, laptop };
  const decoRgb = makeRgb(caseRgb);

  const world = new THREE.Group();
  const deco = new THREE.Group();
  const glassPanel = new THREE.Group();
  scene.add(world, deco);
  let built = new Map<string, Built>();
  let spin = new Map<string, number>();
  let heat = new Map<string, number>();
  let tint = new Map<string, string | 'dim'>();
  let maxFps = 60;
  let raf = 0;
  let last = 0;
  let dirty = true;
  let count = 0;
  let frameCb: (() => void) | null = null;
  let hoverCb: ((id: string | null, x: number, y: number) => void) | null = null;
  let disposed = false;
  let clock = 0;
  let explodeE = 0;
  let focused: string | null = null;
  let composer: EffectComposer | null = null;
  let bloom: UnrealBloomPass | null = null;

  function free(o: THREE.Object3D): void {
    o.traverse((x) => {
      const m = x as THREE.Mesh;
      m.geometry?.dispose();
      const ms = Array.isArray(m.material) ? m.material : m.material ? [m.material] : [];
      for (const mm of ms) mm.dispose();
    });
  }

  // ----- The case, the laptop's base, the stage -------------------------------------------------

  const shadow = new THREE.Mesh(
    new THREE.PlaneGeometry(1, 1),
    new THREE.MeshBasicMaterial({ map: keep(shadowTexture()), transparent: true, depthWrite: false, opacity: pal.light ? 0.5 : 0.85 }),
  );
  shadow.rotation.x = -Math.PI / 2;
  deco.add(shadow);

  function buildCase(): void {
    const [W, H, D] = CASE;
    const t = 5;
    // Powder-coated steel, matte: glossier, the inside caught the room's ceiling light from
    // above and the whole interior turned white (1.2's first field check).
    const shell = mat(pal.light ? 0xe4e8ee : 0x16191f, { metalness: 0.3, roughness: 0.8 });
    deco.add(boxMesh([W, H, t], shell, [W / 2, H / 2, -t / 2])); // behind the board
    deco.add(boxMesh([W, t, D], shell, [W / 2, H + t / 2, D / 2])); // top
    deco.add(boxMesh([W, t, D], shell, [W / 2, -t / 2, D / 2])); // floor
    deco.add(boxMesh([t, H, D], shell, [-t / 2, H / 2, D / 2])); // back
    // Front: a mesh panel with a light strip down its edge.
    deco.add(boxMesh([t, H, D], mat(pal.light ? 0xcfd5dd : 0x0f1216, { metalness: 0.3, roughness: 0.8 }), [W + t / 2, H / 2, D / 2]));
    deco.add(boxMesh([2, H * 0.86, 2.5], decoRgb(), [W + t + 0.6, H / 2, D - 10]));
    // Top vents.
    const slotMat = mat(0x07090c, { roughness: 0.9 });
    for (let i = 0; i < 9; i++) deco.add(boxMesh([W * 0.55, 0.8, 5], slotMat, [W * 0.5, H + t + 0.4, 30 + i * 18]));
    // Feet.
    const foot = mat(0x0b0d10, { roughness: 0.9 });
    for (const x of [30, W - 30]) for (const z of [25, D - 25]) deco.add(rounded([46, 12, 30], 4, foot, [x, -t - 6, z]));
    // PCIe brackets and the rear I/O at the back.
    const bracket = mat(0x8c95a3, { metalness: 0.9, roughness: 0.3 });
    for (let i = 0; i < 7; i++) deco.add(boxMesh([1.5, 15, 22], bracket, [1, 268 - i * 20.32, 70]));
    deco.add(boxMesh([1.5, 110, 26], mat(0x0b0d10), [1, 380, 14]));
    // The PSU shroud: a plate over the bottom with a smoked window towards the glass.
    deco.add(boxMesh([W, 4, D], shell, [W / 2, 100, D / 2]));
    const smoke = new THREE.MeshStandardMaterial({ color: 0x0c0f14, metalness: 0, roughness: 0.35, transparent: true, opacity: 0.55, envMapIntensity: 0.3 });
    deco.add(boxMesh([W - 4, 96, 2], smoke, [W / 2, 50, D - 3]));
    deco.add(boxMesh([W * 0.86, 1.2, 1.5], decoRgb(), [W / 2, 102.4, D - 6]));
    // Tempered glass on the open side, in a thin dark frame; its reflections kept faint so the
    // parts show through. Gone under the thermal map, which is about the parts.
    const pane = new THREE.MeshPhysicalMaterial({
      color: 0x9fb3c8,
      metalness: 0,
      roughness: 0.05,
      transparent: true,
      opacity: 0.04,
      envMapIntensity: 0.25,
      depthWrite: false,
    });
    glassPanel.add(boxMesh([W - 6, H - 6, 2], pane, [W / 2, H / 2, D + 1.5]));
    const frame = mat(pal.light ? 0xc9cfd8 : 0x0d1014, { metalness: 0.3, roughness: 0.75 });
    glassPanel.add(
      boxMesh([W, 5, 3], frame, [W / 2, H - 2.5, D + 1.5]),
      boxMesh([W, 5, 3], frame, [W / 2, 2.5, D + 1.5]),
      boxMesh([5, H, 3], frame, [2.5, H / 2, D + 1.5]),
      boxMesh([5, H, 3], frame, [W - 2.5, H / 2, D + 1.5]),
    );
    deco.add(glassPanel);
    // Sleeved cables: the board's 24-pin into the shroud, the CPU's 8-pin out the back.
    const sleeve = mat(0xffffff, { map: keep(sleeveTexture()), roughness: 0.85 });
    deco.add(tube([new THREE.Vector3(262, 300, 14), new THREE.Vector3(305, 296, 50), new THREE.Vector3(310, 160, 52), new THREE.Vector3(300, 102, 50)], 7, sleeve));
    deco.add(tube([new THREE.Vector3(40, 432, 12), new THREE.Vector3(30, 448, 30), new THREE.Vector3(10, 450, 40)], 3.2, sleeve, 20));
    shadow.scale.set(W * 1.7, D * 2.6, 1);
    shadow.position.set(W / 2, -t - 12, D / 2);
  }

  // The laptop's base and the keyboard deck (the deck changes with x-ray).
  const deckGroup = new THREE.Group();
  const screen = { tex: null as THREE.CanvasTexture | null, info: { rows: [], load: null } as ScreenInfo, history: [] as number[] };

  function buildLaptopBase(): void {
    const [W, Hh, D] = LAPTOP;
    deco.add(rounded([W, 6, D], 3, mat(pal.light ? 0xd5d9df : 0x2a2e35, { metalness: 0.8, roughness: 0.32 }), [W / 2, 3, D / 2]));
    const wall = mat(pal.light ? 0xc6ccd4 : 0x24282e, { metalness: 0.8, roughness: 0.35 });
    deco.add(boxMesh([W, Hh - 6, 3], wall, [W / 2, Hh / 2 + 1, D - 1.5]));
    deco.add(boxMesh([3, Hh - 6, D], wall, [1.5, Hh / 2 + 1, D / 2]), boxMesh([3, Hh - 6, D], wall, [W - 1.5, Hh / 2 + 1, D / 2]));
    // Back wall with the vents behind the fins.
    deco.add(boxMesh([W, Hh - 6, 3], wall, [W / 2, Hh / 2 + 1, 1.5]));
    const vent = mat(0x07090c);
    for (const x of [70, 290]) for (let i = 0; i < 8; i++) deco.add(boxMesh([1.5, Hh - 10, 1], vent, [x - 28 + i * 8, Hh / 2 + 1, -0.2]));
    deco.add(deckGroup);
    shadow.scale.set(W * 1.8, D * 2.2, 1);
    shadow.position.set(W / 2, -2, D / 2 - 20);
  }

  function buildDeck(): void {
    for (const c of [...deckGroup.children]) {
      deckGroup.remove(c);
      free(c);
    }
    const [W, Hh, D] = LAPTOP;
    const deck = built.get('deck');
    const top = opts.xray
      ? new THREE.MeshPhysicalMaterial({ color: pal.light ? 0xffffff : 0x9fb3c8, metalness: 0, roughness: 0.08, transparent: true, opacity: 0.14, clearcoat: 1, depthWrite: false })
      : mat(pal.light ? 0xdde1e6 : 0x30353d, { metalness: 0.8, roughness: 0.3 });
    deckGroup.add(rounded([W, 2, D], 1, top, [W / 2, Hh - 1, D / 2]));
    // Keys: caps on a well, lit from below by the accent.
    const keyMat = mat(pal.light ? 0xf4f6f8 : 0x16191e, { roughness: 0.55, transparent: opts.xray, opacity: opts.xray ? 0.35 : 1 });
    const cols = 15;
    const rows = 6;
    const keys = new THREE.InstancedMesh(new RoundedBoxGeometry(16.5, 1.6, 15.5, 2, 1.2), keyMat, cols * rows);
    const m4 = new THREE.Matrix4();
    let n = 0;
    for (let r = 0; r < rows; r++)
      for (let c = 0; c < cols; c++) {
        m4.makeTranslation(36 + c * 19.2, Hh + 0.9, 58 + r * 18.5);
        keys.setMatrixAt(n++, m4);
      }
    deckGroup.add(keys);
    const glowPlane = new THREE.Mesh(
      new THREE.PlaneGeometry(cols * 19.2, rows * 18.5),
      new THREE.MeshBasicMaterial({ color: accent, transparent: true, opacity: opts.xray ? 0.05 : 0.14, depthWrite: false }),
    );
    glowPlane.rotation.x = -Math.PI / 2;
    glowPlane.position.set(36 + ((cols - 1) * 19.2) / 2, Hh + 0.05, 58 + ((rows - 1) * 18.5) / 2);
    deckGroup.add(glowPlane);
    // Trackpad.
    deckGroup.add(
      rounded([110, 0.8, 62], 3, mat(pal.light ? 0xe9ecf0 : 0x262a31, { roughness: 0.25, metalness: 0.4, transparent: opts.xray, opacity: opts.xray ? 0.3 : 1 }), [W / 2, Hh + 0.2, 208]),
    );
    if (deck) deckGroup.position.copy(deck.explode).multiplyScalar(explodeE);
  }

  function drawScreen(): void {
    const c = screen.tex?.image as HTMLCanvasElement | undefined;
    if (!c) return;
    const ctx = c.getContext('2d')!;
    const w = c.width;
    const h = c.height;
    const g = ctx.createLinearGradient(0, 0, w, h);
    g.addColorStop(0, '#0b111c');
    g.addColorStop(1, '#141c2c');
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = pal.accent;
    ctx.font = '700 34px sans-serif';
    ctx.fillText('V Y R O', 36, 62);
    ctx.fillStyle = '#8593a5';
    ctx.font = '400 20px sans-serif';
    ctx.fillText('hardware monitor', 200, 60);
    screen.info.rows.slice(0, 4).forEach(([k, v], i) => {
      const x = 36 + (i % 2) * 290;
      const y = 110 + Math.floor(i / 2) * 100;
      ctx.fillStyle = 'rgba(255,255,255,0.05)';
      ctx.fillRect(x, y, 270, 84);
      ctx.fillStyle = '#8593a5';
      ctx.font = '600 18px sans-serif';
      ctx.fillText(k.toUpperCase(), x + 16, y + 30);
      ctx.fillStyle = '#e6edf3';
      ctx.font = '700 34px sans-serif';
      ctx.fillText(v, x + 16, y + 70);
    });
    // CPU use over the last minute.
    const hist = screen.history;
    if (hist.length > 1) {
      ctx.strokeStyle = pal.accent;
      ctx.lineWidth = 3;
      ctx.beginPath();
      hist.forEach((v, i) => {
        const x = 36 + (i / 59) * (w - 72);
        const y = h - 26 - (v / 100) * 90;
        if (i) ctx.lineTo(x, y);
        else ctx.moveTo(x, y);
      });
      ctx.stroke();
    }
    screen.tex!.needsUpdate = true;
  }

  function buildLid(): THREE.Group {
    const g = new THREE.Group();
    g.add(rounded([LID.width, LID.height, LID.depth], 4, mat(pal.light ? 0xd5d9df : 0x2a2e35, { metalness: 0.8, roughness: 0.32 }), [0, LID.height / 2, -LID.depth / 2]));
    g.add(boxMesh([LID.width - 6, LID.height - 6, 0.5], mat(0x050608, { roughness: 0.2, metalness: 0.2 }), [0, LID.height / 2, 0.3]));
    screen.tex = keep(canvasTexture(640, 400, () => {}));
    const panel = new THREE.Mesh(new THREE.PlaneGeometry(LID.width - 26, LID.height - 30), new THREE.MeshBasicMaterial({ map: screen.tex, toneMapped: false }));
    panel.position.set(0, LID.height / 2 + 4, 0.7);
    g.add(panel);
    const cam = new THREE.Mesh(new THREE.CircleGeometry(1.4, 16), mat(0x111111));
    cam.position.set(0, LID.height - 8, 0.8);
    g.add(cam);
    g.rotation.x = -((LID_OPEN - 90) * Math.PI) / 180;
    drawScreen();
    return g;
  }

  if (view === 'pc') buildCase();
  else if (laptop) buildLaptopBase();
  else {
    shadow.scale.set(700, 380, 1);
    shadow.position.set(0, -CARD_HEIGHT / 2 - 30, 0);
  }

  // ----- Camera ---------------------------------------------------------------------------------

  const home = { pos: new THREE.Vector3(), target: new THREE.Vector3() };
  if (view === 'pc') {
    home.pos.set(CASE[0] * 1.05, CASE[1] * 0.8, CASE[2] * 4.1);
    home.target.set(CASE[0] * 0.48, CASE[1] * 0.52, CASE[2] * 0.3);
  } else if (laptop) {
    home.pos.set(LAPTOP[0] * 1.45, 360, 640);
    home.target.set(LAPTOP[0] / 2, 80, 40);
  } else home.pos.set(-120, 140, 520);
  camera.position.copy(home.pos);
  controls.target.copy(home.target);
  controls.update();
  /** The way the camera looks at the model from home (from the model towards the camera). */
  const homeDir = home.pos.clone().sub(home.target).normalize();
  /** Nobody has turned, zoomed or focused the view since it was last framed. */
  let atHome = true;

  /** A camera move under way. */
  let flight: { p0: THREE.Vector3; t0: THREE.Vector3; p1: THREE.Vector3; t1: THREE.Vector3; start: number; ms: number } | null = null;
  const easeOut = (x: number) => 1 - Math.pow(1 - x, 3);
  function fly(p1: THREE.Vector3, t1: THREE.Vector3, ms: number): void {
    flight = { p0: camera.position.clone(), t0: controls.target.clone(), p1, t1, start: performance.now(), ms };
    invalidate();
  }

  /** The model's bounds with the parts lifted out by `e` (0 together, 1 exploded): the case or
   * the laptop's base and every part, not the shadow or the airflow. */
  function modelBox(e: number): THREE.Box3 {
    const box = new THREE.Box3();
    for (const c of deco.children) if (c !== shadow && c !== flow?.mesh) box.expandByObject(c);
    for (const b of built.values()) {
      const was = b.root.position.clone();
      b.root.position.copy(b.base).addScaledVector(b.explode, e);
      b.root.updateMatrixWorld(true);
      box.expandByObject(b.root);
      b.root.position.copy(was);
      b.root.updateMatrixWorld(true);
    }
    return box;
  }

  /** Frames the whole model for the canvas's shape (1.2: a wide view used to cut the case's top
   * and bottom off). Home is where every corner fits; a camera still at home goes there, by
   * `move`, keeping the way it looks while the view turns by itself. */
  function frameModel(move: 'jump' | 'fly' | 'none'): void {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    const box = modelBox(opts.explode ? 1 : 0);
    if (w === 0 || h === 0 || box.isEmpty()) return;
    const target = box.getCenter(new THREE.Vector3());
    const turned = atHome && !flight ? camera.position.clone().sub(controls.target).normalize() : homeDir;
    const dist = (dir: THREE.Vector3) => fitDistance(corners(box), target, dir, camera.up, camera.fov, w / h);
    home.target.copy(target);
    home.pos.copy(target).addScaledVector(homeDir, dist(homeDir));
    controls.maxDistance = Math.max(farthest, home.pos.distanceTo(target) * 1.6);
    if (!atHome || focused) return;
    const pos = target.clone().addScaledVector(turned, dist(turned));
    if (flight) {
      flight.p1.copy(home.pos);
      flight.t1.copy(home.target);
    } else if (move === 'fly') fly(pos, target.clone(), 700);
    else if (move === 'jump') {
      camera.position.copy(pos);
      controls.target.copy(target);
      controls.update();
    }
  }

  let shape = '';
  let flown = false;
  function size(): boolean {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (w === 0 || h === 0) return false;
    if (canvas.width !== Math.round(w * renderer.getPixelRatio()) || canvas.height !== Math.round(h * renderer.getPixelRatio())) {
      renderer.setSize(w, h, false);
      composer?.setSize(w, h);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    }
    // A new shape (the first draw, a resized window): frame the model again.
    if (shape !== `${w}x${h}`) {
      shape = `${w}x${h}`;
      frameModel('jump');
    }
    return true;
  }

  // ----- Bloom ----------------------------------------------------------------------------------

  function setupBloom(): void {
    composer?.dispose();
    bloom?.dispose();
    composer = null;
    bloom = null;
    if (opts.quality !== 'high') {
      scene.background = null;
      return;
    }
    // Bloom needs an opaque background: the card's colour behind the canvas.
    scene.background = new THREE.Color(pal.bg);
    composer = new EffectComposer(renderer);
    composer.addPass(new RenderPass(scene, camera));
    bloom = new UnrealBloomPass(new THREE.Vector2(256, 256), pal.light ? 0.35 : 0.55, 0.45, 0.82);
    composer.addPass(bloom);
    composer.addPass(new OutputPass());
    composer.setPixelRatio(renderer.getPixelRatio());
    composer.setSize(canvas.clientWidth || 1, canvas.clientHeight || 1);
  }
  setupBloom();

  // ----- Airflow --------------------------------------------------------------------------------

  // Smoke lines as in a wind tunnel (1.2, after the first field check): the streamlines from
  // `airflow.ts`, each a thin ribbon facing the camera, with streaks running along it at the fans'
  // pace. Their colour goes from cool at the intakes to warm as the air takes up the CPU's and the
  // GPU's heat (from the live readings). Built once per layout; a frame only moves a uniform.
  const flowUniforms = {
    uTime: { value: 0 },
    uSpeed: { value: 120 },
    uWidth: { value: 1.6 },
    uViewport: { value: new THREE.Vector2(1, 1) },
    uHeat: { value: new THREE.Vector2(0, 0) },
    uOpacity: { value: 1 },
    uCool: { value: new THREE.Color(0x4cc9f0) },
    uWarm: { value: new THREE.Color(0xff8a4c) },
    uHot: { value: new THREE.Color(0xff3b3b) },
  };
  let flow: { mesh: THREE.Mesh; mat: THREE.ShaderMaterial } | null = null;
  function buildFlow(parts: readonly Part[]): void {
    if (flow) {
      deco.remove(flow.mesh);
      flow.mesh.geometry.dispose();
      flow.mat.dispose();
      flow = null;
    }
    if (view === 'gpu') return;
    const lines = laptop ? laptopAirflow(parts) : towerAirflow(parts);
    const n = lines.reduce((a, l) => a + l.points.length, 0);
    if (n === 0) return;
    const pos = new Float32Array(n * 6);
    const tan = new Float32Array(n * 6);
    const side = new Float32Array(n * 2);
    const along = new Float32Array(n * 2);
    const total = new Float32Array(n * 2);
    const warmth = new Float32Array(n * 4);
    const phase = new Float32Array(n * 2);
    const index: number[] = [];
    let v = 0;
    lines.forEach((l, li) => {
      const m = l.points.length;
      const end = l.along[m - 1]!;
      for (let i = 0; i < m; i++) {
        const p = l.points[i]!;
        const a = l.points[Math.max(0, i - 1)]!;
        const b = l.points[Math.min(m - 1, i + 1)]!;
        for (const sd of [-1, 1]) {
          pos.set(p, v * 3);
          tan.set([b[0] - a[0], b[1] - a[1], b[2] - a[2]], v * 3);
          side[v] = sd;
          along[v] = l.along[i]!;
          total[v] = end;
          warmth.set(l.heat[i]!, v * 2);
          // Streaks out of step from line to line.
          phase[v] = (li * 0.618034) % 1;
          v++;
        }
        if (i < m - 1) index.push(v - 2, v - 1, v, v - 1, v + 1, v);
      }
    });
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    geo.setAttribute('tangent', new THREE.BufferAttribute(tan, 3));
    geo.setAttribute('side', new THREE.BufferAttribute(side, 1));
    geo.setAttribute('along', new THREE.BufferAttribute(along, 1));
    geo.setAttribute('total', new THREE.BufferAttribute(total, 1));
    geo.setAttribute('warmth', new THREE.BufferAttribute(warmth, 2));
    geo.setAttribute('phase', new THREE.BufferAttribute(phase, 1));
    geo.setIndex(index);
    const mat = new THREE.ShaderMaterial({
      uniforms: flowUniforms,
      transparent: true,
      depthWrite: false,
      // A ribbon turns its back to the camera as often as not.
      side: THREE.DoubleSide,
      blending: THREE.AdditiveBlending,
      vertexShader: /* glsl */ `
        uniform float uWidth;
        uniform vec2 uViewport;
        attribute vec3 tangent;
        attribute float side;
        attribute float along;
        attribute float total;
        attribute vec2 warmth;
        attribute float phase;
        varying float vSide;
        varying float vAlong;
        varying float vTotal;
        varying vec2 vWarmth;
        varying float vPhase;
        void main() {
          vec4 c = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
          vec4 t = projectionMatrix * modelViewMatrix * vec4(position + normalize(tangent), 1.0);
          vec2 dir = (t.xy / t.w - c.xy / c.w) * uViewport;
          dir = length(dir) > 1e-6 ? normalize(dir) : vec2(1.0, 0.0);
          // A strip uWidth pixels wide across the line, whatever the distance.
          c.xy += vec2(-dir.y, dir.x) * side * uWidth / uViewport * c.w;
          gl_Position = c;
          vSide = side;
          vAlong = along;
          vTotal = total;
          vWarmth = warmth;
          vPhase = phase;
        }`,
      fragmentShader: /* glsl */ `
        uniform float uTime;
        uniform float uSpeed;
        uniform vec2 uHeat;
        uniform float uOpacity;
        uniform vec3 uCool;
        uniform vec3 uWarm;
        uniform vec3 uHot;
        varying float vSide;
        varying float vAlong;
        varying float vTotal;
        varying vec2 vWarmth;
        varying float vPhase;
        void main() {
          float core = 1.0 - smoothstep(0.35, 1.0, abs(vSide));
          float ends = smoothstep(0.0, 30.0, vAlong) * smoothstep(0.0, 45.0, vTotal - vAlong);
          // Streaks: a bright head with a tail behind it, running the way the air goes.
          float s = fract((vAlong - uTime * uSpeed) / 110.0 + vPhase);
          float streak = pow(s, 7.0);
          float w = clamp(vWarmth.x * (0.3 + 0.7 * uHeat.x) + vWarmth.y * (0.3 + 0.7 * uHeat.y), 0.0, 1.0);
          vec3 col = w < 0.6 ? mix(uCool, uWarm, w / 0.6) : mix(uWarm, uHot, (w - 0.6) / 0.4);
          float a = (0.22 + 0.9 * streak) * core * ends * uOpacity;
          gl_FragColor = vec4(col * (1.0 + 0.8 * streak), a);
        }`,
    });
    const mesh = new THREE.Mesh(geo, mat);
    mesh.frustumCulled = false;
    mesh.renderOrder = 10;
    flow = { mesh, mat };
    deco.add(mesh);
    stepFlow();
  }
  const viewport = new THREE.Vector2();
  function stepFlow(): void {
    if (!flow) return;
    // Out of the way while the parts are lifted apart (the air's course is the assembled PC's).
    const opacity = opts.airflow ? 1 - explodeE : 0;
    flow.mesh.visible = opacity > 0.01;
    if (!flow.mesh.visible) return;
    const rates = [...spin.values()].filter((r) => r > 0);
    const rate = rates.length ? rates.reduce((a, b) => a + b, 0) / rates.length : 0.35;
    flowUniforms.uTime.value = clock;
    flowUniforms.uSpeed.value = 60 + 110 * rate;
    flowUniforms.uOpacity.value = opacity;
    renderer.getDrawingBufferSize(viewport);
    flowUniforms.uViewport.value.copy(viewport);
    flowUniforms.uWidth.value = 2.2 * renderer.getPixelRatio();
    let gpu = 0;
    for (const [id, h] of heat) if (id.startsWith('gpu')) gpu = Math.max(gpu, h);
    flowUniforms.uHeat.value.set(heat.get('cpu') ?? 0, gpu);
  }

  // ----- Frames ---------------------------------------------------------------------------------

  let idleSince = performance.now();
  let interacting = false;
  controls.addEventListener('start', () => {
    interacting = true;
    atHome = false;
    flight = null;
    controls.autoRotate = false;
  });
  controls.addEventListener('end', () => {
    interacting = false;
    idleSince = performance.now();
  });

  const high = () => opts.quality === 'high' && maxFps > 0;
  const spinning = () => maxFps > 0 && [...built.entries()].some(([id, b]) => b.spins.length > 0 && (spin.get(id) ?? 0) > 0);
  const moving = () =>
    spinning() ||
    flight != null ||
    explodeE !== (opts.explode ? 1 : 0) ||
    (maxFps > 0 && opts.airflow && flow != null && explodeE < 1) ||
    (high() && (caseRgb.length + partRgb.length > 0 || controls.autoRotate));

  function frame(t: number): void {
    raf = 0;
    if (disposed || document.hidden) return;
    const going = moving();
    const gap = 1000 / Math.max(1, maxFps || 30);
    if (!dirty && !going && !interacting) return;
    if (t - last < gap - 1) {
      raf = requestAnimationFrame(frame);
      return;
    }
    const dt = last === 0 ? 0 : Math.min(0.1, (t - last) / 1000);
    last = t;
    clock += dt;
    // Fans: at speed, the blades blur into a disc.
    if (maxFps > 0)
      for (const [id, b] of built) {
        const r = spin.get(id) ?? 0;
        for (const s of b.spins) {
          s.g.rotation[s.axis] += r * Math.PI * 2 * dt;
          if (s.blade && s.disc) {
            const blur = Math.max(0, Math.min(1, (r - 0.9) / 1.4));
            s.blade.opacity = 1 - blur * 0.55;
            s.disc.opacity = blur * 0.5;
          }
        }
      }
    // RGB drifts around the accent; dimmed under the thermal map.
    const thermal = tint.size > 0;
    let phase = 0;
    for (const m of [...caseRgb, ...partRgb]) {
      const h = high() ? (hsl.h + 0.06 * Math.sin(clock * 0.5 + phase) + 1) % 1 : hsl.h;
      phase += 0.7;
      m.emissive.setHSL(h, Math.min(1, hsl.s * 1.05), Math.min(0.6, hsl.l));
      m.color.copy(m.emissive);
      m.emissiveIntensity = thermal ? 0.1 : 1.6;
    }
    // Exploded view.
    const goal = opts.explode ? 1 : 0;
    if (explodeE !== goal) {
      explodeE += (goal - explodeE) * (1 - Math.exp(-dt / 0.16));
      if (Math.abs(goal - explodeE) < 0.002 || maxFps === 0) explodeE = goal;
      for (const b of built.values()) b.root.position.copy(b.base).addScaledVector(b.explode, explodeE);
      const deck = built.get('deck');
      if (deck) deckGroup.position.copy(deck.explode).multiplyScalar(explodeE);
    }
    // Camera: a flight under way, else a slow turn once nobody has dragged for a while.
    if (flight) {
      const k = maxFps === 0 ? 1 : Math.min(1, (performance.now() - flight.start) / flight.ms);
      const e = easeOut(k);
      camera.position.lerpVectors(flight.p0, flight.p1, e);
      controls.target.lerpVectors(flight.t0, flight.t1, e);
      if (k >= 1) {
        flight = null;
        idleSince = performance.now();
      }
    } else if (high() && !interacting && !focused && performance.now() - idleSince > 8000) controls.autoRotate = true;
    controls.update(dt);
    stepFlow();
    if (hoverDirty) pickHover();
    if (size()) {
      if (composer) composer.render(dt);
      else renderer.render(scene, camera);
      count++;
      frameCb?.();
    }
    dirty = false;
    if (moving() || interacting) raf = requestAnimationFrame(frame);
  }

  function invalidate(): void {
    dirty = true;
    if (!raf && !disposed) raf = requestAnimationFrame(frame);
  }

  controls.addEventListener('change', invalidate);
  const ro = new ResizeObserver(invalidate);
  ro.observe(canvas);
  const onVisible = () => !document.hidden && invalidate();
  document.addEventListener('visibilitychange', onVisible);

  // ----- Picking, hover, focus ------------------------------------------------------------------

  const ray = new THREE.Raycaster();
  const ndc = new THREE.Vector2();
  function hit(x: number, y: number): string | null {
    const r = canvas.getBoundingClientRect();
    ndc.set(((x - r.left) / r.width) * 2 - 1, -((y - r.top) / r.height) * 2 + 1);
    ray.setFromCamera(ndc, camera);
    const h = ray.intersectObjects(world.children, true)[0];
    return (h?.object.userData.part as string | undefined) ?? null;
  }
  let down: [number, number] | null = null;
  const onDown = (e: PointerEvent) => (down = [e.clientX, e.clientY]);
  const onUp = (e: PointerEvent) => {
    if (!down || Math.hypot(e.clientX - down[0], e.clientY - down[1]) > 5) return;
    down = null;
    onpick(hit(e.clientX, e.clientY));
  };
  let pointer: [number, number] | null = null;
  let hoverDirty = false;
  let hovered: string | null = null;
  const onMove = (e: PointerEvent) => {
    pointer = [e.clientX, e.clientY];
    hoverDirty = true;
    invalidate();
  };
  const onLeave = () => {
    pointer = null;
    hoverDirty = true;
    invalidate();
  };
  function pickHover(): void {
    hoverDirty = false;
    const id = pointer ? hit(pointer[0], pointer[1]) : null;
    const r = canvas.getBoundingClientRect();
    if (id !== hovered) {
      hovered = id;
      canvas.style.cursor = id ? 'pointer' : '';
      glow();
    }
    hoverCb?.(id, pointer ? pointer[0] - r.left : 0, pointer ? pointer[1] - r.top : 0);
  }
  /** Warm parts glow in their heat colour; the one under the pointer in the accent. */
  function glow(): void {
    const thermal = tint.size > 0;
    for (const [id, b] of built) {
      const h = heat.get(id) ?? 0;
      const warmth = thermal ? 0 : Math.max(0, (h - 0.45) / 0.55) * 0.5;
      const [r, g, bl] = heatColor(h);
      for (const { m } of b.tinted) {
        if (id === hovered) {
          m.emissive.copy(accent);
          m.emissiveIntensity = 0.28;
        } else if (warmth > 0) {
          m.emissive.setRGB(r / 255, g / 255, bl / 255);
          m.emissiveIntensity = warmth;
        } else {
          m.emissive.setHex(0x000000);
          m.emissiveIntensity = 1;
        }
      }
    }
    invalidate();
  }
  function focus(id: string | null): void {
    focused = id;
    controls.autoRotate = false;
    const b = id ? built.get(id) : null;
    if (!b) {
      focused = null;
      atHome = true;
      frameModel('none');
      fly(home.pos.clone(), home.target.clone(), 900);
      return;
    }
    atHome = false;
    const at = b.anchor.clone().addScaledVector(b.explode, explodeE);
    const dir = camera.position.clone().sub(controls.target).normalize();
    const dist = Math.max(controls.minDistance * 1.05, camera.position.distanceTo(controls.target) * 0.55);
    fly(at.clone().addScaledVector(dir, dist), at, 900);
  }
  const onDouble = (e: MouseEvent) => {
    const id = hit(e.clientX, e.clientY);
    focus(id === focused ? null : id);
  };
  canvas.addEventListener('pointerdown', onDown);
  canvas.addEventListener('pointerup', onUp);
  canvas.addEventListener('pointermove', onMove);
  canvas.addEventListener('pointerleave', onLeave);
  canvas.addEventListener('dblclick', onDouble);

  return {
    setParts(parts) {
      for (const b of built.values()) {
        world.remove(b.root);
        free(b.root);
      }
      partRgb.splice(0);
      built = new Map();
      for (const p of parts) {
        if (view === 'gpu') {
          // The card alone, fans towards the viewer.
          const c = card(p.gpu!.shape, kit.rgb());
          c.g.traverse((o) => (o.userData.part = p.id));
          world.add(c.g);
          built.set(p.id, {
            root: c.g,
            base: new THREE.Vector3(),
            explode: new THREE.Vector3(),
            tinted: [{ m: c.body, base: c.body.color.getHex() }],
            spins: c.spins,
            anchor: new THREE.Vector3(0, CARD_HEIGHT / 2, 0),
          });
          continue;
        }
        const b = build(p, kit);
        if (p.kind === 'lid') {
          const lid = buildLid();
          lid.traverse((o) => (o.userData.part = p.id));
          b.root.add(lid);
        }
        b.root.position.copy(b.base).addScaledVector(b.explode, explodeE);
        world.add(b.root);
        built.set(p.id, b);
      }
      if (laptop) buildDeck();
      buildFlow(parts);
      glow();
      frameModel('jump');
      // The first time: fly in from further out and lower, unless the profile keeps it calm.
      if (!flown && built.size) {
        flown = true;
        if (high()) {
          const from = home.pos.clone().sub(home.target).applyAxisAngle(new THREE.Vector3(0, 1, 0), -0.6).multiplyScalar(1.7).add(home.target);
          from.y -= 120;
          camera.position.copy(from);
          fly(home.pos.clone(), home.target.clone(), 1800);
        }
      }
      invalidate();
    },
    setTint(t) {
      tint = t;
      glassPanel.visible = t.size === 0;
      const dim = new THREE.Color(DIM);
      for (const [id, b] of built) {
        const c = t.get(id);
        for (const { m, base } of b.tinted) {
          if (c === 'dim') m.color.copy(dim);
          else if (c) m.color.set(c);
          else m.color.setHex(base);
          // Under the map a part shows its colour, not the room reflected in its metal.
          m.envMapIntensity = c ? 0.3 : 1;
        }
      }
      glow();
    },
    setSpin(s) {
      spin = s;
      invalidate();
    },
    setHeat(h) {
      heat = h;
      glow();
    },
    setMaxFps(fps) {
      maxFps = fps;
      if (fps === 0) controls.autoRotate = false;
      invalidate();
    },
    setOptions(o) {
      const was = { ...opts };
      Object.assign(opts, o);
      if (was.quality !== opts.quality) {
        setupBloom();
        if (opts.quality !== 'high') controls.autoRotate = false;
      }
      if (laptop && was.xray !== opts.xray) buildDeck();
      if (was.explode !== opts.explode) {
        idleSince = performance.now();
        // Lifted apart the parts reach out of the case: frame them all.
        frameModel('fly');
      }
      invalidate();
    },
    setScreen(info) {
      screen.info = info;
      if (info.load != null) {
        screen.history.push(info.load);
        if (screen.history.length > 60) screen.history.shift();
      }
      drawScreen();
      invalidate();
    },
    onHover(cb) {
      hoverCb = cb;
    },
    focus,
    project() {
      const out = new Map<string, { x: number; y: number; front: boolean }>();
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      const v = new THREE.Vector3();
      for (const [id, b] of built) {
        v.copy(b.anchor).addScaledVector(b.explode, explodeE).project(camera);
        out.set(id, { x: ((v.x + 1) / 2) * w, y: ((1 - v.y) / 2) * h, front: v.z < 1 });
      }
      return out;
    },
    onFrame(cb) {
      frameCb = cb;
    },
    invalidate,
    frames: () => count,
    dispose() {
      disposed = true;
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      ro.disconnect();
      document.removeEventListener('visibilitychange', onVisible);
      canvas.removeEventListener('pointerdown', onDown);
      canvas.removeEventListener('pointerup', onUp);
      canvas.removeEventListener('pointermove', onMove);
      canvas.removeEventListener('pointerleave', onLeave);
      canvas.removeEventListener('dblclick', onDouble);
      controls.dispose();
      composer?.dispose();
      bloom?.dispose();
      free(scene);
      for (const t of textures) t.dispose();
      env.dispose();
      room.dispose();
      pmrem.dispose();
      renderer.dispose();
      renderer.forceContextLoss();
    },
  };
}

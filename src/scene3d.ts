// My PC in 3-D (P23): the parts from `pcmodel.ts` as simple stylized meshes, drawn with Three.js.
// Loaded with the My PC tab and gone with it: `dispose` stops the loop and frees the WebGL
// context. Frames are drawn only when something moves (a fan spins, the view is dragged) or a
// reading changes, at most `maxFps` a second.

import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type { GpuShape } from './gpuspecs';
import { CASE, type Part } from './pcmodel';

export type SceneView = 'pc' | 'gpu';

export interface ScenePalette {
  /** Case edges and empty slots. */
  line: string;
  /** The board. */
  board: string;
  accent: string;
  light: boolean;
}

export interface PcScene {
  /** The parts to draw (for the GPU view, the card to show). */
  setParts(parts: Part[]): void;
  /** Per part: a colour (thermal map), 'dim' (not in the thermal mode) or nothing (own look). */
  setTint(tint: Map<string, string | 'dim'>): void;
  /** Fan turns per second, by part id. */
  setSpin(spin: Map<string, number>): void;
  /** At most this many frames a second while something moves; 0: nothing spins. */
  setMaxFps(fps: number): void;
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
const METAL = 0x8a94a6;
const DARK = 0x161b22;
const FAN = 0x2b313b;
const DIM = 0x3a4250;
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
  return new THREE.MeshStandardMaterial({ color, roughness: 0.65, metalness: 0.15, ...opts });
}

function boxMesh(size: [number, number, number], m: THREE.Material, at: [number, number, number] = [0, 0, 0]): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(...size), m);
  mesh.position.set(...at);
  return mesh;
}

/** A fan facing +z: frame ring, hub and blades (the blades turn). */
function fan(diameter: number, frameColor = DARK): THREE.Group {
  const g = new THREE.Group();
  const r = diameter / 2;
  const ring = new THREE.Mesh(new THREE.TorusGeometry(r - 2, 2.5, 6, 32), mat(frameColor));
  const hub = new THREE.Mesh(new THREE.CylinderGeometry(r * 0.28, r * 0.28, 6, 20), mat(FAN));
  hub.rotation.x = Math.PI / 2;
  const blades = new THREE.Group();
  blades.name = 'blades';
  const bladeMat = mat(FAN, { side: THREE.DoubleSide });
  for (let i = 0; i < 7; i++) {
    const b = new THREE.Mesh(new THREE.BoxGeometry(r * 0.66, r * 0.3, 1.2), bladeMat);
    b.position.set(r * 0.58, 0, 0);
    b.rotation.x = 0.45;
    const arm = new THREE.Group();
    arm.add(b);
    arm.rotation.z = (i / 7) * Math.PI * 2;
    blades.add(arm);
  }
  g.add(ring, hub, blades);
  return g;
}

/** Fins: thin plates across a box, for heatsinks and radiators. */
function fins(size: [number, number, number], along: 0 | 1 | 2, color = METAL): THREE.Group {
  const g = new THREE.Group();
  const m = mat(color, { metalness: 0.5, roughness: 0.4 });
  const n = Math.max(4, Math.floor(size[along] / 6));
  const plate: [number, number, number] = [...size];
  plate[along] = 1;
  for (let i = 0; i < n; i++) {
    const p = boxMesh(plate, m);
    const at: [number, number, number] = [0, 0, 0];
    at[along] = -size[along] / 2 + (i + 0.5) * (size[along] / n);
    p.position.set(...at);
    g.add(p);
  }
  return g;
}

/** A graphics card in its own frame: length along x, height along y, fans facing +z. */
export function card(shape: GpuShape, accent: number): THREE.Group {
  const g = new THREE.Group();
  const len = shape.lengthMm;
  const thick = shape.slots * 20.32;
  const body = mat(CARD_COLOURS[shape.colour], { metalness: 0.35, roughness: 0.5 });
  body.name = 'body';
  g.add(boxMesh([len, CARD_HEIGHT, thick - 6], body, [0, 0, 1]));
  g.add(boxMesh([len - 4, CARD_HEIGHT - 12, 2], mat(0x22272e, { metalness: 0.5 }), [0, 0, -thick / 2 + 1]));
  g.add(boxMesh([len - 10, 110, 1.6], mat(0x10301c), [0, -4, -thick / 2 + 3.5]));
  // Bracket and the light strip along the top.
  g.add(boxMesh([2, 120, thick], mat(METAL, { metalness: 0.8, roughness: 0.3 }), [-len / 2 - 1, -2, 0]));
  g.add(boxMesh([len * 0.6, 2, 3], mat(accent, { emissive: accent, emissiveIntensity: 0.6 }), [len * 0.05, CARD_HEIGHT / 2 + 0.5, thick / 2 - 6]));
  // Power plugs on the top edge, towards the far end.
  const plugs = shape.power === 'none' ? 0 : shape.power === '3x8' ? 3 : shape.power === '2x8' || shape.power === '8+6' ? 2 : 1;
  const plugW = shape.power === '16' ? 20 : 22;
  for (let i = 0; i < plugs; i++)
    g.add(boxMesh([plugW, 8, 9], mat(0x111111), [len / 2 - 40 - i * (plugW + 4), CARD_HEIGHT / 2 + 4, -thick / 2 + 12]));
  // Fans.
  const d = Math.min(96, (shape.flow ? len / 2 : len / shape.fans) - 10);
  const fans: THREE.Group[] = [];
  if (shape.flow) {
    const a = fan(d);
    a.position.set(-len / 2 + d / 2 + 18, 0, thick / 2 - 1.5);
    const b = fan(d);
    b.position.set(len / 2 - d / 2 - 18, 0, -thick / 2 - 1);
    b.rotation.y = Math.PI;
    fans.push(a, b);
  } else {
    for (let i = 0; i < shape.fans; i++) {
      const f = fan(d);
      f.position.set(-len / 2 + (i + 0.5) * (len / shape.fans), 0, thick / 2 - 1.5);
      fans.push(f);
    }
  }
  for (const f of fans) {
    f.name = 'fan';
    g.add(f);
  }
  return g;
}

interface Built {
  root: THREE.Object3D;
  /** Materials coloured by the thermal map, with their own colour. */
  tinted: Array<{ m: THREE.MeshStandardMaterial; base: number }>;
  /** Blade groups and the axis they turn about. */
  blades: Array<{ g: THREE.Object3D; axis: 'x' | 'y' | 'z' }>;
  anchor: THREE.Vector3;
}

/** One part as meshes, placed in the case. */
function build(p: Part, pal: ScenePalette, accent: number): Built {
  const root = new THREE.Group();
  const tinted: Built['tinted'] = [];
  const blades: Built['blades'] = [];
  const own = (color: number, opts?: Partial<THREE.MeshStandardMaterialParameters>) => {
    const m = mat(color, opts);
    tinted.push({ m, base: color });
    return m;
  };
  const [sx, sy, sz] = p.size;
  switch (p.kind) {
    case 'board': {
      // A little lighter than the panels, so it reads as a board.
      const c = new THREE.Color(pal.board).lerp(new THREE.Color(pal.light ? 0x6b7a90 : 0x3c4c66), 0.45);
      root.add(boxMesh([sx, sy, sz], own(c.getHex())));
      break;
    }
    case 'cooler':
    case 'radiator':
      root.add(boxMesh([sx, sy, 4], own(0x2a2f37), [0, 0, -sz / 2 + 2]));
      root.add(fins([sx, sy, sz - 6], p.kind === 'cooler' ? 2 : 0));
      root.children.at(-1)!.position.z = 3;
      for (const plate of (root.children.at(-1) as THREE.Group).children as THREE.Mesh[]) {
        const m = plate.material as THREE.MeshStandardMaterial;
        if (!tinted.some((t) => t.m === m)) tinted.push({ m, base: METAL });
      }
      break;
    case 'aio': {
      root.add(boxMesh([sx, sy, sz], own(0x1d2128)));
      const ring = new THREE.Mesh(new THREE.TorusGeometry(sx * 0.32, 2, 8, 32), mat(accent, { emissive: accent, emissiveIntensity: 0.7 }));
      ring.position.z = sz / 2 + 0.5;
      const rotor = new THREE.Group();
      rotor.add(boxMesh([sx * 0.5, 3, 1], mat(FAN)));
      rotor.position.z = sz / 2 + 1;
      root.add(ring, rotor);
      blades.push({ g: rotor, axis: 'z' });
      break;
    }
    case 'vrm':
    case 'chipset':
      root.add(boxMesh([sx, sy, sz], own(0x59616e, { metalness: 0.6, roughness: 0.35 })));
      break;
    case 'cpu':
      root.add(boxMesh([sx, sy, sz], own(0xc9ccd2, { metalness: 0.7, roughness: 0.3 })));
      break;
    case 'ram':
      root.add(boxMesh([sx, sy, sz - 4], own(0x2a3446)));
      root.add(boxMesh([sx + 0.4, sy - 6, 4], mat(accent, { emissive: accent, emissiveIntensity: 0.5 }), [0, 0, sz / 2 - 2]));
      break;
    case 'slot':
      root.add(boxMesh([sx, sy, sz], mat(new THREE.Color(pal.line).getHex())));
      break;
    case 'm2':
      root.add(boxMesh([sx, sy, sz], own(0x1a1f27)));
      root.add(boxMesh([sx * 0.7, sy * 0.6, 0.5], mat(0x3a4250), [0, 0, sz / 2 + 0.3]));
      break;
    case 'drive':
      root.add(boxMesh([sx, sy, sz], own(sy > 10 ? 0x9aa3ae : 0x20252d, { metalness: sy > 10 ? 0.6 : 0.2 })));
      break;
    case 'psu':
      root.add(boxMesh([sx, sy, sz], mat(0x14181f)));
      break;
    case 'fan': {
      // Its diameter is the size across its face (the middle of the three).
      const f = fan([sx, sy, sz].sort((a, b) => a - b)[1]! - 4);
      // Face the fan along its axis.
      if (p.fan?.axis === 'x') f.rotation.y = Math.PI / 2;
      if (p.fan?.axis === 'y') f.rotation.x = Math.PI / 2;
      root.add(f);
      blades.push({ g: f.getObjectByName('blades')!, axis: 'z' });
      break;
    }
    case 'gpu': {
      const c = card(p.gpu!.shape, accent);
      // In the case the card stands out from the board with its fans facing down.
      c.rotation.x = Math.PI / 2;
      root.add(c);
      c.traverse((o) => {
        const m = (o as THREE.Mesh).material as THREE.MeshStandardMaterial | undefined;
        if (m && m.name === 'body') tinted.push({ m, base: m.color.getHex() });
      });
      for (const f of c.children.filter((o) => o.name === 'fan')) blades.push({ g: f.getObjectByName('blades')!, axis: 'z' });
      break;
    }
  }
  root.position.set(...p.pos);
  root.traverse((o) => (o.userData.part = p.id));
  return { root, tinted, blades, anchor: new THREE.Vector3(...p.anchor) };
}

export function createScene(canvas: HTMLCanvasElement, view: SceneView, pal: ScenePalette, onpick: (id: string | null) => void): PcScene | null {
  let renderer: THREE.WebGLRenderer;
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true, powerPreference: 'low-power' });
  } catch {
    return null;
  }
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(32, 1, 5, 5000);
  scene.add(new THREE.HemisphereLight(0xffffff, pal.light ? 0x9aa3ae : 0x3a4352, pal.light ? 2.2 : 2.4));
  scene.add(new THREE.AmbientLight(0xffffff, 0.5));
  const sun = new THREE.DirectionalLight(0xffffff, 2.2);
  sun.position.set(500, 800, 1100);
  const fill = new THREE.DirectionalLight(0xbfd4ff, 0.8);
  fill.position.set(-400, 200, 600);
  scene.add(sun, fill);

  const accent = new THREE.Color(pal.accent).getHex();
  const controls = new OrbitControls(camera, canvas);
  controls.enablePan = false;
  controls.minDistance = view === 'gpu' ? 200 : 450;
  controls.maxDistance = view === 'gpu' ? 900 : 1800;

  const world = new THREE.Group();
  scene.add(world);
  let built = new Map<string, Built>();
  let spin = new Map<string, number>();
  let maxFps = 30;
  let raf = 0;
  let last = 0;
  let dirty = true;
  let count = 0;
  let frameCb: (() => void) | null = null;
  let disposed = false;

  if (view === 'pc') {
    // The case: its edges and a floor.
    const edges = new THREE.LineSegments(new THREE.EdgesGeometry(new THREE.BoxGeometry(...CASE)), new THREE.LineBasicMaterial({ color: pal.line }));
    edges.position.set(CASE[0] / 2, CASE[1] / 2, CASE[2] / 2);
    const shroud = new THREE.Mesh(new THREE.BoxGeometry(CASE[0], 100, CASE[2]), new THREE.MeshStandardMaterial({ color: 0x0d1117, transparent: true, opacity: 0.35 }));
    shroud.position.set(CASE[0] / 2, 50, CASE[2] / 2);
    scene.add(edges, shroud);
    camera.position.set(CASE[0] * 1.05, CASE[1] * 0.8, CASE[2] * 4.1);
    controls.target.set(CASE[0] * 0.48, CASE[1] * 0.52, CASE[2] * 0.3);
  } else {
    camera.position.set(-120, 140, 520);
    controls.target.set(0, 0, 0);
  }
  controls.update();

  let fitted = false;
  function size(): boolean {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (w === 0 || h === 0) return false;
    if (canvas.width !== Math.round(w * renderer.getPixelRatio()) || canvas.height !== Math.round(h * renderer.getPixelRatio())) {
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    }
    // First draw: step back on a narrow view so the whole case (or card) fits across.
    if (!fitted) {
      fitted = true;
      const k = Math.max(1, 1.25 / camera.aspect);
      camera.position.sub(controls.target).multiplyScalar(k).add(controls.target);
      controls.update();
    }
    return true;
  }

  const spinning = () => maxFps > 0 && [...built.entries()].some(([id, b]) => b.blades.length > 0 && (spin.get(id) ?? 0) > 0);

  function frame(t: number): void {
    raf = 0;
    if (disposed || document.hidden) return;
    const moving = spinning();
    const gap = 1000 / Math.max(1, maxFps || 30);
    if (!dirty && !moving) return;
    if (t - last < gap - 1) {
      raf = requestAnimationFrame(frame);
      return;
    }
    const dt = last === 0 ? 0 : Math.min(0.25, (t - last) / 1000);
    last = t;
    if (moving)
      for (const [id, b] of built)
        for (const { g, axis } of b.blades) g.rotation[axis] += (spin.get(id) ?? 0) * Math.PI * 2 * dt;
    if (size()) {
      renderer.render(scene, camera);
      count++;
      frameCb?.();
    }
    dirty = false;
    if (spinning()) raf = requestAnimationFrame(frame);
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

  // Picking: a click (not a drag) on a part.
  const ray = new THREE.Raycaster();
  let down: [number, number] | null = null;
  const onDown = (e: PointerEvent) => (down = [e.clientX, e.clientY]);
  const onUp = (e: PointerEvent) => {
    if (!down || Math.hypot(e.clientX - down[0], e.clientY - down[1]) > 5) return;
    down = null;
    const r = canvas.getBoundingClientRect();
    ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1), camera);
    const hit = ray.intersectObjects(world.children, true)[0];
    onpick((hit?.object.userData.part as string | undefined) ?? null);
  };
  canvas.addEventListener('pointerdown', onDown);
  canvas.addEventListener('pointerup', onUp);

  function free(o: THREE.Object3D): void {
    o.traverse((x) => {
      const m = x as THREE.Mesh;
      m.geometry?.dispose();
      const ms = Array.isArray(m.material) ? m.material : m.material ? [m.material] : [];
      for (const mm of ms) mm.dispose();
    });
  }

  return {
    setParts(parts) {
      for (const b of built.values()) {
        world.remove(b.root);
        free(b.root);
      }
      built = new Map();
      for (const p of parts) {
        if (view === 'gpu') {
          // The card alone, fans towards the viewer.
          const c = card(p.gpu!.shape, accent);
          const one: Built = { root: c, tinted: [], blades: [], anchor: new THREE.Vector3(0, CARD_HEIGHT / 2, 0) };
          c.traverse((o) => {
            o.userData.part = p.id;
            const m = (o as THREE.Mesh).material as THREE.MeshStandardMaterial | undefined;
            if (m && m.name === 'body') one.tinted.push({ m, base: m.color.getHex() });
          });
          for (const f of c.children.filter((o) => o.name === 'fan')) one.blades.push({ g: f.getObjectByName('blades')!, axis: 'z' });
          world.add(c);
          built.set(p.id, one);
          continue;
        }
        const b = build(p, pal, accent);
        world.add(b.root);
        built.set(p.id, b);
      }
      invalidate();
    },
    setTint(tint) {
      const dim = new THREE.Color(DIM);
      for (const [id, b] of built) {
        const t = tint.get(id);
        for (const { m, base } of b.tinted) {
          if (t === 'dim') m.color.copy(dim);
          else if (t) m.color.set(t);
          else m.color.setHex(base);
        }
      }
      invalidate();
    },
    setSpin(s) {
      spin = s;
      invalidate();
    },
    setMaxFps(fps) {
      maxFps = fps;
      invalidate();
    },
    project() {
      const out = new Map<string, { x: number; y: number; front: boolean }>();
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      const v = new THREE.Vector3();
      for (const [id, b] of built) {
        v.copy(b.anchor).project(camera);
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
      controls.dispose();
      free(scene);
      renderer.dispose();
      renderer.forceContextLoss();
    },
  };
}

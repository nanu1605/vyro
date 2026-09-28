// Animation core (spec §4.3): data arrives at ~1 Hz, frames run at display rate. Values ease
// toward their latest target with `cur += (target - cur) * (1 - e^(-dt/τ))`.
//
// A single requestAnimationFrame loop drives every subscriber and stops itself once nothing is
// moving, so an idle dashboard costs no frames between updates. It also stops while the page
// is hidden.

/** Easing time constant, seconds. */
export const TAU = 0.22;

/** Distance below which a value is considered settled. */
const EPSILON = 0.01;

/** Longest frame step honoured, so a stalled tab doesn't jump. */
const MAX_DT = 0.1;

/** Frames closer than this are skipped: about 60 fps, also on 120–240 Hz displays, where
 * easing looks the same and full-rate frames only cost CPU and GPU time. */
const MIN_FRAME_MS = 1000 / 60 - 2;

export function ease(cur: number, target: number, dt: number, tau = TAU): number {
  return cur + (target - cur) * (1 - Math.exp(-dt / tau));
}

export class Tween {
  cur: number;
  target: number;

  constructor(value = 0) {
    this.cur = value;
    this.target = value;
  }

  set(target: number): void {
    this.target = target;
  }

  /** Advances by `dt` seconds. Returns true while still moving. */
  step(dt: number, snap = false, tau = TAU): boolean {
    if (snap || Math.abs(this.target - this.cur) < EPSILON) {
      this.cur = this.target;
      return false;
    }
    this.cur = ease(this.cur, this.target, dt, tau);
    return true;
  }
}

/** Per-frame callback; return true to request another frame. */
export type FrameFn = (dt: number) => boolean;

/** Animation level (profiles): full easing, shorter easing, or none. */
export type MotionLevel = 'full' | 'reduced' | 'off';

/** Easing time constant for the reduced level: settles in about a third of the time. */
const TAU_REDUCED = 0.08;

export class Animator {
  /** The OS asks for reduced motion (always treated as off). */
  readonly reducedMotion: boolean;
  private level: MotionLevel = 'full';
  private subs = new Set<FrameFn>();
  private raf = 0;
  private last = 0;
  private hidden = false;
  private frames = 0;
  private windowStart = 0;
  private measuredFps = 0;

  constructor(reducedMotion = false) {
    this.reducedMotion = reducedMotion;
  }

  setMotion(level: MotionLevel): void {
    this.level = level;
    this.wake();
  }

  /** Jump to new values instead of easing. */
  get snap(): boolean {
    return this.reducedMotion || this.level === 'off';
  }

  /** Easing time constant for the current level. */
  get tau(): number {
    return this.level === 'reduced' ? TAU_REDUCED : TAU;
  }

  add(fn: FrameFn): () => void {
    this.subs.add(fn);
    this.wake();
    return () => this.subs.delete(fn);
  }

  /** Requests frames after new targets were set. */
  wake(): void {
    if (this.raf || this.hidden || typeof requestAnimationFrame === 'undefined') return;
    this.last = performance.now();
    this.raf = requestAnimationFrame(this.loop);
  }

  setHidden(hidden: boolean): void {
    this.hidden = hidden;
    if (hidden && this.raf) {
      cancelAnimationFrame(this.raf);
      this.raf = 0;
    } else if (!hidden) {
      this.wake();
    }
  }

  /** Frames rendered over the last second (0 when idle — the loop sleeps). */
  get fps(): number {
    return performance.now() - this.windowStart > 2000 ? 0 : this.measuredFps;
  }

  private loop = (now: number): void => {
    if (now - this.last < MIN_FRAME_MS) {
      this.raf = this.hidden ? 0 : requestAnimationFrame(this.loop);
      return;
    }
    const dt = Math.min((now - this.last) / 1000, MAX_DT);
    this.last = now;
    this.countFrame(now);
    let busy = false;
    for (const fn of this.subs) busy = fn(dt) || busy;
    this.raf = busy && !this.hidden ? requestAnimationFrame(this.loop) : 0;
  };

  private countFrame(now: number): void {
    this.frames++;
    if (now - this.windowStart >= 1000) {
      this.measuredFps = this.windowStart ? Math.round((this.frames * 1000) / (now - this.windowStart)) : 0;
      this.frames = 0;
      this.windowStart = now;
    }
  }
}

const prefersReducedMotion =
  typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

export const animator = new Animator(prefersReducedMotion);

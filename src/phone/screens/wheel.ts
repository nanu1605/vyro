// The phone wheel (2.6): the phone, held like a steering wheel, steers games on the PC. What is
// worked out here is plain arithmetic, tested on its own: the steering angle from the phone's
// sensors, the pedals from where a finger is, and the 12-byte packet the PC reads
// (`crates/clokrix-core/src/wheel/packet.rs`).
//
// Steering: the gravity the phone feels (its accelerometer) is taken in the phone's own axes when
// Centre is pressed, and each reading after is compared with it, in the phone's screen plane: the
// signed angle between the two is how far the phone is turned, right positive. A phone that
// reports gravity the other way round (iPhones) flips both, so the angle is the same. The gyro
// smooths it between readings (a complementary filter), its sign learnt from the accelerometer,
// and carries it alone where gravity says little (the phone held near flat).

/** The packet's layout version, kinds and length (as the PC's). */
export const VERSION = 1;
export const KIND_STATE = 0;
export const KIND_NEUTRAL = 1;
export const KIND_ECHO = 2;
export const KIND_RUMBLE = 3;
export const LEN = 12;

/** An Xbox controller's buttons, as Windows' XInput numbers them. Guide is never sent. */
export const BUTTONS = {
  up: 0x0001,
  down: 0x0002,
  left: 0x0004,
  right: 0x0008,
  start: 0x0010,
  back: 0x0020,
  lthumb: 0x0040,
  rthumb: 0x0080,
  lb: 0x0100,
  rb: 0x0200,
  a: 0x1000,
  b: 0x2000,
  x: 0x4000,
  y: 0x8000,
} as const;
export type Button = keyof typeof BUTTONS;

export interface Pad {
  /** -1 (full left) to 1 (full right). */
  steer: number;
  /** 0 to 1. */
  throttle: number;
  brake: number;
  buttons: number;
}

/** A packet for the PC: the controller now, or (`neutral`) everything let go. */
export function encode(seq: number, t: number, pad: Pad | null): Uint8Array<ArrayBuffer> {
  const b = new Uint8Array(LEN);
  const v = new DataView(b.buffer);
  b[0] = VERSION;
  b[1] = pad ? KIND_STATE : KIND_NEUTRAL;
  v.setUint16(2, seq & 0xffff, true);
  v.setUint16(4, t & 0xffff, true);
  if (pad) {
    const steer = Math.round(Math.max(-1, Math.min(1, pad.steer || 0)) * 32767);
    v.setInt16(6, steer, true);
    b[8] = Math.round(Math.max(0, Math.min(1, pad.throttle || 0)) * 255);
    b[9] = Math.round(Math.max(0, Math.min(1, pad.brake || 0)) * 255);
    v.setUint16(10, pad.buttons & ~0x0c00 & 0xffff, true);
  }
  return b;
}

/** What the PC sent back: the newest packet echoed (its number and the phone's time), or the
 * game's rumble (each motor 0 to 255). Null for anything else. */
export function decode(data: ArrayBuffer | Uint8Array): { echo: { seq: number; t: number } } | { rumble: { large: number; small: number } } | null {
  const b = data instanceof Uint8Array ? data : new Uint8Array(data);
  if (b.length < 4 || b[0] !== VERSION) return null;
  const v = new DataView(b.buffer, b.byteOffset, b.byteLength);
  if (b[1] === KIND_ECHO && b.length >= 6) return { echo: { seq: v.getUint16(2, true), t: v.getUint16(4, true) } };
  if (b[1] === KIND_RUMBLE) return { rumble: { large: b[2]!, small: b[3]! } };
  return null;
}

/** The time since `t` (a 16-bit millisecond clock) on that clock: a round trip. */
export function since(now: number, t: number): number {
  return (Math.round(now) - t) & 0xffff;
}

export type Vec = { x: number; y: number; z: number };

/** The signed angle (radians, right positive) the phone turned in its screen plane from `from`
 * to `to`, both gravity in the phone's axes; null where either says too little (the phone near
 * flat, so its screen plane holds almost none of gravity). */
export function turned(from: Vec, to: Vec): number | null {
  const a = Math.hypot(from.x, from.y);
  const b = Math.hypot(to.x, to.y);
  const g = Math.hypot(to.x, to.y, to.z);
  if (a < 1e-6 || b < 1e-6 || !(g > 0) || b / g < 0.25 || a / Math.hypot(from.x, from.y, from.z) < 0.25) return null;
  const cross = from.x * to.y - from.y * to.x;
  const dot = from.x * to.x + from.y * to.y;
  return Math.atan2(cross, dot);
}

/** The steering, sample after sample: gravity's angle from Centre, smoothed with the gyro. */
export class Steering {
  /** Gravity when Centre was pressed (or the first reading). */
  private centre: Vec | null = null;
  /** The angle now, radians, right positive. */
  angle = 0;
  /** The gyro's rate about the screen's axis counts as turning right (1) or left (-1); learnt
   * from the accelerometer as the phone turns. */
  private sign = 0;
  private evidence = 0;
  private last: number | null = null;

  /** Turning the phone this far (degrees) is full lock. */
  constructor(public range = 90) {}

  /** Centre is where the phone is now. */
  recentre(): void {
    this.centre = null;
    this.angle = 0;
  }

  /** A reading: `g`, gravity in the phone's axes (accelerationIncludingGravity); `rate`, the
   * gyro's degrees a second about the screen's axis (rotationRate.alpha), if any; `dt`, seconds
   * since the last. Gives the steering, -1 to 1. */
  sample(g: Vec, rate: number | null, dt: number): number {
    if (!this.centre) {
      if (Math.hypot(g.x, g.y) > 1) this.centre = { ...g };
      this.last = 0;
      return 0;
    }
    const measured = turned(this.centre, g);
    const step = dt > 0 && dt < 0.5 ? dt : 0;
    let gyro: number | null = null;
    if (rate !== null && Number.isFinite(rate) && step > 0) {
      const d = (rate * Math.PI) / 180;
      // Learn which way the gyro counts: against how the accelerometer's angle moved.
      if (measured !== null && this.last !== null) {
        const moved = measured - this.last;
        if (Math.abs(moved) > 0.002 && Math.abs(d) > 0.05) this.evidence += Math.sign(moved * d);
        if (Math.abs(this.evidence) >= 5) this.sign = Math.sign(this.evidence);
      }
      if (this.sign !== 0) gyro = this.angle + this.sign * d * step;
    }
    if (measured !== null) {
      this.angle = gyro === null ? measured : 0.85 * gyro + 0.15 * measured;
      this.last = measured;
    } else if (gyro !== null) {
      this.angle = gyro;
    }
    const lock = (Math.max(15, Math.min(450, this.range)) * Math.PI) / 180;
    return Math.max(-1, Math.min(1, this.angle / lock));
  }
}

/** A pedal pressed at `y` within an area from `top` to `bottom`: 0 at the bottom, 1 at the top
 * (a finger slides up to press harder). */
export function pedal(y: number, top: number, bottom: number): number {
  if (!(bottom > top)) return 0;
  return Math.max(0, Math.min(1, (bottom - y) / (bottom - top)));
}

/** How long the phone vibrates for the game's rumble, ms (0: stop). */
export function vibration(large: number, small: number): number {
  const s = Math.max(large, small);
  return s < 16 ? 0 : Math.round(30 + (s / 255) * 70);
}

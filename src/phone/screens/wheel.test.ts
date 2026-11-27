import { describe, expect, it } from 'vitest';
import { BUTTONS, decode, encode, LEN, pedal, since, Steering, turned, vibration, type Vec } from './wheel';

/** Gravity in the phone's axes, held upright and turned `deg` clockwise (as seen facing its
 * screen), leaning back by `back` degrees; `flip` as an iPhone reports it. */
function gravity(deg: number, back = 0, flip = false): Vec {
  const r = (deg * Math.PI) / 180;
  const l = (back * Math.PI) / 180;
  const k = flip ? -9.81 : 9.81;
  return { x: -Math.sin(r) * Math.cos(l) * k, y: Math.cos(r) * Math.cos(l) * k, z: Math.sin(l) * k };
}

describe('the phone wheel (2.6)', () => {
  it('packs the controller as the PC reads it', () => {
    const b = encode(0x1_0007, 0x1_fde8, { steer: -0.5, throttle: 1, brake: 0.2, buttons: BUTTONS.a | BUTTONS.rb | 0x0400 });
    expect(b.length).toBe(LEN);
    const v = new DataView(b.buffer);
    expect([b[0], b[1]]).toEqual([1, 0]);
    expect(v.getUint16(2, true)).toBe(7);
    expect(v.getUint16(4, true)).toBe(0xfde8);
    expect(v.getInt16(6, true)).toBe(-16383);
    expect([b[8], b[9]]).toEqual([255, 51]);
    // Guide is never sent.
    expect(v.getUint16(10, true)).toBe(BUTTONS.a | BUTTONS.rb);
    // Out of range is held to it.
    const far = new DataView(encode(1, 1, { steer: -3, throttle: 2, brake: -1, buttons: 0 }).buffer);
    expect([far.getInt16(6, true), far.getUint8(8), far.getUint8(9)]).toEqual([-32767, 255, 0]);
    const neutral = encode(2, 3, null);
    expect([neutral[1], ...neutral.slice(6)]).toEqual([1, 0, 0, 0, 0, 0, 0]);
  });

  it('reads the echo and the rumble, and times the round trip across the wrap', () => {
    expect(decode(new Uint8Array([1, 2, 7, 0, 0xe8, 0xfd]))).toEqual({ echo: { seq: 7, t: 0xfde8 } });
    expect(decode(new Uint8Array([1, 3, 200, 10]))).toEqual({ rumble: { large: 200, small: 10 } });
    expect(decode(new Uint8Array([2, 3, 1, 1]))).toBeNull();
    expect(decode(new Uint8Array([1, 9, 1, 1]))).toBeNull();
    expect(since(70_000, 70_000 - 25)).toBe(25);
    expect(since(65_546, 65_520)).toBe(26);
  });

  it('turns right as positive, whichever way the phone reports gravity, upright or leaning back', () => {
    for (const flip of [false, true]) {
      for (const back of [0, 40]) {
        const a = turned(gravity(0, back, flip), gravity(30, back, flip))!;
        expect(a * (180 / Math.PI)).toBeCloseTo(30, 0);
        const b = turned(gravity(10, back, flip), gravity(-20, back, flip))!;
        expect(b * (180 / Math.PI)).toBeCloseTo(-30, 0);
      }
    }
    // Near flat, gravity says too little.
    expect(turned(gravity(0, 85), gravity(20, 85))).toBeNull();
  });

  it('steers by the turn from centre, to full lock at the range', () => {
    const s = new Steering(90);
    expect(s.sample(gravity(12), null, 0.016)).toBe(0); // centre taken here
    expect(s.sample(gravity(57), null, 0.016)).toBeCloseTo(0.5, 2);
    expect(s.sample(gravity(12 + 120), null, 0.016)).toBe(1);
    expect(s.sample(gravity(12 - 45), null, 0.016)).toBeCloseTo(-0.5, 2);
    s.recentre();
    s.sample(gravity(-33), null, 0.016);
    expect(s.sample(gravity(-33), null, 0.016)).toBeCloseTo(0, 5);
  });

  it('learns which way the gyro counts and holds steady through a reading gravity cannot give', () => {
    for (const gyroSign of [1, -1]) {
      const s = new Steering(90);
      const dt = 1 / 60;
      s.sample(gravity(0), 0, dt);
      // Turning right at 60° a second for a second: the gyro (in its own sign) and gravity agree.
      let deg = 0;
      for (let i = 0; i < 60; i++) {
        deg += 1;
        s.sample(gravity(deg), gyroSign * 60, dt);
      }
      expect(s.angle * (180 / Math.PI)).toBeCloseTo(60, 0);
      // Held still near flat (gravity useless) the angle stays where the gyro has it.
      const before = s.angle;
      for (let i = 0; i < 30; i++) s.sample(gravity(60, 85), 0, dt);
      expect(s.angle).toBeCloseTo(before, 5);
    }
  });

  it('presses the pedals by how far up the finger is, and vibrates with the rumble', () => {
    expect(pedal(100, 0, 100)).toBe(0);
    expect(pedal(0, 0, 100)).toBe(1);
    expect(pedal(25, 0, 100)).toBeCloseTo(0.75);
    expect(pedal(-50, 0, 100)).toBe(1);
    expect(pedal(10, 50, 50)).toBe(0);
    expect(vibration(0, 10)).toBe(0);
    expect(vibration(255, 0)).toBe(100);
    expect(vibration(0, 128)).toBeGreaterThan(30);
  });
});

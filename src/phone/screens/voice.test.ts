import { describe, expect, it } from 'vitest';
import { FRAME_SAMPLES, Framer, frame, join, level, pieces, RATE, Resampler, toI16, wav } from './voice';

describe('the phone’s voice', () => {
  it('takes 48 kHz down to 16 kHz, a block at a time, without losing samples between blocks', () => {
    const r = new Resampler(48_000);
    const second = new Float32Array(48_000).fill(0.5);
    let n = 0;
    for (let at = 0; at < second.length; at += 128) n += r.push(second.subarray(at, at + 128)).length;
    expect(n).toBe(RATE);
  });

  it('takes 44.1 kHz down to 16 kHz', () => {
    const r = new Resampler(44_100);
    const out = r.push(new Float32Array(44_100).fill(-0.25));
    expect(Math.abs(out.length - RATE)).toBeLessThanOrEqual(1);
    expect(out[10]).toBe(toI16(-0.25));
  });

  it('averages what one sample covers (a tone too high for 16 kHz goes)', () => {
    const r = new Resampler(48_000);
    const alternating = Float32Array.from({ length: 300 }, (_, i) => (i % 3 === 0 ? 1 : -0.5));
    for (const s of r.push(alternating)) expect(Math.abs(s)).toBeLessThan(10);
  });

  it('clips to 16 bits', () => {
    expect(toI16(2)).toBe(0x7fff);
    expect(toI16(-2)).toBe(-0x8000);
    expect(toI16(0)).toBe(0);
  });

  it('writes a WAV the PC can read', () => {
    const pcm = Int16Array.from([0, 1000, -1000, 32767]);
    const file = wav(pcm);
    const v = new DataView(file.buffer);
    const text = (at: number, n: number) => String.fromCharCode(...file.subarray(at, at + n));
    expect(text(0, 4)).toBe('RIFF');
    expect(text(8, 4)).toBe('WAVE');
    expect(v.getUint32(24, true)).toBe(RATE);
    expect(v.getUint16(34, true)).toBe(16);
    expect(v.getUint32(40, true)).toBe(8);
    expect(v.getInt16(46, true)).toBe(1000);
    expect(file.length).toBe(44 + 8);
  });

  it('splits into whole 20 ms frames and keeps the rest', () => {
    const f = new Framer();
    expect(f.push(new Int16Array(FRAME_SAMPLES - 1))).toHaveLength(0);
    const two = f.push(new Int16Array(FRAME_SAMPLES + 1));
    expect(two).toHaveLength(2);
    expect(two.every((x) => x.length === FRAME_SAMPLES)).toBe(true);
    const msg = frame(7, two[0]!);
    expect(new DataView(msg.buffer).getUint32(0, true)).toBe(7);
    expect(msg.length).toBe(4 + FRAME_SAMPLES * 2);
  });

  it('cuts bytes into pieces and joins blocks', () => {
    expect(pieces(new Uint8Array(10), 4).map((p) => p.length)).toEqual([4, 4, 2]);
    expect(Array.from(join([Int16Array.from([1, 2]), Int16Array.from([3])]))).toEqual([1, 2, 3]);
  });

  it('says how loud', () => {
    expect(level(new Int16Array(100))).toBe(0);
    expect(level(new Int16Array(100).fill(8000))).toBe(1);
    expect(level(new Int16Array(100).fill(4000))).toBeCloseTo(0.5);
  });
});

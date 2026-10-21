// The phone's voice for the PC (1.7): a clip recorded here and put on the PC's clipboard, and the
// live microphone (Part E). Both take the microphone at the phone's own rate and keep 16 kHz mono
// 16-bit, enough for a voice. The pure parts (resampling, framing, WAV) are tested; `capture`
// needs a browser.

/** What the PC is sent: 16 kHz, mono, 16-bit. */
export const RATE = 16_000;
/** A live frame: 20 ms. */
export const FRAME_SAMPLES = RATE / 50;
/** The longest clip: on a direct connection, and through the relay (the slow view). */
export const CLIP_SECS = { direct: 300, relay: 60 } as const;
/** A clip's bytes go in pieces of this many (data channel messages; relay requests). */
export const PIECE_DIRECT = 16 * 1024;
export const PIECE_RELAY = 24 * 1024;

/** 48 or 44.1 kHz (whatever the phone's microphone runs at) to 16 kHz, a block at a time: each
 * output sample is the average of the input it covers, which keeps what a voice needs and drops
 * what 16 kHz cannot carry. It carries the remainder between blocks. */
export class Resampler {
  private step: number;
  private acc = 0;
  private count = 0;
  private pos = 0;

  constructor(from: number, to = RATE) {
    this.step = from / to;
  }

  push(input: Float32Array): Int16Array {
    const out: number[] = [];
    for (let i = 0; i < input.length; i++) {
      this.acc += input[i] ?? 0;
      this.count++;
      this.pos += 1;
      if (this.pos >= this.step) {
        this.pos -= this.step;
        out.push(toI16(this.acc / this.count));
        this.acc = 0;
        this.count = 0;
      }
    }
    return Int16Array.from(out);
  }
}

export function toI16(v: number): number {
  const c = Math.max(-1, Math.min(1, v));
  return Math.round(c < 0 ? c * 0x8000 : c * 0x7fff);
}

/** How loud a block is, 0–1 (for the level meter). */
export function level(samples: Int16Array): number {
  if (!samples.length) return 0;
  let sum = 0;
  for (const s of samples) sum += s * s;
  return Math.min(1, Math.sqrt(sum / samples.length) / 8000);
}

/** A WAV file of 16-bit mono PCM at `rate`. */
export function wav(pcm: Int16Array, rate = RATE): Uint8Array {
  const data = pcm.length * 2;
  const out = new Uint8Array(44 + data);
  const v = new DataView(out.buffer);
  const text = (at: number, s: string) => [...s].forEach((c, i) => v.setUint8(at + i, c.charCodeAt(0)));
  text(0, 'RIFF');
  v.setUint32(4, 36 + data, true);
  text(8, 'WAVE');
  text(12, 'fmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true); // PCM
  v.setUint16(22, 1, true); // mono
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  text(36, 'data');
  v.setUint32(40, data, true);
  for (let i = 0; i < pcm.length; i++) v.setInt16(44 + i * 2, pcm[i] ?? 0, true);
  return out;
}

/** Blocks of PCM joined. */
export function join(blocks: Int16Array[]): Int16Array {
  const out = new Int16Array(blocks.reduce((n, b) => n + b.length, 0));
  let at = 0;
  for (const b of blocks) {
    out.set(b, at);
    at += b.length;
  }
  return out;
}

/** `bytes` in pieces of at most `size`. */
export function pieces(bytes: Uint8Array, size: number): Uint8Array[] {
  const out: Uint8Array[] = [];
  for (let at = 0; at < bytes.length; at += size) out.push(bytes.subarray(at, Math.min(bytes.length, at + size)));
  return out;
}

/** A live frame for the PC: its number (u32, little-endian), then the samples. */
export function frame(seq: number, samples: Int16Array): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(4 + samples.length * 2);
  const v = new DataView(out.buffer);
  v.setUint32(0, seq >>> 0, true);
  for (let i = 0; i < samples.length; i++) v.setInt16(4 + i * 2, samples[i] ?? 0, true);
  return out;
}

/** Splits PCM into whole live frames, keeping what is left for the next block. */
export class Framer {
  private rest = new Int16Array(0);
  push(samples: Int16Array): Int16Array[] {
    const all = join([this.rest, samples]);
    const out: Int16Array[] = [];
    let at = 0;
    for (; at + FRAME_SAMPLES <= all.length; at += FRAME_SAMPLES) out.push(all.slice(at, at + FRAME_SAMPLES));
    this.rest = all.slice(at);
    return out;
  }
}

/** The microphone, as 16 kHz blocks until `stop()`. Echo cancellation and noise suppression are
 * the phone's own. */
export interface Capture {
  stop(): void;
}

/** The audio thread's tap, served as a file of the app's own: the relay's policy runs no
 * script from anywhere else, `blob:` included. */
const WORKLET = new URL('./tap.worklet.js', import.meta.url);

export async function capture(onBlock: (pcm: Int16Array) => void): Promise<Capture> {
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const ctx = new AudioContext();
  try {
    await ctx.audioWorklet.addModule(WORKLET);
    const source = ctx.createMediaStreamSource(stream);
    const tap = new AudioWorkletNode(ctx, 'vyro-tap');
    const resampler = new Resampler(ctx.sampleRate);
    tap.port.onmessage = (e: MessageEvent<Float32Array>) => {
      const pcm = resampler.push(e.data);
      if (pcm.length) onBlock(pcm);
    };
    source.connect(tap);
    // Nothing is played: the tap is not connected to the speakers.
    return {
      stop() {
        tap.port.onmessage = null;
        source.disconnect();
        for (const t of stream.getTracks()) t.stop();
        void ctx.close().catch(() => {});
      },
    };
  } catch (e) {
    for (const t of stream.getTracks()) t.stop();
    void ctx.close().catch(() => {});
    throw e;
  }
}

/** Why the microphone could not be used, in words. */
export function micError(e: unknown): string {
  const name = e instanceof DOMException ? e.name : '';
  if (name === 'NotAllowedError') return 'This phone did not allow the microphone. Allow it for this app in the phone’s settings.';
  if (name === 'NotFoundError') return 'This phone has no microphone it lets apps use.';
  return e instanceof Error ? e.message : String(e);
}

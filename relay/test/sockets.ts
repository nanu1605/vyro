// PCs and phones as the relay tests play them: WebSockets with a queue of what they heard. The
// frames are stand-ins: the relay never opens them.

import { createHash, randomBytes } from 'node:crypto';
import { expect } from 'vitest';
import { MIN_SEALED } from '../src/rules';

export type Msg = string | Uint8Array;

export class Sock {
  readonly ws: WebSocket;
  private queue: Msg[] = [];
  private waiters: Array<(m: Msg) => void> = [];
  closed: Promise<{ code: number; reason: string }>;
  opened: Promise<void>;

  constructor(url: string, protocols: string[]) {
    this.ws = new WebSocket(url, protocols);
    this.ws.binaryType = 'arraybuffer';
    this.opened = new Promise((resolve, reject) => {
      this.ws.addEventListener('open', () => resolve());
      this.ws.addEventListener('error', () => reject(new Error('socket failed')));
    });
    this.closed = new Promise((resolve) => this.ws.addEventListener('close', (e) => resolve({ code: e.code, reason: e.reason })));
    this.ws.addEventListener('message', (e) => {
      const m: Msg = typeof e.data === 'string' ? e.data : new Uint8Array(e.data as ArrayBuffer);
      const w = this.waiters.shift();
      if (w) w(m);
      else this.queue.push(m);
    });
  }

  next(ms = 5000): Promise<Msg> {
    const q = this.queue.shift();
    if (q !== undefined) return Promise.resolve(q);
    return new Promise((resolve, reject) => {
      const w = (m: Msg) => {
        clearTimeout(t);
        resolve(m);
      };
      // A waiter that gave up leaves the line, so the next message is not lost to it.
      const t = setTimeout(() => {
        this.waiters = this.waiters.filter((x) => x !== w);
        reject(new Error('no message'));
      }, ms);
      this.waiters.push(w);
    });
  }

  async json(ms?: number): Promise<unknown> {
    const m = await this.next(ms);
    if (typeof m !== 'string') throw new Error('expected text');
    return JSON.parse(m) as unknown;
  }

  /** Nothing arrives for `ms`. */
  async quiet(ms = 300): Promise<boolean> {
    try {
      await this.next(ms);
      return false;
    } catch {
      return true;
    }
  }

  close(): void {
    this.ws.close();
  }
}

export const b64url = (b: Uint8Array) => Buffer.from(b).toString('base64url');
export const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');
export const room = () => randomBytes(16).toString('hex');
export const key = () => new Uint8Array(randomBytes(32));
/** A stand-in for a sealed frame (version, nonce, ciphertext, tag). */
export const sealed = (fill: number, n = MIN_SEALED + 8) => new Uint8Array(n).fill(fill);

/** Sockets to the relay at `base` (http://host:port). */
export function relayAt(base: string) {
  function sock(path: string, k?: Uint8Array): Sock {
    return new Sock(`${base.replace(/^http/, 'ws')}${path}`, k ? ['vyro.1', `k.${b64url(k)}`] : ['vyro.1']);
  }

  async function pc(r: string, k: Uint8Array): Promise<Sock> {
    const s = sock(`/pc/${r}`, k);
    await s.opened;
    expect(await s.json()).toEqual({ t: 'here', phones: [], push: true });
    return s;
  }

  async function register(p: Sock, phones: Array<{ id: string; auth: Uint8Array }>): Promise<void> {
    p.ws.send(JSON.stringify({ t: 'phones', list: phones.map((x) => ({ id: x.id, hash: sha(x.auth) })) }));
    expect(await p.json()).toMatchObject({ t: 'here', listed: true });
  }

  return { sock, pc, register };
}

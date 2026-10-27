// The relay's own key for push services (2.0): VAPID (RFC 8292). One for the whole relay, made the
// first time it is needed and kept in this Durable Object. A phone subscribes with its public
// half (`/push/key`), and push services take notifications for that subscription only when the
// relay signs for them with the private half, which never leaves here.

import { DurableObject } from 'cloudflare:workers';
import { b64url, vapidToken } from './rules';

/** The Durable Object's name: there is one. */
export const KEYS_NAME = 'vapid';
/** A token is valid for 12 hours (push services take at most 24) and reused for one. */
const TOKEN_S = 12 * 3600;
const REUSE_MS = 3600_000;

interface Stored {
  jwk: JsonWebKey;
  /** The public key, uncompressed, base64url. */
  pub: string;
}

const ALGORITHM = { name: 'ECDSA', namedCurve: 'P-256' } as const;

export class Keys extends DurableObject {
  private key: Promise<{ privateKey: CryptoKey; publicKey: string }> | null = null;
  private tokens = new Map<string, { token: string; until: number }>();

  private load(): Promise<{ privateKey: CryptoKey; publicKey: string }> {
    // Two first requests at once share one key.
    this.key ??= this.ctx.blockConcurrencyWhile(async () => {
      let stored = await this.ctx.storage.get<Stored>('vapid');
      if (!stored) {
        const pair = (await crypto.subtle.generateKey(ALGORITHM, true, ['sign', 'verify'])) as CryptoKeyPair;
        const jwk = (await crypto.subtle.exportKey('jwk', pair.privateKey)) as JsonWebKey;
        const raw = new Uint8Array((await crypto.subtle.exportKey('raw', pair.publicKey)) as ArrayBuffer);
        stored = { jwk, pub: b64url(raw) };
        await this.ctx.storage.put('vapid', stored);
      }
      const privateKey = await crypto.subtle.importKey('jwk', stored.jwk, ALGORITHM, false, ['sign']);
      return { privateKey, publicKey: stored.pub };
    });
    return this.key;
  }

  async publicKey(): Promise<string> {
    return (await this.load()).publicKey;
  }

  /** The `Authorization` header for a push service at `aud` (its origin); `sub` names the relay
   * (its address), as push services ask. */
  async authorization(aud: string, sub: string): Promise<string> {
    const k = await this.load();
    const now = Date.now();
    let kept = this.tokens.get(aud);
    if (!kept || kept.until <= now) {
      kept = { token: await vapidToken(k.privateKey, aud, sub, Math.floor(now / 1000) + TOKEN_S), until: now + REUSE_MS };
      this.tokens.set(aud, kept);
    }
    return `vapid t=${kept.token}, k=${k.publicKey}`;
  }
}

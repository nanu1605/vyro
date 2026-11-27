// Every order and key (2.5), in one SQLite-backed Durable Object. An order is what the checkout
// made with Razorpay; once paid it gets a licence: its key, and the one PC it is Pro on. Nothing
// about the buyer is kept here: Razorpay has that.
//
// A licence bought while the checkout ran on Razorpay's Test keys is marked `test`: anyone can
// pay there with Razorpay's public test cards. Once the checkout runs on Live keys, such a key no
// longer turns Pro on (the owner, 2026-11-16: "Cancel test keys at go-live").
//
// A key given on the key page (2.5.5), not sold, has no order: its payment is `given:` and a
// random id, and it is never test, so it turns Pro on whichever keys the checkout runs on. The
// page gives one only to the owner's passkey (webauthn.ts), kept here with the setup it was let
// in under: a hash of the setup passphrase, so a new passphrase starts the setup again.

import { DurableObject } from 'cloudflare:workers';
import { CHALLENGE_SECONDS, MAX_DEVICES, MOVE_DAYS, newKey, RATE_PER_MIN, WRONG_PER_HOUR } from './rules';

export const LEDGER_NAME = 'ledger';

export interface Licence {
  lic: string;
  key: string;
  pc: string | null;
  revoked: boolean;
}

export type Activated =
  | { ok: true; lic: string }
  | { ok: false; status: number; error: 'no-key' | 'revoked' | 'in-use' | 'moved-recently'; until?: number };

export type Checked = { ok: true } | { ok: false; reason: 'no-key' | 'revoked' | 'moved' | 'released' };

/** The owner's passkey: its credential id, the website it is for, its public key (SPKI, base64url)
 * and algorithm, its signature counter, the setup it was let in under, and its device's name
 * (rules.ts, deviceName). */
export interface Passkey {
  id: string;
  rp: string;
  spki: string;
  alg: number;
  count: number;
  setup: string;
  name: string | null;
}

/** A device set up, as the owner sees it to remove one: when it was set up and last signed in. */
export interface Device {
  id: string;
  rp: string;
  name: string | null;
  created: number;
  used: number | null;
}

const DAY = 86_400;

export class Ledger extends DurableObject<object> {
  private sql = this.ctx.storage.sql;

  constructor(ctx: DurableObjectState, env: object) {
    super(ctx, env);
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS orders (
        id TEXT PRIMARY KEY, claim TEXT, created INTEGER NOT NULL, lic TEXT, collected INTEGER NOT NULL DEFAULT 0
      );
      CREATE INDEX IF NOT EXISTS orders_claim ON orders (claim);
      CREATE TABLE IF NOT EXISTS licences (
        lic TEXT PRIMARY KEY, key TEXT NOT NULL UNIQUE, payment TEXT NOT NULL UNIQUE, issued INTEGER NOT NULL,
        pc TEXT, activated INTEGER, moved INTEGER, revoked INTEGER
      );
      CREATE TABLE IF NOT EXISTS hits (ip TEXT NOT NULL, minute INTEGER NOT NULL, n INTEGER NOT NULL, PRIMARY KEY (ip, minute));
      CREATE TABLE IF NOT EXISTS wrong (at INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS passkeys (
        id TEXT PRIMARY KEY, rp TEXT NOT NULL, spki TEXT NOT NULL, alg INTEGER NOT NULL, count INTEGER NOT NULL, setup TEXT NOT NULL,
        created INTEGER NOT NULL
      );
      CREATE TABLE IF NOT EXISTS used (value TEXT PRIMARY KEY, at INTEGER NOT NULL);
      CREATE TABLE IF NOT EXISTS codes (code TEXT PRIMARY KEY, challenge TEXT NOT NULL, at INTEGER NOT NULL);
    `);
    // Added after the first deploy, when every key yet had been bought under Test keys: so they
    // all count as test.
    const cols = this.sql.exec<{ name: string }>('PRAGMA table_info(licences)').toArray();
    if (!cols.some((c) => c.name === 'test')) this.sql.exec('ALTER TABLE licences ADD COLUMN test INTEGER NOT NULL DEFAULT 1');
    // Added with removing a device: a passkey set up before has no name until it signs in.
    const keyCols = this.sql.exec<{ name: string }>('PRAGMA table_info(passkeys)').toArray();
    if (!keyCols.some((c) => c.name === 'name')) this.sql.exec('ALTER TABLE passkeys ADD COLUMN name TEXT');
    if (!keyCols.some((c) => c.name === 'used')) this.sql.exec('ALTER TABLE passkeys ADD COLUMN used INTEGER');
  }

  private now(): number {
    return Math.floor(Date.now() / 1000);
  }

  /** One more request from `ip`: false once it has made too many this minute. */
  hit(ip: string, limit = RATE_PER_MIN): boolean {
    const minute = Math.floor(Date.now() / 60_000);
    this.sql.exec('DELETE FROM hits WHERE minute < ?', minute - 1);
    const n = this.sql
      .exec<{ n: number }>('INSERT INTO hits (ip, minute, n) VALUES (?, ?, 1) ON CONFLICT (ip, minute) DO UPDATE SET n = n + 1 RETURNING n', ip, minute)
      .one().n;
    return n <= limit;
  }

  addOrder(id: string, claim: string | null): void {
    this.sql.exec('INSERT OR IGNORE INTO orders (id, claim, created) VALUES (?, ?, ?)', id, claim, this.now());
  }

  /** Started from the app (it waits for the key itself)? */
  fromApp(id: string): boolean {
    return this.sql.exec<{ claim: string | null }>('SELECT claim FROM orders WHERE id = ?', id).toArray()[0]?.claim != null;
  }

  hasOrder(id: string): boolean {
    return this.sql.exec('SELECT 1 FROM orders WHERE id = ?', id).toArray().length > 0;
  }

  /** The order's licence, made now if the order has none: a page and the webhook for the same
   * payment get the same key. `test`: paid under Razorpay's Test keys. */
  issue(orderId: string, paymentId: string, test: boolean): Licence | null {
    if (!this.hasOrder(orderId)) return null;
    return this.ctx.storage.transactionSync(() => {
      const had = this.sql.exec<{ lic: string | null }>('SELECT lic FROM orders WHERE id = ?', orderId).one().lic;
      if (had) return this.licence(had);
      // A payment already given a key (another order cannot have it, but be sure).
      const paid = this.sql.exec<{ lic: string }>('SELECT lic FROM licences WHERE payment = ?', paymentId).toArray()[0];
      const lic = paid?.lic ?? crypto.randomUUID().replace(/-/g, '').slice(0, 16);
      if (!paid) {
        this.sql.exec('INSERT INTO licences (lic, key, payment, issued, test) VALUES (?, ?, ?, ?, ?)', lic, newKey(), paymentId, this.now(), test ? 1 : 0);
      }
      this.sql.exec('UPDATE orders SET lic = ? WHERE id = ?', lic, orderId);
      return this.licence(lic);
    });
  }

  /** A key given, not sold (the key page). */
  give(): Licence {
    const lic = crypto.randomUUID().replace(/-/g, '').slice(0, 16);
    const payment = `given:${crypto.randomUUID()}`;
    this.sql.exec('INSERT INTO licences (lic, key, payment, issued, test) VALUES (?, ?, ?, ?, 0)', lic, newKey(), payment, this.now());
    return this.licence(lic)!;
  }

  /** The passkeys let in under `setup`. */
  passkeys(setup: string): Passkey[] {
    return this.sql.exec<Passkey & Record<string, string | number | null>>('SELECT id, rp, spki, alg, count, setup, name FROM passkeys WHERE setup = ?', setup).toArray();
  }

  /** The devices set up under `setup`, the oldest first. */
  devices(setup: string): Device[] {
    return this.sql
      .exec<Device & Record<string, string | number | null>>('SELECT id, rp, name, created, used FROM passkeys WHERE setup = ? ORDER BY created, rowid', setup)
      .toArray();
  }

  /** A passkey let in under `setup`: the `first` (set up with the passphrase) only while none
   * is, another only while one is and fewer than MAX_DEVICES are. False otherwise, or if the
   * passkey is already kept. */
  addPasskey(p: Passkey, first: boolean): boolean {
    return this.ctx.storage.transactionSync(() => {
      const n = this.passkeys(p.setup).length;
      if (first ? n > 0 : n === 0 || n >= MAX_DEVICES) return false;
      if (this.sql.exec('SELECT 1 FROM passkeys WHERE id = ?', p.id).toArray().length > 0) return false;
      this.sql.exec('INSERT INTO passkeys (id, rp, spki, alg, count, setup, created, name) VALUES (?, ?, ?, ?, ?, ?, ?, ?)', p.id, p.rp, p.spki, p.alg, p.count, p.setup, this.now(), p.name);
      return true;
    });
  }

  /** A sign-in: its signature counter, kept so that a copied passkey's sign-in is refused, and
   * its time; and the device's `name`, if the passkey has none yet (set up before names were
   * kept). */
  signedIn(id: string, count: number, name: string | null): void {
    this.sql.exec('UPDATE passkeys SET count = ?, used = ?, name = COALESCE(name, ?) WHERE id = ?', count, this.now(), name, id);
  }

  /** Passkey `id` removed from `setup`, by a sign-in on `rp` with passkey `by`: refused if `by`
   * is not set up there (removed meanwhile), or if no other device would be left to sign in there
   * (then the passphrase alone would set one up again). One already gone is removed. */
  removePasskey(setup: string, rp: string, by: string, id: string): 'removed' | 'signed-out' | 'last' {
    return this.ctx.storage.transactionSync(() => {
      const here = this.passkeys(setup).filter((p) => p.rp === rp);
      if (!here.some((p) => p.id === by)) return 'signed-out';
      if (!here.some((p) => p.id !== id)) return 'last';
      this.sql.exec('DELETE FROM passkeys WHERE id = ? AND setup = ?', id, setup);
      return 'removed';
    });
  }

  /** A complete ceremony's challenge, marked used: false if it was already (a ceremony sent
   * again). Kept only as long as a challenge is good. */
  useChallenge(value: string): boolean {
    return this.ctx.storage.transactionSync(() => {
      const now = this.now();
      this.sql.exec('DELETE FROM used WHERE at < ?', now - CHALLENGE_SECONDS - 60);
      if (this.sql.exec('SELECT 1 FROM used WHERE value = ?', value).toArray().length > 0) return false;
      this.sql.exec('INSERT INTO used (value, at) VALUES (?, ?)', value, now);
      return true;
    });
  }

  /** A code naming an `add` challenge, to type on the other device itself. Kept only as long as
   * a challenge is good. */
  addCode(code: string, challenge: string): void {
    const now = this.now();
    this.sql.exec('DELETE FROM codes WHERE at < ?', now - CHALLENGE_SECONDS);
    this.sql.exec('INSERT INTO codes (code, challenge, at) VALUES (?, ?, ?)', code, challenge, now);
  }

  /** The challenge `code` names, once: null if it names none, or one too old, or was taken. */
  takeCode(code: string): string | null {
    return this.ctx.storage.transactionSync(() => {
      this.sql.exec('DELETE FROM codes WHERE at < ?', this.now() - CHALLENGE_SECONDS);
      const r = this.sql.exec<{ challenge: string }>('SELECT challenge FROM codes WHERE code = ?', code).toArray()[0];
      if (!r) return null;
      this.sql.exec('DELETE FROM codes WHERE code = ?', code);
      return r.challenge;
    });
  }

  /** The key page takes a passphrase now: fewer than WRONG_PER_HOUR wrong ones in the last hour. */
  passphraseTaken(limit = WRONG_PER_HOUR): boolean {
    const hourAgo = this.now() - 3600;
    this.sql.exec('DELETE FROM wrong WHERE at <= ?', hourAgo);
    return this.sql.exec<{ n: number }>('SELECT count(*) AS n FROM wrong').one().n < limit;
  }

  /** One more wrong passphrase on the key page. */
  wrongPassphrase(): void {
    this.sql.exec('INSERT INTO wrong (at) VALUES (?)', this.now());
  }

  private licence(lic: string): Licence | null {
    const r = this.sql
      .exec<{ lic: string; key: string; pc: string | null; revoked: number | null }>('SELECT lic, key, pc, revoked FROM licences WHERE lic = ?', lic)
      .toArray()[0];
    return r ? { lic: r.lic, key: r.key, pc: r.pc, revoked: !!r.revoked } : null;
  }

  /** The key of a paid purchase the app started, once: a code seen again gets nothing. */
  claim(code: string): { key: string } | { status: 404 | 410 } {
    return this.ctx.storage.transactionSync(() => {
      const r = this.sql
        .exec<{ id: string; lic: string | null; collected: number }>('SELECT id, lic, collected FROM orders WHERE claim = ? AND lic IS NOT NULL', code)
        .toArray()[0];
      if (!r?.lic) return { status: 404 as const };
      if (r.collected) return { status: 410 as const };
      this.sql.exec('UPDATE orders SET collected = 1 WHERE id = ?', r.id);
      const l = this.licence(r.lic);
      return l ? { key: l.key } : { status: 404 as const };
    });
  }

  /** The key of a paid order, for its page shown again. */
  keyOf(claim: string): string | null {
    const r = this.sql.exec<{ lic: string | null }>('SELECT lic FROM orders WHERE claim = ? AND lic IS NOT NULL', claim).toArray()[0];
    return r?.lic ? (this.licence(r.lic)?.key ?? null) : null;
  }

  private byKey(key: string) {
    return this.sql
      .exec<{ lic: string; pc: string | null; moved: number | null; revoked: number | null; test: number }>(
        'SELECT lic, pc, moved, revoked, test FROM licences WHERE key = ?',
        key,
      )
      .toArray()[0];
  }

  /** Refunded, or bought under Test keys while the checkout is `live`: it turns nothing on. The app
   * hears either as `revoked`, a word 2.5.0 already knows. */
  private off(l: { revoked: number | null; test: number }, live: boolean): boolean {
    return !!l.revoked || (live && !!l.test);
  }

  /** Pro on `pc`: the PC the key is on already, or a key on none. With `move`, also a key on another
   * PC, at most once in MOVE_DAYS: that PC is then no longer Pro. */
  activate(key: string, pc: string, move: boolean, live: boolean): Activated {
    return this.ctx.storage.transactionSync((): Activated => {
      const l = this.byKey(key);
      if (!l) return { ok: false, status: 404, error: 'no-key' };
      if (this.off(l, live)) return { ok: false, status: 403, error: 'revoked' };
      const now = this.now();
      if (l.pc && l.pc !== pc) {
        if (!move) return { ok: false, status: 409, error: 'in-use' };
        if (l.moved && now - l.moved < MOVE_DAYS * DAY) return { ok: false, status: 409, error: 'moved-recently', until: l.moved + MOVE_DAYS * DAY };
        this.sql.exec('UPDATE licences SET pc = ?, activated = ?, moved = ? WHERE lic = ?', pc, now, now, l.lic);
      } else if (l.pc !== pc) {
        this.sql.exec('UPDATE licences SET pc = ?, activated = ? WHERE lic = ?', pc, now, l.lic);
      }
      return { ok: true, lic: l.lic };
    });
  }

  /** The key off `pc`, so another PC can take it. */
  release(key: string, pc: string): boolean {
    const l = this.byKey(key);
    if (!l || l.pc !== pc) return false;
    this.sql.exec('UPDATE licences SET pc = NULL WHERE lic = ?', l.lic);
    return true;
  }

  /** Still Pro on `pc`? */
  check(key: string, pc: string, live: boolean): Checked {
    const l = this.byKey(key);
    if (!l) return { ok: false, reason: 'no-key' };
    if (this.off(l, live)) return { ok: false, reason: 'revoked' };
    if (l.pc === pc) return { ok: true };
    return { ok: false, reason: l.pc ? 'moved' : 'released' };
  }

  /** A refunded payment's key stops working. */
  revoke(paymentId: string): boolean {
    const r = this.sql.exec('UPDATE licences SET revoked = ? WHERE payment = ? AND revoked IS NULL', this.now(), paymentId);
    return r.rowsWritten > 0;
  }
}

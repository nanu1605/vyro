// Every order and key (2.5), in one SQLite-backed Durable Object. An order is what the checkout
// made with Razorpay; once paid it gets a licence: its key, and the one PC it is Pro on. Nothing
// about the buyer is kept here: Razorpay has that.

import { DurableObject } from 'cloudflare:workers';
import { MOVE_DAYS, newKey, RATE_PER_MIN } from './rules';

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
    `);
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
   * payment get the same key. */
  issue(orderId: string, paymentId: string): Licence | null {
    if (!this.hasOrder(orderId)) return null;
    return this.ctx.storage.transactionSync(() => {
      const had = this.sql.exec<{ lic: string | null }>('SELECT lic FROM orders WHERE id = ?', orderId).one().lic;
      if (had) return this.licence(had);
      // A payment already given a key (another order cannot have it, but be sure).
      const paid = this.sql.exec<{ lic: string }>('SELECT lic FROM licences WHERE payment = ?', paymentId).toArray()[0];
      const lic = paid?.lic ?? crypto.randomUUID().replace(/-/g, '').slice(0, 16);
      if (!paid) {
        this.sql.exec('INSERT INTO licences (lic, key, payment, issued) VALUES (?, ?, ?, ?)', lic, newKey(), paymentId, this.now());
      }
      this.sql.exec('UPDATE orders SET lic = ? WHERE id = ?', lic, orderId);
      return this.licence(lic);
    });
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
      .exec<{ lic: string; pc: string | null; moved: number | null; revoked: number | null }>(
        'SELECT lic, pc, moved, revoked FROM licences WHERE key = ?',
        key,
      )
      .toArray()[0];
  }

  /** Pro on `pc`: the PC the key is on already, or a key on none. With `move`, also a key on another
   * PC, at most once in MOVE_DAYS: that PC is then no longer Pro. */
  activate(key: string, pc: string, move: boolean): Activated {
    return this.ctx.storage.transactionSync((): Activated => {
      const l = this.byKey(key);
      if (!l) return { ok: false, status: 404, error: 'no-key' };
      if (l.revoked) return { ok: false, status: 403, error: 'revoked' };
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
  check(key: string, pc: string): Checked {
    const l = this.byKey(key);
    if (!l) return { ok: false, reason: 'no-key' };
    if (l.revoked) return { ok: false, reason: 'revoked' };
    if (l.pc === pc) return { ok: true };
    return { ok: false, reason: l.pc ? 'moved' : 'released' };
  }

  /** A refunded payment's key stops working. */
  revoke(paymentId: string): boolean {
    const r = this.sql.exec('UPDATE licences SET revoked = ? WHERE payment = ? AND revoked IS NULL', this.now(), paymentId);
    return r.rowsWritten > 0;
  }
}

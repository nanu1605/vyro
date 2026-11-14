// Clokrix Pro's checkout and keys (2.5), run as deployed (workerd), with the tests' own Razorpay:
// it makes orders, holds payments the tests set, and captures them. Its signatures are made here
// with the same secrets the Worker is given.

import { createServer, type Server } from 'node:http';
import { createHmac, webcrypto } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const KEY_ID = 'rzp_test_clokrix';
const KEY_SECRET = 'test-key-secret';
const HOOK_SECRET = 'test-webhook-secret';

interface Pay {
  id: string;
  order_id: string;
  amount: number;
  currency: string;
  status: string;
}

let server: TestHarness;
let base: string;
let rzp: Server;
const orders: Array<{ id: string; body: { amount: number; currency: string; notes: Record<string, string> } }> = [];
const payments = new Map<string, Pay>();
let captured = 0;
let publicKey: webcrypto.CryptoKey;

beforeAll(async () => {
  rzp = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      const ok = req.headers.authorization === `Basic ${Buffer.from(`${KEY_ID}:${KEY_SECRET}`).toString('base64')}`;
      const send = (status: number, v: unknown) => res.writeHead(status, { 'Content-Type': 'application/json' }).end(JSON.stringify(v));
      if (!ok) return send(401, { error: 'auth' });
      const url = req.url ?? '';
      if (req.method === 'POST' && url === '/v1/orders') {
        const id = `order_T${orders.length + 1}`;
        orders.push({ id, body: JSON.parse(Buffer.concat(chunks).toString()) });
        return send(200, { id });
      }
      const pay = /^\/v1\/payments\/([^/]+)(\/capture)?$/.exec(url);
      const p = pay ? payments.get(decodeURIComponent(pay[1]!)) : undefined;
      if (!p) return send(404, { error: 'no-payment' });
      if (pay?.[2] && req.method === 'POST') {
        captured++;
        p.status = 'captured';
      }
      return send(200, p);
    });
  });
  await new Promise<void>((done) => rzp.listen(0, '127.0.0.1', done));
  const addr = rzp.address();
  const api = `http://127.0.0.1:${typeof addr === 'object' && addr ? addr.port : 0}`;
  const pair = (await webcrypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as webcrypto.CryptoKeyPair;
  publicKey = pair.publicKey;
  const pkcs8 = Buffer.from(await webcrypto.subtle.exportKey('pkcs8', pair.privateKey)).toString('base64');
  server = createTestHarness({
    workers: [
      {
        configPath: fileURLToPath(new URL('./wrangler.test.jsonc', import.meta.url)),
        vars: { RAZORPAY_API: api },
        secrets: {
          RAZORPAY_KEY_ID: KEY_ID,
          RAZORPAY_KEY_SECRET: KEY_SECRET,
          RAZORPAY_WEBHOOK_SECRET: HOOK_SECRET,
          LICENCE_SIGNING_KEY: pkcs8,
        },
      },
    ],
  });
  base = (await server.listen()).url.toString().replace(/\/$/, '');
});

afterAll(async () => {
  await server.close();
  rzp.close();
});

/** Each test comes from its own address, so the limit on requests is its own. */
let addresses = 0;
function from(): Record<string, string> {
  return { 'CF-Connecting-IP': `10.0.0.${++addresses}` };
}

const claimCode = () => Buffer.from(webcrypto.getRandomValues(new Uint8Array(16))).toString('hex');
const pcId = () => Buffer.from(webcrypto.getRandomValues(new Uint8Array(32))).toString('hex');
const sign = (secret: string, text: string) => createHmac('sha256', secret).update(text).digest('hex');

async function order(claim: string | null, ip = from()): Promise<string> {
  const res = await fetch(`${base}/order`, { method: 'POST', headers: { ...ip, 'Content-Type': 'application/json' }, body: JSON.stringify({ c: claim }) });
  expect(res.status).toBe(200);
  return ((await res.json()) as { order_id: string }).order_id;
}

function payment(orderId: string, x: Partial<Pay> = {}): Pay {
  const p: Pay = { id: `pay_${orderId}`, order_id: orderId, amount: 10000, currency: 'USD', status: 'captured', ...x };
  payments.set(p.id, p);
  return p;
}

async function paid(p: Pay, signature = sign(KEY_SECRET, `${p.order_id}|${p.id}`)): Promise<Response> {
  const form = new URLSearchParams({ razorpay_payment_id: p.id, razorpay_order_id: p.order_id, razorpay_signature: signature });
  return fetch(`${base}/paid`, { method: 'POST', headers: from(), body: form });
}

async function webhook(event: unknown, secret = HOOK_SECRET): Promise<Response> {
  const raw = JSON.stringify(event);
  return fetch(`${base}/razorpay/webhook`, { method: 'POST', headers: { 'X-Razorpay-Signature': sign(secret, raw) }, body: raw });
}

const keyIn = (page: string) => /CLKX(-[0-9A-Z]{5}){4}/.exec(page)?.[0] ?? null;

async function claim(code: string): Promise<{ status: number; key?: string }> {
  const res = await fetch(`${base}/claim/${code}`, { headers: from() });
  return { status: res.status, ...((await res.json()) as { key?: string }) };
}

async function post(path: string, body: unknown, ip = from()): Promise<{ status: number; body: Record<string, unknown> }> {
  const res = await fetch(`${base}${path}`, { method: 'POST', headers: { ...ip, 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  return { status: res.status, body: (await res.json()) as Record<string, unknown> };
}

/** A key, bought the way the app buys one. */
async function bought(): Promise<string> {
  const c = claimCode();
  const p = payment(await order(c));
  expect((await paid(p)).status).toBe(200);
  const got = await claim(c);
  expect(got.status).toBe(200);
  return got.key!;
}

describe('the checkout page', () => {
  it('shows the price and Razorpay, and makes no order just for being looked at', async () => {
    const before = orders.length;
    const res = await fetch(`${base}/buy?c=${claimCode()}`, { headers: from() });
    expect(res.status).toBe(200);
    const page = await res.text();
    expect(page).toContain('Pay $100');
    expect(page).toContain(KEY_ID);
    expect(page).toContain('https://checkout.razorpay.com/v1/checkout.js');
    expect(page).toContain('Test Seller');
    expect(res.headers.get('Content-Security-Policy')).toContain("form-action 'self'");
    expect(orders.length).toBe(before);
    // The page's own script is whole.
    const inline = [...page.matchAll(/<script nonce="[^"]+">([\s\S]*?)<\/script>/g)].map((m) => m[1]!);
    expect(inline.length).toBe(1);
    for (const s of inline) expect(() => new Function(s)).not.toThrow();
  });

  it('refuses a code that is not one of the app\'s', async () => {
    expect((await fetch(`${base}/buy?c=not-a-code`, { headers: from() })).status).toBe(400);
    expect((await post('/order', { c: 'nope' })).status).toBe(400);
  });

  it('makes an order of $100 when Pay is pressed, with the app\'s code', async () => {
    const c = claimCode();
    const id = await order(c);
    const o = orders.find((x) => x.id === id)!;
    expect(o.body.amount).toBe(10000);
    expect(o.body.currency).toBe('USD');
    expect(o.body.notes).toEqual({ product: 'clokrix-pro', claim: c });
  });
});

describe('a payment', () => {
  it('with a signature that is not Razorpay\'s gives no key', async () => {
    const c = claimCode();
    const p = payment(await order(c));
    const res = await paid(p, sign('someone-else', `${p.order_id}|${p.id}`));
    expect(res.status).toBe(400);
    expect(keyIn(await res.text())).toBeNull();
    expect((await claim(c)).status).toBe(404);
  });

  it('of less than $100, or in another currency, gives no key', async () => {
    for (const x of [{ amount: 5000 }, { currency: 'INR' }, { status: 'failed' }]) {
      const c = claimCode();
      const p = payment(await order(c), x);
      expect((await paid(p)).status).toBe(402);
      expect((await claim(c)).status).toBe(404);
    }
  });

  it('for an order this checkout did not make gives no key', async () => {
    const p = payment('order_elsewhere');
    expect((await paid(p)).status).toBe(400);
  });

  it('paid gives one key, the same on the page again and from the webhook, and to the app once', async () => {
    const c = claimCode();
    const p = payment(await order(c));
    const first = await paid(p);
    expect(first.status).toBe(200);
    const key = keyIn(await first.text());
    expect(key).toMatch(/^CLKX-/);
    expect(keyIn(await (await paid(p)).text())).toBe(key);
    expect((await webhook({ event: 'payment.captured', payload: { payment: { entity: p } } })).status).toBe(200);
    expect(await claim(c)).toEqual({ status: 200, key });
    // Given once: a code seen again gets nothing.
    expect((await claim(c)).status).toBe(410);
    // The page opened again from the app shows it.
    expect(keyIn(await (await fetch(`${base}/buy?c=${c}`, { headers: from() })).text())).toBe(key);
  });

  it('only authorised is captured, then gives a key', async () => {
    const c = claimCode();
    const was = captured;
    const p = payment(await order(c), { status: 'authorized' });
    expect((await paid(p)).status).toBe(200);
    expect(captured).toBe(was + 1);
    expect((await claim(c)).status).toBe(200);
  });

  it('heard first from the webhook (the page closed) gives the key to the app', async () => {
    const c = claimCode();
    const p = payment(await order(c));
    expect((await webhook({ event: 'order.paid', payload: { payment: { entity: p } } }, 'wrong')).status).toBe(400);
    expect((await claim(c)).status).toBe(404);
    expect((await webhook({ event: 'order.paid', payload: { payment: { entity: p } } })).status).toBe(200);
    const got = await claim(c);
    expect(got.status).toBe(200);
    expect(keyIn(await (await paid(p)).text())).toBe(got.key);
  });
});

describe('a key on one PC', () => {
  async function verified(token: string): Promise<Record<string, unknown>> {
    const [payload, sig] = token.split('.') as [string, string];
    const bytes = Buffer.from(payload, 'base64url');
    expect(await webcrypto.subtle.verify('Ed25519', publicKey, Buffer.from(sig, 'base64url'), bytes)).toBe(true);
    return JSON.parse(bytes.toString());
  }

  it('is Pro on the first PC, signed for that PC, and refused on a second', async () => {
    const key = await bought();
    const a = pcId();
    const b = pcId();
    const on = await post('/activate', { key, pc: a });
    expect(on.status).toBe(200);
    const said = await verified(on.body.activation as string);
    expect(said).toMatchObject({ v: 1, pc: a, edition: 'pro' });
    // Typed loosely: lower case, spaces, O for 0.
    const loose = key.toLowerCase().replace(/-/g, ' ').replace(/0/g, 'o');
    expect((await post('/activate', { key: loose, pc: a })).status).toBe(200);
    expect(await post('/activate', { key, pc: b })).toEqual({ status: 409, body: { error: 'in-use' } });
    expect((await post('/check', { key, pc: a })).body).toEqual({ ok: true });
    expect((await post('/check', { key, pc: b })).body).toEqual({ ok: false, reason: 'moved' });
  });

  it('moves to another PC once in 30 days, and the old one is no longer Pro', async () => {
    const key = await bought();
    const a = pcId();
    const b = pcId();
    expect((await post('/activate', { key, pc: a })).status).toBe(200);
    const moved = await post('/activate', { key, pc: b, move: true });
    expect(moved.status).toBe(200);
    expect((await verified(moved.body.activation as string)).pc).toBe(b);
    expect((await post('/check', { key, pc: a })).body).toEqual({ ok: false, reason: 'moved' });
    const back = await post('/activate', { key, pc: a, move: true });
    expect(back.status).toBe(409);
    expect(back.body.error).toBe('moved-recently');
    expect(back.body.until).toBeGreaterThan(Date.now() / 1000 + 29 * 86400);
  });

  it('freed on its PC can be taken by another without a move', async () => {
    const key = await bought();
    const a = pcId();
    const b = pcId();
    expect((await post('/activate', { key, pc: a })).status).toBe(200);
    expect((await post('/release', { key, pc: b })).status).toBe(409);
    expect((await post('/release', { key, pc: a })).status).toBe(200);
    expect((await post('/check', { key, pc: a })).body).toEqual({ ok: false, reason: 'released' });
    expect((await post('/activate', { key, pc: b })).status).toBe(200);
  });

  it('stops working once its payment is refunded', async () => {
    const c = claimCode();
    const p = payment(await order(c));
    await paid(p);
    const key = (await claim(c)).key!;
    const a = pcId();
    expect((await post('/activate', { key, pc: a })).status).toBe(200);
    const refund = { event: 'refund.processed', payload: { refund: { entity: { id: 'rfnd_1', payment_id: p.id } } } };
    expect((await webhook(refund)).status).toBe(200);
    expect((await post('/check', { key, pc: a })).body).toEqual({ ok: false, reason: 'revoked' });
    expect((await post('/activate', { key, pc: a })).status).toBe(403);
  });

  it('refuses what is not a key or a PC', async () => {
    expect((await post('/activate', { key: 'CLKX-nope', pc: pcId() })).body).toEqual({ error: 'key' });
    expect((await post('/activate', { key: 'CLKX-00000-00000-00000-00000', pc: 'x' })).body).toEqual({ error: 'pc' });
    expect(await post('/activate', { key: 'CLKX-00000-00000-00000-00000', pc: pcId() })).toEqual({ status: 404, body: { error: 'no-key' } });
  });
});

describe('one address asking too often', () => {
  it('waits after 40 requests in a minute', async () => {
    const ip = from();
    const code = claimCode();
    const statuses: number[] = [];
    for (let i = 0; i < 41; i++) statuses.push((await fetch(`${base}/claim/${code}`, { headers: ip })).status);
    expect(statuses.slice(0, 40).every((s) => s === 404)).toBe(true);
    expect(statuses[40]).toBe(429);
  });
});

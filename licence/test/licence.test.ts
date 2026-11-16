// Clokrix Pro's checkout and keys (2.5), run as deployed (workerd), with the tests' own Razorpay:
// it makes orders, holds payments the tests set, and captures them. Its signatures are made here
// with the same secrets the Worker is given.

import { createServer, type Server } from 'node:http';
import { createHmac, webcrypto } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness, type TestHarnessOptions } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const KEY_ID = 'rzp_test_clokrix';
/** Razorpay's Live keys, as the checkout runs once Razorpay has checked the website. */
const LIVE_KEY_ID = 'rzp_live_clokrix';
const KEY_SECRET = 'test-key-secret';
const HOOK_SECRET = 'test-webhook-secret';
/** How the website says to reach the seller (constructed). */
const CONTACT = { CONTACT_EMAIL: 'help@example.test', CONTACT_PHONE: '+91 00000 00000', CONTACT_ADDRESS: '1 <Test> Road\nPune, India' };

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
let api = '';
let pkcs8 = '';

/** The checkout as the tests run it: on `keyId`, with `vars` added to the usual. */
function options(keyId = KEY_ID, vars: Record<string, string> = {}): TestHarnessOptions {
  return {
    workers: [
      {
        configPath: fileURLToPath(new URL('./wrangler.test.jsonc', import.meta.url)),
        vars: { RAZORPAY_API: api, ...CONTACT, ...vars },
        secrets: {
          RAZORPAY_KEY_ID: keyId,
          RAZORPAY_KEY_SECRET: KEY_SECRET,
          RAZORPAY_WEBHOOK_SECRET: HOOK_SECRET,
          LICENCE_SIGNING_KEY: pkcs8,
        },
      },
    ],
  };
}

/** The same checkout started again with other settings; its ledger is kept. */
async function restart(keyId = KEY_ID, vars: Record<string, string> = {}): Promise<void> {
  await server.update(options(keyId, vars));
  base = (await server.listen()).url.toString().replace(/\/$/, '');
}

beforeAll(async () => {
  rzp = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => chunks.push(c));
    req.on('end', () => {
      const ok = [KEY_ID, LIVE_KEY_ID].some((id) => req.headers.authorization === `Basic ${Buffer.from(`${id}:${KEY_SECRET}`).toString('base64')}`);
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
  api = `http://127.0.0.1:${typeof addr === 'object' && addr ? addr.port : 0}`;
  const pair = (await webcrypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])) as webcrypto.CryptoKeyPair;
  publicKey = pair.publicKey;
  pkcs8 = Buffer.from(await webcrypto.subtle.exportKey('pkcs8', pair.privateKey)).toString('base64');
  server = createTestHarness(options());
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

describe('the price: ₹10,000 in India, $100 elsewhere', () => {
  /** A buyer whose connection Cloudflare places in `country`. */
  const at = (country: string) => ({ ...from(), 'CF-IPCountry': country });

  it('is in rupees for a buyer in India, including all taxes, with dollars a click away', async () => {
    const c = claimCode();
    const page = await (await fetch(`${base}/buy?c=${c}`, { headers: at('IN') })).text();
    expect(page).toContain('Pay ₹10,000');
    expect(page).toContain('including all taxes');
    expect(page).toContain(`href="/buy?c=${c}&amp;cur=usd">Not in India? Pay $100 instead.`);
    expect(page).toContain('"amount":1000000,"currency":"INR"');
    expect(page).not.toContain('Pay $100<');
  });

  it('is in dollars elsewhere, with rupees a click away, and the buyer\'s choice wins', async () => {
    const page = await (await fetch(`${base}/buy`, { headers: at('US') })).text();
    expect(page).toContain('Pay $100');
    expect(page).toContain('href="/buy?cur=inr">In India? Pay ₹10,000 instead.');
    expect(page).not.toContain('including all taxes');
    const chosen = await (await fetch(`${base}/buy?cur=INR`, { headers: at('US') })).text();
    expect(chosen).toContain('Pay ₹10,000');
    expect(await (await fetch(`${base}/buy?cur=usd`, { headers: at('IN') })).text()).toContain('Pay $100');
    // Not a currency it takes: the country decides.
    expect(await (await fetch(`${base}/buy?cur=eur`, { headers: at('IN') })).text()).toContain('Pay ₹10,000');
  });

  it('makes the order in the currency the page shows, or by the country when none is sent', async () => {
    const make = async (body: unknown, country: string) => {
      const res = await fetch(`${base}/order`, { method: 'POST', headers: { ...at(country), 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      expect(res.status).toBe(200);
      const id = ((await res.json()) as { order_id: string }).order_id;
      return orders.find((o) => o.id === id)!.body;
    };
    expect(await make({ c: null, cur: 'INR' }, 'US')).toMatchObject({ amount: 1000000, currency: 'INR' });
    expect(await make({ c: null, cur: 'USD' }, 'IN')).toMatchObject({ amount: 10000, currency: 'USD' });
    expect(await make({ c: null }, 'IN')).toMatchObject({ amount: 1000000, currency: 'INR' });
    expect(await make({ c: null }, 'DE')).toMatchObject({ amount: 10000, currency: 'USD' });
  });

  it('paid in rupees gives a key; ₹100 or a dollar price in rupees does not', async () => {
    const c = claimCode();
    const p = payment(await order(c), { amount: 1000000, currency: 'INR' });
    expect((await paid(p)).status).toBe(200);
    expect((await claim(c)).status).toBe(200);
    for (const x of [{ amount: 10000, currency: 'INR' }, { amount: 1000000, currency: 'USD' }]) {
      const d = claimCode();
      expect((await paid(payment(await order(d), x))).status).toBe(402);
      expect((await claim(d)).status).toBe(404);
    }
  });

  it('is named on the website: the visitor\'s on the home page, both in the terms', async () => {
    const india = await (await fetch(`${base}/`, { headers: at('IN') })).text();
    expect(india).toContain('Buy Pro, ₹10,000');
    expect(india).toContain('Pro: ₹10,000, once, for life');
    expect(india).toContain('₹10,000 in India, including all taxes, and $100 elsewhere');
    expect(await (await fetch(`${base}/`, { headers: at('GB') })).text()).toContain('Buy Pro, $100');
    const terms = await (await fetch(`${base}/terms`, { headers: at('GB') })).text();
    expect(terms).toContain('₹10,000 in India, including all taxes, and $100 elsewhere');
    expect(terms).toContain('lets you pay the other instead');
    const refunds = await (await fetch(`${base}/refunds`, { headers: at('IN') })).text();
    expect(refunds).toContain('refund the full price you paid');
  });
});

describe('the website', () => {
  const PAGES = ['/', '/terms', '/privacy', '/refunds', '/delivery', '/contact'];

  it('has every page Razorpay asks for, each naming the seller and leading to the others', async () => {
    const ip = from();
    for (const path of PAGES) {
      const res = await fetch(`${base}${path}`, { headers: ip });
      expect(res.status, path).toBe(200);
      const page = await res.text();
      expect(page, path).toContain('Test Seller');
      for (const to of ['/buy', ...PAGES]) expect(page, `${path} → ${to}`).toContain(`href="${to}"`);
    }
  });

  it('shows Clokrix Pro, its price and the download, and how refunds work', async () => {
    const home = await (await fetch(`${base}/`, { headers: from() })).text();
    expect(home).toContain('$100');
    expect(home).toContain('Download for Windows');
    expect(home).toContain('7 days');
    const refunds = await (await fetch(`${base}/refunds`, { headers: from() })).text();
    expect(refunds).toContain('within 14 days');
    expect(refunds).toContain('help@example.test');
  });

  it('shows how to reach the seller, as set, and escaped', async () => {
    const page = await (await fetch(`${base}/contact`, { headers: from() })).text();
    expect(page).toContain('href="mailto:help@example.test"');
    expect(page).toContain('href="tel:+910000000000"');
    expect(page).toContain('1 &lt;Test&gt; Road');
    expect(page).not.toContain('<Test>');
  });

  it('puts the policies next to the payment too', async () => {
    const page = await (await fetch(`${base}/buy`, { headers: from() })).text();
    expect(page).toContain('Pay $100');
    for (const to of ['/terms', '/privacy', '/refunds', '/delivery', '/contact']) expect(page).toContain(`href="${to}"`);
  });
});

describe('one address for the website', () => {
  const SITE = 'https://clokrix.example.test';

  it('sends its pages there, the app\'s code kept, and answers the app wherever it asks', async () => {
    await restart(KEY_ID, { SITE });
    try {
      const c = claimCode();
      const buy = await fetch(`${base}/buy?c=${c}`, { headers: from(), redirect: 'manual' });
      expect(buy.status).toBe(302);
      expect(buy.headers.get('location')).toBe(`${SITE}/buy?c=${c}`);
      for (const path of ['/', '/terms', '/contact']) {
        const res = await fetch(`${base}${path}`, { headers: from(), redirect: 'manual' });
        expect(res.headers.get('location'), path).toBe(`${SITE}${path}`);
      }
      // At its own address, the website is served.
      const there = await server.fetch(`${SITE}/terms`, { headers: from() });
      expect(there.status).toBe(200);
      expect(await there.text()).toContain('Terms of sale');
      // The app's side and Razorpay's webhook are not pages: they work on any address.
      expect((await claim(c)).status).toBe(404);
      expect((await post('/check', { key: 'CLKX-AAAAA-AAAAA-AAAAA-AAAAA', pc: pcId() })).body).toEqual({ ok: false, reason: 'no-key' });
      expect((await webhook({ event: 'payment.authorized' })).status).toBe(200);
    } finally {
      await restart();
    }
  });
});

describe('keys bought under Razorpay\'s Test keys', () => {
  it('stop turning Pro on once the checkout runs on Live keys, and keys bought then work', async () => {
    const testKey = await bought();
    const pc = pcId();
    expect((await post('/activate', { key: testKey, pc })).status).toBe(200);
    await restart(LIVE_KEY_ID);
    try {
      expect((await post('/check', { key: testKey, pc })).body).toEqual({ ok: false, reason: 'revoked' });
      const elsewhere = await post('/activate', { key: testKey, pc: pcId(), move: true });
      expect(elsewhere.status).toBe(403);
      expect(elsewhere.body.error).toBe('revoked');
      const liveKey = await bought();
      const livePc = pcId();
      expect((await post('/activate', { key: liveKey, pc: livePc })).status).toBe(200);
      expect((await post('/check', { key: liveKey, pc: livePc })).body).toEqual({ ok: true });
    } finally {
      await restart();
    }
    // Back on Test keys, it works again: the key was not deleted, only refused while Live.
    expect((await post('/check', { key: testKey, pc })).body).toEqual({ ok: true });
  });

  it('count every key from before keys were marked', async () => {
    const worker = server.getWorker();
    const ledger = await worker.getDurableObjectStorage('LEDGER', { name: 'ledger' });
    // The ledger as the first deploy made it, with a key in it.
    await ledger.exec('ALTER TABLE licences DROP COLUMN test');
    const key = 'CLKX-0AAAA-AAAAA-AAAAA-AAAAA';
    await ledger.exec('INSERT INTO licences (lic, key, payment, issued) VALUES (?, ?, ?, ?)', 'firstdeploy00001', key, 'pay_first', 1);
    await worker.evictDurableObject('LEDGER', { name: 'ledger' });
    const pc = pcId();
    expect((await post('/activate', { key, pc })).status).toBe(200);
    await restart(LIVE_KEY_ID);
    try {
      expect((await post('/check', { key, pc })).body).toEqual({ ok: false, reason: 'revoked' });
    } finally {
      await restart();
    }
  });
});

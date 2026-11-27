// Clokrix Pro's checkout and keys (2.5), run as deployed (workerd), with the tests' own Razorpay:
// it makes orders, holds payments the tests set, and captures them. Its signatures are made here
// with the same secrets the Worker is given.

import { createServer, type Server } from 'node:http';
import { createHash, createHmac, webcrypto } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { createTestHarness, type TestHarness, type TestHarnessOptions } from 'wrangler';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { deviceName } from '../src/rules';

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

/** The checkout as the tests run it: on `keyId`, with `vars` and `secrets` added to the usual. */
function options(keyId = KEY_ID, vars: Record<string, string> = {}, secrets: Record<string, string> = {}): TestHarnessOptions {
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
          ...secrets,
        },
      },
    ],
  };
}

/** The same checkout started again with other settings; its ledger is kept. */
async function restart(keyId = KEY_ID, vars: Record<string, string> = {}, secrets: Record<string, string> = {}): Promise<void> {
  await server.update(options(keyId, vars, secrets));
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

describe('downloads and updates (2.5.2)', () => {
  const TAG = 'v9.9.9';
  const MSI = 'Clokrix_9.9.9_x64_en-US.msi';
  /** An installer's bytes (constructed), long enough to ask for a part of. */
  const bytes = new Uint8Array(4096).map((_, i) => (i * 7) % 256);
  const manifest = {
    version: '9.9.9',
    notes: 'Clokrix 9.9.9',
    pub_date: '2026-11-16T00:00:00Z',
    platforms: {
      'windows-x86_64': { signature: 'constructed', url: `https://clokrix.example.test/download/${TAG}/${MSI}` },
      'windows-x86_64-msi': { signature: 'constructed', url: `https://clokrix.example.test/download/${TAG}/${MSI}` },
    },
  };
  const bucket = async () => (await server.getWorker<{ DOWNLOADS: R2Bucket }>().getEnv()).DOWNLOADS;

  it('say there is no release yet until the first release is here', async () => {
    const res = await fetch(`${base}/download`, { headers: from(), redirect: 'manual' });
    expect(res.status).toBe(404);
    expect(res.headers.get('location')).toBeNull();
    expect(await res.text()).toBe('No release yet.');
    expect((await fetch(`${base}/updates/latest.json`, { headers: from() })).status).toBe(404);
    const home = await (await fetch(`${base}/`, { headers: from() })).text();
    expect(home).toContain('href="/download"');
  });

  it('serve the newest release: its manifest, the installer whole or in part, and its checksums', async () => {
    const b = await bucket();
    await b.put(`${TAG}/${MSI}`, bytes);
    await b.put(`${TAG}/SHA256SUMS.txt`, `abc  ${MSI}\n`);
    await b.put('latest.json', JSON.stringify(manifest));
    try {
      const m = await fetch(`${base}/updates/latest.json`, { headers: from() });
      expect(m.status).toBe(200);
      expect(m.headers.get('cache-control')).toBe('no-store');
      expect(await m.json()).toEqual(manifest);

      // The Download button: the newest installer, on the address asked.
      const latest = await fetch(`${base}/download`, { headers: from(), redirect: 'manual' });
      expect(latest.status).toBe(302);
      expect(latest.headers.get('location')).toBe(`${base}/download/${TAG}/${MSI}`);

      const whole = await fetch(`${base}/download/${TAG}/${MSI}`, { headers: from() });
      expect(whole.status).toBe(200);
      expect(whole.headers.get('content-type')).toBe('application/x-msi');
      expect(whole.headers.get('content-disposition')).toBe(`attachment; filename="${MSI}"`);
      expect(new Uint8Array(await whole.arrayBuffer())).toEqual(bytes);
      const etag = whole.headers.get('etag')!;
      expect(etag).toBeTruthy();

      // A download picked up where it stopped, and one already had.
      const part = await fetch(`${base}/download/${TAG}/${MSI}`, { headers: { ...from(), Range: 'bytes=1000-1999' } });
      expect(part.status).toBe(206);
      expect(part.headers.get('content-range')).toBe('bytes 1000-1999/4096');
      expect(new Uint8Array(await part.arrayBuffer())).toEqual(bytes.slice(1000, 2000));
      const tail = await fetch(`${base}/download/${TAG}/${MSI}`, { headers: { ...from(), Range: 'bytes=-96' } });
      expect(tail.headers.get('content-range')).toBe('bytes 4000-4095/4096');
      const had = await fetch(`${base}/download/${TAG}/${MSI}`, { headers: { ...from(), 'If-None-Match': etag } });
      expect(had.status).toBe(304);

      const head = await fetch(`${base}/download/${TAG}/${MSI}`, { method: 'HEAD', headers: from() });
      expect(head.status).toBe(200);
      expect(head.headers.get('content-length')).toBe('4096');

      const sums = await fetch(`${base}/download/${TAG}/SHA256SUMS.txt`, { headers: from() });
      expect(sums.headers.get('content-type')).toBe('text/plain; charset=utf-8');
      expect(await sums.text()).toContain(MSI);
    } finally {
      await b.delete(['latest.json', `${TAG}/${MSI}`, `${TAG}/SHA256SUMS.txt`]);
    }
  });

  it('serve nothing else from the bucket, and only to read', async () => {
    const b = await bucket();
    await b.put('private/notes.txt', 'not a download');
    try {
      for (const path of ['/download/private/notes.txt', `/download/${TAG}`, `/download/${TAG}/`, '/download/v1.0/x.msi', `/download/${TAG}/a/b.msi`]) {
        expect((await fetch(`${base}${path}`, { headers: from() })).status, path).toBe(404);
      }
      expect((await fetch(`${base}/updates/latest.json`, { method: 'POST', headers: from() })).status).toBe(405);
      expect((await fetch(`${base}/download`, { method: 'DELETE', headers: from(), redirect: 'manual' })).status).toBe(405);
    } finally {
      await b.delete('private/notes.txt');
    }
  });

  it('are not counted against purchases: every PC behind one address checks', async () => {
    const ip = from();
    for (let i = 0; i < 60; i++) expect((await fetch(`${base}/updates/latest.json`, { headers: ip })).status).toBe(404);
    expect((await fetch(`${base}/claim/${claimCode()}`, { headers: ip })).status).toBe(404);
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

describe('the key page (2.5.5)', () => {
  /** The setup passphrase (constructed). */
  const PASSPHRASE = 'a passphrase for the tests only';
  const open = { KEY_PAGE_PASSPHRASE: PASSPHRASE };
  /** Wrong setup passphrases sent so far: every test here counts against the same hour. */
  let wrong = 0;
  const b64u = (b: Uint8Array | ArrayBuffer) => Buffer.from(b instanceof ArrayBuffer ? new Uint8Array(b) : b).toString('base64url');
  const sha = (b: Uint8Array) => createHash('sha256').update(b).digest();

  /** A passkey as an authenticator keeps one: ES256 (DER signatures) or EdDSA; with a signature
   * counter, or, as some (Windows Hello among them) without, always 0. */
  class Device {
    id = b64u(webcrypto.getRandomValues(new Uint8Array(32)));
    count = 0;
    private constructor(
      readonly alg: number,
      private keys: webcrypto.CryptoKeyPair,
      readonly counts: boolean,
    ) {}
    static async make(alg: -7 | -8 = -7, counts = true): Promise<Device> {
      const params = alg === -7 ? { name: 'ECDSA', namedCurve: 'P-256' } : { name: 'Ed25519' };
      return new Device(alg, (await webcrypto.subtle.generateKey(params, true, ['sign', 'verify'])) as webcrypto.CryptoKeyPair, counts);
    }
    async spki(): Promise<string> {
      return b64u(await webcrypto.subtle.exportKey('spki', this.keys.publicKey));
    }
    async sign(data: Uint8Array): Promise<Buffer> {
      if (this.alg === -8) return Buffer.from(await webcrypto.subtle.sign('Ed25519', this.keys.privateKey, data));
      const raw = Buffer.from(await webcrypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, this.keys.privateKey, data));
      // As authenticators give it: DER, each half without leading zeros and positive.
      const int = (x: Buffer) => {
        let i = 0;
        while (i < x.length - 1 && x[i] === 0) i++;
        const v = x.subarray(i);
        const body = v[0]! & 0x80 ? Buffer.concat([Buffer.from([0]), v]) : v;
        return Buffer.concat([Buffer.from([2, body.length]), body]);
      };
      const seq = Buffer.concat([int(raw.subarray(0, 32)), int(raw.subarray(32))]);
      return Buffer.concat([Buffer.from([0x30, seq.length]), seq]);
    }
  }

  const rpId = () => new URL(base).hostname;
  /** The authenticator's data: the website's hash, the flags (present 1, verified 4), the
   * counter. */
  function authenticator(flags: number, count: number, rp = rpId()): Buffer {
    const c = Buffer.alloc(4);
    c.writeUInt32BE(count);
    return Buffer.concat([sha(Buffer.from(rp)), Buffer.from([flags]), c]);
  }
  const client = (type: string, challenge: string, origin = base) => Buffer.from(JSON.stringify({ type, challenge, origin, crossOrigin: false }));

  async function setUp(d: Device, passphrase = PASSPHRASE): Promise<{ status: number; body: Record<string, unknown> }> {
    const start = await post('/give/setup/start', { passphrase });
    if (start.status === 403) wrong++;
    if (start.status !== 200) return start;
    return post('/give/setup/finish', {
      passphrase,
      id: d.id,
      spki: await d.spki(),
      alg: d.alg,
      authenticatorData: b64u(authenticator(0x45, 0)),
      clientDataJSON: b64u(client('webauthn.create', start.body.challenge as string)),
    });
  }

  /** A sign-in by `d`, as its browser sends it; `o` changes one part of it. */
  async function signInBody(d: Device, o: { flags?: number; origin?: string; type?: string; challenge?: string; count?: number; rp?: string } = {}) {
    const start = await post('/give/start', {});
    expect(start.status).toBe(200);
    expect(start.body.ids).toContain(d.id);
    if (d.counts) d.count++;
    const cd = client(o.type ?? 'webauthn.get', o.challenge ?? (start.body.challenge as string), o.origin ?? base);
    const ad = authenticator(o.flags ?? 0x05, o.count ?? d.count, o.rp);
    const sig = await d.sign(Buffer.concat([ad, sha(cd)]));
    return { id: d.id, authenticatorData: b64u(ad), clientDataJSON: b64u(cd), signature: b64u(sig) };
  }

  async function signIn(d: Device, o: Parameters<typeof signInBody>[1] = {}) {
    return post('/give/key', await signInBody(d, o));
  }

  /** A challenge made here as the checkout makes one (the time, random bytes, a MAC with the
   * setup passphrase), for `kind` at `at`. */
  function challengeFor(kind: string, at: number, passphrase = PASSPHRASE): string {
    const body = Buffer.alloc(20);
    body.writeUInt32BE(at);
    body.set(webcrypto.getRandomValues(new Uint8Array(16)), 4);
    const tag = createHmac('sha256', passphrase).update(Buffer.concat([Buffer.from(kind), body])).digest().subarray(0, 16);
    return b64u(Buffer.concat([body, tag]));
  }

  /** Another device's new passkey, as the page sends it, answering `challenge`. */
  async function addBody(d: Device, challenge: string): Promise<Record<string, unknown>> {
    return {
      id: d.id,
      spki: await d.spki(),
      alg: d.alg,
      authenticatorData: b64u(authenticator(0x45, 0)),
      clientDataJSON: b64u(client('webauthn.create', challenge)),
    };
  }

  /** `d` added after a sign-in by `by`. */
  async function add(by: Device, d: Device): Promise<{ status: number; body: Record<string, unknown> }> {
    const start = await post('/give/add/start', await signInBody(by));
    if (start.status !== 200) return start;
    return post('/give/add/finish', await addBody(d, start.body.challenge as string));
  }

  async function page(): Promise<{ status: number; page: string }> {
    const res = await fetch(`${base}/give`, { headers: from() });
    return { status: res.status, page: await res.text() };
  }

  let device: Device;

  it('is closed without a passphrase, or with one shorter than 20 characters', async () => {
    expect((await page()).status).toBe(503);
    expect((await post('/give/setup/start', { passphrase: PASSPHRASE })).status).toBe(503);
    expect((await post('/give/start', {})).status).toBe(503);
    await restart(KEY_ID, {}, { KEY_PAGE_PASSPHRASE: 'nineteen characters' });
    try {
      expect((await page()).status).toBe(503);
      expect((await post('/give/setup/start', { passphrase: 'nineteen characters' })).status).toBe(503);
    } finally {
      await restart();
    }
  });

  it('sets up one device, with the passphrase only, then offers only to sign in', async () => {
    await restart(KEY_ID, {}, open);
    try {
      const first = await page();
      expect(first.status).toBe(200);
      expect(first.page).toContain('<meta name="robots" content="noindex">');
      expect(first.page).toContain('<input id="passphrase"');
      // Nothing on the page says what it gives.
      expect(first.page.slice(first.page.indexOf('<main'), first.page.indexOf('<div id="got"'))).not.toMatch(/\bkeys?\b/i);
      expect(await post('/give/start', {})).toEqual({ status: 409, body: { error: 'not-set-up' } });

      device = await Device.make();
      expect((await setUp(device, 'not the passphrase at all, no')).status).toBe(403);
      // The ceremony must be this page's, for this website, with the owner checked.
      const start = await post('/give/setup/start', { passphrase: PASSPHRASE });
      const finish = (o: { origin?: string; flags?: number; alg?: number }) =>
        device.spki().then((spki) =>
          post('/give/setup/finish', {
            passphrase: PASSPHRASE,
            id: device.id,
            spki,
            alg: o.alg ?? -7,
            authenticatorData: b64u(authenticator(o.flags ?? 0x45, 0)),
            clientDataJSON: b64u(client('webauthn.create', start.body.challenge as string, o.origin)),
          }),
        );
      expect((await finish({ origin: 'https://elsewhere.example' })).status).toBe(400);
      expect((await setUp(device)).body).toEqual({ ok: true });

      const after = await page();
      expect(after.page).toContain('>Sign in</button>');
      expect(after.page).not.toContain('<input id="passphrase"');
      // Adding a device is two clicks: the second, for the other device, waits hidden.
      expect(after.page).toContain('<div id="next" class="after" hidden>');
      expect(after.page).toContain('>Set up the other device</button>');
      // Still nothing on the page says what it gives (a security key is a device to sign in with).
      const asks = after.page.slice(after.page.indexOf('<main'), after.page.indexOf('<div id="got"'));
      expect(asks.replace(/security\s+key/g, '')).not.toMatch(/\bkeys?\b/i);
      expect((await setUp(await Device.make())).body).toEqual({ error: 'set-up' });
      // The old way, the passphrase alone, gives nothing.
      expect((await fetch(`${base}/give`, { method: 'POST', headers: from(), body: new URLSearchParams({ passphrase: PASSPHRASE }) })).status).toBe(405);
      expect((await post('/give/key', { passphrase: PASSPHRASE })).status).toBe(403);
    } finally {
      await restart();
    }
  });

  it('gives a key to the device set up, once it has checked the owner, once per sign-in', async () => {
    await restart(KEY_ID, {}, open);
    try {
      const ok = await signIn(device);
      expect(ok.status).toBe(200);
      const key = ok.body.key as string;
      expect(keyIn(key)).toBe(key);
      const again = await signIn(device);
      expect(again.body.key).not.toBe(key);

      // A sign-in sent again, by someone who saw it, gives nothing.
      const seen = await signInBody(device);
      expect((await post('/give/key', seen)).status).toBe(200);
      expect((await post('/give/key', seen)).status).toBe(403);
      // Signed by another device, under this one's name.
      const other = await Device.make();
      const forged = await signInBody(device);
      const cd = Buffer.from(forged.clientDataJSON, 'base64url');
      const ad = Buffer.from(forged.authenticatorData, 'base64url');
      forged.signature = b64u(await other.sign(Buffer.concat([ad, sha(cd)])));
      expect((await post('/give/key', forged)).status).toBe(403);
      // Not checked (present only), from another page, another website, another ceremony, or an
      // unknown challenge; and a counter that did not go up (a copy of the passkey).
      expect((await signIn(device, { flags: 0x01 })).status).toBe(403);
      expect((await signIn(device, { origin: 'https://elsewhere.example' })).status).toBe(403);
      expect((await signIn(device, { rp: 'elsewhere.example' })).status).toBe(403);
      expect((await signIn(device, { type: 'webauthn.create' })).status).toBe(403);
      expect((await signIn(device, { challenge: b64u(webcrypto.getRandomValues(new Uint8Array(32))) })).status).toBe(403);
      expect((await signIn(device, { count: 1 })).status).toBe(403);
      expect((await signIn(device)).status).toBe(200);
      // Challenges are good for 5 minutes, for their own ceremony only.
      const now = Math.floor(Date.now() / 1000);
      expect((await signIn(device, { challenge: challengeFor('get', now) })).status).toBe(200);
      expect((await signIn(device, { challenge: challengeFor('get', now - 301) })).status).toBe(403);
      expect((await signIn(device, { challenge: challengeFor('create', now) })).status).toBe(403);
      expect((await signIn(device, { challenge: challengeFor('add', now) })).status).toBe(403);
    } finally {
      await restart();
    }
  });

  it('gives a key that turns Pro on as a bought one, on Live keys too, recorded as given', async () => {
    await restart(LIVE_KEY_ID, {}, open);
    try {
      const key = (await signIn(device)).body.key as string;
      const a = pcId();
      expect((await post('/activate', { key, pc: a })).status).toBe(200);
      expect((await post('/check', { key, pc: a })).body).toEqual({ ok: true });
      expect(await post('/activate', { key, pc: pcId() })).toEqual({ status: 409, body: { error: 'in-use' } });
      const ledger = await server.getWorker().getDurableObjectStorage('LEDGER', { name: 'ledger' });
      const rows = (await ledger.exec('SELECT payment, test FROM licences WHERE key = ?', key)) as Array<Record<string, unknown>>;
      expect(rows).toEqual([{ payment: expect.stringMatching(/^given:/), test: 0 }]);
    } finally {
      await restart();
    }
  });

  it('adds another device only after a sign-in with one set up, up to 5', async () => {
    await restart(KEY_ID, {}, open);
    try {
      const phone = await Device.make(-8, false);
      // Not without a sign-in: none, the passphrase, or another device's signature.
      expect((await post('/give/add/start', {})).status).toBe(403);
      expect((await post('/give/add/start', { passphrase: PASSPHRASE })).status).toBe(403);
      const forged = await signInBody(device);
      const cd = Buffer.from(forged.clientDataJSON, 'base64url');
      const ad = Buffer.from(forged.authenticatorData, 'base64url');
      forged.signature = b64u(await phone.sign(Buffer.concat([ad, sha(cd)])));
      expect((await post('/give/add/start', forged)).status).toBe(403);
      // Nor with a challenge not given for adding: setup's, a sign-in's, or one too old.
      const now = Math.floor(Date.now() / 1000);
      for (const ch of [challengeFor('create', now), challengeFor('get', now), challengeFor('add', now - 301)]) {
        expect((await post('/give/add/finish', await addBody(phone, ch))).status).toBe(400);
      }

      // After a sign-in: the start names the devices set up, so the browser offers another.
      const start = await post('/give/add/start', await signInBody(device));
      expect(start.status).toBe(200);
      expect(start.body.exclude).toEqual([device.id]);
      const body = await addBody(phone, start.body.challenge as string);
      expect(await post('/give/add/finish', body)).toEqual({ status: 200, body: { ok: true, devices: 2 } });
      expect((await post('/give/add/finish', body)).status).toBe(400);
      // Either device signs in now; the passphrase still sets up nothing.
      expect((await signIn(phone)).status).toBe(200);
      expect((await signIn(device)).status).toBe(200);
      expect((await setUp(await Device.make())).body).toEqual({ error: 'set-up' });
      // An added device adds too, up to 5 in all.
      for (let n = 3; n <= 5; n++) expect((await add(phone, await Device.make())).body).toEqual({ ok: true, devices: n });
      expect((await post('/give/add/start', await signInBody(device))).body).toEqual({ error: 'devices' });
    } finally {
      await restart();
    }
  });

  it('adds the other device itself, with the code a device set up shows, once', async () => {
    const pass = 'a passphrase for the code tests';
    await restart(KEY_ID, {}, { KEY_PAGE_PASSPHRASE: pass });
    try {
      const pc = await Device.make();
      expect((await setUp(pc, pass)).body).toEqual({ ok: true });
      // A sign-in's start gives a code: a short name for its challenge.
      const start = await post('/give/add/start', await signInBody(pc));
      const code = start.body.code as string;
      expect(code).toMatch(/^[0-9A-HJKMNP-TV-Z]{5}-[0-9A-HJKMNP-TV-Z]{5}$/);
      // Not a code, or one not shown: refused.
      expect(await post('/give/add/code', {})).toEqual({ status: 400, body: { error: 'code' } });
      expect(await post('/give/add/code', { code: 'not a code' })).toEqual({ status: 400, body: { error: 'code' } });
      const unshown = (code[0] === '0' ? '1' : '0') + code.slice(1);
      expect(await post('/give/add/code', { code: unshown })).toEqual({ status: 403, body: { error: 'code' } });

      // On the phone, typed as people type it: the same ceremony as the PC's.
      const phone = await Device.make(-8, false);
      const got = await post('/give/add/code', { code: ` ${code.replace('-', '').toLowerCase()} ` });
      expect(got.status).toBe(200);
      expect(got.body.challenge).toBe(start.body.challenge);
      expect(got.body.rp).toEqual(start.body.rp);
      expect(got.body.exclude).toEqual([pc.id]);
      // Once: seen again, the code gives nothing.
      expect(await post('/give/add/code', { code })).toEqual({ status: 403, body: { error: 'code' } });
      const body = await addBody(phone, got.body.challenge as string);
      expect(await post('/give/add/finish', body)).toEqual({ status: 200, body: { ok: true, devices: 2 } });
      expect((await post('/give/add/finish', body)).status).toBe(400);
      expect((await signIn(phone)).status).toBe(200);

      // The PC's own button and the code share a challenge: one device each sign-in.
      const shared = await post('/give/add/start', await signInBody(pc));
      expect((await post('/give/add/finish', await addBody(await Device.make(), shared.body.challenge as string))).body).toEqual({ ok: true, devices: 3 });
      const late = await post('/give/add/code', { code: shared.body.code });
      expect(late.status).toBe(200);
      expect((await post('/give/add/finish', await addBody(await Device.make(), late.body.challenge as string))).status).toBe(400);

      // Up to 5 in all, by code too; a code shown before then gives nothing once 5 are set up.
      const codes: string[] = [];
      for (let i = 0; i < 3; i++) codes.push((await post('/give/add/start', await signInBody(phone))).body.code as string);
      for (const [i, c] of codes.slice(0, 2).entries()) {
        const o = await post('/give/add/code', { code: c });
        expect((await post('/give/add/finish', await addBody(await Device.make(), o.body.challenge as string))).body).toEqual({ ok: true, devices: 4 + i });
      }
      expect(await post('/give/add/code', { code: codes[2] })).toEqual({ status: 409, body: { error: 'devices' } });
    } finally {
      await restart();
    }
  });

  it('names a device by its system and browser, in fixed words only', () => {
    // User-Agents as browsers send them (constructed).
    const named: Array<[string, string]> = [
      ['Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0', 'Windows, Edge'],
      ['Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:145.0) Gecko/20100101 Firefox/145.0', 'Windows, Firefox'],
      ['Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 OPR/125.0.0.0', 'Windows, Opera'],
      ['Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36', 'Android, Chrome'],
      ['Mozilla/5.0 (Linux; Android 14; SM-S921B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/28.0 Chrome/130.0.0.0 Mobile Safari/537.36', 'Android, Samsung Internet'],
      ['Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1', 'iPhone, Safari'],
      ['Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/142.0.0.0 Mobile/15E148 Safari/604.1', 'iPhone, Chrome'],
      ['Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Safari/605.1.15', 'Mac, Safari'],
      ['Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36', 'ChromeOS, Chrome'],
      ['Mozilla/5.0 (X11; Linux x86_64; rv:145.0) Gecko/20100101 Firefox/145.0', 'Linux, Firefox'],
    ];
    for (const [ua, name] of named) expect(deviceName(ua)).toBe(name);
    // Nothing but the fixed words is kept.
    expect(deviceName('Windows <img src=x onerror=alert(1)>')).toBe('Windows');
    expect(deviceName('node')).toBeNull();
    expect(deviceName(undefined)).toBeNull();
    // Made on another device from the browser's window: that device, whatever the browser is.
    expect(deviceName(named[0]![0], 'cross-platform', ['hybrid', 'internal'])).toBe('A phone, by its QR code');
    expect(deviceName(named[0]![0], 'cross-platform', ['nfc', 'usb'])).toBe('A security key');
    expect(deviceName(named[0]![0], 'cross-platform')).toBeNull();
    expect(deviceName(named[0]![0], 'platform', ['hybrid'])).toBe('Windows, Edge');
  });

  it('lists the devices after a sign-in, by name, and removes one, never the last', async () => {
    const pass = 'a passphrase for the removing tests';
    await restart(KEY_ID, {}, { KEY_PAGE_PASSPHRASE: pass });
    const as = (ua: string) => ({ ...from(), 'User-Agent': ua });
    const EDGE = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36 Edg/142.0.0.0';
    const PHONE = 'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36';
    /** A sign-in by `d`, which is no longer offered, refused. */
    const refused = async (d: Device) => {
      const start = await post('/give/start', {});
      expect(start.body.ids).not.toContain(d.id);
      const cd = client('webauthn.get', start.body.challenge as string);
      const ad = authenticator(0x05, d.counts ? ++d.count : 0);
      const sig = await d.sign(Buffer.concat([ad, sha(cd)]));
      expect((await post('/give/key', { id: d.id, authenticatorData: b64u(ad), clientDataJSON: b64u(cd), signature: b64u(sig) })).status).toBe(403);
    };
    try {
      // The PC with the passphrase, the phone with a code, and a security key from the PC.
      const pc = await Device.make();
      const start = await post('/give/setup/start', { passphrase: pass });
      expect((await post('/give/setup/finish', { passphrase: pass, ...(await addBody(pc, start.body.challenge as string)) }, as(EDGE))).body).toEqual({ ok: true });
      expect((await page()).page).toContain('<button id="list-open" class="quiet" type="button">Devices set up here</button>');
      const code = (await post('/give/add/start', await signInBody(pc))).body.code;
      const phone = await Device.make(-8, false);
      const o = await post('/give/add/code', { code });
      expect((await post('/give/add/finish', await addBody(phone, o.body.challenge as string), as(PHONE))).body).toEqual({ ok: true, devices: 2 });
      const usb = await Device.make();
      const fromPc = await post('/give/add/start', await signInBody(pc));
      const made = { ...(await addBody(usb, fromPc.body.challenge as string)), attachment: 'cross-platform', transports: ['nfc', 'usb'] };
      expect((await post('/give/add/finish', made, as(EDGE))).body).toEqual({ ok: true, devices: 3 });

      // Not without a sign-in; after one, each device by name, with when it was set up and used.
      expect((await post('/give/devices', {})).status).toBe(403);
      expect((await post('/give/devices', { passphrase: pass })).status).toBe(403);
      const seen = await post('/give/devices', await signInBody(phone));
      expect(seen.status).toBe(200);
      expect(seen.body.by).toBe(phone.id);
      const at = expect.any(Number);
      expect(seen.body.devices).toEqual([
        { id: pc.id, name: 'Windows, Edge', created: at, used: at, here: true },
        { id: phone.id, name: 'Android, Chrome', created: at, used: at, here: true },
        { id: usb.id, name: 'A security key', created: at, used: null, here: true },
      ]);

      // A device set up before names were kept is named at its next sign-in on it, not from
      // another device in the browser's window.
      const ledger = await server.getWorker().getDurableObjectStorage('LEDGER', { name: 'ledger' });
      await ledger.exec('UPDATE passkeys SET name = NULL WHERE id = ?', pc.id);
      const names = async () => ((await post('/give/devices', await signInBody(pc))).body.devices as Array<{ name: string | null }>).map((d) => d.name);
      expect((await post('/give/key', { ...(await signInBody(pc)), attachment: 'cross-platform' }, as(PHONE))).status).toBe(200);
      expect(await names()).toEqual([null, 'Android, Chrome', 'A security key']);
      expect((await post('/give/key', { ...(await signInBody(pc)), attachment: 'platform' }, as(EDGE))).status).toBe(200);
      expect(await names()).toEqual(['Windows, Edge', 'Android, Chrome', 'A security key']);
      // Kept once given: a sign-in from elsewhere does not rename it.
      expect((await post('/give/key', await signInBody(pc), as(PHONE))).status).toBe(200);
      expect((await names())[0]).toBe('Windows, Edge');

      // Not without its pass: none, another device's name on it, a sign-in's challenge, one too
      // old, or one made under another passphrase.
      const removing = (id: string, x: Record<string, unknown> = {}) => post('/give/remove', { pass: seen.body.pass, by: phone.id, id, ...x });
      const now = Math.floor(Date.now() / 1000);
      expect((await post('/give/remove', { by: phone.id, id: usb.id })).status).toBe(403);
      expect((await removing(usb.id, { by: pc.id })).status).toBe(403);
      expect((await removing(usb.id, { pass: challengeFor('get', now, pass) })).status).toBe(403);
      expect((await removing(usb.id, { pass: challengeFor(`remove:${phone.id}`, now - 301, pass) })).status).toBe(403);
      expect((await removing(usb.id, { pass: challengeFor(`remove:${phone.id}`, now, PASSPHRASE) })).status).toBe(403);
      expect((await removing(usb.id, { pass: challengeFor(`remove:${phone.id}`, now, pass) })).status).toBe(200);
      expect((await post('/give/devices', await signInBody(pc))).body.devices).toHaveLength(2);
      await refused(usb);

      // The pass removes more than one; the device it names cannot remove the last.
      expect((await removing(usb.id)).body).toEqual({ ok: true, devices: [{ id: pc.id, name: 'Windows, Edge', created: at, used: at, here: true }, { id: phone.id, name: 'Android, Chrome', created: at, used: at, here: true }] });
      expect((await removing(pc.id)).status).toBe(200);
      await refused(pc);
      expect(await removing(phone.id)).toEqual({ status: 409, body: { error: 'last' } });
      expect((await signIn(phone)).status).toBe(200);
      // One left, the passphrase still sets up nothing.
      expect((await post('/give/setup/start', { passphrase: pass })).body).toEqual({ error: 'set-up' });

      // A device removes itself; its pass then removes nothing more.
      const laptop = await Device.make();
      expect((await add(phone, laptop)).body).toEqual({ ok: true, devices: 2 });
      const own = await post('/give/devices', await signInBody(laptop));
      const self = (id: string) => post('/give/remove', { pass: own.body.pass, by: laptop.id, id });
      expect((await self(laptop.id)).body).toEqual({ ok: true, devices: [{ id: phone.id, name: 'Android, Chrome', created: at, used: at, here: true }] });
      await refused(laptop);
      expect(await self(phone.id)).toEqual({ status: 403, body: { error: 'pass' } });
      expect((await signIn(phone)).status).toBe(200);
    } finally {
      await restart();
    }
  });

  it('starts setup again under a new passphrase: the old device then gives nothing', async () => {
    const next = { KEY_PAGE_PASSPHRASE: 'another passphrase for the tests' };
    await restart(KEY_ID, {}, next);
    try {
      expect((await page()).page).toContain('<input id="passphrase"');
      expect(await post('/give/start', {})).toEqual({ status: 409, body: { error: 'not-set-up' } });
      const stale = await post('/give/key', { id: device.id, authenticatorData: 'AA', clientDataJSON: 'AA', signature: 'AA' });
      expect(stale.status).toBe(403);
      // An EdDSA passkey without a counter, as some authenticators make: a sign-in sent again is
      // refused by its challenge, used once, alone.
      const ed = await Device.make(-8, false);
      expect((await setUp(ed, next.KEY_PAGE_PASSPHRASE)).body).toEqual({ ok: true });
      expect((await signIn(ed)).status).toBe(200);
      const seen = await signInBody(ed);
      expect((await post('/give/key', seen)).status).toBe(200);
      expect((await post('/give/key', seen)).status).toBe(403);
      expect((await signIn(ed)).status).toBe(200);
    } finally {
      await restart();
    }
  });

  it('takes no setup passphrase, not even the right one, after 20 wrong ones in an hour', async () => {
    const fresh = 'a third passphrase for the tests';
    await restart(KEY_ID, {}, { KEY_PAGE_PASSPHRASE: fresh });
    try {
      while (wrong < 20) expect((await setUp(await Device.make(), `wrong passphrase number ${wrong}`)).status).toBe(403);
      expect(await post('/give/setup/start', { passphrase: fresh })).toEqual({ status: 429, body: { error: 'wait' } });
      expect((await post('/give/setup/start', { passphrase: 'one more wrong passphrase' })).status).toBe(429);
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

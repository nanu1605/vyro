// Clokrix Pro's checkout and keys (2.5).
//
// The buyer's side, in a browser:
//   GET  /buy[?c=<claim>]    the page that pays. `c` is the code of the app that opened it, which
//                            then turns Pro on by itself.
//   POST /order              {c?}: a Razorpay order, made when Pay is pressed.
//   POST /paid               where Razorpay's checkout sends a payment: checked, then the key.
//   POST /razorpay/webhook   Razorpay's own word of a payment (or refund), in case the page closed.
// The app's side, as JSON:
//   GET  /claim/<claim>      the key of the purchase the app opened, once paid; given once.
//   POST /activate           {key, pc, move?}: Pro on this PC; answers a signed activation.
//   POST /release            {key, pc}: the key off this PC, for another to take.
//   POST /check              {key, pc}: still Pro here? (not if moved, released or refunded).

import { capture, createOrder, getPayment, paidFor, type Payment, type Razorpay } from './razorpay';
import { checkoutPage, errorPage, pageHeaders, paidPage } from './page';
import { LEDGER_NAME, type Ledger } from './ledger';
import { hmacHex, isClaim, isPc, normalKey, same, signActivation, signingKey } from './rules';

export { Ledger } from './ledger';

interface Env {
  LEDGER: DurableObjectNamespace<Ledger>;
  SELLER?: string;
  /** Razorpay's API; the tests point it at their stand-in. */
  RAZORPAY_API?: string;
  RAZORPAY_KEY_ID?: string;
  RAZORPAY_KEY_SECRET?: string;
  RAZORPAY_WEBHOOK_SECRET?: string;
  LICENCE_SIGNING_KEY?: string;
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff' },
  });
}

function nonce(): string {
  return crypto.randomUUID().replace(/-/g, '');
}

function html(status: number, n: string, body: string): Response {
  return new Response(body, { status, headers: pageHeaders(n) });
}

function razorpay(env: Env): Razorpay | null {
  if (!env.RAZORPAY_KEY_ID || !env.RAZORPAY_KEY_SECRET) return null;
  return { api: env.RAZORPAY_API || 'https://api.razorpay.com', keyId: env.RAZORPAY_KEY_ID, keySecret: env.RAZORPAY_KEY_SECRET };
}

async function body(req: Request): Promise<Record<string, unknown> | null> {
  try {
    const v: unknown = await req.json();
    return v && typeof v === 'object' ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** A captured payment for `orderId`, capturing one only authorised; null if it is not paid in full. */
async function settled(r: Razorpay, paymentId: string, orderId: string): Promise<Payment | null> {
  let p = await getPayment(r, paymentId);
  if (p.status === 'authorized' && p.order_id === orderId) p = await capture(r, p);
  return paidFor(p, orderId) ? p : null;
}

export default {
  async fetch(req, env): Promise<Response> {
    const url = new URL(req.url);
    const ledger = env.LEDGER.get(env.LEDGER.idFromName(LEDGER_NAME));
    const ip = req.headers.get('CF-Connecting-IP') ?? 'local';
    const path = url.pathname;
    const n = nonce();

    if (path === '/razorpay/webhook') {
      if (req.method !== 'POST') return json(405, { error: 'not-allowed' });
      const secret = env.RAZORPAY_WEBHOOK_SECRET;
      const raw = await req.text();
      const sig = req.headers.get('X-Razorpay-Signature') ?? '';
      if (!secret || !same(await hmacHex(secret, raw), sig)) return json(400, { error: 'signature' });
      let event: { event?: string; payload?: Record<string, { entity?: Record<string, unknown> }> };
      try {
        event = JSON.parse(raw);
      } catch {
        return json(400, { error: 'body' });
      }
      const payment = event.payload?.payment?.entity as unknown as Payment | undefined;
      if ((event.event === 'payment.captured' || event.event === 'order.paid') && payment?.order_id) {
        if (paidFor(payment, payment.order_id)) await ledger.issue(payment.order_id, payment.id);
      } else if (event.event === 'refund.processed' || event.event === 'payment.refunded') {
        const id = (event.payload?.refund?.entity?.payment_id as string | undefined) ?? payment?.id;
        if (id) await ledger.revoke(id);
      }
      // Anything else signed is taken and left alone, so Razorpay does not send it again.
      return json(200, { ok: true });
    }

    if (!(await ledger.hit(ip))) return json(429, { error: 'too-many' });

    if (path === '/' || path === '/buy') {
      if (req.method !== 'GET' && req.method !== 'HEAD') return json(405, { error: 'not-allowed' });
      const c = url.searchParams.get('c');
      if (c !== null && !isClaim(c)) return html(400, n, errorPage(n, 'Not a Clokrix link', 'Open Buy Pro in Clokrix again.'));
      if (c) {
        const key = await ledger.keyOf(c);
        if (key) return html(200, n, paidPage({ nonce: n, key, fromApp: true }));
      }
      const r = razorpay(env);
      if (!r) return html(503, n, errorPage(n, 'Not open yet', 'Clokrix Pro cannot be bought here yet.'));
      // The order is made when Pay is pressed, not for every look at the page.
      return html(200, n, checkoutPage({ nonce: n, seller: env.SELLER || 'Clokrix', keyId: r.keyId, claim: c }));
    }

    if (path === '/order') {
      if (req.method !== 'POST') return json(405, { error: 'not-allowed' });
      const r = razorpay(env);
      if (!r) return json(503, { error: 'not-open' });
      const b = await body(req);
      const c = b?.c ?? null;
      if (c !== null && !isClaim(c as string)) return json(400, { error: 'claim' });
      const claim = c as string | null;
      let order: string;
      try {
        order = await createOrder(r, `clokrix-${n.slice(0, 20)}`, claim);
      } catch {
        return json(502, { error: 'razorpay' });
      }
      await ledger.addOrder(order, claim);
      return json(200, { order_id: order });
    }

    if (path === '/paid') {
      if (req.method !== 'POST') return json(405, { error: 'not-allowed' });
      const r = razorpay(env);
      if (!r) return html(503, n, errorPage(n, 'Not open yet', 'Clokrix Pro cannot be bought here yet.'));
      const form = await req.formData().catch(() => null);
      const paymentId = String(form?.get('razorpay_payment_id') ?? '');
      const orderId = String(form?.get('razorpay_order_id') ?? '');
      const sig = String(form?.get('razorpay_signature') ?? '');
      const sorry = 'If you were charged, the key comes by itself within minutes: open Buy Pro in Clokrix again, or write to the seller with your Razorpay receipt.';
      if (!paymentId || !orderId || !same(await hmacHex(r.keySecret, `${orderId}|${paymentId}`), sig)) {
        return html(400, n, errorPage(n, 'Payment not confirmed', sorry));
      }
      if (!(await ledger.hasOrder(orderId))) return html(400, n, errorPage(n, 'Payment not confirmed', sorry));
      let paid: Payment | null;
      try {
        paid = await settled(r, paymentId, orderId);
      } catch {
        return html(502, n, errorPage(n, 'Payment not confirmed yet', sorry));
      }
      if (!paid) return html(402, n, errorPage(n, 'Payment not complete', sorry));
      const l = await ledger.issue(orderId, paid.id);
      if (!l) return html(400, n, errorPage(n, 'Payment not confirmed', sorry));
      return html(200, n, paidPage({ nonce: n, key: l.key, fromApp: await ledger.fromApp(orderId) }));
    }

    const claimAt = /^\/claim\/([0-9a-f]{32})$/.exec(path);
    if (claimAt) {
      if (req.method !== 'GET') return json(405, { error: 'not-allowed' });
      const got = await ledger.claim(claimAt[1]!);
      return 'key' in got ? json(200, { key: got.key }) : json(got.status, { error: got.status === 410 ? 'collected' : 'not-paid' });
    }

    if (path === '/activate' || path === '/release' || path === '/check') {
      if (req.method !== 'POST') return json(405, { error: 'not-allowed' });
      const b = await body(req);
      const key = typeof b?.key === 'string' ? normalKey(b.key) : null;
      const pc = b?.pc;
      if (!key) return json(400, { error: 'key' });
      if (!isPc(pc)) return json(400, { error: 'pc' });
      if (path === '/release') return (await ledger.release(key, pc)) ? json(200, { ok: true }) : json(409, { error: 'not-here' });
      if (path === '/check') return json(200, await ledger.check(key, pc));
      if (!env.LICENCE_SIGNING_KEY) return json(503, { error: 'not-open' });
      const a = await ledger.activate(key, pc, b?.move === true);
      if (!a.ok) return json(a.status, { error: a.error, ...(a.until ? { until: a.until } : {}) });
      const token = await signActivation(await signingKey(env.LICENCE_SIGNING_KEY), {
        v: 1,
        lic: a.lic,
        pc,
        edition: 'pro',
        issued: Math.floor(Date.now() / 1000),
      });
      return json(200, { activation: token, key });
    }

    return json(404, { error: 'not-found' });
  },
} satisfies ExportedHandler<Env>;

// Clokrix Pro's checkout and keys (2.5), and Clokrix's website around them (site.ts).
//
// The buyer's side, in a browser:
//   GET  /, /terms, /privacy, /refunds, /delivery, /contact
//                            the website: what Razorpay checks before it gives Live keys.
//   GET  /buy[?c=<claim>][&cur=inr|usd]
//                            the page that pays. `c` is the code of the app that opened it, which
//                            then turns Pro on by itself. ₹10,000 in India, $100 elsewhere, by the
//                            connection's country; `cur` is the buyer's choice of the other.
// With SITE set, those pages are only at SITE: asked for on another address (the workers.dev one
// 2.5.0's app opens), they send the browser there, so Razorpay sees payments from the website's
// own domain.
//   POST /order              {c?, cur?}: a Razorpay order, made when Pay is pressed, at the price in
//                            `cur` (INR or USD; by the buyer's country if not given).
//   POST /paid               where Razorpay's checkout sends a payment: checked, then the key.
//   POST /razorpay/webhook   Razorpay's own word of a payment (or refund), in case the page closed.
// The app's side, as JSON:
//   GET  /claim/<claim>      the key of the purchase the app opened, once paid; given once.
//   POST /activate           {key, pc, move?}: Pro on this PC; answers a signed activation.
//   POST /release            {key, pc}: the key off this PC, for another to take.
//   POST /check              {key, pc}: still Pro here? (not if moved, released or refunded).
// The seller's side (2.5.5), in a browser: keys given, not sold (the owner, 2026-11-26: "build
// the key page"), only to the owner's own device (webauthn.ts; the owner: "Use some safety and
// privacy so only i can generate the key in whole jainav infotech"). Closed without a
// KEY_PAGE_PASSPHRASE of at least MIN_PASSPHRASE characters.
//   GET  /give               sets up the device's passkey, with that passphrase, while none is
//                            set up; then signs in with it.
//   POST /give/setup/start, /give/setup/finish
//                            the passkey made and kept: with the passphrase, while none is set up.
//                            WRONG_PER_HOUR wrong passphrases in an hour close setup for the rest
//                            of it.
//   POST /give/start, /give/key
//                            a sign-in with the passkey, checked (fingerprint, face or PIN); then a
//                            new key.
//   POST /give/add/start, /give/add/finish
//                            another device's passkey (up to MAX_DEVICES), after a sign-in with one
//                            set up. The start also gives a code for the other device itself:
//   POST /give/add/code      {code}: the same ceremony, begun on the other device, once.
// Clokrix itself (2.5.2, downloads.ts), from the R2 bucket the Release workflow fills:
//   GET  /download, /download/<tag>/<file>, /updates/latest.json

import { capture, createOrder, getPayment, paidFor, type Payment, type Razorpay } from './razorpay';
import { checkoutPage, errorPage, keyPage, pageHeaders, paidPage } from './page';
import { ALGS, authData, bytes, clientData, freshChallenge, newChallenge, ours, publicKey, signed, vouched } from './webauthn';
import { isSitePage, sitePage, type Seller } from './site';
import { download, isDownload } from './downloads';
import { LEDGER_NAME, type Ledger, type Passkey } from './ledger';
import {
  b64url,
  chosenCurrency,
  currencyFor,
  hmacHex,
  isClaim,
  isPc,
  MAX_DEVICES,
  MIN_PASSPHRASE,
  newCode,
  normalCode,
  normalKey,
  same,
  samePassphrase,
  signActivation,
  setupId,
  signingKey,
  unb64,
  type Currency,
} from './rules';

export { Ledger } from './ledger';

interface Env {
  LEDGER: DurableObjectNamespace<Ledger>;
  /** Clokrix's installers and the updater's manifest (2.5.2); the Release workflow fills it. */
  DOWNLOADS?: R2Bucket;
  SELLER?: string;
  /** The website's address (https://host), where its pages are served; unset, they are served
   * anywhere. */
  SITE?: string;
  /** How to reach the seller, for the website's pages. */
  CONTACT_EMAIL?: string;
  CONTACT_PHONE?: string;
  CONTACT_ADDRESS?: string;
  /** Razorpay's API; the tests point it at their stand-in. */
  RAZORPAY_API?: string;
  RAZORPAY_KEY_ID?: string;
  RAZORPAY_KEY_SECRET?: string;
  RAZORPAY_WEBHOOK_SECRET?: string;
  LICENCE_SIGNING_KEY?: string;
  /** The key page's passphrase (2.5.5); unset or shorter than MIN_PASSPHRASE, the page is
   * closed. */
  KEY_PAGE_PASSPHRASE?: string;
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

/** The checkout runs on Razorpay's Live keys: real payments. Keys bought before, under Test keys,
 * then no longer turn Pro on (the owner, 2026-11-16: "Cancel test keys at go-live"). */
function live(env: Env): boolean {
  return (env.RAZORPAY_KEY_ID ?? '').startsWith('rzp_live_');
}

function seller(env: Env): Seller {
  return {
    name: env.SELLER || 'Clokrix',
    email: env.CONTACT_EMAIL?.trim() || undefined,
    phone: env.CONTACT_PHONE?.trim() || undefined,
    address: env.CONTACT_ADDRESS?.trim() || undefined,
  };
}

/** SITE's origin, or null when unset or not an address. */
function siteOrigin(env: Env): string | null {
  if (!env.SITE) return null;
  try {
    return new URL(env.SITE).origin;
  } catch {
    return null;
  }
}

/** The buyer's currency: the one they chose (`cur`), else by their connection's country, as
 * Cloudflare gives it in CF-IPCountry and in request.cf (the header first, so the tests can set
 * it). Either way it picks only the price shown first: the page offers the other. */
function currency(req: Request, chosen: unknown): Currency {
  return chosenCurrency(chosen) ?? currencyFor(req.headers.get('CF-IPCountry') ?? (req.cf?.country as string | undefined));
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
        if (paidFor(payment, payment.order_id)) await ledger.issue(payment.order_id, payment.id, !live(env));
      } else if (event.event === 'refund.processed' || event.event === 'payment.refunded') {
        const id = (event.payload?.refund?.entity?.payment_id as string | undefined) ?? payment?.id;
        if (id) await ledger.revoke(id);
      }
      // Anything else signed is taken and left alone, so Razorpay does not send it again.
      return json(200, { ok: true });
    }

    // Downloads and update checks: on every address, and not counted against purchases.
    if (isDownload(path)) return download(req, env.DOWNLOADS);

    const s = seller(env);
    const at = { nonce: n, seller: s.name };
    const isPage = path === '/buy' || path === '/give' || isSitePage(path);
    const origin = siteOrigin(env);
    // By host: Cloudflare, not this, sends http to https.
    if (isPage && origin && url.host !== new URL(origin).host && (req.method === 'GET' || req.method === 'HEAD')) {
      // 302, not 301: a browser would keep a permanent one even after the website moved.
      return Response.redirect(`${origin}${path}${url.search}`, 302);
    }

    if (!(await ledger.hit(ip))) return json(429, { error: 'too-many' });

    if (isSitePage(path)) {
      if (req.method !== 'GET' && req.method !== 'HEAD') return json(405, { error: 'not-allowed' });
      return html(200, n, sitePage(path, s, n, currency(req, null)));
    }

    if (path === '/buy') {
      if (req.method !== 'GET' && req.method !== 'HEAD') return json(405, { error: 'not-allowed' });
      const c = url.searchParams.get('c');
      if (c !== null && !isClaim(c)) return html(400, n, errorPage(at, 'Not a Clokrix link', 'Open Buy Pro in Clokrix again.'));
      if (c) {
        const key = await ledger.keyOf(c);
        if (key) return html(200, n, paidPage({ ...at, key, fromApp: true }));
      }
      const r = razorpay(env);
      if (!r) return html(503, n, errorPage(at, 'Not open yet', 'Clokrix Pro cannot be bought here yet.'));
      // The order is made when Pay is pressed, not for every look at the page.
      return html(200, n, checkoutPage({ ...at, keyId: r.keyId, claim: c, currency: currency(req, url.searchParams.get('cur')) }));
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
        order = await createOrder(r, `clokrix-${n.slice(0, 20)}`, claim, currency(req, b?.cur));
      } catch {
        return json(502, { error: 'razorpay' });
      }
      await ledger.addOrder(order, claim);
      return json(200, { order_id: order });
    }

    if (path === '/paid') {
      if (req.method !== 'POST') return json(405, { error: 'not-allowed' });
      const r = razorpay(env);
      if (!r) return html(503, n, errorPage(at, 'Not open yet', 'Clokrix Pro cannot be bought here yet.'));
      const form = await req.formData().catch(() => null);
      const paymentId = String(form?.get('razorpay_payment_id') ?? '');
      const orderId = String(form?.get('razorpay_order_id') ?? '');
      const sig = String(form?.get('razorpay_signature') ?? '');
      const sorry = 'If you were charged, the key comes by itself within minutes: open Buy Pro in Clokrix again, or write to the seller with your Razorpay receipt.';
      if (!paymentId || !orderId || !same(await hmacHex(r.keySecret, `${orderId}|${paymentId}`), sig)) {
        return html(400, n, errorPage(at, 'Payment not confirmed', sorry));
      }
      if (!(await ledger.hasOrder(orderId))) return html(400, n, errorPage(at, 'Payment not confirmed', sorry));
      let paid: Payment | null;
      try {
        paid = await settled(r, paymentId, orderId);
      } catch {
        return html(502, n, errorPage(at, 'Payment not confirmed yet', sorry));
      }
      if (!paid) return html(402, n, errorPage(at, 'Payment not complete', sorry));
      const l = await ledger.issue(orderId, paid.id, !live(env));
      if (!l) return html(400, n, errorPage(at, 'Payment not confirmed', sorry));
      return html(200, n, paidPage({ ...at, key: l.key, fromApp: await ledger.fromApp(orderId) }));
    }

    if (path === '/give' || path.startsWith('/give/')) {
      const set = (env.KEY_PAGE_PASSPHRASE ?? '').trim();
      const open = set.length >= MIN_PASSPHRASE;
      // A passkey belongs to the setup passphrase it was let in under: a new one starts again.
      const setup = await setupId(set);
      // The website a passkey is for, as the browser sees it.
      const rpId = url.hostname;
      const now = Math.floor(Date.now() / 1000);
      if (path === '/give') {
        if (req.method !== 'GET' && req.method !== 'HEAD') return json(405, { error: 'not-allowed' });
        if (!open) return html(503, n, errorPage(at, 'Not open', 'Nothing is here.'));
        const ready = (await ledger.passkeys(setup)).some((p) => p.rp === rpId);
        return html(200, n, keyPage({ ...at, mode: ready ? 'sign-in' : 'setup' }));
      }
      if (req.method !== 'POST') return json(405, { error: 'not-allowed' });
      if (!open) return json(503, { error: 'not-open' });
      const b = (await body(req)) ?? {};

      // Every passkey set up (on any of the checkout's addresses), and those for this website.
      const all = await ledger.passkeys(setup);
      const passkeys = all.filter((p) => p.rp === rpId);
      // The same name for everyone, and a random user id: nothing about the owner.
      const making = async (kind: 'create' | 'add', challenge?: string) => ({
        challenge: challenge ?? (await newChallenge(set, kind, now)),
        rp: { id: rpId, name: 'Clokrix' },
        user: { id: b64url(crypto.getRandomValues(new Uint8Array(16))), name: 'Clokrix Pro keys' },
        algs: ALGS,
        exclude: all.map((p) => p.id),
      });

      /** A passkey this browser just made, for a challenge of `kind` (`create` at setup, `add` for
       * another device): the passkey to keep, or why it is refused. */
      const made = async (kind: 'create' | 'add'): Promise<Passkey | string> => {
        const id = typeof b.id === 'string' && /^[A-Za-z0-9_-]{16,1364}$/.test(b.id) ? b.id : null;
        const spki = bytes(b.spki);
        const auth = bytes(b.authenticatorData);
        const client = bytes(b.clientDataJSON);
        const alg = typeof b.alg === 'number' ? b.alg : NaN;
        if (!id || !spki || !auth || !client) return 'body';
        const c = clientData(client);
        if (!c || !ours(c, 'webauthn.create', url.origin) || !(await freshChallenge(set, kind, c.challenge, now))) return 'ceremony';
        const a = authData(auth);
        if (!a || !(await vouched(a, rpId))) return 'authenticator';
        if (!(await publicKey(spki, alg))) return 'algorithm';
        if (!(await ledger.useChallenge(c.challenge))) return 'ceremony';
        return { id, rp: rpId, spki: b64url(spki), alg, count: a.count, setup };
      };

      /** A sign-in with a passkey set up here: the passkey, once all is checked and the
       * challenge used; null if it is not one. No wait after failures: a passkey's signature
       * cannot be guessed, and a wait would let anyone keep the owner out. */
      const signedIn = async (): Promise<Passkey | null> => {
        const pk = passkeys.find((p) => p.id === b.id);
        const auth = bytes(b.authenticatorData);
        const client = bytes(b.clientDataJSON);
        const sig = bytes(b.signature, 1024);
        if (!pk || !auth || !client || !sig) return null;
        const c = clientData(client);
        if (!c || !ours(c, 'webauthn.get', url.origin) || !(await freshChallenge(set, 'get', c.challenge, now))) return null;
        const a = authData(auth);
        const key = await publicKey(unb64(pk.spki), pk.alg);
        if (!a || !key || !(await vouched(a, rpId)) || !(await signed(key, pk.alg, auth, client, sig))) return null;
        // A counter that did not go up is a copy of the passkey (authenticators without one say 0).
        if ((a.count !== 0 || pk.count !== 0) && a.count <= pk.count) return null;
        // Last, once all else holds: a sign-in sent again finds its challenge used.
        if (!(await ledger.useChallenge(c.challenge))) return null;
        await ledger.setCount(pk.id, a.count);
        return pk;
      };

      if (path === '/give/setup/start' || path === '/give/setup/finish') {
        // Waiting, the right passphrase is refused too: otherwise the wait would not slow guessing.
        if (!(await ledger.passphraseTaken())) return json(429, { error: 'wait' });
        if (typeof b.passphrase !== 'string' || !(await samePassphrase(b.passphrase.trim(), set))) {
          await ledger.wrongPassphrase();
          return json(403, { error: 'passphrase' });
        }
        // The passphrase sets up the first device only; more are added from a device set up.
        if (all.length > 0) return json(409, { error: 'set-up' });
        if (path === '/give/setup/start') return json(200, await making('create'));
        const p = await made('create');
        if (typeof p === 'string') return json(400, { error: p });
        return (await ledger.addPasskey(p, true)) ? json(200, { ok: true }) : json(409, { error: 'set-up' });
      }

      if (path === '/give/start') {
        if (passkeys.length === 0) return json(409, { error: 'not-set-up' });
        return json(200, { challenge: await newChallenge(set, 'get', now), rpId, ids: passkeys.map((p) => p.id) });
      }

      if (path === '/give/key') {
        if (!(await signedIn())) return json(403, { error: 'passkey' });
        return json(200, { key: (await ledger.give()).key });
      }

      // Another device (2.5.5; the owner, 2026-11-26: "A backup device for the key page"): only
      // after a sign-in with a device set up, which gives the challenge the new passkey answers.
      if (path === '/give/add/start') {
        if (!(await signedIn())) return json(403, { error: 'passkey' });
        if (all.length >= MAX_DEVICES) return json(409, { error: 'devices' });
        const o = await making('add');
        // The same challenge by a short name, to type on the other device itself.
        const code = newCode();
        await ledger.addCode(code, o.challenge);
        return json(200, { ...o, code });
      }

      // The other device itself, with the code a device set up showed (the owner, 2026-11-27,
      // when the browser's window on the PC could not reach another: "It throws an error"). No
      // Bluetooth or browser window between the two. The code names that sign-in's challenge,
      // once, while the challenge is good; its passkey is then checked as any other's.
      if (path === '/give/add/code') {
        const code = normalCode(b.code);
        if (!code) return json(400, { error: 'code' });
        if (all.length >= MAX_DEVICES) return json(409, { error: 'devices' });
        const challenge = await ledger.takeCode(code);
        if (!challenge || !(await freshChallenge(set, 'add', challenge, now))) return json(403, { error: 'code' });
        return json(200, await making('add', challenge));
      }

      if (path === '/give/add/finish') {
        const p = await made('add');
        if (typeof p === 'string') return json(400, { error: p });
        if (!(await ledger.addPasskey(p, false))) return json(409, { error: 'devices' });
        return json(200, { ok: true, devices: all.length + 1 });
      }

      return json(404, { error: 'not-found' });
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
      if (path === '/check') return json(200, await ledger.check(key, pc, live(env)));
      if (!env.LICENCE_SIGNING_KEY) return json(503, { error: 'not-open' });
      const a = await ledger.activate(key, pc, b?.move === true, live(env));
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

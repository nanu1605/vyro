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
//   POST /order              {c?, cur?, name}: a Razorpay order, made when Pay is pressed, at the
//                            price in `cur` (INR or USD; by the buyer's country if not given),
//                            with the buyer's name (2.6), which Razorpay keeps with it.
//   POST /paid               where Razorpay's checkout sends a payment: checked, then the key.
//   POST /razorpay/webhook   Razorpay's own word of a payment (or refund), in case the page closed.
// The app's side, as JSON:
//   GET  /claim/<claim>      the key of the purchase the app opened, once paid; given once.
//   POST /activate           {key, pc, move?, about?}: Pro on this PC; answers a signed activation.
//   POST /release            {key, pc}: the key off this PC, for another to take.
//   POST /check              {key, pc, about?}: still Pro here? (not if moved, released, refunded
//                            or cancelled). `about` (2.6) is what the app says of the PC (rules.ts,
//                            aboutPc), kept for the dashboard.
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
//   POST /give/devices       the devices set up, after a sign-in, with a pass for removing them;
//   POST /give/remove        {pass, by, id}: that device removed, within CHALLENGE_SECONDS of the
//                            sign-in. Not the last that signs in here.
// The owner's dashboard (2.6; the owner, 2026-11-27: "Make an admin panel dashboard where i can see
// keys which are currently in use and also including the system the key is in use and names of
// the person who owns that key"), signed in with the same passkeys:
//   GET  /dashboard          the page: nothing in it until a sign-in.
//   POST /dashboard/data     every key, with its PC and its buyer's name, email and phone (from
//                            Razorpay now, never kept), after a sign-in; or again with a pass.
//   POST /dashboard/cancel, /dashboard/restore, /dashboard/free, /dashboard/label
//                            {pass, by, at, lic, label?}: a key turned off or on again, taken off
//                            its PC, or named. Each answers a new pass; a pass is good for
//                            CHALLENGE_SECONDS, and none past DASH_SECONDS after the sign-in.
//   POST /dashboard/give     {label?}: a new key, after a sign-in, as /give/key, given to `label`.
// Clokrix itself (2.5.2, downloads.ts), from the R2 bucket the Release workflow fills:
//   GET  /download, /download/<tag>/<file>, /updates/latest.json

import { capture, createOrder, getPayment, listOrders, listPayments, note, paidFor, type Payment, type Razorpay } from './razorpay';
import { checkoutPage, errorPage, keyPage, pageHeaders, paidPage } from './page';
import { dashboardPage } from './dashboard';
import { ALGS, authData, bytes, clientData, freshChallenge, newChallenge, ours, publicKey, signed, vouched } from './webauthn';
import { isSitePage, sitePage, type Seller } from './site';
import { download, isDownload } from './downloads';
import { LEDGER_NAME, type KeyRow, type Ledger, type Passkey } from './ledger';
import {
  aboutPc,
  b64url,
  chosenCurrency,
  cleanName,
  currencyFor,
  DASH_SECONDS,
  deviceName,
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

/** A buyer as Razorpay has them: what they typed into its window, and the name at checkout. */
interface Buyer {
  name: string | null;
  email: string | null;
  phone: string | null;
}

/** A key as the dashboard shows it: its PC by a short code of its hash, never the hash itself. */
function shown(k: KeyRow, buyer: Buyer | null) {
  const source = k.payment.startsWith('given:') ? 'given' : k.test ? 'test' : 'bought';
  return {
    lic: k.lic,
    key: k.key,
    source,
    payment: source === 'given' ? null : k.payment,
    order: k.order,
    issued: k.issued,
    pc: k.pc ? k.pc.slice(0, 8).toUpperCase() : null,
    activated: k.activated,
    moved: k.moved,
    checked: k.checked,
    about: k.about,
    revoked: k.revoked,
    revokedBy: k.revokedBy,
    amount: k.amount,
    currency: k.currency,
    label: k.label,
    buyer,
  };
}

/** Every key for the dashboard, each bought one with its buyer, read from Razorpay's lists now
 * (`razorpay`: 'ok', 'off' without Razorpay's keys, or 'failed' when it could not be read: the
 * keys come anyway). A price not kept yet is kept from the list. */
async function dashboard(ledger: DurableObjectStub<Ledger>, r: Razorpay | null) {
  const rows = await ledger.all();
  const sold = rows.filter((k) => !k.payment.startsWith('given:'));
  const buyers = new Map<string, Buyer>();
  let state: 'ok' | 'off' | 'failed' = r ? 'ok' : 'off';
  if (r && sold.length > 0) {
    try {
      // From two days before the first sale: its order and payment came a little before its key.
      const from = Math.min(...sold.map((k) => k.issued)) - 2 * 86_400;
      const [pays, ords] = await Promise.all([listPayments(r, from), listOrders(r, from)]);
      const named = new Map(ords.map((o) => [o.id, note(o.notes, 'name')]));
      for (const p of pays) {
        if (typeof p?.id !== 'string') continue;
        const str = (v: unknown) => (typeof v === 'string' && v ? v : null);
        buyers.set(p.id, {
          name: cleanName(note(p.notes, 'name') ?? (p.order_id ? named.get(p.order_id) : null)),
          email: str(p.email),
          phone: str(p.contact),
        });
        const k = sold.find((x) => x.payment === p.id);
        if (k && k.amount === null && typeof p.amount === 'number' && typeof p.currency === 'string') {
          await ledger.setPrice(k.lic, p.amount, p.currency);
          k.amount = p.amount;
          k.currency = p.currency;
        }
      }
    } catch {
      state = 'failed';
    }
  }
  return { keys: rows.map((k) => shown(k, buyers.get(k.payment) ?? null)), razorpay: state };
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
        if (paidFor(payment, payment.order_id)) await ledger.issue(payment.order_id, payment.id, !live(env), payment.amount, payment.currency);
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
    const isPage = path === '/buy' || path === '/give' || path === '/dashboard' || isSitePage(path);
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
      // The buyer's name (2.6), for the owner's dashboard: kept by Razorpay with the order.
      const name = cleanName(b?.name);
      if (!name) return json(400, { error: 'name' });
      let order: string;
      try {
        order = await createOrder(r, `clokrix-${n.slice(0, 20)}`, claim, currency(req, b?.cur), name);
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
      const l = await ledger.issue(orderId, paid.id, !live(env), paid.amount, paid.currency);
      if (!l) return html(400, n, errorPage(at, 'Payment not confirmed', sorry));
      return html(200, n, paidPage({ ...at, key: l.key, fromApp: await ledger.fromApp(orderId) }));
    }

    if (path === '/give' || path.startsWith('/give/') || path === '/dashboard' || path.startsWith('/dashboard/')) {
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
      if (path === '/dashboard') {
        if (req.method !== 'GET' && req.method !== 'HEAD') return json(405, { error: 'not-allowed' });
        if (!open) return html(503, n, errorPage(at, 'Not open', 'Nothing is here.'));
        const ready = (await ledger.passkeys(setup)).some((p) => p.rp === rpId);
        return html(200, n, dashboardPage({ ...at, ready }));
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
        const name = deviceName(req.headers.get('User-Agent'), b.attachment, b.transports);
        return { id, rp: rpId, spki: b64url(spki), alg, count: a.count, setup, name };
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
        // A sign-in from another device in the browser's window names no device.
        await ledger.signedIn(pk.id, a.count, deviceName(req.headers.get('User-Agent'), b.attachment));
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

      // Removing a device (the owner, 2026-11-27: "build the remove device option"), one that is
      // lost or in the wrong hands, without starting again under a new passphrase. The devices
      // come after a sign-in, with a pass for removing them: a challenge of its own kind, named
      // for the passkey that signed in, good while that passkey is set up, for CHALLENGE_SECONDS.
      const listed = async () =>
        (await ledger.devices(setup)).map((d) => ({ id: d.id, name: d.name, created: d.created, used: d.used, here: d.rp === rpId }));
      if (path === '/give/devices') {
        const pk = await signedIn();
        if (!pk) return json(403, { error: 'passkey' });
        return json(200, { pass: await newChallenge(set, `remove:${pk.id}`, now), by: pk.id, devices: await listed() });
      }

      if (path === '/give/remove') {
        const by = typeof b.by === 'string' && b.by.length <= 1364 ? b.by : null;
        if (!by || typeof b.pass !== 'string' || !(await freshChallenge(set, `remove:${by}`, b.pass, now))) return json(403, { error: 'pass' });
        if (typeof b.id !== 'string') return json(400, { error: 'body' });
        const r = await ledger.removePasskey(setup, rpId, by, b.id);
        if (r === 'signed-out') return json(403, { error: 'pass' });
        if (r === 'last') return json(409, { error: 'last' });
        return json(200, { ok: true, devices: await listed() });
      }

      // The dashboard (2.6). A sign-in gives a pass, named for the passkey that signed in and the
      // time it did: good for CHALLENGE_SECONDS, while that passkey is set up, and never past
      // DASH_SECONDS after the sign-in. Each request with one answers the next.
      const passFor = async (by: string, signedAt: number) => ({ pass: await newChallenge(set, `dash:${by}:${signedAt}`, now), by, at: signedAt });
      const passed = async (): Promise<{ by: string; at: number } | null> => {
        const by = typeof b.by === 'string' && b.by.length <= 1364 ? b.by : null;
        const signedAt = typeof b.at === 'number' && Number.isInteger(b.at) ? b.at : null;
        if (!by || signedAt === null || typeof b.pass !== 'string') return null;
        if (signedAt > now || now - signedAt > DASH_SECONDS) return null;
        if (!passkeys.some((p) => p.id === by)) return null;
        return (await freshChallenge(set, `dash:${by}:${signedAt}`, b.pass, now)) ? { by, at: signedAt } : null;
      };

      if (path === '/dashboard/data') {
        let who = typeof b.pass === 'string' ? await passed() : null;
        if (!who && typeof b.pass !== 'string') {
          const pk = await signedIn();
          if (pk) who = { by: pk.id, at: now };
        }
        if (!who) return json(403, { error: typeof b.pass === 'string' ? 'pass' : 'passkey' });
        const { keys, razorpay: rz } = await dashboard(ledger, razorpay(env));
        return json(200, { ...(await passFor(who.by, who.at)), keys, razorpay: rz, live: live(env) });
      }

      if (path === '/dashboard/give') {
        const pk = await signedIn();
        if (!pk) return json(403, { error: 'passkey' });
        const label = cleanName(b.label);
        const l = await ledger.give(label);
        const row = (await ledger.all()).find((x) => x.lic === l.lic);
        return json(200, { ...(await passFor(pk.id, now)), key: l.key, row: row ? shown(row, null) : null });
      }

      const action = /^\/dashboard\/(cancel|restore|free|label)$/.exec(path)?.[1];
      if (action) {
        const who = await passed();
        if (!who) return json(403, { error: 'pass' });
        const lic = typeof b.lic === 'string' && /^[0-9a-f]{16}$/.test(b.lic) ? b.lic : null;
        if (!lic) return json(400, { error: 'body' });
        const done =
          action === 'cancel'
            ? await ledger.cancel(lic)
            : action === 'restore'
              ? await ledger.restore(lic)
              : action === 'free'
                ? await ledger.free(lic)
                : await ledger.setLabel(lic, cleanName(b.label));
        const row = (await ledger.all()).find((x) => x.lic === lic);
        if (!row) return json(404, { error: 'no-key' });
        return json(done ? 200 : 409, { ...(await passFor(who.by, who.at)), ...(done ? {} : { error: 'state' }), row: shown(row, null) });
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
      const about = aboutPc(b?.about);
      if (path === '/check') return json(200, await ledger.check(key, pc, live(env), about));
      if (!env.LICENCE_SIGNING_KEY) return json(503, { error: 'not-open' });
      const a = await ledger.activate(key, pc, b?.move === true, live(env), about);
      if (!a.ok) return json(a.status, { error: a.error, ...(a.until ? { until: a.until } : {}), ...(a.by ? { by: a.by } : {}) });
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

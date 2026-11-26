// The checkout's pages (2.5): the one that takes the payment, through Razorpay's own checkout, and
// the one that shows the key once paid; and, since 2.5's website (site.ts), the pages around them.
// Plain HTML in the system font, light or dark as the buyer's system is; the only script from
// elsewhere is Razorpay's.

import { PRICES, REFUND_DAYS, shownPrice, type Currency } from './rules';

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}

const STYLE = `
  :root { color-scheme: light dark; --bg: #f6f7fb; --card: rgb(255 255 255 / 72%); --text: #141821; --muted: #5b6475; --line: #d9dde6;
    --accent: #2563eb; --on-accent: #fff; --wash-1: #d4e4fa; --wash-2: #e8def8; --wash-3: #d6f0e0; --rim: rgb(255 255 255 / 95%); --edge: rgb(23 32 43 / 11%); }
  @media (prefers-color-scheme: dark) { :root { --bg: #0b0f16; --card: rgb(18 24 34 / 70%); --text: #e8ecf3; --muted: #9aa4b5; --line: #263042;
    --accent: #5aa2ff; --on-accent: #0b1220; --wash-1: #173470; --wash-2: #461a6c; --wash-3: #0b3f50; --rim: rgb(255 255 255 / 20%); --edge: rgb(255 255 255 / 9%); } }
  * { box-sizing: border-box; }
  /* Clokrix's glass (2.5): a pane over a colour wash, lit along its top edge. */
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; align-content: center; gap: 18px; padding: 16px; color: var(--text);
    background: radial-gradient(40rem 30rem at 10% 0%, var(--wash-1), transparent 65%), radial-gradient(36rem 28rem at 95% 20%, var(--wash-2), transparent 62%),
      radial-gradient(44rem 30rem at 50% 100%, var(--wash-3), transparent 65%), var(--bg);
    font: 16px/1.5 'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, sans-serif; }
  main { width: min(30rem, 100%); padding: 28px; border: 1px solid var(--edge); border-radius: 20px; background: var(--card); background-clip: padding-box;
    -webkit-backdrop-filter: blur(24px) saturate(170%); backdrop-filter: blur(24px) saturate(170%);
    box-shadow: inset 0 1px 0 var(--rim), 0 24px 56px -28px rgb(0 0 0 / 45%); }
  main.wide { width: min(46rem, 100%); }
  h1 { margin: 0 0 4px; font-size: 1.4rem; }
  h2 { margin: 22px 0 6px; font-size: 1.05rem; }
  .price { margin: 0 0 16px; font-size: 2rem; font-weight: 700; }
  ul { margin: 0 0 20px; padding-left: 1.2rem; color: var(--muted); }
  li { margin-bottom: 4px; }
  p { margin: 0 0 12px; }
  a { color: var(--accent); }
  a:focus-visible { outline: 3px solid var(--text); outline-offset: 2px; border-radius: 4px; }
  .muted { color: var(--muted); font-size: 0.88rem; }
  button { width: 100%; padding: 12px 16px; border: 0; border-radius: 10px; background: var(--accent); color: var(--on-accent); font: inherit; font-weight: 600; cursor: pointer; }
  button:focus-visible { outline: 3px solid var(--text); outline-offset: 2px; }
  .actions { display: flex; flex-wrap: wrap; gap: 10px; margin: 16px 0 8px; }
  .button { display: inline-block; padding: 11px 18px; border: 1px solid var(--accent); border-radius: 10px; background: var(--accent); color: var(--on-accent);
    font-weight: 600; text-decoration: none; }
  .button.quiet { border-color: var(--line); background: transparent; color: var(--text); }
  .key { display: block; margin: 8px 0 12px; padding: 12px; border: 1px dashed var(--line); border-radius: 10px;
    font: 600 1.05rem/1.4 ui-monospace, 'Cascadia Mono', Consolas, monospace; text-align: center; user-select: all; overflow-wrap: anywhere; }
  .after { margin-top: 12px; }
  .err { color: #d14343; }
  label { display: block; margin: 0 0 6px; font-weight: 600; }
  input { width: 100%; margin: 0 0 14px; padding: 11px 12px; border: 1px solid var(--line); border-radius: 10px; background: transparent;
    color: var(--text); font: inherit; }
  input:focus-visible { outline: 3px solid var(--accent); outline-offset: 1px; }
  address { font-style: normal; white-space: pre-line; }
  footer { width: min(46rem, 100%); text-align: center; color: var(--muted); font-size: 0.85rem; }
  footer nav { display: flex; flex-wrap: wrap; justify-content: center; gap: 4px 16px; margin-bottom: 4px; }
  footer a { color: inherit; }
`;

/** Each page's footer: the seller, and the way to every page Razorpay asks a website for. */
function footer(seller: string): string {
  const links = [
    ['/', 'Clokrix'],
    ['/buy', 'Buy Pro'],
    ['/terms', 'Terms'],
    ['/privacy', 'Privacy'],
    ['/refunds', 'Refunds'],
    ['/delivery', 'Delivery'],
    ['/contact', 'Contact'],
  ];
  return `<footer>
<nav aria-label="Clokrix and its policies">${links.map(([href, text]) => `<a href="${href}">${text}</a>`).join('')}</nav>
<p>© ${new Date().getUTCFullYear()} ${esc(seller)}</p>
</footer>`;
}

export interface Page {
  title: string;
  body: string;
  nonce: string;
  /** Who sells Clokrix Pro, as the footer names them. */
  seller: string;
  /** A page to read (the website's), not the narrow card of the checkout. */
  wide?: boolean;
  /** For search engines: the website's pages, not the checkout's own. */
  index?: boolean;
}

export function page(p: Page): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
${p.index ? '' : '<meta name="robots" content="noindex">\n'}<title>${esc(p.title)}</title>
<style nonce="${p.nonce}">${STYLE}</style>
</head>
<body>
<main${p.wide ? ' class="wide"' : ''}>
${p.body}
</main>
${footer(p.seller)}
</body>
</html>`;
}

/** Headers for a page: nothing cached, Razorpay's checkout and nothing else from elsewhere. */
export function pageHeaders(nonce: string): Record<string, string> {
  return {
    'Content-Type': 'text/html; charset=utf-8',
    'Cache-Control': 'no-store',
    'X-Content-Type-Options': 'nosniff',
    'Referrer-Policy': 'no-referrer',
    'Content-Security-Policy': [
      "default-src 'none'",
      `script-src 'nonce-${nonce}' https://checkout.razorpay.com`,
      // Razorpay's checkout adds its own styles to the page.
      "style-src 'self' 'unsafe-inline'",
      'frame-src https://api.razorpay.com https://checkout.razorpay.com',
      "connect-src 'self' https://*.razorpay.com",
      'img-src https: data:',
      "form-action 'self'",
      "base-uri 'none'",
      "frame-ancestors 'none'",
    ].join('; '),
  };
}

/** `/buy` in `currency`, the app's code kept. */
function buyIn(currency: Currency, claim: string | null): string {
  const q = new URLSearchParams(claim ? { c: claim } : {});
  q.set('cur', currency.toLowerCase());
  return `/buy?${q}`;
}

/** The page that takes the payment, at the price in `currency` (₹ for a buyer in India, $ for the
 * rest; each can pay the other instead). Pay makes the order (`/order`) and opens Razorpay's
 * checkout; once paid, the checkout's answer comes here (`/paid`) as a form, and the key shows. */
export function checkoutPage(o: { nonce: string; seller: string; keyId: string; claim: string | null; currency: Currency }): string {
  const p = PRICES[o.currency];
  const price = shownPrice(o.currency);
  const options = { key: o.keyId, amount: p.amount, currency: p.currency, name: o.seller, description: 'Clokrix Pro: lifetime, one PC' };
  const other =
    o.currency === 'INR'
      ? `<a href="${esc(buyIn('USD', o.claim))}">Not in India? Pay ${shownPrice('USD')} instead.</a>`
      : `<a href="${esc(buyIn('INR', o.claim))}">In India? Pay ${shownPrice('INR')} instead.</a>`;
  return page({
    title: 'Clokrix Pro',
    seller: o.seller,
    nonce: o.nonce,
    body: `<h1>Clokrix Pro</h1>
<p class="price">${price} <span class="muted">once, for life${o.currency === 'INR' ? ', including all taxes' : ''}</span></p>
<ul>
  <li>Every part of Clokrix, not only the Dashboard</li>
  <li>On one PC; you can move it to another</li>
  <li>Every update, for good</li>
</ul>
<form id="paid" method="post" action="/paid">
  <input type="hidden" name="razorpay_payment_id">
  <input type="hidden" name="razorpay_order_id">
  <input type="hidden" name="razorpay_signature">
</form>
<button id="pay" type="button">Pay ${price}</button>
<p class="muted after">${o.claim ? 'Clokrix turns Pro on by itself once you have paid. ' : ''}Sold by ${esc(o.seller)}; Razorpay
takes the payment. <a href="/refunds">Refundable within ${REFUND_DAYS} days.</a></p>
<p class="muted">${other}</p>
<p id="err" class="err" role="alert"></p>
<script nonce="${o.nonce}" src="https://checkout.razorpay.com/v1/checkout.js"></script>
<script nonce="${o.nonce}">
  const options = ${JSON.stringify(options).replace(/</g, '\\u003c')};
  const claim = ${JSON.stringify(o.claim)};
  const cur = ${JSON.stringify(o.currency)};
  const pay = document.getElementById('pay');
  const err = document.getElementById('err');
  options.handler = (r) => {
    const f = document.getElementById('paid');
    for (const k of ['razorpay_payment_id', 'razorpay_order_id', 'razorpay_signature']) f.elements[k].value = r[k];
    f.submit();
  };
  options.modal = { confirm_close: true, ondismiss: () => (pay.disabled = false) };
  pay.addEventListener('click', async () => {
    err.textContent = '';
    if (typeof Razorpay === 'undefined') {
      err.textContent = 'The payment window could not load. Check your connection and try again.';
      return;
    }
    pay.disabled = true;
    try {
      const res = await fetch('/order', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ c: claim, cur }) });
      if (!res.ok) throw new Error(String(res.status));
      options.order_id = (await res.json()).order_id;
    } catch {
      err.textContent = 'The payment could not start. Try again in a moment.';
      pay.disabled = false;
      return;
    }
    const rzp = new Razorpay(options);
    rzp.on('payment.failed', (e) => {
      err.textContent = 'The payment did not go through: ' + ((e.error && e.error.description) || 'try again.');
    });
    rzp.open();
  });
</script>`,
  });
}

/** The key, once paid. */
export function paidPage(o: { nonce: string; seller: string; key: string; fromApp: boolean }): string {
  return page({
    title: 'Clokrix Pro: your key',
    seller: o.seller,
    nonce: o.nonce,
    body: `<h1>Thank you</h1>
<p>${o.fromApp ? 'Clokrix turns Pro on by itself in a moment. ' : ''}Your key:</p>
<code class="key">${esc(o.key)}</code>
<p class="muted">Keep it: you need it to set Pro up again after reinstalling Windows, or to move Pro to
another PC. In Clokrix: Settings → Clokrix Pro → Enter key. Razorpay emails you a receipt.</p>`,
  });
}

/** The key page (2.5.5): `setup` to set this device's passkey up with the setup passphrase, then
 * `sign-in`, where the passkey (fingerprint, face or PIN) gets a key. Worded so a visitor learns
 * nothing from it: it names no keys until one is given. */
export function keyPage(o: { nonce: string; seller: string; mode: 'setup' | 'sign-in' }): string {
  const setup = o.mode === 'setup';
  const body = setup
    ? `<h1>Set up</h1>
<p>Type the setup passphrase, then set up this device's passkey. From then on, only this device,
once it has checked you (fingerprint, face or PIN), signs in here.</p>
<form id="go">
  <label for="passphrase">Setup passphrase</label>
  <input id="passphrase" name="passphrase" type="password" autocomplete="current-password" required>
  <button type="submit">Set up this device</button>
</form>`
    : `<h1>Sign in</h1>
<p>With the passkey set up on this device.</p>
<form id="go"><button type="submit">Sign in</button></form>`;
  return page({
    title: 'Clokrix',
    seller: o.seller,
    nonce: o.nonce,
    body: `<div id="ask">${body}</div>
<div id="got" hidden>
  <h1>A Clokrix Pro key</h1>
  <code id="key" class="key"></code>
  <p class="muted">Given, not sold: no payment or receipt goes with it. Like a bought key, it is Pro
  on one PC at a time and can move to another. In Clokrix: Settings → Clokrix Pro → Enter key. Keep
  it: it is not shown again, and each sign-in makes a new one.</p>
  <p><a href="/give">Sign in again</a></p>
</div>
<p id="err" class="err after" role="alert"></p>
<script nonce="${o.nonce}">
  const setup = ${JSON.stringify(setup)};
  const err = document.getElementById('err');
  const enc = (b) => btoa(String.fromCharCode(...new Uint8Array(b))).replace(/\\+/g, '-').replace(/\\//g, '_').replace(/=+$/, '');
  const dec = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));
  const said = {
    passphrase: 'That is not the setup passphrase.',
    wait: 'Too many wrong passphrases: try again in an hour.',
    'set-up': 'A device is already set up here.',
    'not-set-up': 'No device is set up here yet.',
  };
  async function api(path, body) {
    const r = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    const j = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(said[j.error] || 'That did not work. Try again.');
    return j;
  }
  async function setUp(passphrase) {
    const o = await api('/give/setup/start', { passphrase });
    const c = await navigator.credentials.create({
      publicKey: {
        challenge: dec(o.challenge),
        rp: o.rp,
        user: { id: dec(o.user.id), name: o.user.name, displayName: o.user.name },
        pubKeyCredParams: o.algs.map((alg) => ({ type: 'public-key', alg })),
        authenticatorSelection: { userVerification: 'required', residentKey: 'preferred' },
        attestation: 'none',
        timeout: 120000,
      },
    });
    const r = c.response;
    const spki = r.getPublicKey && r.getPublicKey();
    if (!spki) throw new Error('This browser cannot set a passkey up here: try Edge or Chrome.');
    await api('/give/setup/finish', {
      passphrase,
      id: enc(c.rawId),
      spki: enc(spki),
      alg: r.getPublicKeyAlgorithm(),
      authenticatorData: enc(r.getAuthenticatorData()),
      clientDataJSON: enc(r.clientDataJSON),
    });
    location.reload();
  }
  async function signIn() {
    const o = await api('/give/start', {});
    const c = await navigator.credentials.get({
      publicKey: {
        challenge: dec(o.challenge),
        rpId: o.rpId,
        allowCredentials: o.ids.map((id) => ({ type: 'public-key', id: dec(id) })),
        userVerification: 'required',
        timeout: 120000,
      },
    });
    const r = c.response;
    const got = await api('/give/key', {
      id: enc(c.rawId),
      authenticatorData: enc(r.authenticatorData),
      clientDataJSON: enc(r.clientDataJSON),
      signature: enc(r.signature),
    });
    document.getElementById('key').textContent = got.key;
    document.getElementById('ask').hidden = true;
    document.getElementById('got').hidden = false;
  }
  document.getElementById('go').addEventListener('submit', async (e) => {
    e.preventDefault();
    err.textContent = '';
    if (!window.PublicKeyCredential) {
      err.textContent = 'This browser cannot use passkeys.';
      return;
    }
    const button = e.target.querySelector('button');
    button.disabled = true;
    try {
      await (setup ? setUp(document.getElementById('passphrase').value) : signIn());
    } catch (x) {
      err.textContent = x && x.name === 'NotAllowedError' ? 'Cancelled, or this device has no passkey for this page.' : x.message;
    } finally {
      button.disabled = false;
    }
  });
</script>`,
  });
}

/** Something went wrong, said plainly. */
export function errorPage(o: { nonce: string; seller: string }, title: string, text: string): string {
  return page({ title, seller: o.seller, nonce: o.nonce, body: `<h1>${esc(title)}</h1><p>${esc(text)}</p>` });
}

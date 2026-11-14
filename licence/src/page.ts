// The checkout's pages (2.5): the one that takes the payment, through Razorpay's own checkout, and
// the one that shows the key once paid. Plain HTML in the system font, light or dark as the buyer's
// system is; the only script from elsewhere is Razorpay's.

import { PRICE } from './rules';

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}

const STYLE = `
  :root { color-scheme: light dark; --bg: #f6f7fb; --card: #fff; --text: #141821; --muted: #5b6475; --line: #d9dde6; --accent: #2563eb; }
  @media (prefers-color-scheme: dark) { :root { --bg: #0b0f16; --card: #121822; --text: #e8ecf3; --muted: #9aa4b5; --line: #263042; --accent: #5aa2ff; } }
  * { box-sizing: border-box; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 16px; background: var(--bg); color: var(--text);
    font: 16px/1.5 'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, sans-serif; }
  main { width: min(30rem, 100%); padding: 28px; border: 1px solid var(--line); border-radius: 16px; background: var(--card); }
  h1 { margin: 0 0 4px; font-size: 1.4rem; }
  .price { margin: 0 0 16px; font-size: 2rem; font-weight: 700; }
  ul { margin: 0 0 20px; padding-left: 1.2rem; color: var(--muted); }
  p { margin: 0 0 12px; }
  .muted { color: var(--muted); font-size: 0.88rem; }
  button { width: 100%; padding: 12px 16px; border: 0; border-radius: 10px; background: var(--accent); color: #fff; font: inherit; font-weight: 600; cursor: pointer; }
  button:focus-visible { outline: 3px solid var(--text); outline-offset: 2px; }
  .key { display: block; margin: 8px 0 12px; padding: 12px; border: 1px dashed var(--line); border-radius: 10px;
    font: 600 1.05rem/1.4 ui-monospace, 'Cascadia Mono', Consolas, monospace; text-align: center; user-select: all; overflow-wrap: anywhere; }
  .after { margin-top: 12px; }
  .err { color: #d14343; }
`;

function page(title: string, body: string, nonce: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>${esc(title)}</title>
<style nonce="${nonce}">${STYLE}</style>
</head>
<body>
<main>
${body}
</main>
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

const price = `$${PRICE.amount / 100}`;

/** The page that takes the payment. Pay makes the order (`/order`) and opens Razorpay's checkout;
 * once paid, the checkout's answer comes here (`/paid`) as a form, and the key shows. */
export function checkoutPage(o: { nonce: string; seller: string; keyId: string; claim: string | null }): string {
  const options = { key: o.keyId, amount: PRICE.amount, currency: PRICE.currency, name: o.seller, description: 'Clokrix Pro: lifetime, one PC' };
  return page(
    'Clokrix Pro',
    `<h1>Clokrix Pro</h1>
<p class="price">${price} <span class="muted">once, for life</span></p>
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
takes the payment.</p>
<p id="err" class="err" role="alert"></p>
<script nonce="${o.nonce}" src="https://checkout.razorpay.com/v1/checkout.js"></script>
<script nonce="${o.nonce}">
  const options = ${JSON.stringify(options).replace(/</g, '\\u003c')};
  const claim = ${JSON.stringify(o.claim)};
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
      const res = await fetch('/order', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ c: claim }) });
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
    o.nonce,
  );
}

/** The key, once paid. */
export function paidPage(o: { nonce: string; key: string; fromApp: boolean }): string {
  return page(
    'Clokrix Pro: your key',
    `<h1>Thank you</h1>
<p>${o.fromApp ? 'Clokrix turns Pro on by itself in a moment. ' : ''}Your key:</p>
<code class="key">${esc(o.key)}</code>
<p class="muted">Keep it: you need it to set Pro up again after reinstalling Windows, or to move Pro to
another PC. In Clokrix: Settings → Clokrix Pro → Enter key. Razorpay emails you a receipt.</p>`,
    o.nonce,
  );
}

/** Something went wrong, said plainly. */
export function errorPage(nonce: string, title: string, text: string): string {
  return page(title, `<h1>${esc(title)}</h1><p>${esc(text)}</p>`, nonce);
}

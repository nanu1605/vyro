// Clokrix's website (2.5): what Razorpay checks before it gives Live keys. A home page with
// Clokrix, Pro and its price, and the terms, privacy, refund, delivery and contact pages, all
// naming the seller. They are served by the checkout itself, so the website and the payment page
// share one address (the owner's, 2026-11-16: "Company domain on Cloudflare"). The contact details
// are the Worker's vars, set at deploy from the repository's variables, never written into the
// code.

import { esc, page } from './page';
import { MOVE_DAYS, REFUND_DAYS, shownPrice, type Currency } from './rules';

/** Who sells Clokrix Pro and how to reach them. Any detail not set is left out. */
export interface Seller {
  name: string;
  email?: string;
  phone?: string;
  address?: string;
}

const DOWNLOAD = 'https://github.com/nanu1605/vyro-releases/releases/latest';
/** When these pages last changed in what they promise. */
const UPDATED = '16 November 2026';

/** The website's paths, each with its page. */
export const SITE_PAGES = ['/', '/terms', '/privacy', '/refunds', '/delivery', '/contact'] as const;
export type SitePath = (typeof SITE_PAGES)[number];

export function isSitePage(path: string): path is SitePath {
  return (SITE_PAGES as readonly string[]).includes(path);
}

/** "email us at …", or "contact us" when no email is set. */
function writeTo(s: Seller): string {
  return s.email ? `email us at <a href="mailto:${esc(s.email)}">${esc(s.email)}</a>` : 'write to us (see <a href="/contact">Contact</a>)';
}

function policy(title: string, body: string): string {
  return `<h1>${esc(title)}</h1>
<p class="muted">Last updated ${UPDATED}</p>
${body}`;
}

/** Both prices, said once: ₹10,000 in India, including all taxes, and $100 elsewhere. */
const BOTH = `${shownPrice('INR')} in India, including all taxes, and ${shownPrice('USD')} elsewhere`;

function home(s: Seller, c: Currency): string {
  const price = shownPrice(c);
  return `<h1>Clokrix</h1>
<p>A light hardware monitor for Windows: CPU and GPU speedometers, every sensor, history, health and alerts,
fan control, GPU tuning, desktop widgets, a game overlay and a phone app. It reads the hardware directly,
with no other monitoring app.</p>
<div class="actions">
  <a class="button" href="${DOWNLOAD}">Download for Windows</a>
  <a class="button quiet" href="/buy">Buy Pro, ${price}</a>
</div>
<p class="muted">For Windows 10 and 11 (64-bit). Every PC gets Pro free for 7 days from Clokrix's first start; no card
is needed.</p>
<h2>Free</h2>
<p>The Dashboard, with its layouts and editing, and Settings.</p>
<h2>Pro: ${price}, once, for life</h2>
<p class="muted">${BOTH}.</p>
<ul>
  <li>Everything: Health and alerts, Insights, Gaming, the Benchmark, Cooling (fan control), GPU tuning, History,
  My PC, every part of this PC, desktop widgets, the game overlay, profiles and the phone app.</li>
  <li>On one PC at a time. You can move it to another PC once every ${MOVE_DAYS} days.</li>
  <li>Every update, for good. It is one payment, not a subscription.</li>
  <li>Buy it in Clokrix (Settings → Clokrix Pro → Buy Pro) or <a href="/buy">here</a>. Razorpay takes the payment.</li>
</ul>
<p class="muted">Sold by ${esc(s.name)}. A full refund if you ask within ${REFUND_DAYS} days: see <a href="/refunds">Refunds</a>.</p>`;
}

function terms(s: Seller): string {
  return policy(
    'Terms of sale',
    `<p>These terms cover buying Clokrix Pro from ${esc(s.name)} ("we", "us"). By paying, you agree to them.</p>
<h2>What you buy</h2>
<p>A Clokrix Pro key. It turns on every part of Clokrix on one Windows PC at a time, for life, with every update.
You can move it to another PC once every ${MOVE_DAYS} days, in Clokrix (Settings → Clokrix Pro).</p>
<h2>Price and payment</h2>
<p>Clokrix Pro costs ${BOTH} (in US dollars), paid once. There is no subscription and nothing
renews. The payment page shows the price for where you are, and lets you pay the other instead.
Razorpay takes the payment and emails you a receipt. The price is shown before you pay.</p>
<h2>Trying it first</h2>
<p>Every PC gets Pro free for 7 days from Clokrix's first start, with no card needed, so you can check that Clokrix
works on your PC before you buy.</p>
<h2>Using your key</h2>
<p>The key is for you, on one PC at a time. Please do not share, publish or resell it. A key that is refunded,
charged back, or shared is turned off, and the PC it is on goes back to Clokrix Free.</p>
<h2>The software</h2>
<p>Clokrix reads your PC's sensors and, if you ask it to, changes fan speeds and GPU settings. It is provided as it
is. Fan and GPU changes are made at your own risk; Clokrix can return both to their defaults at any time. As far
as the law allows, our liability is limited to what you paid for Clokrix Pro.</p>
<h2>Refunds</h2>
<p>See <a href="/refunds">Refunds</a>: a full refund if you ask within ${REFUND_DAYS} days of buying.</p>
<h2>Changes</h2>
<p>We may update these terms. The terms on the day you bought apply to that purchase.</p>
<h2>Law</h2>
<p>These terms are governed by the laws of India.</p>
<h2>Questions</h2>
<p>For any question about these terms, ${writeTo(s)}.</p>`,
  );
}

function privacy(s: Seller): string {
  return policy(
    'Privacy',
    `<p>${esc(s.name)} sells Clokrix Pro. This page says what we keep, and what others keep, when you buy and use it.</p>
<h2>What we keep</h2>
<ul>
  <li>For each purchase: Razorpay's order and payment ids, the key, and when it was paid.</li>
  <li>For each key: the PC it is on, as a one-way hash of Windows' installation id (never the id itself), and when it
  was turned on or moved.</li>
</ul>
<p>We keep nothing else about you here: no name, email, address or card details.</p>
<h2>What Razorpay keeps</h2>
<p>What you type into Razorpay's payment window (such as your name, email, phone and payment details) goes to
Razorpay, which takes the payment under its own privacy policy. We see your payment in our Razorpay account, to
send receipts, answer questions and make refunds.</p>
<h2>The app</h2>
<p>Clokrix's readings stay on your PC. For Pro, the app sends this website only the key and the PC's hash: to turn
Pro on, to move it, and at most once a week to check the key is still good.</p>
<h2>This website</h2>
<p>No cookies, no analytics and no advertising. To stop abuse, each address's requests are counted for a minute,
then forgotten.</p>
<h2>Questions</h2>
<p>To see or delete what we keep about your key, ${writeTo(s)}.</p>`,
  );
}

function refunds(s: Seller): string {
  return policy(
    'Refunds and cancellation',
    `<h2>${REFUND_DAYS} days, no questions</h2>
<p>If you ask within ${REFUND_DAYS} days of buying Clokrix Pro, we refund the full price you paid. You do not need to
give a reason. To ask, ${writeTo(s)} with the payment id from Razorpay's receipt (it starts with <code>pay_</code>).</p>
<p>The refund goes back to how you paid, through Razorpay. Banks usually show it within 5 to 7 working days.</p>
<p>Once refunded, the key stops turning Pro on, and the PC goes back to Clokrix Free at its next check.</p>
<h2>After ${REFUND_DAYS} days</h2>
<p>Purchases are final, except a payment taken twice or a payment that never gave you a key: those we refund
whenever you tell us.</p>
<h2>Cancellation</h2>
<p>Clokrix Pro is one payment, not a subscription, so there is nothing to cancel and nothing renews.</p>`,
  );
}

function delivery(s: Seller): string {
  return policy(
    'Delivery',
    `<p>Clokrix Pro is digital: nothing is shipped.</p>
<ul>
  <li>Right after you pay, the page shows your key.</li>
  <li>If you opened Buy Pro from Clokrix, the app turns Pro on by itself within a minute.</li>
  <li>Razorpay emails you a receipt.</li>
</ul>
<p>Clokrix itself is a free download, here: <a href="${DOWNLOAD}">Download for Windows</a>.</p>
<h2>If the key did not come</h2>
<p>Open Buy Pro in Clokrix again: the app collects the key of a purchase it started. Otherwise, ${writeTo(s)} with the
payment id from Razorpay's receipt, and we send the key within 2 working days.</p>`,
  );
}

function contact(s: Seller): string {
  const lines = [
    `<p><strong>${esc(s.name)}</strong></p>`,
    s.address ? `<h2>Address</h2>\n<address>${esc(s.address)}</address>` : '',
    s.email ? `<h2>Email</h2>\n<p><a href="mailto:${esc(s.email)}">${esc(s.email)}</a></p>` : '',
    s.phone ? `<h2>Phone</h2>\n<p><a href="tel:${esc(s.phone.replace(/[^+0-9]/g, ''))}">${esc(s.phone)}</a></p>` : '',
    '<p class="muted">We answer within 2 working days. For a refund or a missing key, include the payment id from Razorpay\'s receipt.</p>',
  ];
  return `<h1>Contact</h1>\n${lines.filter(Boolean).join('\n')}`;
}

const PAGES: Record<SitePath, { title: string; body: (s: Seller, c: Currency) => string }> = {
  '/': { title: 'Clokrix: a hardware monitor for Windows', body: home },
  '/terms': { title: 'Clokrix: terms of sale', body: terms },
  '/privacy': { title: 'Clokrix: privacy', body: privacy },
  '/refunds': { title: 'Clokrix: refunds and cancellation', body: refunds },
  '/delivery': { title: 'Clokrix: delivery', body: delivery },
  '/contact': { title: 'Clokrix: contact', body: contact },
};

/** One of the website's pages, with the price in `c` (the visitor's) where it names one. */
export function sitePage(path: SitePath, s: Seller, nonce: string, c: Currency): string {
  const p = PAGES[path];
  return page({ title: p.title, body: p.body(s, c), nonce, seller: s.name, wide: true, index: true });
}

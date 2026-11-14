// Razorpay's API, the parts the checkout uses (2.5): making an order, reading a payment, and
// capturing one that was only authorised. Its signatures are checked in rules.ts.

import { PRICE } from './rules';

export interface Razorpay {
  /** https://api.razorpay.com, or the tests' stand-in. */
  api: string;
  keyId: string;
  keySecret: string;
}

export interface Payment {
  id: string;
  order_id: string | null;
  amount: number;
  currency: string;
  status: string;
}

function auth(r: Razorpay): string {
  return `Basic ${btoa(`${r.keyId}:${r.keySecret}`)}`;
}

async function call<T>(r: Razorpay, method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${r.api}${path}`, {
    method,
    headers: { Authorization: auth(r), ...(body ? { 'Content-Type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) throw new Error(`Razorpay ${method} ${path}: ${res.status}`);
  return (await res.json()) as T;
}

/** A new order for Clokrix Pro; `claim` is the app's code, when the app started the purchase. */
export async function createOrder(r: Razorpay, receipt: string, claim: string | null): Promise<string> {
  const notes: Record<string, string> = { product: 'clokrix-pro' };
  if (claim) notes.claim = claim;
  const o = await call<{ id: string }>(r, 'POST', '/v1/orders', { ...PRICE, receipt, notes });
  if (typeof o.id !== 'string' || !o.id) throw new Error('Razorpay gave no order id');
  return o.id;
}

export async function getPayment(r: Razorpay, id: string): Promise<Payment> {
  return call<Payment>(r, 'GET', `/v1/payments/${encodeURIComponent(id)}`);
}

/** A payment only authorised (an account set to capture by hand) is captured here. */
export async function capture(r: Razorpay, p: Payment): Promise<Payment> {
  return call<Payment>(r, 'POST', `/v1/payments/${encodeURIComponent(p.id)}/capture`, { amount: p.amount, currency: p.currency });
}

/** Paid in full for this order: captured, $100 in US dollars. */
export function paidFor(p: Payment, orderId: string): boolean {
  return p.status === 'captured' && p.order_id === orderId && p.amount === PRICE.amount && p.currency === PRICE.currency;
}

// Razorpay's API, the parts the checkout uses (2.5): making an order, reading a payment, and
// capturing one that was only authorised. Its signatures are checked in rules.ts. Since 2.6 the
// dashboard also reads its lists of payments and orders, for each buyer's name, email and phone:
// read when the dashboard opens, never kept here.

import { isPrice, PRICES, type Currency } from './rules';

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

/** A payment as Razorpay's list gives it: what the buyer typed into its window comes with it. */
export interface Listed extends Payment {
  email?: string | null;
  contact?: string | null;
  notes?: Record<string, string> | unknown[] | null;
  created_at?: number;
}

export interface ListedOrder {
  id: string;
  notes?: Record<string, string> | unknown[] | null;
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

/** A new order for Clokrix Pro at its price in `currency`; `claim` is the app's code, when the app
 * started the purchase, and `name` the buyer's (2.6), kept by Razorpay with the order. */
export async function createOrder(r: Razorpay, receipt: string, claim: string | null, currency: Currency, name?: string): Promise<string> {
  const notes: Record<string, string> = { product: 'clokrix-pro' };
  if (claim) notes.claim = claim;
  if (name) notes.name = name;
  const o = await call<{ id: string }>(r, 'POST', '/v1/orders', { ...PRICES[currency], receipt, notes });
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

/** Paid in full for this order: captured, at Clokrix Pro's price in rupees or in dollars. (Razorpay
 * takes a payment for an order only at the order's own amount, which this checkout set.) */
export function paidFor(p: Payment, orderId: string): boolean {
  return p.status === 'captured' && p.order_id === orderId && isPrice(p.amount, p.currency);
}

/** Lists past this many pages are not read: a Worker makes only so many requests elsewhere. */
const PAGES = 20;

/** Every item of a Razorpay list made since `from` (Unix seconds), 100 at a time. */
async function list<T>(r: Razorpay, path: string, from: number): Promise<T[]> {
  const got: T[] = [];
  for (let page = 0; page < PAGES; page++) {
    const q = new URLSearchParams({ from: String(Math.max(0, Math.floor(from))), count: '100', skip: String(page * 100) });
    const l = await call<{ items?: T[] }>(r, 'GET', `${path}?${q}`);
    const items = Array.isArray(l.items) ? l.items : [];
    got.push(...items);
    if (items.length < 100) break;
  }
  return got;
}

export function listPayments(r: Razorpay, from: number): Promise<Listed[]> {
  return list<Listed>(r, '/v1/payments', from);
}

export function listOrders(r: Razorpay, from: number): Promise<ListedOrder[]> {
  return list<ListedOrder>(r, '/v1/orders', from);
}

/** A note Razorpay kept (it gives none as an empty list), as text. */
export function note(notes: Listed['notes'], k: string): string | null {
  if (!notes || Array.isArray(notes)) return null;
  const v = (notes as Record<string, unknown>)[k];
  return typeof v === 'string' && v ? v : null;
}

// Touches on the PC's screen (1.7), as the PC's input: tap to click, hold for a right click,
// drag to drag, two fingers to scroll, pinch to zoom the picture here. Positions are fractions
// of the PC's monitor, so the phone never needs its size. No DOM here: the Screen tab feeds it
// pointer events and sends what comes out.

/** One message for the PC (`vyro_core::screen::input`). */
export type Input =
  | { t: 'move'; x: number; y: number }
  | { t: 'button'; b: 'left' | 'right' | 'middle'; down: boolean }
  | { t: 'scroll'; dx: number; dy: number }
  | { t: 'text'; s: string }
  | { t: 'key'; k: Key; down: boolean }
  | { t: 'press'; c: string }
  | { t: 'here' };

export type Key =
  | 'Escape'
  | 'Tab'
  | 'Enter'
  | 'Backspace'
  | 'Delete'
  | 'ArrowLeft'
  | 'ArrowRight'
  | 'ArrowUp'
  | 'ArrowDown'
  | 'Home'
  | 'End'
  | 'PageUp'
  | 'PageDown'
  | 'Meta'
  | 'Control'
  | 'Alt'
  | 'Shift';

export interface Pt {
  x: number;
  y: number;
}

/** A box on the phone's screen, in CSS pixels. */
export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Where a `pw`×`ph` picture shows inside `el` when fitted whole (object-fit: contain). */
export function contentBox(el: Box, pw: number, ph: number): Box {
  if (pw <= 0 || ph <= 0 || el.width <= 0 || el.height <= 0) return el;
  const scale = Math.min(el.width / pw, el.height / ph);
  const width = pw * scale;
  const height = ph * scale;
  return { left: el.left + (el.width - width) / 2, top: el.top + (el.height - height) / 2, width, height };
}

/** `p` as fractions of `box`, or null outside it. */
export function toFraction(p: Pt, box: Box): Pt | null {
  if (box.width <= 0 || box.height <= 0) return null;
  const x = (p.x - box.left) / box.width;
  const y = (p.y - box.top) / box.height;
  if (x < 0 || x > 1 || y < 0 || y > 1) return null;
  return { x: round(x), y: round(y) };
}

const round = (v: number) => Math.round(v * 10_000) / 10_000;

/** Movement under this many pixels is still a tap. */
export const SLOP = 10;
/** A touch held this long is a right click. */
export const LONG_MS = 500;
/** Two fingers moving this far together are one wheel notch. */
export const NOTCH_PX = 30;
/** Two fingers moving apart or together this far are a pinch. */
export const PINCH_PX = 24;

interface Hooks {
  /** Messages for the PC, in order. */
  send(msgs: Input[]): void;
  /** Phone pixels to fractions of the PC's monitor (null: outside the picture). */
  map(p: Pt): Pt | null;
  /** The picture is zoomed in here (two fingers then pan it rather than scroll the PC). */
  zoomed(): boolean;
  /** A pinch: the picture's zoom times `factor` about `center`. */
  zoom(factor: number, center: Pt): void;
  /** Two fingers moved the zoomed picture by (dx, dy) phone pixels. */
  pan(dx: number, dy: number): void;
  /** The pointer is now at these fractions (the phone draws its own pointer). */
  pointer(p: Pt): void;
}

type State =
  | { k: 'idle' }
  | { k: 'pending'; id: number; start: Pt; at: Pt; t0: number }
  | { k: 'dragging'; id: number; last: Pt }
  | { k: 'two'; mode: 'undecided' | 'scroll' | 'zoom' | 'pan'; d0: number; c0: Pt; c: Pt; d: number; acc: number }
  /** Used up (a right click fired, or a gesture ended): nothing until every finger is up. */
  | { k: 'done' };

/** Turns touches into the PC's input. Times are milliseconds (`performance.now()`). */
export class Gestures {
  private s: State = { k: 'idle' };
  private touches = new Map<number, Pt>();

  /** The two fingers down, when there are two. */
  private pair(): [Pt, Pt] | null {
    const [a, b] = [...this.touches.values()];
    return this.touches.size === 2 && a && b ? [a, b] : null;
  }

  constructor(private h: Hooks) {}

  down(id: number, p: Pt, t: number): void {
    this.touches.set(id, p);
    if (this.touches.size === 1 && this.s.k === 'idle') {
      this.s = { k: 'pending', id, start: p, at: p, t0: t };
      return;
    }
    const two = this.pair();
    if (two) {
      if (this.s.k === 'dragging') this.release(this.s.last);
      const [a, b] = two;
      const d = dist(a, b);
      const c = mid(a, b);
      this.s = { k: 'two', mode: 'undecided', d0: d, c0: c, c, d, acc: 0 };
      return;
    }
    if (this.touches.size > 2) this.s = { k: 'done' };
  }

  move(id: number, p: Pt, _t: number): void {
    if (!this.touches.has(id)) return;
    this.touches.set(id, p);
    const s = this.s;
    const two = s.k === 'two' ? this.pair() : null;
    if (s.k === 'pending' && s.id === id) {
      if (dist(p, s.start) < SLOP) {
        s.at = p;
        return;
      }
      const from = this.h.map(s.start);
      if (!from) {
        this.s = { k: 'done' };
        return;
      }
      const to = this.h.map(p);
      const msgs: Input[] = [move(from), { t: 'button', b: 'left', down: true }];
      if (to) msgs.push(move(to));
      this.h.send(msgs);
      this.h.pointer(to ?? from);
      this.s = { k: 'dragging', id, last: p };
    } else if (s.k === 'dragging' && s.id === id) {
      const to = this.h.map(p);
      if (to) {
        this.h.send([move(to)]);
        this.h.pointer(to);
        s.last = p;
      }
    } else if (s.k === 'two' && two) {
      const [a, b] = two;
      const d = dist(a, b);
      const c = mid(a, b);
      if (s.mode === 'undecided') {
        if (Math.abs(d - s.d0) > PINCH_PX) s.mode = 'zoom';
        else if (dist(c, s.c0) > SLOP) s.mode = this.h.zoomed() ? 'pan' : 'scroll';
        else return;
      }
      if (s.mode === 'zoom') {
        if (s.d > 0) this.h.zoom(d / s.d, c);
        this.h.pan(c.x - s.c.x, c.y - s.c.y);
      } else if (s.mode === 'pan') {
        this.h.pan(c.x - s.c.x, c.y - s.c.y);
      } else {
        // Fingers moving up scroll down, as on the phone itself.
        s.acc += s.c.y - c.y;
        const notches = Math.trunc(s.acc / NOTCH_PX);
        if (notches !== 0) {
          s.acc -= notches * NOTCH_PX;
          this.h.send([{ t: 'scroll', dx: 0, dy: notches }]);
        }
      }
      s.c = c;
      s.d = d;
    }
  }

  up(id: number, p: Pt, t: number): void {
    if (!this.touches.has(id)) return;
    this.touches.delete(id);
    const s = this.s;
    if (s.k === 'pending' && s.id === id) {
      this.click(s.start, t - s.t0 >= LONG_MS ? 'right' : 'left');
      this.s = { k: 'idle' };
    } else if (s.k === 'dragging' && s.id === id) {
      this.release(p);
      this.s = this.touches.size ? { k: 'done' } : { k: 'idle' };
    } else if (this.touches.size === 0) {
      this.s = { k: 'idle' };
    } else if (s.k === 'two') {
      this.s = { k: 'done' };
    }
  }

  /** A touch the browser took back (a system gesture): forget it without clicking. */
  cancel(id: number): void {
    this.touches.delete(id);
    if (this.s.k === 'dragging' && this.s.id === id) this.release(this.s.last);
    this.s = this.touches.size ? { k: 'done' } : { k: 'idle' };
  }

  /** Called now and then: a touch held long enough is a right click at once. */
  tick(t: number): void {
    const s = this.s;
    if (s.k === 'pending' && t - s.t0 >= LONG_MS) {
      this.click(s.start, 'right');
      this.s = { k: 'done' };
    }
  }

  private click(at: Pt, b: 'left' | 'right'): void {
    const p = this.h.map(at);
    if (!p) return;
    this.h.send([move(p), { t: 'button', b, down: true }, { t: 'button', b, down: false }]);
    this.h.pointer(p);
  }

  private release(at: Pt): void {
    const p = this.h.map(at);
    this.h.send(p ? [move(p), { t: 'button', b: 'left', down: false }] : [{ t: 'button', b: 'left', down: false }]);
  }
}

const move = (p: Pt): Input => ({ t: 'move', x: p.x, y: p.y });
const dist = (a: Pt, b: Pt) => Math.hypot(a.x - b.x, a.y - b.y);
const mid = (a: Pt, b: Pt): Pt => ({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });

/** The longest text one message types (the PC's limit). */
export const MAX_TEXT = 200;

/**
 * What typing `text` sends: with a modifier held, a single letter or digit is a key press
 * (a shortcut such as Ctrl+C); anything else is typed as it is, in pieces the PC takes.
 */
export function typed(text: string, modifier: boolean): Input[] {
  if (!text) return [];
  if (modifier && /^[A-Za-z0-9]$/.test(text)) return [{ t: 'press', c: text }];
  const out: Input[] = [];
  const chars = [...text.replace(/\r\n?/g, '\n')].filter((c) => c === '\n' || c === '\t' || c >= ' ');
  for (let i = 0; i < chars.length; i += MAX_TEXT) out.push({ t: 'text', s: chars.slice(i, i + MAX_TEXT).join('') });
  return out;
}

/**
 * Input waiting for the slow view's next message: pointer moves in a row collapse to the last,
 * everything else keeps its order. At most `max` go in one message.
 */
export function coalesce(queue: Input[], max = 20): { batch: Input[]; rest: Input[] } {
  const merged: Input[] = [];
  for (const i of queue) {
    if (i.t === 'move' && merged.at(-1)?.t === 'move') merged[merged.length - 1] = i;
    else merged.push(i);
  }
  return { batch: merged.slice(0, max), rest: merged.slice(max) };
}

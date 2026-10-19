import { describe, expect, it } from 'vitest';
import { coalesce, contentBox, Gestures, type Input, LONG_MS, MAX_TEXT, NOTCH_PX, type Pt, toFraction, typed } from './gestures';

// A 100×100 picture filling the element at (0, 0).
const box = { left: 0, top: 0, width: 100, height: 100 };

function setup(zoomed = false) {
  const sent: Input[] = [];
  const zooms: number[] = [];
  const pans: Pt[] = [];
  let pointer: Pt | null = null;
  const g = new Gestures({
    send: (m) => sent.push(...m),
    map: (p) => toFraction(p, box),
    zoomed: () => zoomed,
    zoom: (f) => zooms.push(f),
    pan: (dx, dy) => pans.push({ x: dx, y: dy }),
    pointer: (p) => (pointer = p),
  });
  return { g, sent, zooms, pans, pointer: () => pointer };
}

describe('where a touch lands', () => {
  it('fits the picture whole and centres it', () => {
    // A 16:9 picture in a 390×600 element: full width, centred vertically.
    expect(contentBox({ left: 0, top: 0, width: 390, height: 600 }, 1920, 1080)).toEqual({
      left: 0,
      top: (600 - 219.375) / 2,
      width: 390,
      height: 219.375,
    });
    // Not yet known: the element itself.
    expect(contentBox(box, 0, 0)).toEqual(box);
  });

  it('gives fractions of the picture, none outside', () => {
    const b = { left: 10, top: 20, width: 200, height: 100 };
    expect(toFraction({ x: 110, y: 70 }, b)).toEqual({ x: 0.5, y: 0.5 });
    expect(toFraction({ x: 10, y: 20 }, b)).toEqual({ x: 0, y: 0 });
    expect(toFraction({ x: 9, y: 70 }, b)).toBeNull();
    expect(toFraction({ x: 110, y: 121 }, b)).toBeNull();
  });
});

describe('touches', () => {
  it('a tap is a left click where it landed', () => {
    const { g, sent, pointer } = setup();
    g.down(1, { x: 50, y: 25 }, 0);
    g.move(1, { x: 53, y: 27 }, 50);
    g.up(1, { x: 53, y: 27 }, 120);
    expect(sent).toEqual([
      { t: 'move', x: 0.5, y: 0.25 },
      { t: 'button', b: 'left', down: true },
      { t: 'button', b: 'left', down: false },
    ]);
    expect(pointer()).toEqual({ x: 0.5, y: 0.25 });
  });

  it('a held touch is a right click, at once', () => {
    const { g, sent } = setup();
    g.down(1, { x: 10, y: 10 }, 0);
    g.tick(LONG_MS - 1);
    expect(sent).toEqual([]);
    g.tick(LONG_MS);
    expect(sent.map((m) => m.t === 'button' && m.b)).toEqual([false, 'right', 'right']);
    // Letting go clicks nothing more.
    g.up(1, { x: 10, y: 10 }, LONG_MS + 300);
    expect(sent).toHaveLength(3);
  });

  it('a drag holds the left button from where it started', () => {
    const { g, sent } = setup();
    g.down(1, { x: 10, y: 10 }, 0);
    g.move(1, { x: 30, y: 10 }, 40);
    g.move(1, { x: 60, y: 20 }, 80);
    g.up(1, { x: 60, y: 20 }, 120);
    expect(sent).toEqual([
      { t: 'move', x: 0.1, y: 0.1 },
      { t: 'button', b: 'left', down: true },
      { t: 'move', x: 0.3, y: 0.1 },
      { t: 'move', x: 0.6, y: 0.2 },
      { t: 'move', x: 0.6, y: 0.2 },
      { t: 'button', b: 'left', down: false },
    ]);
  });

  it('a touch outside the picture does nothing', () => {
    const { g, sent } = setup();
    g.down(1, { x: 150, y: 10 }, 0);
    g.up(1, { x: 150, y: 10 }, 50);
    g.down(2, { x: 150, y: 10 }, 100);
    g.move(2, { x: 50, y: 10 }, 150);
    g.up(2, { x: 50, y: 10 }, 200);
    expect(sent).toEqual([]);
  });

  it('two fingers moving up scroll the PC down, a notch per step', () => {
    const { g, sent } = setup();
    g.down(1, { x: 40, y: 80 }, 0);
    g.down(2, { x: 60, y: 80 }, 10);
    // Both fingers, a few pixels at a time, as events come.
    for (let y = 75; y >= 80 - NOTCH_PX * 2 - 5; y -= 5) {
      g.move(1, { x: 40, y }, 50);
      g.move(2, { x: 60, y }, 50);
    }
    g.up(1, { x: 40, y: 15 }, 90);
    g.up(2, { x: 60, y: 15 }, 95);
    // No click from either finger.
    expect(sent.filter((m) => m.t !== 'scroll')).toEqual([]);
    expect(sent.reduce((n, m) => n + (m.t === 'scroll' ? m.dy : 0), 0)).toBe(2);
  });

  it('a pinch zooms the picture here and sends nothing', () => {
    const { g, sent, zooms } = setup();
    g.down(1, { x: 40, y: 50 }, 0);
    g.down(2, { x: 60, y: 50 }, 0);
    g.move(1, { x: 10, y: 50 }, 30);
    g.move(2, { x: 90, y: 50 }, 30);
    expect(sent).toEqual([]);
    expect(zooms.reduce((a, b) => a * b, 1)).toBeCloseTo(4);
  });

  it('zoomed in, two fingers pan the picture instead of scrolling', () => {
    const { g, sent, pans } = setup(true);
    g.down(1, { x: 40, y: 50 }, 0);
    g.down(2, { x: 60, y: 50 }, 0);
    for (let y = 45; y >= 20; y -= 5) {
      g.move(1, { x: 40, y }, 30);
      g.move(2, { x: 60, y }, 30);
    }
    expect(sent).toEqual([]);
    expect(pans.reduce((a, p) => a + p.y, 0)).toBeLessThan(-20);
  });

  it('a second finger during a drag lets the button go', () => {
    const { g, sent } = setup();
    g.down(1, { x: 10, y: 10 }, 0);
    g.move(1, { x: 40, y: 10 }, 30);
    g.down(2, { x: 60, y: 60 }, 60);
    expect(sent.at(-1)).toEqual({ t: 'button', b: 'left', down: false });
    g.cancel(1);
    g.cancel(2);
    // Next touch starts afresh.
    g.down(3, { x: 50, y: 50 }, 200);
    g.up(3, { x: 50, y: 50 }, 250);
    expect(sent.at(-1)).toEqual({ t: 'button', b: 'left', down: false });
    expect(sent.at(-3)).toEqual({ t: 'move', x: 0.5, y: 0.5 });
  });
});

describe('typing', () => {
  it('types text, in pieces the PC takes', () => {
    expect(typed('hello\r\nthere', false)).toEqual([{ t: 'text', s: 'hello\nthere' }]);
    const long = typed('x'.repeat(MAX_TEXT + 5), false);
    expect(long.map((m) => (m.t === 'text' ? m.s.length : 0))).toEqual([MAX_TEXT, 5]);
    expect(typed('', false)).toEqual([]);
    // Control characters are dropped (keys are sent from the key bar).
    expect(typed('a\u0007b', false)).toEqual([{ t: 'text', s: 'ab' }]);
  });

  it('with a modifier held, one letter is a shortcut key', () => {
    expect(typed('c', true)).toEqual([{ t: 'press', c: 'c' }]);
    expect(typed('7', true)).toEqual([{ t: 'press', c: '7' }]);
    expect(typed('é', true)).toEqual([{ t: 'text', s: 'é' }]);
    expect(typed('cv', true)).toEqual([{ t: 'text', s: 'cv' }]);
  });
});

describe('the slow view', () => {
  it('collapses moves in a row and keeps everything else in order', () => {
    const q: Input[] = [
      { t: 'move', x: 0.1, y: 0.1 },
      { t: 'move', x: 0.2, y: 0.2 },
      { t: 'button', b: 'left', down: true },
      { t: 'move', x: 0.3, y: 0.3 },
      { t: 'move', x: 0.4, y: 0.4 },
      { t: 'button', b: 'left', down: false },
    ];
    expect(coalesce(q).batch).toEqual([
      { t: 'move', x: 0.2, y: 0.2 },
      { t: 'button', b: 'left', down: true },
      { t: 'move', x: 0.4, y: 0.4 },
      { t: 'button', b: 'left', down: false },
    ]);
    const many: Input[] = Array.from({ length: 30 }, (_, i) => ({ t: 'key', k: 'Tab', down: i % 2 === 0 }));
    const { batch, rest } = coalesce(many);
    expect([batch.length, rest.length]).toEqual([20, 10]);
  });
});

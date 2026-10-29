import { describe, expect, it } from 'vitest';
import { fitColumns, mostColumns, placeCards, planCards, stretch, tighten, type Box } from './card-layout';
import { rowSpan } from './packing';

// The views' cards as the browser preview measured them (px tall, at the columns it had then),
// with the grid's width and its narrowest column: the owner's screen sizes, 1280 to 2560 px.
const GAP = 16;
const UNIT = 4;
const SLACK = 4;
interface View {
  width: number;
  min: number;
  cards: Array<[w: string, h: number]>;
}
const VIEWS: Record<string, View> = {
  'Settings 2560': {
    width: 2049,
    min: 352,
    // Appearance, Profile, General, Gaming, Overlay, Remote, Units, Hidden sensors, Service, Updates
    cards: [['1', 714], ['1', 422], ['1', 157], ['1', 331], ['2', 710], ['2', 442], ['1', 171], ['1', 98], ['1', 83], ['1', 215]],
  },
  'Settings 1920': {
    width: 1409,
    min: 352,
    cards: [['1', 622], ['1', 377], ['1', 141], ['1', 280], ['2', 710], ['2', 442], ['1', 171], ['1', 98], ['1', 83], ['1', 215]],
  },
  // Ping, Ethernet, Wi-Fi: three cards, in five columns before.
  'Network 1920': { width: 1616, min: 320, cards: [['1', 326], ['1', 501], ['1', 714]] },
  'Network 2560': { width: 2258, min: 320, cards: [['1', 326], ['1', 501], ['1', 729]] },
  // Five fans, Safety, Recent events.
  'Cooling 1920': { width: 1614, min: 288, cards: [['1', 218], ['1', 265], ['1', 270], ['1', 270], ['1', 270], ['1', 277], ['1', 121]] },
  // Two drives in six columns before.
  'Storage 2560': { width: 2258, min: 320, cards: [['1', 892], ['1', 684]] },
  // Operating system, Uptime, Security, Startup apps, Power history.
  'System 1280': { width: 995, min: 288, cards: [['1', 190], ['1', 168], ['1', 203], ['2', 295], ['2', 209]] },
  // PC changes, Processor, Memory, Motherboard, Graphics.
  'Hardware 2560': { width: 2258, min: 320, cards: [['2', 641], ['2', 435], ['1', 350], ['1', 317], ['1', 235]] },
  'Devices 1920': { width: 1616, min: 352, cards: [['full', 987], ['full', 324]] },
};
const boxes = (v: View): Box[] => v.cards.map(([w, h]) => ({ w, rows: rowSpan(h, UNIT, GAP) }));
/** The share of the grid the cards cover as they are, before any reaches down. */
function covered(b: Box[], order: number[], cols: number): number {
  const spots = placeCards(b, order, cols, SLACK);
  const { bottom } = stretch(spots, cols);
  return spots.reduce((a, s) => a + (s.end - s.row) * s.span, 0) / (bottom * cols);
}

describe('card layout', () => {
  it('fits columns of the narrowest width with their gaps', () => {
    // Four of 352 px and three gaps are 1456 px.
    expect(fitColumns(1456, 352, 16)).toBe(4);
    expect(fitColumns(1455, 352, 16)).toBe(3);
    expect(fitColumns(300, 352, 16)).toBe(1);
  });

  it('never takes more columns than the cards fill', () => {
    const v = (...w: string[]): Box[] => w.map((x) => ({ w: x, rows: 50 }));
    expect(mostColumns(v('1', '1', '1'), 6)).toBe(3);
    // A wide card counts two once there are three columns, one before.
    expect(mostColumns(v('2', '1', '1'), 6)).toBe(4);
    expect(mostColumns(v('2', '1'), 2)).toBe(2);
    // Cards across every column fill any number.
    expect(mostColumns(v('full', 'full'), 6)).toBe(1);
    expect(mostColumns(boxes(VIEWS['Settings 2560']!), 5)).toBe(5);
  });

  it('reaches each card down to the next one, or the last line', () => {
    // Two columns: 30 rows over 20 on the left, 70 on the right. The 20 reaches down 20 rows.
    const spots = placeCards([{ w: '1', rows: 30 }, { w: '1', rows: 70 }, { w: '1', rows: 20 }], [0, 1, 2], 2, SLACK);
    expect(spots).toEqual([
      { col: 0, span: 1, row: 0, end: 30 },
      { col: 1, span: 1, row: 0, end: 70 },
      { col: 0, span: 1, row: 30, end: 50 },
    ]);
    expect(stretch(spots, 2)).toEqual({ fill: [0, 0, 20], holes: 0, bottom: 70 });
  });

  it('counts the room no card can reach', () => {
    // A wide card over columns ending at 10 and 40 reaches 10 rows less in the second: a hole.
    const spots = [
      { col: 0, span: 1, row: 0, end: 10 },
      { col: 1, span: 1, row: 0, end: 10 },
      { col: 0, span: 2, row: 10, end: 30 },
      { col: 0, span: 1, row: 30, end: 40 },
      { col: 1, span: 1, row: 40, end: 50 },
    ];
    const s = stretch(spots, 2);
    expect(s.fill).toEqual([0, 0, 0, 10, 0]);
    expect(s.holes).toBe(10);
    // A column with no card at all is all hole.
    expect(stretch([{ col: 0, span: 1, row: 0, end: 30 }], 2).holes).toBe(30);
  });

  it('tightens an order only where that leaves less room', () => {
    for (const [name, v] of Object.entries(VIEWS)) {
      const b = boxes(v);
      const cols = mostColumns(b, fitColumns(v.width, v.min, GAP));
      const order = tighten(b, cols, SLACK);
      expect([...order].sort((x, y) => x - y), name).toEqual(b.map((_, i) => i));
      const room = (o: number[]) => {
        const { bottom, holes } = stretch(placeCards(b, o, cols, SLACK), cols);
        return bottom + (4 * holes) / cols;
      };
      expect(room(order), name).toBeLessThanOrEqual(room(b.map((_, i) => i)));
      // The same cards always come out the same way.
      expect(tighten(b, cols, SLACK), name).toEqual(order);
    }
    // Cards of one height in one row: nothing to gain, nothing moves.
    const even: Box[] = Array.from({ length: 4 }, () => ({ w: '1', rows: 60 }));
    expect(tighten(even, 4, SLACK)).toEqual([0, 1, 2, 3]);
  });

  it('plans every view without a hole, in no more columns than fit', () => {
    for (const [name, v] of Object.entries(VIEWS)) {
      const b = boxes(v);
      const fit = fitColumns(v.width, v.min, GAP);
      const plan = planCards(b, v.width, v.min, GAP, SLACK);
      expect(plan.cols, name).toBeLessThanOrEqual(mostColumns(b, fit));
      expect(stretch(placeCards(b, plan.order, plan.cols, SLACK), plan.cols).holes, name).toBe(0);
      expect(covered(b, plan.order, plan.cols), name).toBeGreaterThanOrEqual(name === 'Network 2560' ? 0.7 : 0.78);
      expect(planCards(b, v.width, v.min, GAP, SLACK), name).toEqual(plan);
    }
  });

  it('takes fewer, wider columns where the cards fill them better', () => {
    const cols = (name: string) => planCards(boxes(VIEWS[name]!), VIEWS[name]!.width, VIEWS[name]!.min, GAP, SLACK).cols;
    // Wi-Fi beside Ping over Ethernet, not three cards in a row of five columns.
    expect(cols('Network 1920')).toBe(2);
    // Seven cards of about one height: 3 + 2 + 2, not 5 then 2.
    expect(cols('Cooling 1920')).toBe(3);
    expect(cols('Storage 2560')).toBe(2);
    expect(cols('Devices 1920')).toBe(1);
    // Columns past three times the narrowest are not taken: Network on 2560 px keeps three.
    expect(cols('Network 2560')).toBe(3);
  });

  it('closes the gaps Settings had on the owner’s big screen', () => {
    const v = VIEWS['Settings 2560']!;
    const b = boxes(v);
    // As the cards came: Overlay and Remote late, leaving holes and a ragged end.
    expect(covered(b, b.map((_, i) => i), 5)).toBeLessThan(0.8);
    const plan = planCards(b, v.width, v.min, GAP, SLACK);
    expect(covered(b, plan.order, plan.cols)).toBeGreaterThan(0.95);
    // Close to the owner's order: no card more than four places from where it was.
    plan.order.forEach((id, at) => expect(Math.abs(at - id)).toBeLessThanOrEqual(4));
  });
});

import { describe, expect, it } from 'vitest';
import { DEFAULT_TITLE, POWERED_BY, SIZES, TITLE_MAX, cardModel, driveSize, fileName, fit, layout, shortName, type CardSize, type CardStyle, type Measure } from './card';
import { sampleDrives } from './sample-drives';
import { sampleHardware } from './sample';
import type { DriveInfo, HardwareId, OsInfo, PerformanceScore } from './types';

// About Segoe UI's average advance: 0.55 of the size (bold a little wider).
const measure: Measure = (text, size, weight) => text.length * size * (weight >= 600 ? 0.58 : 0.55);

const BOARD_SERIAL = 'BSN-240912345678901';
const DRIVE_SERIAL = 'S5GXNX0T123456';
const hw: HardwareId = { ...sampleHardware, board: { ...sampleHardware.board, serial: BOARD_SERIAL } };
const drives: DriveInfo[] = sampleDrives.map((d) => ({ ...d, serial: DRIVE_SERIAL }));
const os: OsInfo = { name: 'Windows 11 Pro', edition: 'Professional', displayVersion: '24H2', build: '26100.2033', installDate: 0, architecture: 'x64' };
const score = { ts: 0, formula: 1, score: 91.6, band: 'excellent', coverage: 1, windowSecs: 604800, inputs: [] } as PerformanceScore;

const SIZE_IDS = Object.keys(SIZES) as CardSize[];
const STYLE_IDS: CardStyle[] = ['full', 'minimal', 'transparent'];

describe('share cards', () => {
  it('never carry a serial number', () => {
    for (const style of STYLE_IDS) {
      const m = cardModel(hw, os, drives, score, { title: 'Desk rig', style, withScore: true });
      const text = JSON.stringify(m);
      expect(text).not.toContain(BOARD_SERIAL);
      expect(text).not.toContain(DRIVE_SERIAL);
      for (const size of SIZE_IDS) {
        const all = layout(m, size, style, measure)
          .map((op) => (op.t === 'text' ? op.text : ''))
          .join('\n');
        expect(all).not.toContain(BOARD_SERIAL);
        expect(all).not.toContain(DRIVE_SERIAL);
      }
    }
  });

  it('list the specs, shorter when minimal', () => {
    const full = cardModel(hw, os, drives, score, { title: '', style: 'full', withScore: true });
    expect(full.title).toBe(DEFAULT_TITLE);
    expect(full.lines.map((l) => l.label)).toEqual(['CPU', 'GPU', 'RAM', 'Motherboard', 'Storage', 'Windows']);
    expect(full.lines[0]!.value).toBe(`13th Gen Intel Core i7-13700K · ${hw.cpu.cores}C/${hw.cpu.threads}T`);
    expect(full.lines[1]!.value).toBe('NVIDIA GeForce RTX 4070 · 12 GB');
    expect(full.lines[4]!.value).toBe('1 TB NVMe + 2 TB HDD');
    expect(full.lines[5]!.value).toBe('Windows 11 Pro 24H2');
    expect(full.score).toEqual({ value: 92, band: 'Excellent' });
    const minimal = cardModel(hw, os, drives, score, { title: 'x', style: 'minimal', withScore: false });
    expect(minimal.lines.map((l) => l.label)).toEqual(['CPU', 'GPU', 'RAM']);
    expect(minimal.lines[0]!.value).toBe('13th Gen Intel Core i7-13700K');
    expect(minimal.score).toBeNull();
    expect(cardModel(hw, os, drives, { ...score, score: null, band: null }, { title: 'x', style: 'full', withScore: true }).score).toBeNull();
    expect(cardModel(null, null, [], null, { title: 'x'.repeat(60), style: 'full', withScore: true })).toEqual({ title: 'x'.repeat(TITLE_MAX), lines: [], score: null });
  });

  it('leave out the processor graphics when there is a graphics card', () => {
    const igpu = { ...hw.gpus[0]!, name: 'AMD Radeon(TM) Graphics', vramMB: 512 };
    const both = cardModel({ ...hw, gpus: [igpu, ...hw.gpus] }, os, drives, null, { title: 'x', style: 'full', withScore: false });
    expect(both.lines.filter((l) => l.label === 'GPU').map((l) => l.value)).toEqual(['NVIDIA GeForce RTX 4070 · 12 GB']);
    const only = cardModel({ ...hw, gpus: [igpu] }, os, drives, null, { title: 'x', style: 'full', withScore: false });
    expect(only.lines.find((l) => l.label === 'GPU')!.value).toBe('AMD Radeon Graphics');
  });

  it('name parts and drives as they are sold', () => {
    expect(shortName('AMD Ryzen 7 9800X3D 8-Core Processor')).toBe('AMD Ryzen 7 9800X3D');
    expect(shortName('Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz')).toBe('Intel Core i7-8700K');
    expect(driveSize(1_907_729)).toBe('2 TB');
    expect(driveSize(953_869.7)).toBe('1 TB');
    expect(driveSize(488_386)).toBe('512 GB');
    expect(driveSize(3_815_447)).toBe('4 TB');
  });

  it('lay out every size and style inside the picture', () => {
    for (const size of SIZE_IDS) {
      const { w, h } = SIZES[size];
      for (const style of STYLE_IDS) {
        for (const withScore of [true, false]) {
          const m = cardModel(hw, os, drives, score, { title: 'A rather long name for a PC, to see', style, withScore });
          const ops = layout(m, size, style, measure);
          expect(ops.some((o) => o.t === 'background'), `${size}/${style}`).toBe(style !== 'transparent');
          expect(ops.some((o) => o.t === 'panel')).toBe(style === 'full');
          expect(ops.some((o) => o.t === 'ring')).toBe(withScore);
          expect(ops.some((o) => o.t === 'text' && o.text === POWERED_BY)).toBe(true);
          const texts = ops.filter((o) => o.t === 'text');
          for (const o of texts) {
            if (o.t !== 'text') continue;
            const width = measure(o.text, o.size, o.weight);
            const left = o.align === 'left' ? o.x : o.align === 'center' ? o.x - width / 2 : o.x - width;
            const where = `${size}/${style}/${withScore} "${o.text}"`;
            expect(left, where).toBeGreaterThanOrEqual(0);
            expect(left + width, where).toBeLessThanOrEqual(w);
            expect(o.y - o.size, where).toBeGreaterThanOrEqual(0);
            expect(o.y, where).toBeLessThanOrEqual(h);
          }
          // Lines never overlap each other or the footer.
          const ys = texts.map((o) => (o.t === 'text' ? o.y : 0)).filter((_, i) => i > 1);
          expect(new Set(ys).size).toBe(ys.length);
          for (const o of ops) {
            if (o.t !== 'ring') continue;
            expect(o.cx - o.r - o.width).toBeGreaterThanOrEqual(0);
            expect(o.cx + o.r + o.width).toBeLessThanOrEqual(w);
            // The ring's label sits below it, clear of the specs on square cards.
            const specs = texts.filter((t) => t.t === 'text' && t.x < o.cx - o.r && t.y > o.cy - o.r && t.y < o.cy + o.r);
            for (const t of specs) if (t.t === 'text') expect(t.x + measure(t.text, t.size, t.weight), `${size}/${style} "${t.text}"`).toBeLessThanOrEqual(o.cx - o.r);
          }
        }
      }
    }
  });

  it('shorten text that does not fit', () => {
    expect(fit('short', 10, 400, 1000, measure)).toBe('short');
    const s = fit('A very long graphics card name indeed', 20, 400, 200, measure);
    expect(s.endsWith('…')).toBe(true);
    expect(measure(s, 20, 400)).toBeLessThanOrEqual(200);
  });

  it('names the files by size and style', () => {
    expect(fileName('20261015-1024', 'wide', 'full')).toBe('vyro-card-20261015-1024-16x9.png');
    expect(fileName('20261015-1024', 'story', 'transparent')).toBe('vyro-card-20261015-1024-story-transparent.png');
  });
});

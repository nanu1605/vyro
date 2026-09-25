import { describe, expect, it } from 'vitest';
import { emptySnapshot, initialSimState, nextSnapshot } from './sim';

function seeded(seed: number): () => number {
  // mulberry32
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

describe('browser simulator', () => {
  it('starts with every reading unavailable', () => {
    const s = emptySnapshot(1);
    expect(s.cpu.usageTotal).toBeNull();
    expect(s.gpu).toEqual([]);
    expect(s.app.engine).toBe('starting');
  });

  it('stays within physical bounds', () => {
    const st = initialSimState();
    const rand = seeded(7);
    for (let i = 0; i < 500; i++) {
      const s = nextSnapshot(st, i, 1, rand);
      expect(s.cpu.usageTotal).toBeGreaterThanOrEqual(0);
      expect(s.cpu.usageTotal).toBeLessThanOrEqual(100);
      const gpu = s.gpu[0]!;
      expect(gpu.usage).toBeLessThanOrEqual(100);
      expect(gpu.vramUsedMB!).toBeLessThanOrEqual(gpu.vramTotalMB!);
      expect(gpu.engine.tensor).toBeNull();
      expect(s.memory.usedMB! + s.memory.availableMB!).toBe(32768);
    }
  });
});

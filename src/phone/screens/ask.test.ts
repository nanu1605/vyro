import { afterEach, describe, expect, it, vi } from 'vitest';
import { every } from './ask';

describe('asking the PC again', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('waits out a load still under way rather than asking again on top of it', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('document', { hidden: false });
    let answer = () => {};
    let asked = 0;
    const stop = every(3000, () => {
      asked++;
      return new Promise<void>((ok) => (answer = ok));
    });
    expect(asked).toBe(1);
    // A slow link: the answer takes three ticks.
    await vi.advanceTimersByTimeAsync(9000);
    expect(asked).toBe(1);
    answer();
    await vi.advanceTimersByTimeAsync(3000);
    expect(asked).toBe(2);
    stop();
  });

  it('asks again after a load that failed', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('document', { hidden: false });
    let asked = 0;
    const stop = every(3000, () => {
      asked++;
      return Promise.reject(new Error('lost'));
    });
    await vi.advanceTimersByTimeAsync(3000);
    expect(asked).toBe(2);
    stop();
  });
});

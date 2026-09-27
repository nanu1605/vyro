import { describe, expect, it } from 'vitest';
import { cToF, fromUnit, tempsIn, toUnit } from './units';

describe('temperature units', () => {
  it('converts both ways', () => {
    expect(cToF(100)).toBe(212);
    expect(toUnit(-40, 'f')).toBe(-40);
    expect(fromUnit(212, 'f')).toBeCloseTo(100);
    expect(toUnit(55, 'c')).toBe(55);
  });

  it('rewrites readings in messages like the engine does', () => {
    expect(tempsIn('CPU (Tdie) at 95.4 °C (above 95.0 °C for 10 s)', 'f')).toBe('CPU (Tdie) at 203.7 °F (above 203.0 °F for 10 s)');
    expect(tempsIn('CPU running hot: 91 °C', 'f')).toBe('CPU running hot: 196 °F');
    expect(tempsIn('DIMM at -5°C', 'f')).toBe('DIMM at 23°F');
    expect(tempsIn('12 V and 40 °C', 'c')).toBe('12 V and 40 °C');
  });
});

import { describe, expect, it } from 'vitest';
import { chartForSensor, chartsFor } from './presets';
import type { Sensor, SensorTree, Unit } from './types';

const s = (id: string, unit: Unit, max: number | null = 1, history = true): Sensor => ({
  id, group: 'cpu', name: id, unit, value: max, min: 0, max, avg: max, source: 'os', available: true, history,
});

describe('chartsFor', () => {
  const tree: SensorTree = {
    groups: [
      {
        group: 'cpu',
        sensors: [
          s('cpu.usage.total', '%'),
          s('cpu.core0.usage', '%', 5, false),
          s('cpu.temp.package', '°C'),
          s('cpu.vid', 'V', 1.3),
          s('gpu.0.usage', '%'),
          s('fan.0', 'RPM', 1500),
          s('fan.2', 'RPM', 0),
          s('voltage.vin0', 'V', 12.1),
          s('voltage.vin4', 'V', 1.2),
          s('network.if7.down', 'Mbps', 0),
        ],
      },
    ],
  };
  const charts = chartsFor(tree);
  const by = (k: string) => charts.find((c) => c.key === k)?.metrics.map((m) => m.id);

  it('groups history sensors by chart and unit', () => {
    expect(by('usage')).toEqual(['cpu.usage.total', 'gpu.0.usage']);
    expect(by('temps')).toEqual(['cpu.temp.package']);
  });

  it('leaves out never-active fans and adapters, and high rails', () => {
    expect(by('fans')).toEqual(['fan.0']);
    expect(by('network')).toBeUndefined();
    expect(by('volts')).toEqual(['cpu.vid', 'voltage.vin4']);
  });

  it('builds a one-sensor chart with a sensible axis', () => {
    expect(chartForSensor(s('voltage.vin0', 'V', 12)).zero).toBe(false);
    expect(chartForSensor(s('gpu.0.usage', '%')).max).toBe(100);
  });
});

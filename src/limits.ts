// Health's thermal limits in the page (Rust: `vyro_core::health`), shared by My PC's thermal map
// and the remote companion, so the companion page does not load the 3-D model's code.

/** Health's limits, °C: [warning, problem]. */
export const LIMITS = {
  cpu: [90, 99],
  gpu: [85, 92],
  hotspot: [100, 108],
  ram: [75, 85],
  nvme: [75, 85],
  sata: [60, 70],
  vrm: [95, 105],
  chipset: [80, 90],
  system: [55, 65],
} as const satisfies Record<string, readonly [number, number]>;

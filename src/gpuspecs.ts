// GPU shapes for the stylized 3-D card (P23): reference designs by PCI device id, partner cards
// by the GPU's tier, and a generic card otherwise. Our own simple shapes only: no logos, no
// exact renders. The owner can correct a shape ("Does this look like your card?").

import type { GpuId } from './types';

export type PowerPlug = 'none' | '6' | '8' | '8+6' | '2x8' | '3x8' | '16';
export type CardColour = 'black' | 'gunmetal' | 'silver' | 'white';

export interface GpuShape {
  fans: number;
  /** Length, mm. */
  lengthMm: number;
  /** Thickness in slots (2, 2.5, 3, 3.5). */
  slots: number;
  power: PowerPlug;
  colour: CardColour;
  /** NVIDIA Founders Edition flow-through: one fan on each face, at either end. */
  flow: boolean;
}

export const FAN_CHOICES = [1, 2, 3] as const;
export const LENGTH_CHOICES = [170, 200, 240, 270, 300, 340] as const;
export const SLOT_CHOICES = [2, 2.5, 3, 3.5] as const;
export const POWER_CHOICES: PowerPlug[] = ['none', '6', '8', '8+6', '2x8', '3x8', '16'];
export const COLOUR_CHOICES: CardColour[] = ['black', 'gunmetal', 'silver', 'white'];

export const POWER_LABEL: Record<PowerPlug, string> = {
  none: 'None (slot power)',
  '6': '6-pin',
  '8': '8-pin',
  '8+6': '8-pin + 6-pin',
  '2x8': '2 × 8-pin',
  '3x8': '3 × 8-pin',
  '16': '16-pin (12V-2x6)',
};

export const NVIDIA = 0x10de;
export const AMD = 0x1002;
export const INTEL = 0x8086;

const shape = (fans: number, lengthMm: number, slots: number, power: PowerPlug, colour: CardColour, flow = false): GpuShape => ({
  fans,
  lengthMm,
  slots,
  power,
  colour,
  flow,
});

/** Reference designs: Founders Edition, AMD reference ("MBA"), Intel Limited Edition. */
const REFERENCE: Array<{ vendor: number; ids: number[]; design: string; shape: GpuShape }> = [
  // NVIDIA GeForce RTX 50
  { vendor: NVIDIA, ids: [0x2b85], design: 'Founders Edition', shape: shape(2, 304, 2, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2c02], design: 'Founders Edition', shape: shape(2, 304, 2, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2f04], design: 'Founders Edition', shape: shape(2, 242, 2, '16', 'gunmetal', true) },
  // RTX 40
  { vendor: NVIDIA, ids: [0x2684], design: 'Founders Edition', shape: shape(2, 304, 3, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2704, 0x2702], design: 'Founders Edition', shape: shape(2, 304, 3, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2786, 0x2783], design: 'Founders Edition', shape: shape(2, 244, 2, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2803, 0x2805], design: 'Founders Edition', shape: shape(2, 244, 2, '8', 'gunmetal', true) },
  // RTX 30
  { vendor: NVIDIA, ids: [0x2203, 0x2204], design: 'Founders Edition', shape: shape(2, 313, 3, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2206, 0x2216, 0x2208], design: 'Founders Edition', shape: shape(2, 285, 2, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2482], design: 'Founders Edition', shape: shape(2, 270, 2, '16', 'gunmetal', true) },
  { vendor: NVIDIA, ids: [0x2484, 0x2488, 0x2486, 0x2489], design: 'Founders Edition', shape: shape(2, 242, 2, '16', 'gunmetal', true) },
  // RTX 20
  { vendor: NVIDIA, ids: [0x1e04, 0x1e07], design: 'Founders Edition', shape: shape(2, 267, 2, '2x8', 'silver') },
  { vendor: NVIDIA, ids: [0x1e81, 0x1e82, 0x1e87, 0x1e84], design: 'Founders Edition', shape: shape(2, 267, 2, '8+6', 'silver') },
  { vendor: NVIDIA, ids: [0x1f02, 0x1f07, 0x1f06, 0x1f08], design: 'Founders Edition', shape: shape(2, 229, 2, '8', 'silver') },
  // AMD Radeon RX 7000
  { vendor: AMD, ids: [0x744c], design: 'AMD reference', shape: shape(3, 287, 2.5, '2x8', 'black') },
  { vendor: AMD, ids: [0x747e], design: 'AMD reference', shape: shape(2, 267, 2, '2x8', 'black') },
  { vendor: AMD, ids: [0x7480], design: 'AMD reference', shape: shape(2, 204, 2, '8', 'black') },
  // RX 6000
  { vendor: AMD, ids: [0x73a5, 0x73bf], design: 'AMD reference', shape: shape(3, 267, 2.5, '2x8', 'black') },
  { vendor: AMD, ids: [0x73df], design: 'AMD reference', shape: shape(2, 267, 2, '8+6', 'black') },
  // Intel Arc
  { vendor: INTEL, ids: [0x56a0, 0x56a1], design: 'Limited Edition', shape: shape(2, 268, 2, '8+6', 'black') },
  { vendor: INTEL, ids: [0xe20b], design: 'Limited Edition', shape: shape(2, 272, 2, '8', 'black') },
];

/** Board partners by PCI subsystem vendor (the name only: no logos are drawn). */
const PARTNERS: Record<number, string> = {
  0x1043: 'ASUS',
  0x1462: 'MSI',
  0x1458: 'Gigabyte',
  0x3842: 'EVGA',
  0x19da: 'Zotac',
  0x1569: 'Palit',
  0x10b0: 'Gainward',
  0x196e: 'PNY',
  0x1b4c: 'KFA2 / Galax',
  0x7377: 'Colorful',
  0x1da2: 'Sapphire',
  0x148c: 'PowerColor',
  0x1682: 'XFX',
  0x1849: 'ASRock',
  0x1028: 'Dell',
  0x103c: 'HP',
  0x17aa: 'Lenovo',
  0x1048: 'Elsa',
};

/** The card's own key, for the owner's corrections: vendor, device and subsystem. */
export function cardKey(g: Pick<GpuId, 'vendorId' | 'deviceId' | 'subsysId'>): string {
  const hex = (n: number, w: number) => (n >>> 0).toString(16).padStart(w, '0');
  return `${hex(g.vendorId, 4)}:${hex(g.deviceId, 4)}:${hex(g.subsysId, 8)}`;
}

/** 90, 80, 70, 60, 50, ... from the model number in the name; null when there is none. */
export function tier(name: string, vendor: number): number | null {
  if (vendor === INTEL) {
    const m = /\b[AB]([1-9])\d{2}\b/i.exec(name);
    return m ? Number(m[1]) * 10 : null;
  }
  const m = /\b(\d)(\d)(\d)(\d)\b/.exec(name);
  if (!m) return null;
  if (vendor === AMD) {
    // RX 7900 → 90, RX 6650 → 60; RDNA 4 names the class in the last two digits (RX 9070 → 70).
    return m[2] === '0' ? Number(m[3]) * 10 : Number(m[2]) * 10;
  }
  return Number(m[3]) * 10;
}

/** Series for NVIDIA (20, 30, 40, 50), for the power plug of a partner card. */
const series = (name: string): number | null => {
  const m = /\b(\d{2})\d0\b/.exec(name);
  return m ? Number(m[1]) : null;
};

/** A partner card's likely shape from its tier. */
function partnerShape(name: string, vendor: number): GpuShape {
  const t = tier(name, vendor) ?? 60;
  const modern = vendor === NVIDIA && (series(name) ?? 0) >= 40;
  if (t >= 90) return shape(3, 340, 3.5, modern ? '16' : '3x8', 'black');
  if (t >= 80) return shape(3, 320, 3, modern ? '16' : '2x8', 'black');
  if (t >= 70) return shape(3, 300, 2.5, modern ? '16' : vendor === INTEL ? '8+6' : '2x8', 'black');
  if (t >= 60) return shape(2, 250, 2, '8', 'black');
  return shape(2, 200, 2, t <= 30 ? 'none' : '8', 'black');
}

export interface DetectedCard {
  shape: GpuShape;
  /** "Founders Edition", "AMD reference", "Gigabyte card", "Generic card". */
  design: string;
  source: 'reference' | 'partner' | 'generic';
}

/** What the card probably looks like. */
export function detectCard(g: Pick<GpuId, 'name' | 'vendorId' | 'deviceId' | 'subsysId'>): DetectedCard {
  const subVendor = g.subsysId & 0xffff;
  const ref = REFERENCE.find((r) => r.vendor === g.vendorId && r.ids.includes(g.deviceId));
  if (ref && (subVendor === g.vendorId || subVendor === 0)) return { shape: { ...ref.shape }, design: ref.design, source: 'reference' };
  const known = [NVIDIA, AMD, INTEL].includes(g.vendorId) && tier(g.name, g.vendorId) != null;
  if (known) {
    const brand = PARTNERS[subVendor];
    return { shape: partnerShape(g.name, g.vendorId), design: brand ? `${brand} card` : 'Partner card', source: 'partner' };
  }
  return { shape: shape(2, 250, 2, '8', 'black'), design: 'Generic card', source: 'generic' };
}

/** A shape read back from the settings, or null if it is not one. */
export function validShape(x: unknown): GpuShape | null {
  if (typeof x !== 'object' || x == null) return null;
  const s = x as Record<string, unknown>;
  const ok =
    (FAN_CHOICES as readonly unknown[]).includes(s.fans) &&
    typeof s.lengthMm === 'number' &&
    s.lengthMm >= 120 &&
    s.lengthMm <= 400 &&
    (SLOT_CHOICES as readonly unknown[]).includes(s.slots) &&
    POWER_CHOICES.includes(s.power as PowerPlug) &&
    COLOUR_CHOICES.includes(s.colour as CardColour) &&
    typeof s.flow === 'boolean';
  return ok ? { fans: s.fans as number, lengthMm: s.lengthMm as number, slots: s.slots as number, power: s.power as PowerPlug, colour: s.colour as CardColour, flow: s.flow as boolean } : null;
}

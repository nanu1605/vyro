// Representative inventory for browser-only UI work (no Rust engine). Shapes are checked
// against the generated bindings, so they stay in lockstep with the backend.

import type { CoreInfo, HardwareId, SystemInfo } from './types';

const coreMap: CoreInfo[] = [
  ...Array.from({ length: 8 }, (_, i): CoreInfo => ({ index: i, kind: 'performance', logical: [i * 2, i * 2 + 1] })),
  ...Array.from({ length: 8 }, (_, i): CoreInfo => ({ index: 8 + i, kind: 'efficiency', logical: [16 + i] })),
];

export const sampleHardware: HardwareId = {
  cpu: {
    vendor: 'GenuineIntel',
    brand: '13th Gen Intel(R) Core(TM) i7-13700K',
    architecture: 'x86-64',
    family: 6,
    model: 183,
    stepping: 1,
    socket: 'LGA1700',
    packages: 1,
    cores: 16,
    threads: 24,
    performanceCores: 8,
    efficiencyCores: 8,
    hybrid: true,
    coreMap,
    caches: [
      { level: 1, kind: 'data', sizeKb: 48, ways: 12, lineBytes: 64, count: 8 },
      { level: 1, kind: 'data', sizeKb: 32, ways: 8, lineBytes: 64, count: 8 },
      { level: 1, kind: 'instruction', sizeKb: 32, ways: 8, lineBytes: 64, count: 8 },
      { level: 1, kind: 'instruction', sizeKb: 64, ways: 8, lineBytes: 64, count: 8 },
      { level: 2, kind: 'unified', sizeKb: 2048, ways: 16, lineBytes: 64, count: 8 },
      { level: 2, kind: 'unified', sizeKb: 4096, ways: 16, lineBytes: 64, count: 2 },
      { level: 3, kind: 'unified', sizeKb: 30720, ways: 12, lineBytes: 64, count: 1 },
    ],
    isa: ['x86-64', 'SSE4.2', 'AVX', 'AVX2', 'AVX-VNNI', 'FMA3', 'BMI2', 'AES', 'VAES', 'SHA', 'VT-x'],
    baseMhz: 3400,
    maxMhz: 5400,
  },
  memory: {
    totalMb: 32768,
    maxCapacityMb: 196608,
    slotsTotal: 4,
    slotsUsed: 2,
    channels: 2,
    dimms: ['A', 'B'].map((ch) => ({
      slot: `Controller0-Channel${ch}-DIMM1`,
      bank: 'BANK 0',
      manufacturer: 'Kingston',
      partNumber: 'KF560C40-16',
      sizeMB: 16384,
      memoryType: 'DDR5',
      formFactor: 'DIMM',
      speedMts: 4800,
      configuredSpeedMts: 6000,
      configuredVoltageMv: 1350,
      rank: 1,
    })),
  },
  board: {
    systemManufacturer: 'ASUS',
    systemProduct: null,
    manufacturer: 'ASUSTeK COMPUTER INC.',
    product: 'ROG STRIX Z790-E GAMING WIFI',
    version: 'Rev 1.xx',
    serial: '240912345678901',
    biosVendor: 'American Megatrends Inc.',
    biosVersion: '1801',
    biosDate: '07/12/2024',
    chipset: 'Intel Z790',
    formFactor: 'desktop',
  },
  gpus: [
    {
      name: 'NVIDIA GeForce RTX 4070',
      vendor: 'NVIDIA',
      vendorId: 0x10de,
      deviceId: 0x2786,
      subsysId: 0x51411458,
      revision: 0xa1,
      vramMB: 12282,
      sharedMB: 16334,
      driverVersion: '32.0.15.6094',
      vendorDriverVersion: '560.94',
      driverDate: '9-19-2024',
      biosVersion: '95.04.1F.00.2D',
      memoryType: null,
    },
  ],
  smbiosVersion: '3.5',
};

const now = Math.floor(Date.now() / 1000);
const boot = now - (2 * 86400 + 5 * 3600 + 17 * 60);

export const sampleSystem: SystemInfo = {
  os: {
    name: 'Windows 11 Pro',
    edition: 'Professional',
    displayVersion: '24H2',
    build: '26100.2033',
    installDate: now - 180 * 86400,
    architecture: 'x64',
  },
  bootTime: boot,
  uptimeSecs: now - boot,
  security: {
    uefi: true,
    secureBoot: true,
    tpmPresent: true,
    tpmVersion: '2.0',
    virtualizationEnabled: true,
    hypervisorPresent: true,
    bitlockerSystemDrive: null,
  },
  processes: { processes: 287, threads: 3915, handles: 131702 },
  startupApps: [
    { name: 'Discord', command: '"C:\\Users\\me\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe', source: 'registryUser', enabled: false },
    { name: 'OneDrive', command: '"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe" /background', source: 'registryUser', enabled: true },
    { name: 'SecurityHealth', command: '%windir%\\system32\\SecurityHealthSystray.exe', source: 'registryMachine', enabled: true },
    { name: 'Steam', command: '"C:\\Program Files (x86)\\Steam\\steam.exe" -silent', source: 'registryUser', enabled: true },
    { name: 'Vyro updater', command: 'C:\\Program Files\\Vyro\\vyro.exe --check-update', source: 'scheduledTask', enabled: true },
  ],
  powerEvents: [
    { ts: boot + 2 * 86400 + 3600, kind: 'wake', detail: 'wake source: USB Composite Device' },
    { ts: boot + 2 * 86400 - 7 * 3600, kind: 'sleep', detail: null },
    { ts: boot + 86400 + 2 * 3600, kind: 'wake', detail: 'wake source: Power Button' },
    { ts: boot + 86400 - 6 * 3600, kind: 'sleep', detail: null },
    { ts: boot, kind: 'boot', detail: null },
    { ts: boot - 60, kind: 'shutdown', detail: null },
  ],
  elevated: false,
};

/** The browser preview as a laptop (`localStorage['vyro.sim.form'] = 'laptop'`). */
export const sampleLaptop: HardwareId = {
  ...sampleHardware,
  board: {
    ...sampleHardware.board,
    systemManufacturer: 'ASUSTeK COMPUTER INC.',
    systemProduct: 'ROG Zephyrus G16 GU605MV',
    manufacturer: 'ASUSTeK COMPUTER INC.',
    product: 'GU605MV',
    chipset: null,
    formFactor: 'laptop',
  },
  memory: {
    ...sampleHardware.memory,
    slotsTotal: 2,
    dimms: sampleHardware.memory.dimms.slice(0, 2).map((d, i) => ({ ...d, slot: `DIMM ${i}`, formFactor: 'SODIMM' })),
  },
};

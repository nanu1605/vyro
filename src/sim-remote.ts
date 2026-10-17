// Browser preview of Settings → Remote (P26): the companion as the app would report it, kept in
// memory. The QR code is a real one for the sample address (made by `vyro_core::remote::qr`).

import type { RemoteBrief, RemoteStatus } from './types';

const QR = 'M4 4h7v1h-7zM14 4h1v1h-1zM17 4h4v1h-4zM23 4h1v1h-1zM26 4h7v1h-7zM4 5h1v1h-1zM10 5h1v1h-1zM14 5h1v1h-1zM19 5h1v1h-1zM21 5h1v1h-1zM24 5h1v1h-1zM26 5h1v1h-1zM32 5h1v1h-1zM4 6h1v1h-1zM6 6h3v1h-3zM10 6h1v1h-1zM12 6h1v1h-1zM15 6h1v1h-1zM19 6h2v1h-2zM26 6h1v1h-1zM28 6h3v1h-3zM32 6h1v1h-1zM4 7h1v1h-1zM6 7h3v1h-3zM10 7h1v1h-1zM12 7h3v1h-3zM18 7h7v1h-7zM26 7h1v1h-1zM28 7h3v1h-3zM32 7h1v1h-1zM4 8h1v1h-1zM6 8h3v1h-3zM10 8h1v1h-1zM12 8h1v1h-1zM18 8h1v1h-1zM23 8h1v1h-1zM26 8h1v1h-1zM28 8h3v1h-3zM32 8h1v1h-1zM4 9h1v1h-1zM10 9h1v1h-1zM12 9h1v1h-1zM19 9h3v1h-3zM26 9h1v1h-1zM32 9h1v1h-1zM4 10h7v1h-7zM12 10h1v1h-1zM14 10h1v1h-1zM16 10h1v1h-1zM18 10h1v1h-1zM20 10h1v1h-1zM22 10h1v1h-1zM24 10h1v1h-1zM26 10h7v1h-7zM12 11h1v1h-1zM14 11h1v1h-1zM16 11h1v1h-1zM19 11h2v1h-2zM22 11h1v1h-1zM4 12h1v1h-1zM6 12h5v1h-5zM13 12h2v1h-2zM16 12h1v1h-1zM18 12h1v1h-1zM21 12h4v1h-4zM26 12h5v1h-5zM6 13h1v1h-1zM11 13h3v1h-3zM16 13h1v1h-1zM19 13h5v1h-5zM26 13h1v1h-1zM28 13h1v1h-1zM32 13h1v1h-1zM6 14h2v1h-2zM9 14h2v1h-2zM12 14h1v1h-1zM19 14h3v1h-3zM27 14h2v1h-2zM4 15h1v1h-1zM7 15h2v1h-2zM11 15h2v1h-2zM14 15h1v1h-1zM16 15h1v1h-1zM18 15h2v1h-2zM23 15h1v1h-1zM25 15h2v1h-2zM28 15h1v1h-1zM31 15h1v1h-1zM4 16h1v1h-1zM6 16h1v1h-1zM8 16h1v1h-1zM10 16h2v1h-2zM15 16h1v1h-1zM19 16h7v1h-7zM27 16h1v1h-1zM29 16h2v1h-2zM5 17h2v1h-2zM8 17h2v1h-2zM12 17h3v1h-3zM16 17h2v1h-2zM19 17h2v1h-2zM22 17h2v1h-2zM26 17h1v1h-1zM28 17h1v1h-1zM30 17h1v1h-1zM32 17h1v1h-1zM4 18h3v1h-3zM10 18h1v1h-1zM12 18h11v1h-11zM25 18h2v1h-2zM30 18h1v1h-1zM5 19h1v1h-1zM8 19h2v1h-2zM11 19h3v1h-3zM17 19h1v1h-1zM20 19h1v1h-1zM25 19h1v1h-1zM27 19h1v1h-1zM31 19h1v1h-1zM5 20h1v1h-1zM7 20h1v1h-1zM10 20h1v1h-1zM12 20h1v1h-1zM14 20h3v1h-3zM18 20h4v1h-4zM24 20h2v1h-2zM30 20h1v1h-1zM4 21h1v1h-1zM8 21h2v1h-2zM12 21h1v1h-1zM14 21h3v1h-3zM23 21h1v1h-1zM25 21h2v1h-2zM28 21h3v1h-3zM32 21h1v1h-1zM4 22h1v1h-1zM6 22h2v1h-2zM9 22h2v1h-2zM13 22h2v1h-2zM19 22h2v1h-2zM22 22h1v1h-1zM26 22h1v1h-1zM29 22h2v1h-2zM4 23h1v1h-1zM11 23h5v1h-5zM18 23h1v1h-1zM22 23h2v1h-2zM25 23h2v1h-2zM28 23h1v1h-1zM31 23h1v1h-1zM4 24h1v1h-1zM6 24h1v1h-1zM8 24h1v1h-1zM10 24h3v1h-3zM14 24h1v1h-1zM16 24h1v1h-1zM21 24h1v1h-1zM24 24h5v1h-5zM30 24h3v1h-3zM12 25h1v1h-1zM14 25h2v1h-2zM17 25h4v1h-4zM22 25h3v1h-3zM28 25h5v1h-5zM4 26h7v1h-7zM14 26h1v1h-1zM17 26h4v1h-4zM22 26h3v1h-3zM26 26h1v1h-1zM28 26h3v1h-3zM4 27h1v1h-1zM10 27h1v1h-1zM12 27h3v1h-3zM16 27h2v1h-2zM19 27h2v1h-2zM24 27h1v1h-1zM28 27h1v1h-1zM32 27h1v1h-1zM4 28h1v1h-1zM6 28h3v1h-3zM10 28h1v1h-1zM12 28h1v1h-1zM14 28h1v1h-1zM16 28h1v1h-1zM18 28h1v1h-1zM20 28h2v1h-2zM24 28h7v1h-7zM4 29h1v1h-1zM6 29h3v1h-3zM10 29h1v1h-1zM12 29h2v1h-2zM15 29h1v1h-1zM17 29h1v1h-1zM21 29h2v1h-2zM24 29h2v1h-2zM27 29h1v1h-1zM31 29h2v1h-2zM4 30h1v1h-1zM6 30h3v1h-3zM10 30h1v1h-1zM12 30h1v1h-1zM15 30h2v1h-2zM19 30h1v1h-1zM22 30h3v1h-3zM26 30h1v1h-1zM28 30h2v1h-2zM31 30h1v1h-1zM4 31h1v1h-1zM10 31h1v1h-1zM13 31h1v1h-1zM15 31h3v1h-3zM24 31h2v1h-2zM27 31h2v1h-2zM31 31h1v1h-1zM4 32h7v1h-7zM12 32h1v1h-1zM14 32h3v1h-3zM18 32h4v1h-4zM29 32h2v1h-2z';

const now = () => Math.floor(Date.now() / 1000);
const DAY = 86_400;

let status: RemoteStatus = {
  settings: { enabled: false, port: 8976, sessionDays: 30 },
  running: false,
  error: null,
  urls: [],
  devices: [
    {
      id: '1a2b3c4d',
      name: 'Android phone · Chrome',
      paired: now() - 3 * DAY,
      expires: now() + 27 * DAY,
      lastSeen: now() - 40,
      lastAddress: '192.168.1.31',
      watching: false,
      anywhere: false,
    },
  ],
  watching: 0,
  pairing: null,
  firewall: 'unknown',
  relay: { enabled: false, url: 'https://vyro-relay.example.workers.dev', custom: false, state: 'off', error: null, pairing: null, watching: 0 },
};

const listeners = new Set<(b: RemoteBrief) => void>();
function changed(): RemoteStatus {
  const brief = simRemoteBrief();
  for (const cb of listeners) cb(brief);
  return structuredClone(status);
}

export function simRemoteStatus(): RemoteStatus {
  if (status.pairing && status.pairing.expires <= now()) status = { ...status, pairing: null };
  return structuredClone(status);
}

export function simRemoteBrief(): RemoteBrief {
  const relay = status.relay.state;
  return { running: status.running || relay !== 'off', watching: status.watching + status.relay.watching, relay };
}

export function onSimRemote(cb: (b: RemoteBrief) => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function simSetEnabled(on: boolean): RemoteStatus {
  const watching = on ? 1 : 0;
  status = {
    ...status,
    settings: { ...status.settings, enabled: on },
    running: on,
    urls: on ? [`http://192.168.1.20:${status.settings.port}/`, `http://172.20.48.1:${status.settings.port}/`] : [],
    watching,
    pairing: null,
    firewall: on ? 'ask' : 'unknown',
    devices: status.devices.map((d) => ({ ...d, watching: on })),
  };
  return changed();
}

export function simSetOptions(port: number, sessionDays: number): RemoteStatus {
  if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error('Pick a port from 1024 to 65535.');
  status = {
    ...status,
    settings: { ...status.settings, port, sessionDays },
    urls: status.running ? status.urls.map((u) => u.replace(/:\d+\//, `:${port}/`)) : [],
  };
  return changed();
}

export function simStartPairing(): RemoteStatus {
  if (!status.running) throw new Error('Turn the companion on first.');
  const url = `http://192.168.1.20:${status.settings.port}/#pair=K7QM4XPA`;
  status = {
    ...status,
    pairing: { code: 'K7QM-4XPA', expires: now() + 300, triesLeft: 5, url, qr: QR, qrSize: 37 },
  };
  return changed();
}

export function simCancelPairing(): RemoteStatus {
  status = { ...status, pairing: null };
  return changed();
}

export function simSignOut(id: string | null): RemoteStatus {
  const devices = id ? status.devices.filter((d) => d.id !== id) : [];
  status = { ...status, devices, watching: devices.some((d) => d.watching) ? 1 : 0 };
  return changed();
}

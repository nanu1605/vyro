// Generated from vyro_core::network::sample(). Regenerate: cargo run -p vyro-core --example
// inventory -- --no-wait (non-Windows), copy "network".

import type { NetAdapterInfo } from './types';

export const sampleNetwork: NetAdapterInfo[] = [
  {
    "connected": true,
    "description": "Realtek Gaming 2.5GbE Family Controller",
    "dhcp": true,
    "dns": [
      "192.168.1.1",
      "1.1.1.1"
    ],
    "gateways": [
      "192.168.1.1"
    ],
    "id": "if12",
    "ipv4": [
      "192.168.1.24/24"
    ],
    "ipv6": [],
    "linkMbps": 1000.0,
    "mac": null,
    "mtu": 1500,
    "name": "Ethernet",
    "primary": true,
    "type": "ethernet",
    "virtualAdapter": false,
    "wifi": null
  },
  {
    "connected": true,
    "description": "MediaTek Wi-Fi 6E MT7922 160MHz Wireless LAN Card",
    "dhcp": true,
    "dns": [
      "192.168.1.1"
    ],
    "gateways": [
      "192.168.1.1"
    ],
    "id": "if18",
    "ipv4": [
      "192.168.1.31/24"
    ],
    "ipv6": [],
    "linkMbps": 1201.0,
    "mac": null,
    "mtu": 1500,
    "name": "Wi-Fi",
    "primary": false,
    "type": "wifi",
    "virtualAdapter": false,
    "wifi": {
      "band": "5 GHz",
      "bssid": null,
      "channel": 36,
      "frequencyMhz": 5180,
      "note": null,
      "rssiDbm": -52.0,
      "rxMbps": 1201.0,
      "security": "WPA3-Personal",
      "signalPct": 86.0,
      "ssid": null,
      "standard": "Wi-Fi 6 (802.11ax)",
      "txMbps": 960.0
    }
  }
];

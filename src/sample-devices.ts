// Generated from vyro_core::devices::sample() (raw containers run through the real build).
// Regenerate: cargo run -p vyro-core --example inventory -- --no-wait (non-Windows), copy "devices".

import type { Devices } from './types';

export const sampleDevices: Devices = {
  "audio": [
    {
      "default": true,
      "device": "7c1c97df17c0",
      "id": "8b5cc4df7eec",
      "name": "Headphones (Arctis Nova 7)",
      "output": true,
      "plugged": true
    },
    {
      "default": false,
      "device": "0012a3fa000c",
      "id": "cef7fc13a381",
      "name": "Headphones (Galaxy Buds2 Pro)",
      "output": true,
      "plugged": true
    },
    {
      "default": false,
      "device": null,
      "id": "449777124b14",
      "name": "Speakers (Realtek(R) Audio)",
      "output": true,
      "plugged": true
    },
    {
      "default": false,
      "device": null,
      "id": "43700797e2f9",
      "name": "Front headphones (Realtek(R) Audio)",
      "output": true,
      "plugged": false
    },
    {
      "default": true,
      "device": "7c1c97df17c0",
      "id": "ac0f09c0f8bf",
      "name": "Microphone (Arctis Nova 7)",
      "output": false,
      "plugged": true
    }
  ],
  "devices": [
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "9c0abe51c6e6",
      "kind": "keyboard",
      "manufacturer": "Razer",
      "name": "BlackWidow V3",
      "problem": null,
      "wireless": false
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "d0f631ca1ddb",
      "kind": "mouse",
      "manufacturer": "Logitech",
      "name": "G502 HERO Gaming Mouse",
      "problem": null,
      "wireless": false
    },
    {
      "battery": {
        "level": null,
        "percent": 17,
        "source": "bluetoothLe"
      },
      "connected": true,
      "connection": "bluetoothLe",
      "id": "7ed6a8377b92",
      "kind": "mouse",
      "manufacturer": "Logitech",
      "name": "MX Master 3S",
      "problem": null,
      "wireless": true
    },
    {
      "battery": {
        "level": "medium",
        "percent": null,
        "source": "xinput"
      },
      "connected": true,
      "connection": "bluetoothLe",
      "id": "d0bf3e6ee1d6",
      "kind": "controller",
      "manufacturer": "Microsoft",
      "name": "Xbox Wireless Controller",
      "problem": null,
      "wireless": true
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "f28d5b0d6f8b",
      "kind": "receiver",
      "manufacturer": "Logitech",
      "name": "USB Receiver",
      "problem": null,
      "wireless": true
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "7c1c97df17c0",
      "kind": "headset",
      "manufacturer": "SteelSeries",
      "name": "Arctis Nova 7",
      "problem": null,
      "wireless": false
    },
    {
      "battery": {
        "level": null,
        "percent": 64,
        "source": "windows"
      },
      "connected": true,
      "connection": "bluetooth",
      "id": "0012a3fa000c",
      "kind": "headphones",
      "manufacturer": "Samsung",
      "name": "Galaxy Buds2 Pro",
      "problem": null,
      "wireless": true
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "95144b44f2a5",
      "kind": "other",
      "manufacturer": null,
      "name": "AURA LED Controller",
      "problem": null,
      "wireless": false
    },
    {
      "battery": null,
      "connected": false,
      "connection": "bluetoothLe",
      "id": "6db53c9d5a2c",
      "kind": "keyboard",
      "manufacturer": "Logitech",
      "name": "MX Keys",
      "problem": null,
      "wireless": true
    }
  ]
};

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
      "device": "f4baf901d1b9",
      "id": "f33422b95e3b",
      "name": "Microphone (USB Audio Condenser Mic)",
      "output": false,
      "plugged": true
    },
    {
      "default": false,
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
      "parent": null,
      "problem": null,
      "wireless": false
    },
    {
      "battery": {
        "charging": true,
        "level": null,
        "percent": 46,
        "source": "logitech"
      },
      "connected": true,
      "connection": "receiver",
      "id": "58f37e68fb59",
      "kind": "keyboard",
      "manufacturer": "Logitech",
      "name": "G915 TKL",
      "parent": "f28d5b0d6f8b",
      "problem": null,
      "wireless": true
    },
    {
      "battery": {
        "charging": false,
        "level": null,
        "percent": 82,
        "source": "logitech"
      },
      "connected": true,
      "connection": "receiver",
      "id": "f2e4491be8a4",
      "kind": "mouse",
      "manufacturer": "Logitech",
      "name": "G Pro X Superlight 2",
      "parent": "f28d5b0d6f8b",
      "problem": null,
      "wireless": true
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "d0f631ca1ddb",
      "kind": "mouse",
      "manufacturer": "Logitech",
      "name": "G502 HERO Gaming Mouse",
      "parent": null,
      "problem": null,
      "wireless": false
    },
    {
      "battery": {
        "charging": null,
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
      "parent": null,
      "problem": null,
      "wireless": true
    },
    {
      "battery": {
        "charging": true,
        "level": null,
        "percent": 75,
        "source": "playStation"
      },
      "connected": true,
      "connection": "usb",
      "id": "1f311134efe1",
      "kind": "controller",
      "manufacturer": "Sony",
      "name": "DualSense Wireless Controller",
      "parent": null,
      "problem": null,
      "wireless": true
    },
    {
      "battery": {
        "charging": null,
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
      "parent": null,
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
      "parent": null,
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
      "parent": null,
      "problem": null,
      "wireless": false
    },
    {
      "battery": {
        "charging": null,
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
      "parent": null,
      "problem": null,
      "wireless": true
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "f4baf901d1b9",
      "kind": "microphone",
      "manufacturer": null,
      "name": "USB Audio Condenser Mic",
      "parent": null,
      "problem": null,
      "wireless": false
    },
    {
      "battery": null,
      "connected": true,
      "connection": "usb",
      "id": "95144b44f2a5",
      "kind": "other",
      "manufacturer": null,
      "name": "AURA LED Controller",
      "parent": null,
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
      "parent": null,
      "problem": null,
      "wireless": true
    }
  ]
};

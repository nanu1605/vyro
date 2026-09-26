// Generated from vyro_core::storage::sample() (synthetic NVMe + SATA sectors run through the real
// decoders). Regenerate: cargo run -p vyro-core --example inventory -- --no-wait (non-Windows),
// copy "drives".

import type { DriveInfo } from './types';

export const sampleDrives: DriveInfo[] = [
  {
    "bus": "NVMe",
    "capacityMB": 953869.7109375,
    "firmware": "5B2QGXA7",
    "freeMB": 512430.0,
    "health": "ok",
    "healthNotes": [],
    "healthPct": 98.0,
    "id": "disk0",
    "index": 0,
    "interface": "PCIe NVMe 1.3",
    "model": "Samsung SSD 980 PRO 1TB",
    "nvme": {
      "availableSparePct": 100,
      "busyMinutes": 1234.0,
      "criticalTempMinutes": 0,
      "criticalWarning": 0,
      "criticalWarnings": [],
      "dataUnitsRead": 36210345.0,
      "dataUnitsWritten": 29870112.0,
      "errorLogEntries": 1520.0,
      "hostReads": 412345678.0,
      "hostWrites": 523456789.0,
      "mediaErrors": 0.0,
      "percentageUsed": 2,
      "sensorsC": [
        41.0,
        47.0,
        null,
        null,
        null,
        null,
        null,
        null
      ],
      "spareThresholdPct": 10,
      "throttleEvents": [
        3,
        0
      ],
      "throttleSecs": [
        90,
        0
      ],
      "unsafeShutdowns": 37.0,
      "warningTempMinutes": 0
    },
    "powerCycles": 812.0,
    "powerOnHours": 3456.0,
    "removable": false,
    "rpm": null,
    "serial": null,
    "smart": [],
    "smartError": null,
    "ssd": true,
    "system": true,
    "tempC": 41.0,
    "tempCritC": 85.0,
    "tempWarnC": 82.0,
    "totalReadGB": 18539.69664,
    "totalWriteGB": 15293.497344,
    "trim": null,
    "type": "NVMe",
    "usedMB": 440820.0,
    "vendor": "Samsung",
    "volumes": [
      {
        "fileSystem": "NTFS",
        "freeMB": 512430.0,
        "label": "Windows",
        "letter": "C:",
        "totalMB": 953250.0
      }
    ]
  },
  {
    "bus": "SATA",
    "capacityMB": 1907729.0859375,
    "firmware": "0001",
    "freeMB": 1021880.0,
    "health": "warn",
    "healthNotes": [
      "8 reallocated sectors"
    ],
    "healthPct": null,
    "id": "disk1",
    "index": 1,
    "interface": "SATA 6.0 Gb/s (running at 3.0 Gb/s)",
    "model": "ST2000DM008-2UB102",
    "nvme": null,
    "powerCycles": 1530.0,
    "powerOnHours": 13210.0,
    "removable": false,
    "rpm": 7200,
    "serial": null,
    "smart": [
      {
        "current": 100,
        "failing": false,
        "id": 5,
        "name": "Reallocated Sectors Count",
        "prefail": true,
        "raw": 8.0,
        "rawHex": "000000000008",
        "threshold": 10,
        "worst": 100
      },
      {
        "current": 81,
        "failing": false,
        "id": 1,
        "name": "Read Error Rate",
        "prefail": true,
        "raw": 123456789.0,
        "rawHex": "0000075BCD15",
        "threshold": 6,
        "worst": 64
      },
      {
        "current": 97,
        "failing": false,
        "id": 3,
        "name": "Spin-Up Time",
        "prefail": true,
        "raw": 0.0,
        "rawHex": "000000000000",
        "threshold": 0,
        "worst": 97
      },
      {
        "current": 100,
        "failing": false,
        "id": 4,
        "name": "Start/Stop Count",
        "prefail": false,
        "raw": 1532.0,
        "rawHex": "0000000005FC",
        "threshold": 20,
        "worst": 100
      },
      {
        "current": 85,
        "failing": false,
        "id": 9,
        "name": "Power-On Hours",
        "prefail": false,
        "raw": 13210.0,
        "rawHex": "00000000339A",
        "threshold": 0,
        "worst": 85
      },
      {
        "current": 100,
        "failing": false,
        "id": 12,
        "name": "Power Cycle Count",
        "prefail": false,
        "raw": 1530.0,
        "rawHex": "0000000005FA",
        "threshold": 20,
        "worst": 100
      },
      {
        "current": 66,
        "failing": false,
        "id": 190,
        "name": "Airflow Temperature",
        "prefail": false,
        "raw": 128850919458.0,
        "rawHex": "001E001D0022",
        "threshold": 45,
        "worst": 55
      },
      {
        "current": 34,
        "failing": false,
        "id": 194,
        "name": "Temperature",
        "prefail": false,
        "raw": 1179682.0,
        "rawHex": "000000120022",
        "threshold": 0,
        "worst": 45
      },
      {
        "current": 100,
        "failing": false,
        "id": 197,
        "name": "Current Pending Sector Count",
        "prefail": false,
        "raw": 0.0,
        "rawHex": "000000000000",
        "threshold": 0,
        "worst": 100
      },
      {
        "current": 100,
        "failing": false,
        "id": 198,
        "name": "Uncorrectable Sector Count",
        "prefail": false,
        "raw": 0.0,
        "rawHex": "000000000000",
        "threshold": 0,
        "worst": 100
      }
    ],
    "smartError": null,
    "ssd": false,
    "system": false,
    "tempC": 34.0,
    "tempCritC": null,
    "tempWarnC": null,
    "totalReadGB": null,
    "totalWriteGB": null,
    "trim": false,
    "type": "SATA",
    "usedMB": 885832.0,
    "vendor": "Seagate",
    "volumes": [
      {
        "fileSystem": "NTFS",
        "freeMB": 1021880.0,
        "label": "Data",
        "letter": "D:",
        "totalMB": 1907712.0
      }
    ]
  }
];

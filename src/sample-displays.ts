// Generated from vyro_core::display::sample() (two EDID fixtures run through the real merge).
// Regenerate: cargo run -p vyro-core --example inventory -- --no-wait (non-Windows), copy "displays".

import type { DisplayInfo } from './types';

export const sampleDisplays: DisplayInfo[] = [
  {
    "adapter": "Simulated GPU",
    "bitsPerColor": 10,
    "brightnessPct": null,
    "colorEncoding": "RGB",
    "colorSpace": "sRGB",
    "connection": "DisplayPort",
    "diagonalIn": 27.0,
    "dpi": 96,
    "edid": {
      "bitsPerColor": 10,
      "checksumOk": true,
      "extensions": [
        "CTA-861 r3"
      ],
      "gamma": 2.2,
      "hex": "00ffffffffffff001e6d7f5b00000000031f0104b53c22789f8cb5af4f43ab260e5054254b007140818081c0a9c0b300d1c08100d1cf09ec00a0a0a0675030203a0055502100001a000000fd003090e6e63c010a202020202020000000fc004c4720554c545241474541520a000000ff000a20202020202020202020202001aa02031a7123090607e305c000e606050160592846100403011f13565e00a0a0a029503020350055502100001a5aa000a0a0a0465030203a005550210000006fc200a0a0a0555030203a0055502100001a0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000da",
      "interface": "DisplayPort",
      "rangeHz": [
        48,
        144
      ],
      "version": "1.4"
    },
    "gdiName": "\\\\.\\DISPLAY1",
    "hdr": {
      "enabled": false,
      "eotfs": [
        "SDR",
        "PQ (ST 2084)"
      ],
      "gamuts": [
        "BT.2020"
      ],
      "maxFullFrameNits": 343.7,
      "maxNits": 400.0,
      "minNits": 0.0984,
      "sdrWhiteNits": null,
      "supported": true
    },
    "height": 1440,
    "heightMM": 336,
    "id": "sim:1",
    "internal": false,
    "manufactured": "Week 3, 2021",
    "manufacturer": "LG Electronics",
    "manufacturerId": "GSM",
    "maxRefreshHz": 143.97,
    "model": "LG ULTRAGEAR",
    "name": "LG ULTRAGEAR",
    "nativeHeight": 1440,
    "nativeWidth": 2560,
    "orientation": 0,
    "primary": true,
    "productCode": "5B7F",
    "rect": [
      0,
      0,
      2560,
      1440
    ],
    "refreshHz": 143.97,
    "scalePct": 100,
    "serial": null,
    "vrr": {
      "active": null,
      "enabled": true,
      "maxHz": 144.0,
      "minHz": 48.0,
      "source": "NVAPI",
      "supported": true,
      "technology": "G-SYNC Compatible"
    },
    "width": 2560,
    "widthMM": 597
  },
  {
    "adapter": "Simulated GPU",
    "bitsPerColor": 10,
    "brightnessPct": 70,
    "colorEncoding": "RGB",
    "colorSpace": "BT.2100 PQ (HDR10)",
    "connection": "HDMI",
    "diagonalIn": 31.5,
    "dpi": 144,
    "edid": {
      "bitsPerColor": null,
      "checksumOk": true,
      "extensions": [
        "CTA-861 r3",
        "DisplayID 1.2"
      ],
      "gamma": 2.2,
      "hex": "00ffffffffffff0005e309a400000000011e0103804627782ee175ad5045a0250d5054bfef00d1c081803168317c4568457c6168617c4dd000a0f0703e8030403500b9882100001a000000ff000a202020202020202020202020000000fc004147333234555753345234420a000000fd0030901eff8c000a202020202020023a020355f14f9004031f1301125d5e5f60613f757623090707830100006d030c001000383c2000600102036dd85dc401788033003078c3350ce305e301e30f006ce60607016262006d1a000002033090e605623100316fc200a0a0a0555030203500b9882100001e4d6c80a070703e8030203a00b9882100001a000000000000ee7012790000030164f2070204ff0e3f015f807f006f083d0002000400a4a00184ff0e9f002f803f006f083d0002000400305b0104ff0e9f002f803f006f083d00020004001b270104ff0e9f002f803f006f083d000200040064040104ff0e9f002f803f006f083d00020004000000000000000000000000000000000000004c90",
      "interface": null,
      "rangeHz": [
        48,
        144
      ],
      "version": "1.3"
    },
    "gdiName": "\\\\.\\DISPLAY2",
    "hdr": {
      "enabled": true,
      "eotfs": [
        "SDR",
        "HDR (gamma)",
        "PQ (ST 2084)"
      ],
      "gamuts": [
        "BT.2020"
      ],
      "maxFullFrameNits": 417.7,
      "maxNits": 417.7,
      "minNits": null,
      "sdrWhiteNits": 200.0,
      "supported": true
    },
    "height": 2160,
    "heightMM": 392,
    "id": "sim:2",
    "internal": false,
    "manufactured": "Week 1, 2020",
    "manufacturer": "AOC",
    "manufacturerId": "AOC",
    "maxRefreshHz": 144.0,
    "model": "AG324UWS4R4B",
    "name": "AG324UWS4R4B",
    "nativeHeight": 2160,
    "nativeWidth": 3840,
    "orientation": 0,
    "primary": false,
    "productCode": "A409",
    "rect": [
      2560,
      0,
      3840,
      2160
    ],
    "refreshHz": 60.0,
    "scalePct": 150,
    "serial": null,
    "vrr": {
      "active": null,
      "enabled": true,
      "maxHz": 120.0,
      "minHz": 48.0,
      "source": "NVAPI",
      "supported": true,
      "technology": "G-SYNC Compatible"
    },
    "width": 3840,
    "widthMM": 697
  }
];

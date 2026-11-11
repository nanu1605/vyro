// The app's icons and installer pictures (2.2), drawn by Chromium from the brand artwork in
// assets/brand (the owner's, 2026-11-11):
//
//     node scripts/icons.mjs
//
// - src-tauri/icons: icon.ico (16 to 32 px from the small icon, 48 px up from the icon as drawn;
//   its first picture is the 32 px one, which Tauri uses for the window and the tray), 32x32.png,
//   128x128.png, 128x128@2x.png, icon.png and notification.png (64 px, from the small icon);
// - src/phone/public/icons: the phone app's icons. icon-192/512 keep the icon's rounded tile; the
//   maskable and Apple ones are full-bleed (the phone rounds or masks them), with the mark inside
//   the safe zone. icon.svg is the icon as drawn, icon-small.svg the browser tab's;
// - src-tauri/wix: banner.bmp (493 x 58) and dialog.bmp (493 x 312), the installer's pictures:
//   white where Windows Installer writes its black text.
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { chromium } from 'playwright';

const BRAND = 'assets/brand';
const ICON = readFileSync(`${BRAND}/clokrix-icon.svg`, 'utf8');
const SMALL = readFileSync(`${BRAND}/clokrix-icon-small.svg`, 'utf8');
/** The icon's tile reaching the corners: the system rounds or masks it. */
const BLEED = ICON.replace(/<rect width="512" height="512" rx="96" /, '<rect width="512" height="512" ');
if (BLEED === ICON) throw new Error("clokrix-icon.svg: the icon's tile was not found");

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });

/** Draws `layers` ({svg, x, y, w, h} or {fill, x, y, w, h}) on a w x h canvas: PNG and RGBA. */
async function draw(w, h, layers) {
  const out = await page.evaluate(
    async ({ w, h, layers }) => {
      const c = document.createElement('canvas');
      c.width = w;
      c.height = h;
      const ctx = c.getContext('2d');
      for (const l of layers) {
        if (l.fill) {
          ctx.fillStyle = l.fill;
          ctx.fillRect(l.x, l.y, l.w, l.h);
          continue;
        }
        // Sized in the file itself, so Chromium draws the vectors at that size.
        const sized = l.svg.replace('<svg ', `<svg width="${l.w}" height="${l.h}" `);
        const img = new Image();
        img.src = `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(sized)))}`;
        await img.decode();
        ctx.drawImage(img, l.x, l.y, l.w, l.h);
      }
      return { png: c.toDataURL('image/png').split(',')[1], rgba: Array.from(ctx.getImageData(0, 0, w, h).data) };
    },
    { w, h, layers },
  );
  return { png: Buffer.from(out.png, 'base64'), rgba: out.rgba };
}

const icon = (svg, size) => draw(size, size, [{ svg, x: 0, y: 0, w: size, h: size }]);

/** An .ico of PNG pictures, in the order given (Tauri takes the first for the window). */
function ico(frames) {
  const head = Buffer.alloc(6 + 16 * frames.length);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(frames.length, 4);
  let at = head.length;
  frames.forEach(({ size, png }, i) => {
    const e = 6 + 16 * i;
    head.writeUInt8(size >= 256 ? 0 : size, e);
    head.writeUInt8(size >= 256 ? 0 : size, e + 1);
    head.writeUInt16LE(1, e + 4); // planes
    head.writeUInt16LE(32, e + 6); // bits per pixel
    head.writeUInt32LE(png.length, e + 8);
    head.writeUInt32LE(at, e + 12);
    at += png.length;
  });
  return Buffer.concat([head, ...frames.map((f) => f.png)]);
}

/** A 24-bit .bmp (bottom-up rows, padded to 4 bytes), as Windows Installer's dialogs take. */
function bmp(w, h, rgba) {
  const row = Math.ceil((w * 3) / 4) * 4;
  const b = Buffer.alloc(54 + row * h);
  b.write('BM', 0, 'ascii');
  b.writeUInt32LE(b.length, 2);
  b.writeUInt32LE(54, 10);
  b.writeUInt32LE(40, 14);
  b.writeInt32LE(w, 18);
  b.writeInt32LE(h, 22);
  b.writeUInt16LE(1, 26);
  b.writeUInt16LE(24, 28);
  b.writeUInt32LE(row * h, 34);
  b.writeInt32LE(2835, 38); // 72 dpi
  b.writeInt32LE(2835, 42);
  for (let y = 0; y < h; y++) {
    const src = (h - 1 - y) * w * 4;
    const dst = 54 + y * row;
    for (let x = 0; x < w; x++) {
      b[dst + x * 3] = rgba[src + x * 4 + 2];
      b[dst + x * 3 + 1] = rgba[src + x * 4 + 1];
      b[dst + x * 3 + 2] = rgba[src + x * 4];
    }
  }
  return b;
}

// The desktop app.
const frames = [];
for (const size of [32, 16, 20, 24]) frames.push({ size, png: (await icon(SMALL, size)).png });
for (const size of [48, 64, 256]) frames.push({ size, png: (await icon(ICON, size)).png });
writeFileSync('src-tauri/icons/icon.ico', ico(frames));
writeFileSync('src-tauri/icons/32x32.png', frames[0].png);
writeFileSync('src-tauri/icons/128x128.png', (await icon(ICON, 128)).png);
writeFileSync('src-tauri/icons/128x128@2x.png', (await icon(ICON, 256)).png);
writeFileSync('src-tauri/icons/icon.png', (await icon(ICON, 512)).png);
// Notifications' app icon (Windows shows it at 16 to 48 px): the small icon, installed beside the app.
writeFileSync('src-tauri/icons/notification.png', (await icon(SMALL, 64)).png);

// The phone app.
const PHONE = 'src/phone/public/icons';
writeFileSync(`${PHONE}/icon-192.png`, (await icon(ICON, 192)).png);
writeFileSync(`${PHONE}/icon-512.png`, (await icon(ICON, 512)).png);
writeFileSync(`${PHONE}/icon-maskable-512.png`, (await icon(BLEED, 512)).png);
writeFileSync(`${PHONE}/apple-touch-icon.png`, (await icon(BLEED, 180)).png);
copyFileSync(`${BRAND}/clokrix-icon.svg`, `${PHONE}/icon.svg`);
copyFileSync(`${BRAND}/clokrix-icon-small.svg`, `${PHONE}/icon-small.svg`);

// The installer: the banner on every page but the first and last, the dialog picture on those
// two (Windows Installer writes the title from 180 px across, so the logo keeps to the left).
const DARK = '#0B0B16';
const banner = await draw(493, 58, [
  { fill: '#FFFFFF', x: 0, y: 0, w: 493, h: 58 },
  { svg: SMALL, x: 441, y: 7, w: 44, h: 44 },
]);
writeFileSync('src-tauri/wix/banner.bmp', bmp(493, 58, banner.rgba));
// The mark alone (no tile or scan lines), as large as fits the dark panel with a margin.
const MARK = ICON.replace(/ *<rect width="512" height="512"[^>]*\/>\n/, '').replace(/ *<rect x="0" y="\d+" width="512" height="1"[^>]*\/>\n/g, '');
const { rgba: ink } = await icon(MARK, 512);
let [x0, y0, x1, y1] = [512, 512, 0, 0];
for (let i = 3; i < ink.length; i += 4) {
  if (ink[i] < 8) continue;
  const p = (i - 3) / 4;
  const [x, y] = [p % 512, Math.floor(p / 512)];
  [x0, y0, x1, y1] = [Math.min(x0, x), Math.min(y0, y), Math.max(x1, x), Math.max(y1, y)];
}
const PANEL = 164;
const scale = Math.min((PANEL - 44) / (x1 - x0 + 1), (312 - 44) / (y1 - y0 + 1));
const size = Math.round(512 * scale);
const dialog = await draw(493, 312, [
  { fill: '#FFFFFF', x: 0, y: 0, w: 493, h: 312 },
  { fill: DARK, x: 0, y: 0, w: PANEL, h: 312 },
  {
    svg: MARK,
    x: Math.round(PANEL / 2 - ((x0 + x1 + 1) / 2) * scale),
    y: Math.round(312 / 2 - ((y0 + y1 + 1) / 2) * scale),
    w: size,
    h: size,
  },
]);
writeFileSync('src-tauri/wix/dialog.bmp', bmp(493, 312, dialog.rgba));

await browser.close();
console.log('icons written');

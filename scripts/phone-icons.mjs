// The phone app's icons (1.4), drawn from the app icon (src-tauri/icons/icon.svg) by Chromium:
//
//     node scripts/phone-icons.mjs src-tauri/icons/icon.svg src/phone/public/icons
//
// icon-192/512 keep the icon's rounded square; the maskable and Apple ones are full-bleed (the
// phone rounds or masks them), with the mark inside the safe zone.
import { readFileSync } from 'node:fs';
import { chromium } from 'playwright';

const [svgPath, out] = process.argv.slice(2);
const svg = readFileSync(svgPath, 'utf8');
const data = `data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}`;
// Full-bleed: the background square covers the whole icon (the system rounds or masks it).
const bleed = svg.replace(/<rect [^>]*\/>/, '<rect x="0" y="0" width="1024" height="1024" fill="url(#bg)"/>');
const bleedData = `data:image/svg+xml;base64,${Buffer.from(bleed).toString('base64')}`;
const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
async function render(file, size, full) {
  // full: a square with no transparent corners (maskable, Apple touch): the icon's own
  // background fills it, and the mark sits inside the safe zone (80%).
  const html = `<html><body style="margin:0;width:${size}px;height:${size}px;background:transparent">
    <img src="${full ? bleedData : data}" width="${size}" height="${size}" style="display:block"></body></html>`;
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(html);
  await page.waitForFunction(() => document.images[0]?.complete);
  await page.screenshot({ path: `${out}/${file}`, omitBackground: !full });
}
await render('icon-192.png', 192, false);
await render('icon-512.png', 512, false);
await render('icon-maskable-512.png', 512, true);
await render('apple-touch-icon.png', 180, true);
await browser.close();

# Clokrix brand

The owner's artwork (2026-11-11), with only the editor's data taken out:

- `clokrix-icon.svg`: the app icon (512 × 512).
- `clokrix-logo.svg`: the logo for dark backgrounds (640 × 200).
- `clokrix-logo-light.svg`: the logo for light backgrounds.

And, from the icon and the logo:

- `clokrix-icon-small.svg`: the icon at 16–32 px (tray, title bar, taskbar, browser tab), with the
  X drawn to fill the tile and no pixel scatter.
- `clokrix-social.svg` and `clokrix-social.png` (2.3): the repository's social preview, 1280 × 640,
  the logo with the tagline. GitHub takes it only by hand: Settings → General → Social preview →
  Edit → Upload an image, with the PNG.

The name, logo and icon are the owner's and are not covered by the code's licences.

`node scripts/icons.mjs` makes every icon and installer picture from these (`src-tauri/icons`,
`src/phone/public/icons`, `src-tauri/wix`). The app draws the logo itself from `src/brand.ts`, the
same shapes.

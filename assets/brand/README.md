# Clokrix brand

The owner's artwork (2026-11-11), with only the editor's data taken out:

- `clokrix-icon.svg`: the app icon (512 × 512).
- `clokrix-logo.svg`: the logo for dark backgrounds (640 × 200).
- `clokrix-logo-light.svg`: the logo for light backgrounds.

And, from the icon:

- `clokrix-icon-small.svg`: the icon at 16–32 px (tray, title bar, taskbar, browser tab), with the
  X drawn to fill the tile and no pixel scatter.

`node scripts/icons.mjs` makes every icon and installer picture from these (`src-tauri/icons`,
`src/phone/public/icons`, `src-tauri/wix`). The app draws the logo itself from `src/brand.ts`, the
same shapes.

/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port and serves `dist/` in production.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**', '**/crates/**', '**/target/**'] },
  },
  build: {
    target: 'es2022',
    sourcemap: false,
    // The 3-D view's chunk (Three.js, ~570 kB) loads only with the My PC tab (P23).
    chunkSizeWarningLimit: 600,
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});

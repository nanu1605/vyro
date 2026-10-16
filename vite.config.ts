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
    // The remote companion page (/remote.html) against `cargo run -p vyro-core --example companion`.
    proxy: { '/api': 'http://127.0.0.1:8976' },
  },
  build: {
    target: 'es2022',
    sourcemap: false,
    // The 3-D view's chunk (Three.js, ~570 kB) loads only with the My PC tab (P23).
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      // The app, and the remote companion's page (P26), which the app serves to paired phones.
      input: { main: 'index.html', remote: 'remote.html' },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});

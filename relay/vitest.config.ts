import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['test/**/*.test.ts'],
    environment: 'node',
    // The relay tests start the Worker in workerd once per file.
    testTimeout: 30_000,
    hookTimeout: 60_000,
  },
});

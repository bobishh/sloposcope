import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    passWithNoTests: true,
    exclude: ['e2e/**', 'node_modules/**', 'dist/**'],
  },
});

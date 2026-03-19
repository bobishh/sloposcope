import { defineConfig, devices } from '@playwright/test';

const previewPort = Number(process.env.SLOPOSCOPE_PLAYWRIGHT_PORT || '4173');
const previewBaseUrl = `http://127.0.0.1:${previewPort}`;

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : 1,
  reporter: 'list',
  use: {
    baseURL: previewBaseUrl,
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: `npm run build && npx vite preview --host 127.0.0.1 --port ${previewPort} --strictPort`,
    url: previewBaseUrl,
    reuseExistingServer: false,
    timeout: 180000,
  },
});

import { existsSync } from 'node:fs'
import { defineConfig } from '@playwright/test'

// Some CI containers ship a Chromium build that may not match this Playwright version,
// so we launch it by path. On Windows we use Edge, which shares its engine with WebView2.
// Anywhere else, run `npx playwright install chromium` once.
const containerChromium = '/opt/pw-browsers/chromium'
const browser = existsSync(containerChromium)
  ? { launchOptions: { executablePath: containerChromium } }
  : process.platform === 'win32'
    ? { channel: 'msedge' }
    : {}

export default defineConfig({
  testDir: 'e2e',
  outputDir: 'test-results',
  fullyParallel: true,
  // On a cold cache, Vite can reload the page mid-test when it finds a new dependency to bundle.
  // Playwright still reports a test that passed on retry as flaky.
  retries: 1,
  reporter: [['list']],
  use: {
    ...browser,
    baseURL: 'http://localhost:5173',
    viewport: { width: 1280, height: 820 },
  },
  projects: [
    { name: 'smoke', testMatch: 'smoke.spec.ts' },
    { name: 'shots', testMatch: 'shots.spec.ts' },
  ],
  // We reuse a dev server you already have running, including the one `npm run tauri dev` starts.
  webServer: {
    command: 'npm run dev -- --port 5173 --strictPort',
    url: 'http://localhost:5173',
    reuseExistingServer: true,
  },
})

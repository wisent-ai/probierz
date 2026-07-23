import { defineConfig } from '@playwright/test';
import { join } from 'node:path';

// Recording toggle (PROBIERZ_RECORD=1|true). For Electron the reliable
// artifacts are trace + screenshot; Playwright video capture is a browser-
// context feature and does not attach to Electron windows, so it is web/
// mobile-primary. The analyzer inventories whatever media a run produces.
const record = process.env.PROBIERZ_RECORD === '1' || process.env.PROBIERZ_RECORD === 'true';
// PROBIERZ_SPEC scopes the run to one spec (matched by basename anywhere).
const testMatch = process.env.PROBIERZ_SPEC ? `**/${process.env.PROBIERZ_SPEC.split('/').pop()}` : undefined;
const artifactsDir = process.env.PROBIERZ_ARTIFACTS || 'test-results';

export default defineConfig({
  testDir: './tests',
  outputDir: join(artifactsDir, 'playwright'),
  testMatch,
  fullyParallel: false,
  workers: 1,
  reporter: [
    ['html', { open: 'never', outputFolder: join(artifactsDir, 'html-report') }],
    ['list'],
    ['../../agent/playwright-reporter.mjs'],
  ],
  use: {
    trace: record ? 'on' : 'on-first-retry',
    screenshot: record ? 'on' : 'only-on-failure',
  },
});

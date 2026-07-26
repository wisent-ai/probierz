import { defineConfig, devices } from '@playwright/test';
import { join } from 'node:path';

/**
 * Web E2E config. Set BASE_URL to point at the site under test.
 * Runs across Chromium, Firefox and WebKit, plus emulated mobile browsers.
 */
const record = process.env.PROBIERZ_RECORD === '1' || process.env.PROBIERZ_RECORD === 'true';
const cs = process.env.PROBIERZ_COLOR_SCHEME;
const colorScheme = cs === 'dark' ? 'dark' : cs === 'light' ? 'light' : undefined;
// PROBIERZ_SPEC scopes the run to one spec (matched by basename anywhere).
const testMatch = process.env.PROBIERZ_SPEC ? `**/${process.env.PROBIERZ_SPEC.split('/').pop()}` : undefined;
const artifactsDir = process.env.PROBIERZ_ARTIFACTS || 'test-results';

// jeden/omp comparison specs drive real TUIs through tmux; the browser is
// only a deterministic monospace renderer for golden PNGs. Running them once
// instead of once per engine keeps five live app instances (and five
// concurrent control-plane logins) off the machine.
const JEDEN_SPECS = /jeden\.[a-z-]+\.spec\.ts$/;
const JEDEN_MATRIX = /jeden\.matrix\.spec\.ts$/;

export default defineConfig({
  testDir: './tests',
  outputDir: join(artifactsDir, 'playwright'),
  testMatch,
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: [
    ['html', { open: 'never', outputFolder: join(artifactsDir, 'html-report') }],
    ['list'],
    ['../../agent/playwright-reporter.mjs'],
  ],
  use: {
    baseURL: process.env.BASE_URL || 'https://playwright.dev',
    trace: record ? 'on' : 'on-first-retry',
    screenshot: record ? 'on' : 'only-on-failure',
    video: record ? 'on' : 'retain-on-failure',
    locale: process.env.PROBIERZ_LOCALE || undefined,
    colorScheme,
  },
  globalSetup: './harness/reset-checks.ts',
  globalTeardown: './harness/verdict-report.ts',
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] }, testIgnore: JEDEN_SPECS },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] }, testIgnore: JEDEN_SPECS },
    { name: 'webkit', use: { ...devices['Desktop Safari'] }, testIgnore: JEDEN_SPECS },
    { name: 'mobile-chrome', use: { ...devices['Pixel 7'] }, testIgnore: JEDEN_SPECS },
    { name: 'mobile-safari', use: { ...devices['iPhone 14'] }, testIgnore: JEDEN_SPECS },
    {
      name: 'jeden',
      use: { ...devices['Desktop Chrome'] },
      testMatch: JEDEN_SPECS,
      testIgnore: JEDEN_MATRIX,
      // Each test drives a real app instance against one control plane, so
      // tests inside a file run one at a time (files still run in parallel).
      fullyParallel: false,
    },
    // The matrix reads the check ledger the specs above write, so it must
    // observe a finished run — hence the project dependency, not file order.
    {
      name: 'jeden-matrix',
      use: { ...devices['Desktop Chrome'] },
      testMatch: JEDEN_MATRIX,
      dependencies: ['jeden'],
    },
  ],
});

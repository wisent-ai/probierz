import { defineConfig, devices } from '@playwright/test';

/**
 * Web E2E config. Set BASE_URL to point at the site under test.
 * Runs across Chromium, Firefox and WebKit, plus emulated mobile browsers.
 */
const record = process.env.PROBIERZ_RECORD === '1' || process.env.PROBIERZ_RECORD === 'true';
const cs = process.env.PROBIERZ_COLOR_SCHEME;
const colorScheme = cs === 'dark' ? 'dark' : cs === 'light' ? 'light' : undefined;

export default defineConfig({
  testDir: './tests',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: [['html', { open: 'never' }], ['list'], ['json', { outputFile: 'test-results/report.json' }]],
  use: {
    baseURL: process.env.BASE_URL || 'https://playwright.dev',
    trace: record ? 'on' : 'on-first-retry',
    screenshot: record ? 'on' : 'only-on-failure',
    video: record ? 'on' : 'retain-on-failure',
    locale: process.env.PROBIERZ_LOCALE || undefined,
    colorScheme,
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
    { name: 'mobile-chrome', use: { ...devices['Pixel 7'] } },
    { name: 'mobile-safari', use: { ...devices['iPhone 14'] } },
  ],
});

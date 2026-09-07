import { test, expect } from '@playwright/test';
import {
  chromiumPublicationOnly,
  clearProductStorage,
  journeyProgress,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

test('onboarding-first-use observes a real agent-economy dashboard', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  requireProductBaseUrl('ADAM_MONITOR_BASE_URL');

  await page.route('**/api/report', async (route) => {
    await route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"resume-boundary"}' });
  });
  await page.goto('/');
  await clearProductStorage(page, ['adam-monitor.onboarding']);
  await page.reload();

  const panel = page.locator('#onboarding-panel');
  await expect(panel).toBeVisible();
  await expect(page.locator('#onboarding-title')).toHaveText('What this monitor covers');
  await expect.poll(async () => (await journeyProgress(page, 'adam-monitor.onboarding')).status).toBe('in_progress');

  await page.locator('#onboarding-action').click();
  await page.reload();
  await expect(page.locator('#onboarding-title')).toHaveText('Read the live evidence');
  await expect(page.locator('#onboarding-action')).toBeDisabled();
  await expect.poll(async () => (await journeyProgress(page, 'adam-monitor.onboarding')).current_screen_id).toBe('live-evidence');
  await expect.poll(async () => (await journeyProgress(page, 'adam-monitor.onboarding')).status).toBe('in_progress');

  await page.unroute('**/api/report');
  const report = page.waitForResponse((response) => response.url().endsWith('/api/report') && response.ok());
  await page.reload();
  await report;
  await expect(page.locator('#platform-overview')).toContainText('Economy');
  await expect(page.locator('#platform-overview')).toContainText('Agents');
  await expect(page.locator('#onboarding-action')).toBeEnabled();

  await page.locator('#onboarding-action').click();
  await expect(panel).toBeHidden();
  await expect.poll(async () => (await journeyProgress(page, 'adam-monitor.onboarding')).status).toBe('completed');
});

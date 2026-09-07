import { test, expect } from '@playwright/test';
import {
  chromiumPublicationOnly,
  clearProductStorage,
  journeyProgress,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

test('onboarding-first-use observes a real agent-economy market result', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  requireProductBaseUrl('WISENT_TRADE_BASE_URL');

  await page.route('**/api/tokens', async (route) => {
    await route.fulfill({ status: 200, contentType: 'application/json', body: '[]' });
  });
  await page.goto('/');
  await clearProductStorage(page, ['wisent-trade.onboarding', 'wisent-trade.first-use.v1']);
  await page.reload();

  await expect(page.getByRole('heading', { name: 'How the agent economy moves' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'First-use journey complete' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Show me the live economy' }).click();
  await page.reload();

  await expect(page.getByRole('heading', { name: 'Observe a live economy result' })).toBeVisible();
  await expect(page.getByText('Waiting for the market to return its first listed agent token.')).toBeVisible();
  await expect.poll(async () => (await journeyProgress(page, 'wisent-trade.first-use.v1')).current_screen_id).toBe('observe-result');
  await expect.poll(async () => (await journeyProgress(page, 'wisent-trade.first-use.v1')).status).toBe('in_progress');

  await page.unroute('**/api/tokens');
  const tokens = page.waitForResponse((response) => response.url().endsWith('/api/tokens') && response.ok());
  await page.reload();
  await tokens;

  const completion = page.getByRole('region', { name: 'First-use journey complete' });
  await expect(completion).toBeVisible();
  await expect(completion).toContainText('Live agent economy observed');
  await expect(completion).toContainText('AGENT');
  await expect(page.locator('#live-economy')).toContainText('Live Tokens');
  await expect(page.locator('a[href^="/token/"]').first()).toBeVisible();
  await expect.poll(async () => (await journeyProgress(page, 'wisent-trade.first-use.v1')).status).toBe('completed');
});

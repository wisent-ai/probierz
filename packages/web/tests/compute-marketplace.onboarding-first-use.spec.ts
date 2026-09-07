import { test, expect } from '@playwright/test';
import {
  chromiumPublicationOnly,
  clearProductStorage,
  journeyProgress,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

test('onboarding-first-use observes a real authorized machine offer', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  requireProductBaseUrl('COMPUTE_MARKETPLACE_BASE_URL');

  await page.route('**/api/v1/offers*', async (route) => {
    await route.fulfill({ status: 200, contentType: 'application/json', body: '{"offers":[],"total":0}' });
  });
  await page.goto('/marketplace');
  await clearProductStorage(page, ['compute-marketplace.onboarding']);
  await page.reload();

  const journey = page.getByRole('heading', { name: 'Find authorized compute without losing control' });
  await expect(journey).toBeVisible();
  await page.getByRole('button', { name: 'How offers work' }).click();
  await page.reload();

  await expect(page.getByRole('heading', { name: 'An offer is not a running workload' })).toBeVisible();
  await expect.poll(async () => (await journeyProgress(page, 'compute-marketplace.onboarding')).current_screen_id).toBe('control_model');
  await expect.poll(async () => (await journeyProgress(page, 'compute-marketplace.onboarding')).status).toBe('in_progress');

  await page.getByRole('button', { name: 'Inspect live offers' }).click();
  await expect(page.getByRole('heading', { name: 'Inspect a real machine offer' })).toBeVisible();
  await expect(page.getByText('Waiting for a live offer from the marketplace…')).toBeVisible();
  await expect(page.getByText('You have reached a live machine offer')).toHaveCount(0);

  await page.unroute('**/api/v1/offers*');
  const offers = page.waitForResponse((response) => response.url().includes('/api/v1/offers') && response.ok());
  await page.reload();
  await offers;

  await expect(page.getByText('You have reached a live machine offer')).toBeVisible();
  const firstOffer = page.locator('#marketplace-offers tbody tr').first();
  await expect(firstOffer).toBeVisible();
  await expect(firstOffer).not.toContainText('No GPUs available');
  await expect(firstOffer.getByRole('button', { name: 'Rent' })).toBeVisible();
  await expect.poll(async () => (await journeyProgress(page, 'compute-marketplace.onboarding')).status).toBe('completed');
});

import { test, expect } from '@playwright/test';
import {
  chromiumPublicationOnly,
  clearProductStorage,
  journeyProgress,
  requireEnvironment,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

const authState = process.env.WELES_CONSOLE_STORAGE_STATE?.trim();
test.use({ storageState: authState || undefined });

test('onboarding-first-use inspects a real Weles workflow receipt', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  requireProductBaseUrl('WELES_CONSOLE_BASE_URL');
  requireEnvironment('WELES_CONSOLE_STORAGE_STATE');

  await page.goto('/');
  await clearProductStorage(page, ['weles-console.onboarding']);
  await page.reload();

  const journey = page.getByRole('region', { name: 'Weles first-use journey' });
  await expect(journey).toBeVisible();
  await expect(journey.getByRole('heading', { name: 'A queue row is a workflow promise' })).toBeVisible();
  await journey.getByRole('button', { name: 'Continue' }).click();
  await page.reload();

  await expect(journey.getByRole('heading', { name: 'A host claims and runs it' })).toBeVisible();
  await expect.poll(async () => (await journeyProgress(page, 'weles-console.onboarding.2026-08-04.1')).current_screen_id).toBe('host-model');
  await expect.poll(async () => (await journeyProgress(page, 'weles-console.onboarding.2026-08-04.1')).status).toBe('in_progress');

  await journey.getByRole('button', { name: 'Continue' }).click();
  const inspect = journey.getByRole('button', { name: 'Inspect latest receipt' });
  await expect(inspect).toBeEnabled();
  await expect(page.getByText('First-use journey complete: this real workflow receipt was opened and inspected.')).toHaveCount(0);

  await inspect.click();
  await expect(page).not.toHaveURL(/\/$/);
  await expect(page.getByRole('status')).toHaveText('First-use journey complete: this real workflow receipt was opened and inspected.');
  await expect.poll(async () => (await journeyProgress(page, 'weles-console.onboarding.2026-08-04.1')).status).toBe('completed');
});

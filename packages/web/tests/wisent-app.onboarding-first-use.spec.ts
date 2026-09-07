import { test, expect } from '@playwright/test';
import {
  chromiumPublicationOnly,
  clearProductStorage,
  journeyProgress,
  requireEnvironment,
  requireJsonObject,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

const authState = process.env.WISENT_APP_STORAGE_STATE?.trim();
test.use({ storageState: authState || undefined });

test('onboarding-first-use opens a real personalized Wisent home', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  requireProductBaseUrl('WISENT_APP_BASE_URL');
  requireEnvironment('WISENT_APP_STORAGE_STATE');
  const profile = requireJsonObject('WISENT_APP_FIRST_USE_PROFILE_JSON');
  if (typeof profile.name !== 'string' || typeof profile.genderLabel !== 'string' || typeof profile.ageLabel !== 'string') {
    throw new Error('WISENT_APP_FIRST_USE_PROFILE_JSON requires string name, genderLabel, and ageLabel fields');
  }

  await page.goto('/onboarding');
  await clearProductStorage(page, ['wisent-app.onboarding.v1']);
  await page.reload();

  const next = page.getByRole('button', { name: 'Next', exact: true });
  await expect(next).toBeEnabled();
  await expect.poll(async () => (await journeyProgress(page, 'wisent-app.onboarding.v1')).current_screen_id).toBe('welcome');
  await next.click();
  await page.reload();

  await expect.poll(async () => (await journeyProgress(page, 'wisent-app.onboarding.v1')).current_screen_id).toBe('personal_info');
  await expect(next).toBeDisabled();
  await page.getByRole('textbox').first().fill(profile.name);
  await page.getByText(profile.genderLabel, { exact: true }).click();
  await page.getByText(profile.ageLabel, { exact: true }).click();
  await expect(next).toBeEnabled();
  await next.click();

  await expect.poll(async () => (await journeyProgress(page, 'wisent-app.onboarding.v1')).current_screen_id).toBe('community_tutorial');
  await next.click();
  await expect.poll(async () => (await journeyProgress(page, 'wisent-app.onboarding.v1')).current_screen_id).toBe('create_character_tutorial');
  await expect.poll(async () => (await journeyProgress(page, 'wisent-app.onboarding.v1')).status).toBe('in_progress');
  await expect(page).toHaveURL(/\/onboarding(?:\?|$)/);

  await next.click();
  await expect(page).toHaveURL(/\/home(?:\?|$)/);
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(page.getByTestId('character-card').first()).toBeVisible();
  await expect.poll(async () => (await journeyProgress(page, 'wisent-app.onboarding.v1')).status).toBe('completed');
});

import { test, expect } from '@playwright/test';
import {
  chromiumPublicationOnly,
  clearProductStorage,
  fillAccessibleControl,
  requireJsonObject,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

test('onboarding-first-use renders a real representation-engineering result', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  requireProductBaseUrl('WISENT_GRADIO_BASE_URL');
  const inputs = requireJsonObject('WISENT_GRADIO_STEERING_VIZ_INPUTS_JSON');
  if (Object.keys(inputs).length === 0) {
    throw new Error('WISENT_GRADIO_STEERING_VIZ_INPUTS_JSON must provide the real model and steering-viz parser-required artifacts by accessible label');
  }

  await page.goto('/');
  await clearProductStorage(page, ['wisent.onboarding.wisent-gradio.subject']);
  await page.reload();

  const journey = page.getByText('First-use representation journey', { exact: true });
  await expect(journey).toBeVisible();
  await expect(page.getByRole('heading', { name: 'See what representations reveal' })).toBeVisible();
  await page.getByRole('button', { name: 'Open Steering visualization' }).click();
  await expect(page.getByRole('heading', { name: 'Create and inspect a representation result' })).toBeVisible();

  await page.reload();
  await expect(page.getByRole('heading', { name: 'Create and inspect a representation result' })).toBeVisible();
  await expect(page.getByText('Journey complete', { exact: false })).toHaveCount(0);
  await page.getByRole('button', { name: 'Open Steering visualization' }).click();

  for (const [label, value] of Object.entries(inputs)) {
    await fillAccessibleControl(page, label, value);
  }
  await page.getByRole('button', { name: 'Run steering-viz', exact: true }).click();

  await expect(page.getByRole('heading', { name: 'First representation result observed' })).toBeVisible({ timeout: 900_000 });
  const renderedOutput = await page.getByLabel('Output', { exact: true }).evaluateAll((controls) => controls
    .filter((control) => control instanceof HTMLElement && control.offsetParent !== null)
    .map((control) => control instanceof HTMLInputElement || control instanceof HTMLTextAreaElement
      ? control.value.trim()
      : (control.textContent || '').trim())
    .filter(Boolean));
  if (renderedOutput.length === 0) {
    await expect(page.getByText('Visualizations', { exact: true })).toBeVisible();
  }
  await expect(page.getByText('Journey complete', { exact: false })).toBeVisible();
});

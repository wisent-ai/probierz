import { test, expect, type Page } from '@playwright/test';
import {
  chromiumPublicationOnly,
  requireEnvironment,
  requireProductBaseUrl,
} from './helpers/onboarding-first-use';

type JsonRecord = Record<string, unknown>;

function record(value: unknown, label: string): JsonRecord {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${label} must be a JSON object`);
  return value as JsonRecord;
}

async function jsonBody(page: Page): Promise<JsonRecord> {
  const parsed: unknown = JSON.parse(await page.locator('body').innerText());
  return record(parsed, 'response');
}

test('onboarding-first-use observes real managed Skarbiec fleet state', async ({ page, browserName }, testInfo) => {
  chromiumPublicationOnly(browserName, testInfo);
  const baseUrl = requireProductBaseUrl('SKARBIEC_HUB_BASE_URL');
  const tenant = requireEnvironment('SKARBIEC_HUB_TENANT');
  const token = requireEnvironment('SKARBIEC_HUB_MANAGEMENT_TOKEN');
  const headers = { Authorization: `Bearer ${token}`, 'x-tenant': tenant };
  await page.context().setExtraHTTPHeaders(headers);

  const onboardingUrl = new URL(`/v1/onboarding?tenant=${encodeURIComponent(tenant)}`, baseUrl).href;
  const actionsUrl = new URL(`/v1/onboarding/actions?tenant=${encodeURIComponent(tenant)}`, baseUrl).href;
  const fleetUrl = new URL(`/v1/fleet/state?tenant=${encodeURIComponent(tenant)}`, baseUrl).href;

  const reset = await page.request.post(actionsUrl, { headers, data: { action: 'reset' } });
  expect(reset.ok()).toBeTruthy();
  await page.goto(onboardingUrl);
  let document = await jsonBody(page);
  let journey = record(document.journey, 'journey');
  expect(journey.status).toBe('in_progress');
  expect(record(journey.screen, 'journey.screen').id).toBe('managed-fleet');

  const firstAdvance = await page.request.post(actionsUrl, { headers, data: { action: 'continue' } });
  expect(firstAdvance.ok()).toBeTruthy();
  await page.reload();
  document = await jsonBody(page);
  journey = record(document.journey, 'journey');
  expect(journey.resumed).toBe(true);
  expect(record(journey.screen, 'journey.screen').id).toBe('control-boundaries');
  expect(journey.status).toBe('in_progress');

  const secondAdvance = await page.request.post(actionsUrl, { headers, data: { action: 'continue' } });
  expect(secondAdvance.ok()).toBeTruthy();
  await page.goto(onboardingUrl);
  document = await jsonBody(page);
  journey = record(document.journey, 'journey');
  expect(record(journey.screen, 'journey.screen').id).toBe('fleet-state');
  expect(journey.status).toBe('in_progress');
  const actions = journey.actions;
  expect(Array.isArray(actions) && record(actions[0], 'journey.actions[0]').id === 'view_managed_fleet').toBe(true);

  const fleetResponse = await page.goto(fleetUrl);
  expect(fleetResponse?.ok()).toBeTruthy();
  document = await jsonBody(page);
  const fleetState = record(document.fleet_state, 'fleet_state');
  const onboarding = record(document.onboarding, 'onboarding');
  expect(fleetState.managed_by).toBe('skarbiec-hub');
  expect(fleetState.tenant).toBe(tenant);
  expect(typeof fleetState.observed_at).toBe('string');
  expect(Number.isInteger(fleetState.registered_workloads)).toBe(true);
  expect(onboarding.completed).toBe(true);
  expect(onboarding.completion_fact).toBe('managed_fleet_state_observed');
});

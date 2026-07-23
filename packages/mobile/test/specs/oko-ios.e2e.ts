import { $, $$, browser } from '@wdio/globals';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { waitForOtp } from '../../../../apps/oko/otp-broker.mjs';

const artifactsDir = process.env.PROBIERZ_ARTIFACTS || join(process.cwd(), 'test-results');
const runId = process.env.PROBIERZ_RUN_ID || 'local';
const waitTimeout = 90_000;
const networkProfile = process.env.PROBIERZ_NETWORK_PROFILE || 'normal';

const byId = (identifier: string) => $(`~${identifier}`);

async function isPresent(identifier: string): Promise<boolean> {
  return byId(identifier).isExisting();
}

async function accessibleText(identifier: string): Promise<string> {
  const element = byId(identifier);
  await element.waitForExist({ timeout: waitTimeout });
  const text = await element.getText().catch(() => '');
  const value = await element.getAttribute('value').catch(() => '');
  const label = await element.getAttribute('label').catch(() => '');
  return [text, value, label].filter(Boolean).join(' ');
}

async function capturePageSource(name: string): Promise<void> {
  mkdirSync(join(artifactsDir, 'diagnostics'), { recursive: true });
  const source = await browser.getPageSource();
  writeFileSync(join(artifactsDir, 'diagnostics', `${name}.xml`), source, { mode: 0o600 });
}

function appLaunchEnvironment(): Record<string, string> {
  const environment: Record<string, string> = {};
  const supabaseURL = process.env.SUPABASE_URL;
  if (supabaseURL) {
    environment.SUPABASE_URL = networkProfile === 'offline' ? 'http://127.0.0.1:9' : supabaseURL;
  }
  const anonKey = process.env.SUPABASE_ANON_KEY;
  if (anonKey) environment.SUPABASE_ANON_KEY = anonKey;
  if (networkProfile === 'normal') {
    environment.OKO_E2E_OTP_BROKER_MODE = 'admin-generate-link';
  }
  return environment;
}

async function launchOko(bundleId: string): Promise<void> {
  await browser.execute('mobile: launchApp', {
    bundleId,
    environment: appLaunchEnvironment(),
  });
}

async function waitForAuthOrWorkspace(): Promise<void> {
  await browser.waitUntil(
    async () => (await isPresent('oko.auth.screen')) || (await isPresent('oko.main-tabs')),
    { timeout: waitTimeout, timeoutMsg: 'Oko did not show auth or the authenticated workspace' },
  );
}

async function loginIfNeeded(): Promise<void> {
  await waitForAuthOrWorkspace();
  if (!(await isPresent('oko.auth.screen'))) return;

  const email = process.env.OKO_E2E_EMAIL;
  if (!email) throw new Error('OKO_E2E_EMAIL is required for the Oko iOS auth journey');
  const requestedAt = new Date();
  await byId('oko.auth.email').setValue(email);
  await byId('oko.auth.send-code').click();
  await browser.waitUntil(
    async () => (await isPresent('oko.auth.code')) || (await isPresent('oko.auth.error')),
    { timeout: waitTimeout, timeoutMsg: 'Oko produced neither an OTP field nor an explicit auth error' },
  );
  if (await isPresent('oko.auth.error')) {
    throw new Error(`Oko OTP request failed: ${await accessibleText('oko.auth.error')}`);
  }
  const code = await waitForOtp({ after: requestedAt, timeoutMs: waitTimeout });
  await byId('oko.auth.code').setValue(code);
  await byId('oko.auth.verify').click();
  await byId('oko.main-tabs').waitForExist({ timeout: waitTimeout });
}

async function openTab(identifier: string, screenIdentifier: string): Promise<void> {
  let tab = byId(identifier);
  if (!(await tab.isExisting())) {
    const overflowLabels: Record<string, string> = {
      'oko.tab.chat': 'Oko',
      'oko.tab.goals': 'Goals',
      'oko.tab.strategy': 'Strategy',
    };
    const label = overflowLabels[identifier];
    if (!label) throw new Error(`No overflow label configured for ${identifier}`);
    await $('-ios class chain:**/XCUIElementTypeTabBar/**/XCUIElementTypeButton[`name == "More"`]').click();
    const moreBack = $('-ios predicate string:type == "XCUIElementTypeButton" AND name == "BackButton"');
    if (await moreBack.isExisting()) await moreBack.click();
    tab = $(`-ios predicate string:type == "XCUIElementTypeStaticText" AND name == "${label}"`);
  }
  await tab.waitForExist({ timeout: waitTimeout });
  await tab.click();
  await byId(screenIdentifier).waitForExist({ timeout: waitTimeout });
}


describe('Oko iOS reference journeys', () => {
  before(async () => {
    const bundleId = process.env.BUNDLE_ID;
    if (!bundleId) throw new Error('BUNDLE_ID is required for deterministic iOS app-data isolation');
    await browser.execute('mobile: clearApp', { bundleId });
    await browser.execute('mobile: setAppearance', {
      style: process.env.PROBIERZ_COLOR_SCHEME === 'dark' ? 'dark' : 'light',
    });
    await launchOko(bundleId);
  });

  afterEach(async function captureFailureState() {
    if (this.currentTest?.state !== 'failed') return;
    const name = this.currentTest.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase();
    await capturePageSource(`failure-${name}`);
  });
  if (networkProfile === 'offline') {
    it('surfaces an app-scoped offline authentication failure without advancing', async () => {
      await byId('oko.auth.screen').waitForDisplayed({ timeout: waitTimeout });
      const email = process.env.OKO_E2E_EMAIL;
      if (!email) throw new Error('OKO_E2E_EMAIL is required for the offline journey');
      await byId('oko.auth.email').setValue(email);
      await byId('oko.auth.send-code').click();
      const message = await accessibleText('oko.auth.error');
      expect(message).toMatch(/offline|connect|network|failed|timed out|NSURLErrorDomain|operation.*completed|-1004/i);
      expect(await isPresent('oko.auth.screen')).toBe(true);
      expect(await isPresent('oko.main-tabs')).toBe(false);
    });
    return;
  }


  it('authenticates with controlled OTP and refreshes the persisted session after relaunch', async () => {
    await loginIfNeeded();
    expect(await isPresent('oko.main-tabs')).toBe(true);

    const bundleId = process.env.BUNDLE_ID;
    if (!bundleId) throw new Error('BUNDLE_ID is required for the relaunch journey');
    await browser.terminateApp(bundleId);
    await launchOko(bundleId);
    await waitForAuthOrWorkspace();
    expect(await isPresent('oko.auth.screen')).toBe(false);
    expect(await isPresent('oko.main-tabs')).toBe(true);
  });

  it('uses stable tab identifiers and renders the seeded strategy summary', async () => {
    await openTab('oko.tab.strategy', 'oko.strategy.dashboard');
    expect(await accessibleText('oko.strategy.metric.pillars')).toContain('4');
    expect(await accessibleText('oko.strategy.metric.initiatives')).toContain('13');
    expect(await accessibleText('oko.strategy.metric.capacity')).toContain('100%');
  });

  it('renders the goals and velocity metrics', async () => {
    await openTab('oko.tab.goals', 'oko.goals.screen');
    await byId('oko.goals.metric.goals-closed').waitForExist({ timeout: waitTimeout });
    expect(await isPresent('oko.goals.metric.active-goals')).toBe(true);
    expect(await isPresent('oko.goals.metric.closed-all-time')).toBe(true);
  });

  it('completes a chat round-trip and treats every HTTP error as a failure', async () => {
    await openTab('oko.tab.chat', 'oko.chat.screen');
    const assistantSelector = '-ios predicate string:name BEGINSWITH "oko.chat.message.assistant."';
    const before = await $$(assistantSelector).length;
    const marker = `PROBIERZ_${runId.replace(/[^a-z0-9]/gi, '_')}`;
    await byId('oko.chat.composer').setValue(`Reply briefly and include this marker: ${marker}`);
    await byId('oko.chat.send').click();
    await browser.waitUntil(async () =>
      (await $$(assistantSelector).length) > before
        || (await isPresent('oko.chat.error')), {
      timeout: 180_000,
      timeoutMsg: 'Oko chat produced neither a response nor an explicit error',
    });
    if (await isPresent('oko.chat.error')) {
      throw new Error(`Oko chat failed: ${await accessibleText('oko.chat.error')}`);
    }
    const responses = await $$(assistantSelector).getElements();
    const latest = responses.length ? await responses[responses.length - 1].getText() : '';
    expect(latest).toBeTruthy();
    expect(latest).toContain(marker);
    expect(latest).not.toMatch(/\bHTTP\s*[45]\d\d\b|\brequest failed\b|\bunauthorized\b/i);
  });

  it('signs out through a stable control and returns to authentication', async () => {
    await openTab('oko.tab.strategy', 'oko.strategy.dashboard');
    await byId('oko.auth.sign-out').click();
    await byId('oko.auth.screen').waitForDisplayed({ timeout: waitTimeout });
    expect(await isPresent('oko.main-tabs')).toBe(false);
  });
});

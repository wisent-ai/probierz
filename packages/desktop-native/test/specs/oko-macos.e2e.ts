import { $, $$, browser } from '@wdio/globals';
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { waitForOtp } from '../../../../apps/oko/otp-broker.mjs';

const artifactsDir = process.env.PROBIERZ_ARTIFACTS || join(process.cwd(), 'test-results');
const runId = process.env.PROBIERZ_RUN_ID || 'local';
const waitTimeout = 90_000;
const fixtureScript = resolve(__dirname, '../../../../apps/oko/seed.mjs');
const networkProfile = process.env.PROBIERZ_NETWORK_PROFILE || 'normal';

function runFixture(command: 'writer-update' | 'apply-feedback' | 'verify'): Record<string, unknown> {
  const output = execFileSync(process.execPath, [fixtureScript, command], {
    encoding: 'utf8',
    env: process.env,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  return JSON.parse(output) as Record<string, unknown>;
}

const byId = (identifier: string) => $(`~${identifier}`);
const byIdentifierPrefix = (prefix: string) =>
  $$(`//*[@identifier and starts-with(@identifier, "${prefix}")]`);

async function isPresent(identifier: string): Promise<boolean> {
  return byId(identifier).isExisting();
}

async function waitForEnabled(identifier: string, timeout = waitTimeout): Promise<void> {
  await browser.waitUntil(async () => {
    const element = byId(identifier);
    return (await element.isExisting()) && (await element.isEnabled());
  }, { timeout, timeoutMsg: `${identifier} did not become enabled` });
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

async function launchTerminal(): Promise<void> {
  const provider = process.env.OKO_E2E_TERMINAL_PROVIDER || 'codex';
  const button = byId(`oko.terminal.new.${provider}`);
  await button.waitForExist({ timeout: waitTimeout });
  await button.click();
}

async function completeSetupSteps(): Promise<void> {
  for (let step = 0; step < 12; step += 1) {
    if (await isPresent('oko.onboarding.coach.continue')) return;
    const primary = byId('oko.onboarding.primary');
    await primary.waitForExist({ timeout: waitTimeout });
    if (!(await primary.isEnabled())) {
      const choices = await byIdentifierPrefix('oko.onboarding.choice.').getElements();
      let firstVisible: WebdriverIO.Element | undefined;
      for (const choice of choices) {
        if (await choice.isDisplayed()) {
          firstVisible = choice;
          break;
        }
      }
      if (!firstVisible) throw new Error('onboarding quiz has no selectable choice');
      await firstVisible.click();
      await waitForEnabled('oko.onboarding.primary');
    }
    await primary.click();
    await browser.pause(250);
  }
  throw new Error('onboarding did not reach the live-terminal coach');
}

async function completeTerminalCoach(): Promise<string[]> {
  await byId('oko.onboarding.coach.continue').waitForExist({ timeout: waitTimeout });
  await launchTerminal();
  await waitForEnabled('oko.onboarding.coach.continue');
  await byId('oko.onboarding.coach.continue').click();

  await $('//*[@value="Keep two sessions visible, then prove you can return"]')
    .waitForExist({ timeout: waitTimeout });
  await launchTerminal();
  await waitForEnabled('oko.onboarding.coach.continue');
  await byId('oko.onboarding.coach.continue').click();

  const rows = await byIdentifierPrefix('terminalSessionRow-').getElements();
  if (rows.length < 2) throw new Error(`expected two live terminal rows, found ${rows.length}`);
  const identifiers: string[] = [];
  for (const row of rows) {
    const identifier = await row.getAttribute('identifier');
    if (identifier) identifiers.push(identifier);
  }
  for (const row of rows) {
    await row.click();
    const finish = byId('oko.onboarding.coach.continue');
    if (await finish.isEnabled()) break;
  }
  await waitForEnabled('oko.onboarding.coach.continue');
  await byId('oko.onboarding.coach.continue').click();
  await $('//*[@value="Keep two sessions visible, then prove you can return"]')
    .waitForExist({ reverse: true, timeout: waitTimeout });
  return identifiers;
}

async function assistantMessageCount(): Promise<number> {
  return await byIdentifierPrefix('oko.chat.message.assistant.').length;
}

describe('Oko macOS critical journeys', () => {
  afterEach(async function captureFailureState() {
    if (this.currentTest?.state !== 'failed') return;
    await capturePageSource(`failure-${this.currentTest.title.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}`);
  });
  if (networkProfile === 'offline') {
    it('surfaces an app-scoped offline authentication failure without advancing', async () => {
      await byId('oko.auth.email').waitForExist({ timeout: waitTimeout });
      const email = process.env.OKO_E2E_EMAIL;
      if (!email) throw new Error('OKO_E2E_EMAIL is required for the offline journey');
      await byId('oko.auth.email').setValue(email);
      await byId('oko.auth.send-code').click();
      const message = await accessibleText('oko.auth.error');
      expect(message).toMatch(/offline|connect|network|failed|timed out|NSURLErrorDomain|operation.*completed|-1004/i);
      expect(await isPresent('oko.auth.email')).toBe(true);
      expect(await isPresent('oko.tab.terminals')).toBe(false);
    });
    return;
  }


  it('authenticates with controlled OTP and preserves the session after relaunch', async () => {
    await browser.pause(1_000);
    if (await isPresent('oko.auth.email')) {
      const email = process.env.OKO_E2E_EMAIL;
      if (!email) throw new Error('OKO_E2E_EMAIL is required for a clean auth journey');
      const requestedAt = new Date();
      await byId('oko.auth.email').setValue(email);
      await byId('oko.auth.send-code').click();
      await byId('oko.auth.code').waitForExist({ timeout: waitTimeout });
      const code = await waitForOtp({ after: requestedAt, timeoutMs: waitTimeout });
      await byId('oko.auth.code').setValue(code);
      await byId('oko.auth.verify').click();
      await byId('oko.auth.code').waitForExist({ reverse: true, timeout: waitTimeout });
    }

    await browser.reloadSession();
    await browser.pause(1_000);
    expect(await isPresent('oko.auth.email')).toBe(false);
    expect(
      (await isPresent('oko.onboarding.progress')) || (await isPresent('oko.tab.terminals')),
    ).toBe(true);
  });

  it('completes onboarding through the real terminal workspace', async () => {
    if (!(await isPresent('oko.onboarding.progress'))) {
      throw new Error('clean onboarding journey requires an account without completed onboarding');
    }
    const primary = byId('oko.onboarding.primary');
    await waitForEnabled('oko.onboarding.primary');
    await primary.click();
    const interruptedProgress = await accessibleText('oko.onboarding.progress');
    await browser.reloadSession();
    await byId('oko.onboarding.progress').waitForExist({ timeout: waitTimeout });
    expect(await accessibleText('oko.onboarding.progress')).toBe(interruptedProgress);
    await completeSetupSteps();
    const sessionIdentifiers = await completeTerminalCoach();
    expect(await isPresent('oko.tab.terminals')).toBe(true);

    await browser.reloadSession();
    await browser.pause(1_000);
    expect(await isPresent('oko.onboarding.progress')).toBe(false);
    expect(await isPresent('oko.onboarding.coach.continue')).toBe(false);
    await byId('oko.tab.terminals').click();
    await browser.waitUntil(async () => {
      const rows = await byIdentifierPrefix('terminalSessionRow-').getElements();
      const identifiers: string[] = [];
      for (const row of rows) {
        const identifier = await row.getAttribute('identifier');
        if (identifier) identifiers.push(identifier);
      }
      return sessionIdentifiers.every((identifier) => identifiers.includes(identifier));
    }, {
      timeout: waitTimeout,
      timeoutMsg: 'seeded onboarding terminal conversations did not survive relaunch',
    });
    const firstSessionIdentifier = sessionIdentifiers[0];
    if (!firstSessionIdentifier) throw new Error('onboarding produced no persisted terminal conversation');
    await byId(firstSessionIdentifier).click();
    await byId(firstSessionIdentifier.replace('terminalSessionRow-', 'terminalPane-'))
      .waitForExist({ timeout: waitTimeout });
  });

  it('loads seeded strategy metrics and persists a UI edit across relaunch', async () => {
    await byId('oko.tab.strategy').click();
    await byId('oko.strategy.metric.pillars').waitForExist({ timeout: waitTimeout });
    expect(await accessibleText('oko.strategy.metric.pillars')).toContain('4');
    expect(await accessibleText('oko.strategy.metric.initiatives')).toContain('13');

    const source = await browser.getPageSource();
    expect(source).toContain('100% capacity');

    const statement = byId('oko.strategy.editor.statement');
    await statement.waitForExist({ timeout: waitTimeout });
    const before = await statement.getValue();
    const marker = ` [probierz:${runId}]`;
    await statement.setValue(`${before}${marker}`);
    await browser.pause(2_000);

    await browser.reloadSession();
    await byId('oko.tab.strategy').click();
    await byId('oko.strategy.editor.statement').waitForExist({ timeout: waitTimeout });
    await browser.waitUntil(async () =>
      String(await byId('oko.strategy.editor.statement').getValue()).includes(marker), {
      timeout: waitTimeout,
      timeoutMsg: 'strategy edit did not persist across relaunch',
    });
  });

  it('rejects a stale UI writer and reloads the newer strategy revision', async () => {
    await byId('oko.tab.strategy').click();
    const statement = byId('oko.strategy.editor.statement');
    await statement.waitForExist({ timeout: waitTimeout });
    const staleValue = String(await statement.getValue());
    const writer = runFixture('writer-update');
    const marker = String(writer.marker);
    const staleMarker = ` [stale-ui:${runId}]`;
    await statement.setValue(`${staleValue}${staleMarker}`);
    await byId('oko.strategy.error').waitForExist({ timeout: waitTimeout });
    expect(await accessibleText('oko.strategy.error')).toMatch(/stale|conflict|strategy/i);

    await browser.reloadSession();
    await byId('oko.tab.strategy').click();
    await byId('oko.strategy.editor.statement').waitForExist({ timeout: waitTimeout });
    await browser.waitUntil(async () =>
      String(await byId('oko.strategy.editor.statement').getValue()).includes(marker), {
      timeout: waitTimeout,
      timeoutMsg: 'newer writer revision was not visible after reload',
    });
    const reloaded = String(await byId('oko.strategy.editor.statement').getValue());
    expect(reloaded).not.toContain(staleMarker);
  });

  it('applies isolated Slack feedback once and exposes the durable decision', async () => {
    const feedback = runFixture('apply-feedback');
    const marker = String(feedback.marker);
    const verification = runFixture('verify');
    expect(Object.values(verification.checks as Record<string, boolean>).every(Boolean)).toBe(true);

    await browser.reloadSession();
    await byId('oko.tab.strategy').click();
    await byId('oko.strategy.editor.statement').waitForExist({ timeout: waitTimeout });
    await browser.waitUntil(async () =>
      String(await byId('oko.strategy.editor.statement').getValue()).includes(marker), {
      timeout: waitTimeout,
      timeoutMsg: 'Slack feedback mutation was not visible in Oko after reload',
    });
    await browser.waitUntil(async () =>
      (await accessibleText('oko.strategy.metric.decisions')).includes('2'), {
      timeout: waitTimeout,
      timeoutMsg: 'Slack feedback decision was not reflected in the dashboard',
    });
    const source = await browser.getPageSource();
    expect(source).toContain(`Accept Probierz feedback [${runId}]`);
  });

  it('completes an Orchestrator chat round-trip without surfacing an HTTP error', async () => {
    await byId('oko.tab.chat').click();
    await byId('oko.chat.composer').waitForExist({ timeout: waitTimeout });
    const before = await assistantMessageCount();
    const marker = `PROBIERZ_${runId.replace(/[^a-z0-9]/gi, '_')}`;
    await byId('oko.chat.composer').setValue(`Reply briefly and include this marker: ${marker}`);
    await byId('oko.chat.send').click();
    await browser.waitUntil(async () =>
      (await assistantMessageCount()) > before || (await isPresent('oko.chat.error')), {
      timeout: 180_000,
      timeoutMsg: 'Orchestrator did not produce an assistant response',
    });
    if (await isPresent('oko.chat.error')) {
      throw new Error(`Oko chat failed: ${await accessibleText('oko.chat.error')}`);
    }
    const responses = await byIdentifierPrefix('oko.chat.message.assistant.').getElements();
    const latest = responses.length ? await responses[responses.length - 1].getText() : '';
    expect(latest).toBeTruthy();
    expect(latest).toContain(marker);
    expect(latest).not.toMatch(/\bHTTP\s*[45]\d\d\b|\brequest failed\b|\bunauthorized\b/i);
  });
});

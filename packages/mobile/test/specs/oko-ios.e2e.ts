import { execFileSync } from 'node:child_process';
import { join } from 'node:path';
import { $, browser } from '@wdio/globals';

const BUNDLE_ID = 'ai.wisent.oko';
const MOBILE_COMPANION = '~oko.onboarding.mobile-companion';
const WORKSPACE_RESULT = '~oko.onboarding.workspace-result';
const PRIMARY_ACTION = '~oko.onboarding.primary-action';

function requiredEnvironment(name: 'OKO_E2E_EMAIL'): string {
  const value = process.env[name];
  if (!value || value.includes('\0') || /[\r\n]/u.test(value)) {
    throw new Error(`${name} is missing or invalid`);
  }
  return value;
}

function waitForOtp({ after, timeoutMs }: { after: Date; timeoutMs: number }): string {
  const harness = process.env.PROBIERZ_TOOLKIT_ROOT;
  const binary = process.env.PROBIERZ_BIN
    ?? (harness ? join(harness, 'probierz-rs', 'target', 'debug', 'probierz') : 'probierz');
  const harnessArgs = harness ? ['--harness', harness] : [];
  const output = execFileSync(binary, [
    ...harnessArgs,
    'apphook',
    'oko.wait-for-otp',
    '--after',
    after.toISOString(),
    '--timeout-ms',
    String(timeoutMs),
  ], {
    encoding: 'utf8',
    env: process.env,
  });
  const answer = JSON.parse(output) as { code?: unknown };
  if (typeof answer.code !== 'string' || !/^\d{6,8}$/u.test(answer.code)) {
    throw new Error('Probierz returned an OTP outside the supported 6-8 digit range');
  }
  return answer.code;
}

async function relaunch(): Promise<void> {
  await browser.terminateApp(BUNDLE_ID);
  await browser.activateApp(BUNDLE_ID);
}

async function authenticateFreshSubject(): Promise<void> {
  const email = requiredEnvironment('OKO_E2E_EMAIL');
  const authScreen = await $('~oko.auth.screen');
  await authScreen.waitForDisplayed();

  const emailField = await $('~oko.auth.email');
  await emailField.setValue(email);
  const requestedAfter = new Date();
  const sendCode = await $('~oko.auth.send-code');
  await sendCode.waitForEnabled();
  await sendCode.click();

  const codeField = await $('~oko.auth.code');
  await codeField.waitForDisplayed();
  const code = await waitForOtp({ after: requestedAfter, timeoutMs: 90_000 });
  await codeField.setValue(code);

  const verify = await $('~oko.auth.verify');
  await verify.waitForEnabled();
  await verify.click();
}

describe('Oko iOS - onboarding first use', () => {
  it('resumes the fresh journey and completes only after a real workspace result', async () => {
    await browser.switchContext('NATIVE_APP');
    await authenticateFreshSubject();

    const companion = await $(MOBILE_COMPANION);
    await companion.waitForDisplayed();
    await expect(companion).toBeDisplayed();
    const continueAction = await $(PRIMARY_ACTION);
    await expect(continueAction).toHaveText('Continue');
    await continueAction.click();

    const workspaceResult = await $(WORKSPACE_RESULT);
    await workspaceResult.waitForDisplayed();
    await expect(workspaceResult).toBeDisplayed();

    // Navigation persisted, but it did not complete onboarding.
    await relaunch();
    await workspaceResult.waitForDisplayed();
    await expect(workspaceResult).toBeDisplayed();

    const openOrchestrator = await $(PRIMARY_ACTION);
    await expect(openOrchestrator).toHaveText('Open Orchestrator');
    await openOrchestrator.click();

    // Oko calls workspaceResultObserved only after both workspace reads have
    // succeeded, their result is committed to UI state, and the post-dismiss
    // reload has yielded.
    await workspaceResult.waitForDisplayed({ reverse: true });
    const mainTabs = await $('~oko.main-tabs');
    await mainTabs.waitForDisplayed();
    await expect(await $('~Orchestrator')).toBeDisplayed();
    await expect(await $('//XCUIElementTypeStaticText[@name="Open work items"]')).toBeDisplayed();
    await expect(await $('//XCUIElementTypeStaticText[@name="Daily actions"]')).toBeDisplayed();

    // Canonical completion evidence is durable: the onboarding sheet does not
    // return after a process boundary, while the real workspace still does.
    await relaunch();
    await mainTabs.waitForDisplayed();
    expect(await companion.isExisting()).toBe(false);
    expect(await workspaceResult.isExisting()).toBe(false);
    await expect(await $('~Orchestrator')).toBeDisplayed();
    const artifactsDir = process.env.PROBIERZ_ARTIFACTS;
    if (!artifactsDir) throw new Error('PROBIERZ_ARTIFACTS is required for canonical evidence');
    await browser.saveScreenshot(`${artifactsDir}/oko-ios-first-use-2026-08-04.1.png`);
  });
});

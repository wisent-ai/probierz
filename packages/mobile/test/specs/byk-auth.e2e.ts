import { $, browser } from '@wdio/globals';
import { createConnection } from 'node:net';
import { TextDecoder } from 'node:util';

const BUNDLE_ID = 'ai.wisent.byk';
const EMAIL_FIELD = '-ios class chain:**/XCUIElementTypeTextField';
const CODE_FIELD = '-ios class chain:**/XCUIElementTypeTextField[2]';
const WEB_VIEW = '-ios class chain:**/XCUIElementTypeWebView';
const BROKER_RESPONSE_LIMIT_BYTES = 4096;
const NEWLINE = Buffer.from('\n');
const BROKER_ERROR_REASONS: Readonly<Record<string, true>> = {
  invalid_request: true,
  invalid_since: true,
  since_not_fresh: true,
  invalid_budget: true,
  provider_unavailable: true,
  provider_auth_rejected: true,
  provider_rate_limited: true,
  provider_not_supported: true,
  provider_request_rejected: true,
  provider_transport_failed: true,
  provider_bad_request: true,
  provider_payment_required: true,
  provider_not_acceptable: true,
  provider_unprocessable_request: true,
  invalid_provider_response: true,
  code_not_found: true,
  ambiguous_code: true,
  budget_expired: true,
  audit_unavailable: true,
};
const BROKER_REJECTION_PREFIX = 'OTP broker rejected the OTP request: ';

function requiredEnvironment(name: 'BYK_OTP_SOCKET' | 'BYK_TEST_EMAIL'): string {
  const value = process.env[name];
  if (!value || value.includes('\0') || /[\r\n]/u.test(value)) {
    throw new Error(`${name} is missing or invalid`);
  }
  return value;
}

function parseBrokerResponse(bytes: Buffer): string {
  const newline = bytes.indexOf(NEWLINE);
  if (newline < 0 || newline !== bytes.length - NEWLINE.length) {
    throw new Error('OTP broker returned an invalid response');
  }

  let response: unknown;
  try {
    response = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes.subarray(0, newline)));
  } catch {
    throw new Error('OTP broker returned an invalid response');
  }
  if (typeof response !== 'object' || response === null || Array.isArray(response)) {
    throw new Error('OTP broker returned an invalid response');
  }

  const record = response as Record<string, unknown>;
  const keys = Object.keys(record);
  if (record.status === 'ready') {
    if (keys.length !== 2 || typeof record.code !== 'string' || !/^\d{6,8}$/u.test(record.code)) {
      throw new Error('OTP broker returned an invalid response');
    }
    return record.code;
  }
  if (
    record.status !== 'error' ||
    keys.length !== 2 ||
    typeof record.reason !== 'string' ||
    !Object.prototype.hasOwnProperty.call(BROKER_ERROR_REASONS, record.reason)
  ) {
    throw new Error('OTP broker returned an invalid response');
  }
  throw new Error(`${BROKER_REJECTION_PREFIX}${record.reason}`);
}

function requestOtp(socketPath: string, since: string): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const socket = createConnection({ path: socketPath });
    const chunks: Buffer[] = [];
    const budgetMs = 90_000;
    let responseBytes = 0;
    let settled = false;

    const close = (): void => {
      socket.removeAllListeners();
      socket.destroy();
    };
    const fail = (message: string): void => {
      if (settled) return;
      settled = true;
      close();
      reject(new Error(message));
    };
    const succeed = (code: string): void => {
      if (settled) return;
      settled = true;
      close();
      resolve(code);
    };

    socket.setTimeout(budgetMs + 5_000);
    socket.once('connect', () => socket.end(`${JSON.stringify({ since, budget_ms: budgetMs })}\n`));
    socket.on('data', (chunk: Buffer) => {
      responseBytes += chunk.length;
      if (responseBytes > BROKER_RESPONSE_LIMIT_BYTES) {
        fail('OTP broker response exceeded the size limit');
        return;
      }
      chunks.push(chunk);
    });
    socket.once('end', () => {
      try {
        succeed(parseBrokerResponse(Buffer.concat(chunks, responseBytes)));
      } catch (error) {
        fail(
          error instanceof Error && error.message.startsWith(BROKER_REJECTION_PREFIX)
            ? error.message
            : 'OTP broker returned an invalid response',
        );
      }
    });
    socket.once('timeout', () => fail('OTP broker request timed out'));
    socket.once('error', () => fail('OTP broker connection failed'));
    socket.once('close', () => fail('OTP broker closed before returning a response'));
  });
}

async function authenticateFreshSubject(): Promise<void> {
  const socketPath = requiredEnvironment('BYK_OTP_SOCKET');
  const email = requiredEnvironment('BYK_TEST_EMAIL');
  const sendCode = await $('~Send code');
  await sendCode.waitForDisplayed();

  const emailField = await $(EMAIL_FIELD);
  await emailField.setValue(email);
  await sendCode.waitForEnabled();
  const since = new Date().toISOString();
  await sendCode.click();

  const codeField = await $(CODE_FIELD);
  await codeField.waitForDisplayed();
  const code = await requestOtp(socketPath, since);
  await codeField.setValue(code);
  const verify = await $('~Verify & sign in');
  await verify.waitForEnabled();
  await verify.click();
}

async function relaunch(): Promise<void> {
  await browser.terminateApp(BUNDLE_ID);
  await browser.activateApp(BUNDLE_ID);
}

async function findWebViewContext(): Promise<string | undefined> {
  try {
    const contexts = await browser.getContexts();
    return contexts.find(
      (context): context is string => typeof context === 'string' && context.startsWith('WEBVIEW'),
    );
  } catch {
    return undefined;
  }
}

async function assertAuthenticatedTradingContent(): Promise<void> {
  const webContext = await findWebViewContext();
  if (webContext) {
    try {
      await browser.switchContext(webContext);
      const signals = await $('//*[normalize-space(.)="Signals"]');
      const strategies = await $('//*[normalize-space(.)="Strategies"]');
      await signals.waitForDisplayed();
      await strategies.waitForDisplayed();
      expect(await $('//*[normalize-space(.)="Please log in to view trading signals."]').isExisting()).toBe(false);
    } finally {
      await browser.switchContext('NATIVE_APP');
    }
    return;
  }

  await expect(await $('~Signals')).toBeDisplayed();
  await expect(await $('~Strategies')).toBeDisplayed();
  expect(await $('~Please log in to view trading signals.').isExisting()).toBe(false);
}

describe('Byk iOS - onboarding first use', () => {
  it('resumes explanation and completes only when the authenticated trading workspace loads', async () => {
    await browser.switchContext('NATIVE_APP');
    await authenticateFreshSubject();

    const promise = await $('~Turn market context into a decision');
    await promise.waitForDisplayed();
    await expect(promise).toBeDisplayed();
    await (await $('~Continue')).click();

    const controlModel = await $('~Assistant insight is not an order');
    await controlModel.waitForDisplayed();
    await expect(controlModel).toBeDisplayed();

    // Advancing is durable across a process boundary, but is not completion.
    await relaunch();
    await controlModel.waitForDisplayed();
    await expect(controlModel).toBeDisplayed();
    await (await $('~Continue')).click();

    const webView = await $(WEB_VIEW);
    await webView.waitForDisplayed();
    await assertAuthenticatedTradingContent();

    // didFinish on the authenticated trading WKWebView records
    // trading_workspace_loaded=true. Completion must survive another launch.
    await relaunch();
    await webView.waitForDisplayed();
    expect(await promise.isExisting()).toBe(false);
    expect(await controlModel.isExisting()).toBe(false);
    await assertAuthenticatedTradingContent();
    const artifactsDir = process.env.PROBIERZ_ARTIFACTS;
    if (!artifactsDir) throw new Error('PROBIERZ_ARTIFACTS is required for canonical evidence');
    await browser.saveScreenshot(`${artifactsDir}/byk-ios-first-use-2026-08-04.1.png`);
  });
});

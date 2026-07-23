import { browser, $ } from '@wdio/globals';
import { createConnection } from 'node:net';
import { TextDecoder } from 'node:util';

const EMAIL_FIELD = '-ios class chain:**/XCUIElementTypeTextField';
const CODE_FIELD = '-ios class chain:**/XCUIElementTypeTextField[2]';
const WEB_VIEW = '-ios class chain:**/XCUIElementTypeWebView';
const BROKER_RESPONSE_LIMIT_BYTES = Number('4096');
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
  if (!bytes.includes(NEWLINE) || newline !== bytes.length - NEWLINE.length) {
    throw new Error('OTP broker returned an invalid response');
  }

  let response: unknown;
  try {
    const line = new TextDecoder('utf-8', { fatal: true }).decode(
      bytes.subarray(Number('0'), newline),
    );
    response = JSON.parse(line) as unknown;
  } catch {
    throw new Error('OTP broker returned an invalid response');
  }

  if (typeof response !== 'object' || response === null || Array.isArray(response)) {
    throw new Error('OTP broker returned an invalid response');
  }

  const record = response as Record<string, unknown>;
  const keys = Object.keys(record);
  if (record.status === 'ready') {
    if (
      keys.length !== ['status', 'code'].length ||
      !Object.prototype.hasOwnProperty.call(record, 'status') ||
      !Object.prototype.hasOwnProperty.call(record, 'code') ||
      typeof record.code !== 'string' ||
      !/^(?:\d\d\d\d\d\d|\d\d\d\d\d\d\d|\d\d\d\d\d\d\d\d)$/u.test(record.code)
    ) {
      throw new Error('OTP broker returned an invalid response');
    }
    return record.code;
  }

  if (
    record.status !== 'error' ||
    keys.length !== ['status', 'reason'].length ||
    !Object.prototype.hasOwnProperty.call(record, 'status') ||
    !Object.prototype.hasOwnProperty.call(record, 'reason') ||
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
    const budgetMs = Number('90000');
    const deadlineMs = budgetMs + Number('5000');
    let responseBytes = Number('0');
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

    // This external process needs a real I/O deadline; fake timers cannot drive its socket.
    socket.setTimeout(deadlineMs);
    socket.once('connect', () => {
      const request = `${JSON.stringify({ since, budget_ms: budgetMs })}\n`;
      socket.end(request);
    });
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

async function forceLoggedOut(): Promise<void> {
  const signOut = await $('~Sign out');
  const sendCode = await $('~Send code');
  const useDifferentEmail = await $('~Use a different email');

  await browser.waitUntil(
    async () =>
      (await signOut.isExisting()) ||
      (await useDifferentEmail.isExisting()) ||
      (await sendCode.isExisting()),
  );

  if (await signOut.isExisting()) {
    await signOut.waitForDisplayed();
    await signOut.click();
  } else if (await useDifferentEmail.isExisting()) {
    await useDifferentEmail.waitForDisplayed();
    await useDifferentEmail.click();
  }

  await sendCode.waitForDisplayed();
}

async function findWebViewContext(): Promise<string | undefined> {
  try {
    const contexts = await browser.getContexts();
    return contexts.find(
      (context): context is string =>
        typeof context === 'string' && context.startsWith('WEBVIEW'),
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
      await expect(signals).toBeDisplayed();
      await expect(strategies).toBeDisplayed();
      expect(
        await $('//*[normalize-space(.)="Please log in to view trading signals."]').isExisting(),
      ).toBe(false);
    } finally {
      await browser.switchContext('NATIVE_APP');
    }
    return;
  }

  const signals = await $('~Signals');
  const strategies = await $('~Strategies');
  await signals.waitForDisplayed();
  await strategies.waitForDisplayed();
  await expect(signals).toBeDisplayed();
  await expect(strategies).toBeDisplayed();
  expect(await $('~Please log in to view trading signals.').isExisting()).toBe(false);
}

describe('Byk iOS - authenticated email OTP', () => {
  it('accepts the brokered OTP and renders authenticated trading content', async () => {
    const socketPath = requiredEnvironment('BYK_OTP_SOCKET');
    const email = requiredEnvironment('BYK_TEST_EMAIL');

    await browser.switchContext('NATIVE_APP');
    await forceLoggedOut();

    const emailField = await $(EMAIL_FIELD);
    await emailField.waitForDisplayed();
    await emailField.click();
    await emailField.clearValue();
    await emailField.setValue(email);

    const sendCode = await $('~Send code');
    await sendCode.waitForEnabled();
    const since = new Date().toISOString();
    await sendCode.click();

    const codeField = await $(CODE_FIELD);
    const requestError = await $('-ios predicate string:type == "XCUIElementTypeStaticText" AND (value BEGINSWITH "Supabase HTTP " OR value == "Bad URL")');
    await browser.waitUntil(async () =>
      (await codeField.isDisplayed()) || (await requestError.isDisplayed()));
    if (!(await codeField.isDisplayed())) {
      const status = (await requestError.getText()).match(/^Supabase HTTP \d+/u)?.[Number('0')];
      throw new Error(status ? `Byk OTP request failed: ${status}` : 'Byk OTP request failed before code entry');
    }
    const code = await requestOtp(socketPath, since);
    await codeField.click();
    await codeField.setValue(code);

    const verify = await $('~Verify & sign in');
    await verify.waitForEnabled();
    await verify.click();

    const signOut = await $('~Sign out');
    const webView = await $(WEB_VIEW);
    await signOut.waitForDisplayed();
    await webView.waitForDisplayed();
    await expect(signOut).toBeDisplayed();
    await expect(webView).toBeDisplayed();
    await assertAuthenticatedTradingContent();
  });
});

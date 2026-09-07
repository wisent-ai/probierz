import { $, $$, browser } from '@wdio/globals';

const BUNDLE_ID = 'com.wisent.ios';
const JOURNEY = '~wisent-first-use-journey';
const TITLE = '~wisent-first-use-title';
const PRIMARY_ACTION = '~wisent-first-use-primary-action';
const PROMPT = 'Give me one short welcome response.';

async function relaunch(): Promise<void> {
  await browser.terminateApp(BUNDLE_ID);
  await browser.activateApp(BUNDLE_ID);
}

async function visibleStaticText(): Promise<string[]> {
  const elements = await $$('-ios class chain:**/XCUIElementTypeStaticText');
  const values: string[] = [];
  for (const element of elements) {
    if (await element.isDisplayed()) {
      const value = (await element.getText()).trim();
      if (value) values.push(value);
    }
  }
  return values;
}

describe('Wisent iOS - onboarding first use', () => {
  it('resumes the fresh journey and completes only after a real assistant result', async () => {
    await browser.switchContext('NATIVE_APP');

    const journey = await $(JOURNEY);
    await journey.waitForDisplayed();
    await expect(journey).toBeDisplayed();
    await expect(await $(TITLE)).toHaveText('A companion that meets you where you are');
    const continueAction = await $(PRIMARY_ACTION);
    await expect(continueAction).toHaveText('Continue');
    await continueAction.click();

    await expect(await $(TITLE)).toHaveText('Start with one real conversation');

    // The selected step is persisted for the anonymous/authenticated Supabase
    // subject and resumes after a process boundary.
    await relaunch();
    await journey.waitForDisplayed();
    await expect(await $(TITLE)).toHaveText('Start with one real conversation');
    const startConversation = await $(PRIMARY_ACTION);
    await expect(startConversation).toHaveText('Start a conversation');
    await startConversation.click();
    await journey.waitForDisplayed({ reverse: true });

    // Entering the product surface is not success. Without an assistant result,
    // first-success is still in progress and must return after relaunch.
    await relaunch();
    await journey.waitForDisplayed();
    await expect(await $(TITLE)).toHaveText('Your first reply is the result');
    const enterProduct = await $(PRIMARY_ACTION);
    await expect(enterProduct).toHaveText('Go to Wisent');
    await enterProduct.click();
    await journey.waitForDisplayed({ reverse: true });

    const composer = await $('~Ask anything...');
    await composer.waitForDisplayed();
    const beforeResult = new Set(await visibleStaticText());
    await composer.setValue(PROMPT);
    await browser.keys('\uE007');

    let observedResult: string | undefined;
    await browser.waitUntil(async () => {
      const indicators = await $$('-ios class chain:**/XCUIElementTypeProgressIndicator');
      const hasVisibleIndicator = (await Promise.all(indicators.map((item) => item.isDisplayed())))
        .some(Boolean);
      if (hasVisibleIndicator) return false;

      const newText = (await visibleStaticText()).filter(
        (value) => value !== PROMPT && !beforeResult.has(value),
      );
      observedResult = newText.find((value) => value.length > 0);
      return observedResult !== undefined;
    }, { timeout: 120_000, interval: 1_000, timeoutMsg: 'No committed assistant result appeared' });
    expect(observedResult).toBeDefined();

    // WisentMobileResultEvidence is invoked only after assistant content is
    // committed to product state. Its completion must suppress first-use on a
    // new process while leaving the result surface usable.
    await relaunch();
    await composer.waitForDisplayed();
    expect(await journey.isExisting()).toBe(false);
    const artifactsDir = process.env.PROBIERZ_ARTIFACTS;
    if (!artifactsDir) throw new Error('PROBIERZ_ARTIFACTS is required for canonical evidence');
    await browser.saveScreenshot(`${artifactsDir}/wisent-ios-first-use-2026-08-04.1.png`);
  });
});

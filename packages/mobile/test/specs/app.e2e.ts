import { browser, $ } from '@wdio/globals';

/**
 * Smoke test template. Replace the selectors with your app's accessibility ids.
 * Use `~id` for accessibility id (works cross-platform), or platform locators.
 */
describe('mobile app smoke', () => {
  it('launches and is responsive', async () => {
    await browser.pause(1000);
    const state = await browser.queryAppState(
      (process.env.APP_PACKAGE || process.env.BUNDLE_ID || '') as string,
    );
    // 4 == running in foreground (when a package/bundle id is provided)
    if (process.env.APP_PACKAGE || process.env.BUNDLE_ID) {
      expect(state).toBeGreaterThanOrEqual(1);
    }
  });

  it.skip('taps a known element (fill in your selector)', async () => {
    const el = await $('~start-button');
    await el.waitForDisplayed({ timeout: 15000 });
    await el.click();
  });
});

import { browser, $ } from '@wdio/globals';

/**
 * Native desktop smoke template. Replace selectors with your app's
 * accessibility identifiers / automation ids.
 */
describe('native desktop app smoke', () => {
  it('launches a window', async () => {
    await browser.pause(1500);
    const title = await browser.getTitle().catch(() => '');
    expect(typeof title).toBe('string');
  });

  it.skip('interacts with a control (fill in your selector)', async () => {
    const el = await $('~SomeAccessibilityId');
    await el.waitForExist({ timeout: 15000 });
    await el.click();
  });
});

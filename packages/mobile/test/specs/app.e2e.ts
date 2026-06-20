import { browser, $ } from '@wdio/globals';

// The email field is a SwiftUI TextField with no accessibility identifier; its
// placeholder ("Email") lives in the element `value`. It is the only text field
// on the screen, so a class-chain locator targets it deterministically.
const EMAIL_FIELD = '-ios class chain:**/XCUIElementTypeTextField';

/**
 * Swiatowid iOS (ai.wisent.swiatowid) — sign-in screen smoke test.
 *
 * With no Keychain session the app opens on AuthView: a "Swiatowid" title, an
 * "Email" field, and a "Send code" button that stays disabled until an email is
 * typed. These checks need no backend secrets. Waits use the default
 * waitforTimeout from wdio.shared.conf.ts.
 */
describe('Swiatowid iOS — sign-in screen', () => {
  it('launches and shows the sign-in UI', async () => {
    const title = await $('~Swiatowid');
    await title.waitForDisplayed();
    await expect(title).toBeDisplayed();

    const subtitle = await $('~Sign in to your company workspace');
    await expect(subtitle).toBeDisplayed();

    const email = await $(EMAIL_FIELD);
    await expect(email).toBeDisplayed();
  });

  it('keeps "Send code" disabled until an email is entered', async () => {
    const sendBtn = await $('~Send code');
    await sendBtn.waitForExist();

    // No email yet -> the action is disabled.
    await expect(sendBtn).not.toBeEnabled();

    const email = await $(EMAIL_FIELD);
    await email.click();
    await email.setValue('founder@wisent.ai');
    await browser.pause(500);

    // A valid email enables the action.
    await expect(sendBtn).toBeEnabled();
  });
});

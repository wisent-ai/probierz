import { $ } from '@wdio/globals';

// Byk iOS (ai.wisent.byk) sign-in screen smoke test. Deterministic, no
// secrets/network: asserts the auth screen renders its real affordances. Byk's
// UI is an auth screen (email code + Sign in with Apple/Google/GitHub) that,
// after login, swaps to a WKWebView hosting the trading assistant. SwiftUI text
// fields have no accessibility id -> located via iOS class chain; buttons and
// labels match by ~name (the SwiftUI Text/label string).
//
// The authenticated email-OTP flow (Gmail-token gated) is intentionally NOT in
// this file: writing it requires the device-level-test consent gate
// (DEVICE_LEVEL_TESTS_APPROVED=1 set outside the session) plus the shared
// fetchOtp helper. Add it alongside once consent is granted.

const TF = '-ios class chain:**/XCUIElementTypeTextField';

describe('Byk iOS - sign-in screen', () => {
  it('renders the Byk title and workspace subtitle', async () => {
    await expect(await $('~Byk')).toBeDisplayed();
    await expect(await $('~Sign in to your company workspace')).toBeDisplayed();
  });

  it('offers the three social sign-in options', async () => {
    await expect(await $('~Sign in with Apple')).toBeDisplayed();
    await expect(await $('~Continue with Google')).toBeDisplayed();
    await expect(await $('~Continue with GitHub')).toBeDisplayed();
  });

  it('offers email entry and the Send code button', async () => {
    await expect(await $(TF)).toBeDisplayed();
    await expect(await $('~Send code')).toBeDisplayed();
  });
});

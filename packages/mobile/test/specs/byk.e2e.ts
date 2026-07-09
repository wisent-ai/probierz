import { $ } from '@wdio/globals';

// Byk iOS (ai.wisent.byk) sign-in coverage. Byk's UI is an auth screen (email
// code + Sign in with Apple/Google/GitHub) that, after login, swaps to a
// WKWebView hosting the trading assistant. SwiftUI text fields have no
// accessibility id -> located via iOS class chain; buttons/labels match by
// ~name (the SwiftUI Text/label string).
//
// This file covers the sign-in screen: rendering of its affordances and the
// real enable/disable state logic. The full authenticated email-OTP flow
// (drive Send code -> read the real 6-digit code -> verify -> assert the
// trading WebView loads) lives in byk-auth.e2e.ts, which reads the OTP through
// the sanctioned vault-resolved mailbox credential.

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

  it('keeps Send code disabled until an email is entered', async () => {
    const sendCode = await $('~Send code');
    await expect(sendCode).toBeDisplayed();
    // real state logic in AuthFeature: .disabled(model.email.isEmpty) — empty
    // field means the button is not tappable, regardless of rendering.
    await expect(sendCode).toBeDisabled();
    await (await $(TF)).setValue('byk-e2e@wisentmedia.com');
    // typing flips the binding -> the button becomes enabled
    await expect(sendCode).toBeEnabled();
  });
});

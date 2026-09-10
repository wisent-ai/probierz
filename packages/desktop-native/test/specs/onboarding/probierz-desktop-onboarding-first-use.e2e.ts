import { $, browser, expect } from '@wdio/globals';

const TIMEOUT = 120_000;

function requireFreshSubject(): void {
  if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
    throw new Error('Probierz Desktop first use requires PROBIERZ_DATA_STATE=fresh for a dedicated installation identity with no saved onboarding progress');
  }
}

describe('Probierz Desktop onboarding first use', () => {
  it('resumes its provenance explanation and completes only after inspecting a real protected bundle', async () => {
    requireFreshSubject();

    const journey = await $('~Probierz first-use journey');
    await journey.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Evidence stays read-only')).toBeDisplayed();
    await (await $('~Continue')).click();

    await browser.reloadSession();
    await (await $('~Every bundle has provenance')).waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Probierz first-use journey')).toBeDisplayed();
    await (await $('~Continue')).click();

    const showBundles = await $('~Show Evidence Bundles');
    await showBundles.waitForDisplayed({ timeout: TIMEOUT });
    await showBundles.click();

    // Navigation to Artifacts is not completion; the same terminal journey card
    // remains until a real available Protected bundle opens its inspector.
    await expect(await $('~Probierz first-use journey')).toBeDisplayed();
    const inspect = await $('~Inspect');
    await inspect.waitForDisplayed({ timeout: TIMEOUT });
    await inspect.click();

    const inspector = await $('~Protected evidence bundle inspector');
    await inspector.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Opened for read-only provenance inspection')).toBeDisplayed();
    await expect(await $('~The run manifest references a regular, non-symlink .pev file inside its run directory.')).toBeDisplayed();
    await expect(await $('~Probierz first-use journey')).not.toExist();

    await (await $('~Done')).click();
    await browser.reloadSession();
    await expect(await $('~Probierz first-use journey')).not.toExist();
    await (await $('~Probierz artifact metadata')).waitForDisplayed({ timeout: TIMEOUT });
    // Inspector onAppear records evidence_bundle_inspected for the selected
    // available protected artifact, bound to its real artifact identifier.
  });
});

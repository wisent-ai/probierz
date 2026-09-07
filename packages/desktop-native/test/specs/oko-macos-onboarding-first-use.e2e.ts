import { $, $$, browser, expect } from '@wdio/globals';

const TIMEOUT = 120_000;
const TERMINAL_ROW = '-ios predicate string:identifier BEGINSWITH "terminalSessionRow-"';

function requireFreshSubject(): void {
  if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
    throw new Error('oko-macos first use requires PROBIERZ_DATA_STATE=fresh and a dedicated authenticated subject with no saved onboarding progress');
  }
}

function providerSelector(): string {
  const identifier = process.env.PROBIERZ_OKO_PROVIDER_ID;
  const supported: Record<string, true> = {
    'oko.terminal.new.claude': true,
    'oko.terminal.new.codex': true,
    'oko.terminal.new.kimi': true,
  };
  if (!identifier || !supported[identifier]) {
    throw new Error('PROBIERZ_OKO_PROVIDER_ID must name a configured real Oko provider accessibility ID');
  }
  return `~${identifier}`;
}

async function clickPrimary(): Promise<void> {
  const primary = await $('~oko.onboarding.primary');
  await primary.waitForDisplayed({ timeout: TIMEOUT });
  await primary.click();
}

async function waitForTerminalCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await $$(TERMINAL_ROW)).length >= count, {
    timeout: TIMEOUT,
    timeoutMsg: `Oko did not expose ${count} real live terminal rows`,
  });
}

describe('Oko macOS onboarding first use', () => {
  it('resumes a fresh journey and completes only after returning to the same parallel session', async () => {
    requireFreshSubject();

    const progress = await $('~oko.onboarding.progress');
    await progress.waitForDisplayed({ timeout: TIMEOUT });
    await expect(progress).toHaveText(expect.stringContaining('Step 1 of'));
    await expect(await $('~oko.onboarding.coach.status')).not.toExist();

    await clickPrimary();
    await browser.reloadSession();

    const resumedProgress = await $('~oko.onboarding.progress');
    await resumedProgress.waitForDisplayed({ timeout: TIMEOUT });
    await expect(resumedProgress).toHaveText(expect.stringContaining('Step 2 of'));

    await clickPrimary();
    await (await $('~oko.onboarding.choice.friction-sessionVisibility')).click();
    await clickPrimary();
    await (await $('~oko.onboarding.choice.workStyle-oneAtATime')).click();
    await clickPrimary();
    await (await $('~oko.onboarding.choice.firstWin-returnToSession')).click();
    await clickPrimary();

    // Personalized plan, solution, proof, environment, workspace and permissions.
    for (let index = 0; index < 6; index += 1) await clickPrimary();

    const coachStatus = await $('~oko.onboarding.coach.status');
    await coachStatus.waitForDisplayed({ timeout: TIMEOUT });
    const provider = await $(providerSelector());
    await provider.waitForDisplayed({ timeout: TIMEOUT });
    await provider.click();
    await waitForTerminalCount(1);
    await provider.click();
    await waitForTerminalCount(2);

    const coachAction = await $('~oko.onboarding.coach.continue');
    await browser.waitUntil(async () => coachAction.isEnabled(), { timeout: TIMEOUT });
    await coachAction.click();

    const showTerminals = await $('~oko.onboarding.coach.show-terminals');
    await showTerminals.waitForDisplayed({ timeout: TIMEOUT });
    await showTerminals.click();
    await expect(await $('~oko.onboarding.coach.status')).toBeDisplayed();
    await expect(await $('~oko.onboarding.coach.continue')).toHaveText('Close One Terminal Pane');

    const rowsBeforeClose = await $$(TERMINAL_ROW);
    const returnTargetIdentifier = await rowsBeforeClose[0].getAttribute('identifier');
    if (!returnTargetIdentifier?.startsWith('terminalSessionRow-')) {
      throw new Error('Oko did not expose the stable live-session row selected for the parallel return');
    }

    await (await $('~oko.onboarding.coach.continue')).click();
    await expect(await $('~oko.onboarding.coach.status')).toHaveText(expect.stringContaining('Pane closed; session remains live'));
    await (await $(`~${returnTargetIdentifier}`)).click();
    await expect(await $('~oko.onboarding.coach.status')).toHaveText(expect.stringContaining('The same terminal pane is open again'));

    const finish = await $('~oko.onboarding.coach.continue');
    await expect(finish).toHaveText('Finish Onboarding');
    await browser.waitUntil(async () => finish.isEnabled(), { timeout: TIMEOUT });
    await finish.click();

    await expect(await $('~oko.onboarding.coach.status')).not.toExist();
    await waitForTerminalCount(2);
    // The close/reopen identity proof is the product hook for
    // parallel_return_completed; reaching the workspace alone never completes it.
  });
});

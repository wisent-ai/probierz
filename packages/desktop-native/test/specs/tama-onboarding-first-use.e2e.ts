import { $, browser, expect } from '@wdio/globals';

const TIMEOUT = 120_000;

function requireFreshSubject(): void {
  if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
    throw new Error('Tama first use requires PROBIERZ_DATA_STATE=fresh for a dedicated macOS subject with no saved onboarding progress');
  }
}

function requiredSupervisedSessionLabel(): string {
  const label = process.env.PROBIERZ_TAMA_SUPERVISED_AGENT_LABEL?.trim();
  if (!label) {
    throw new Error('PROBIERZ_TAMA_SUPERVISED_AGENT_LABEL must identify the real live supervised session supplied to Tama');
  }
  return label;
}

describe('Tama macOS onboarding first use', () => {
  it('resumes explanation and completes only after a supervised session is observed', async () => {
    requireFreshSubject();
    const supervisedSessionLabel = requiredSupervisedSessionLabel();

    await expect(await $('~Welcome to Tama')).toBeDisplayed({ wait: TIMEOUT });
    await expect(await $('~Know what every agent is allowed to do')).toBeDisplayed();
    await (await $('~Continue')).click();

    await browser.reloadSession();
    await expect(await $('~Policy first, mutations explicit')).toBeDisplayed({ wait: TIMEOUT });
    await expect(await $('~tama.setup')).not.toExist();

    await (await $('~Continue')).click();
    const setup = await $('~tama.setup');
    await setup.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Overview')).not.toExist();

    // Setup/provisioning is only a prerequisite. Crossing into setup does not
    // complete onboarding, and the persisted setup handoff must resume here.
    await browser.reloadSession();
    await (await $('~tama.setup')).waitForDisplayed({ timeout: TIMEOUT });
    await (await $('~Refresh sessions')).click();

    const observedSession = await $(`~${supervisedSessionLabel}`);
    await observedSession.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~A matching kernel-gated session is live')).toBeDisplayed();

    const finish = await $('~tama.setup.finish');
    await browser.waitUntil(async () => finish.isEnabled(), {
      timeout: TIMEOUT,
      timeoutMsg: 'Tama never reported a matching supervised session under the installed policy',
    });
    await finish.click();

    await expect(await $('~Overview')).toBeDisplayed({ wait: TIMEOUT });
    await expect(await $('~tama.setup')).not.toExist();
    // The selected real session plus the matching kernel-policy requirement is
    // the canonical supervised_session_observed evidence.
  });
});

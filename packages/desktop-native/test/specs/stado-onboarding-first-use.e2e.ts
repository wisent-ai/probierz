import { $, browser, expect } from '@wdio/globals';

const TIMEOUT = 120_000;

function requireFreshSubject(): void {
  if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
    throw new Error('Stado first use requires PROBIERZ_DATA_STATE=fresh for a dedicated authenticated subject with no saved onboarding progress');
  }
}

function requiredCompletedJobID(): string {
  const jobID = process.env.PROBIERZ_STADO_COMPLETED_JOB_ID?.trim();
  if (!jobID) {
    throw new Error('PROBIERZ_STADO_COMPLETED_JOB_ID must identify a real authorized job visible in the Stado dashboard snapshot');
  }
  return jobID;
}

describe('Stado macOS onboarding first use', () => {
  it('resumes the explanation and completes only after an authorized job appears', async () => {
    requireFreshSubject();
    const completedJobID = requiredCompletedJobID();

    const promise = await $('~Control a fleet without losing the thread');
    await promise.waitForDisplayed({ timeout: TIMEOUT });
    await (await $('~Continue')).click();

    await browser.reloadSession();
    const resumed = await $('~Observe first, mutate deliberately');
    await resumed.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Stado')).not.toExist();

    // Advancing between explanation screens must not claim first success.
    await (await $('~Continue')).click();
    const consoleTitle = await $('~Stado');
    await consoleTitle.waitForDisplayed({ timeout: TIMEOUT });

    const jobs = await $('~Jobs');
    await jobs.waitForDisplayed({ timeout: TIMEOUT });
    await jobs.click();
    const completedJob = await $(`~${completedJobID}`);
    await completedJob.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~No recent completions')).not.toExist();

    await browser.reloadSession();
    await (await $('~Stado')).waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Control a fleet without losing the thread')).not.toExist();
    await expect(await $('~Observe first, mutate deliberately')).not.toExist();
    // The real job row is the product observation that records
    // authorized_job_completed; opening the console is not sufficient.
  });
});

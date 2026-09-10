import { $, browser, expect } from '@wdio/globals';

const TIMEOUT = 120_000;

function requireFreshSubject(): void {
  if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
    throw new Error('Lem first use requires PROBIERZ_DATA_STATE=fresh for a dedicated device subject with no saved onboarding progress');
  }
}

function requiredPaperTitle(): string {
  const title = process.env.PROBIERZ_LEM_PAPER_TITLE?.trim();
  if (!title) {
    throw new Error('PROBIERZ_LEM_PAPER_TITLE must name a real paper already available in the Lem registry');
  }
  return title;
}

describe('Lem macOS onboarding first use', () => {
  it('resumes its mental model and completes only when a real paper workspace opens', async () => {
    requireFreshSubject();
    const paperTitle = requiredPaperTitle();

    const promise = await $('~Turn papers into a working research queue');
    await promise.waitForDisplayed({ timeout: TIMEOUT });
    await (await $('~Continue')).click();

    await browser.reloadSession();
    const mentalModel = await $('~Sources become paper workspaces');
    await mentalModel.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Open Paper Workspace')).not.toExist();

    // The explanation navigation persisted, but no paper has been opened and
    // therefore the first-success hook cannot have completed.
    await (await $('~Continue')).click();
    const openWorkspace = await $('~Open Paper Workspace');
    await openWorkspace.waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $(`~${paperTitle}`)).toBeDisplayed();
    await openWorkspace.click();

    await expect(await $('~Open Paper Workspace')).not.toExist();
    const openedPaper = await $(`~${paperTitle}`);
    await openedPaper.waitForDisplayed({ timeout: TIMEOUT });

    await browser.reloadSession();
    await (await $(`~${paperTitle}`)).waitForDisplayed({ timeout: TIMEOUT });
    await expect(await $('~Welcome to Lem')).not.toExist();
    // PaperDetailView.onAppear records paper_workspace_opened for this real
    // registry paper; selecting or continuing alone never records completion.
  });
});

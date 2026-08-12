import { $, browser, expect } from '@wdio/globals';
import { randomUUID } from 'node:crypto';
import { mkdirSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const TIMEOUT = 120_000;
const artifacts = path.resolve(process.env.PROBIERZ_ARTIFACTS || 'test-results');
const mediaDir = path.join(artifacts, 'media');

function textContaining(value: string) {
  return $(`//*[contains(@label, "${value}") or contains(@name, "${value}") or contains(@value, "${value}")]`);
}

async function capture(name: string): Promise<void> {
  mkdirSync(mediaDir, { recursive: true });
  await browser.saveScreenshot(path.join(mediaDir, `${name}.png`));
}

describe('Brama Desktop critical operations', () => {
  it('starts its private runtime and completes the local model-source lifecycle', async () => {
    if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
      throw new Error('Brama Desktop critical operations require PROBIERZ_DATA_STATE=fresh on a dedicated macOS host');
    }

    const provider = `probierz-${randomUUID()}`;
    let providerAdded = false;

    try {
      const overview = await $('~Overview');
      await overview.waitForDisplayed({ timeout: TIMEOUT });
      const operational = await textContaining('Operational');
      await operational.waitForDisplayed({ timeout: TIMEOUT });
      await capture('brama-private-runtime-operational');

      await (await $('~Model Sources')).click();
      await (await $('~Add model source')).waitForDisplayed({ timeout: TIMEOUT });
      await (await $('~Add model source')).click();

      await (await $('~Provider (for example openai or anthropic)')).setValue(provider);
      await (await $('~API key or subscription credential')).setValue(`qa-${randomUUID()}`);
      await (await $('~Add')).click();

      const source = await textContaining(provider);
      await source.waitForDisplayed({ timeout: TIMEOUT });
      providerAdded = true;
      await capture('brama-model-source-added');

      await (await $('~Remove')).click();
      await (await $('~Remove model source')).waitForDisplayed({ timeout: TIMEOUT });
      await (await $('~Remove model source')).click();
      await source.waitForExist({ timeout: TIMEOUT, reverse: true });
      providerAdded = false;

      await expect(await textContaining('Operational')).toBeDisplayed();
      await capture('brama-model-source-removed');
    } finally {
      if (providerAdded) {
        spawnSync('security', [
          'delete-generic-password',
          '-s', 'ai.wisent.brama.desktop.providers',
          '-a', provider,
        ], { stdio: 'ignore' });
      }
    }
  });
});

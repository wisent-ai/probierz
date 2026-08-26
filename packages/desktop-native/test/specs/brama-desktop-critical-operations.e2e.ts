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

function keychainValue(provider: string): string | null {
  const result = spawnSync('security', [
    'find-generic-password',
    '-w',
    '-s', 'ai.wisent.brama.desktop.providers',
    '-a', provider,
  ], { encoding: 'utf8' });
  return result.status === 0 ? result.stdout.trim() : null;
}

describe('Brama Desktop administration lifecycles', () => {
  it('adds, replaces, and deletes a provider key and route alias through the real native UI', async () => {
    if (process.env.PROBIERZ_DATA_STATE !== 'fresh') {
      throw new Error('Brama Desktop administration lifecycles require PROBIERZ_DATA_STATE=fresh on a dedicated macOS host');
    }

    const provider = 'openai';
    const firstKey = `qa-${randomUUID()}`;
    const replacementKey = `qa-${randomUUID()}`;
    const alias = `probierz/${randomUUID().replaceAll('-', '')}`;
    let providerAdded = false;

    try {
      await (await $('~Posture')).waitForDisplayed({ timeout: TIMEOUT });
      await (await $('~Subscriptions')).click();
      await (await $('~Add local key')).waitForDisplayed({ timeout: TIMEOUT });
      await (await $('~Add local key')).click();

      await (await $('~Provider')).setValue(provider);
      await (await $('~API key or subscription credential')).setValue(firstKey);
      await (await $('~Add key')).click();

      const source = await textContaining(provider);
      await source.waitForDisplayed({ timeout: TIMEOUT });
      providerAdded = true;
      if (keychainValue(provider) !== firstKey) {
        throw new Error('the add operation did not persist the exact provider key in the isolated Keychain item');
      }
      await capture('brama-provider-key-added');

      await source.click();
      await (await $('~Replace this provider key…')).click();
      await (await $('~API key or subscription credential')).setValue(replacementKey);
      await (await $('~Replace credential')).click();
      await (await textContaining(`${provider} is saved`)).waitForDisplayed({ timeout: TIMEOUT });
      if (keychainValue(provider) !== replacementKey) {
        throw new Error('the replace operation left the previous provider key in the isolated Keychain item');
      }
      await capture('brama-provider-key-replaced');

      await (await $('~Routing')).click();
      await (await $('~Add alias')).waitForDisplayed({ timeout: TIMEOUT });
      await (await $('~Add alias')).click();
      await (await $('~Alias')).setValue(alias);
      await (await $('~Primary target')).setValue('openai/default');
      await (await $('~Create alias')).click();

      const route = await textContaining(alias);
      await route.waitForDisplayed({ timeout: TIMEOUT });
      await capture('brama-route-alias-added');
      await route.click();
      const primary = await $('~Primary target');
      await primary.setValue('openai/fail');
      await (await $('~Review change…')).click();
      await (await $('~Rewrite the route')).click();
      await (await textContaining('openai/fail')).waitForDisplayed({ timeout: TIMEOUT });
      await capture('brama-route-alias-replaced');

      await (await $('~Delete this alias…')).click();
      await (await $('~Delete the alias')).click();
      await route.waitForExist({ timeout: TIMEOUT, reverse: true });
      await capture('brama-route-alias-deleted');

      await (await $('~Subscriptions')).click();
      const savedSource = await textContaining(provider);
      await savedSource.waitForDisplayed({ timeout: TIMEOUT });
      await savedSource.click();
      await (await $('~Remove this provider key…')).click();
      await (await $('~Remove it')).click();
      await savedSource.waitForExist({ timeout: TIMEOUT, reverse: true });
      providerAdded = false;
      if (keychainValue(provider) !== null) {
        throw new Error('the remove operation left the provider key in the isolated Keychain item');
      }
      await capture('brama-provider-key-deleted');
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

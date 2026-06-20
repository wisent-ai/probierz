import { test, expect, _electron as electron } from '@playwright/test';
import path from 'path';

/**
 * Launches an Electron app and drives its renderer window.
 * Set ELECTRON_APP_MAIN to the entry file of the app you want to test;
 * defaults to the bundled sample app in ./fixtures/main.js.
 */
const MAIN = process.env.ELECTRON_APP_MAIN || path.join(__dirname, '..', 'fixtures', 'main.js');

test('electron app opens a window and reacts to clicks', async () => {
  const app = await electron.launch({ args: [MAIN] });
  const window = await app.firstWindow();

  await expect(window.locator('#heading')).toHaveText('Hello from Electron');
  await window.locator('#btn').click();
  await expect(window.locator('#heading')).toHaveText('Clicked');

  await app.close();
});

import { shared } from './wdio.shared.conf';
import type { Options } from '@wdio/types';

const environmentPrefix = 'PROBIERZ_MAC_APP_ENV_';
const appEnvironment = Object.fromEntries(
  Object.entries(process.env)
    .filter(([name, value]) => name.startsWith(environmentPrefix) && value !== undefined)
    .map(([name, value]) => [name.slice(environmentPrefix.length), value as string]),
);
const appArguments = process.env.PROBIERZ_MAC_APP_ARGS
  ? JSON.parse(process.env.PROBIERZ_MAC_APP_ARGS) as string[]
  : [];

/**
 * macOS native apps via the Appium Mac2 driver (XCTest).
 * Set MAC_BUNDLE_ID (e.g. com.apple.TextEdit) or MAC_APP_PATH to a .app bundle.
 */
export const config: Options.Testrunner = {
  ...shared,
  port: 4723,
  services: process.env.PROBIERZ_EXTERNAL_APPIUM === '1'
    ? []
    : [['appium', { args: { relaxedSecurity: true } }]],
  capabilities: [
    {
      platformName: 'mac',
      'appium:automationName': 'Mac2',
      ...(process.env.MAC_BUNDLE_ID ? { 'appium:bundleId': process.env.MAC_BUNDLE_ID } : {}),
      ...(process.env.MAC_APP_PATH ? { 'appium:appPath': process.env.MAC_APP_PATH } : {}),
      'appium:processArguments': { env: appEnvironment, args: appArguments },
      'appium:showServerLogs': true,
    },
  ],
} as Options.Testrunner;

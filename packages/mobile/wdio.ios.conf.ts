import { shared } from './wdio.shared.conf';
import type { Options } from '@wdio/types';

/**
 * iOS (XCUITest, macOS only). Set APP_IOS to an absolute path to your .app/.ipa,
 * or set BUNDLE_ID to test an already-installed app on the simulator/device.
 */
export const config: Options.Testrunner = {
  ...shared,
  capabilities: [
    {
      platformName: 'iOS',
      'appium:automationName': 'XCUITest',
      'appium:deviceName': process.env.IOS_DEVICE || 'iPhone 15',
      ...(process.env.IOS_VERSION ? { 'appium:platformVersion': process.env.IOS_VERSION } : {}),
      ...(process.env.APP_IOS ? { 'appium:app': process.env.APP_IOS } : {}),
      ...(process.env.BUNDLE_ID ? { 'appium:bundleId': process.env.BUNDLE_ID } : {}),
      'appium:newCommandTimeout': 240,
    },
  ],
} as Options.Testrunner;

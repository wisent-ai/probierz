import { shared } from './wdio.shared.conf';
import type { Options } from '@wdio/types';

/**
 * Android (UiAutomator2). Set APP_ANDROID to an absolute path to your .apk,
 * or set APP_PACKAGE/APP_ACTIVITY to test an already-installed app.
 */
export const config: Options.Testrunner = {
  ...shared,
  capabilities: [
    {
      platformName: 'Android',
      'appium:automationName': 'UiAutomator2',
      'appium:deviceName': process.env.ANDROID_DEVICE || 'Android Emulator',
      'appium:platformVersion': process.env.ANDROID_VERSION,
      ...(process.env.APP_ANDROID ? { 'appium:app': process.env.APP_ANDROID } : {}),
      ...(process.env.APP_PACKAGE ? { 'appium:appPackage': process.env.APP_PACKAGE } : {}),
      ...(process.env.APP_ACTIVITY ? { 'appium:appActivity': process.env.APP_ACTIVITY } : {}),
      'appium:newCommandTimeout': 240,
    },
  ],
} as Options.Testrunner;

import { shared } from './wdio.shared.conf';
import type { Options } from '@wdio/types';

const locale = process.env.PROBIERZ_LOCALE || 'en';
const networkProfile = process.env.PROBIERZ_NETWORK_PROFILE || 'normal';
const appEnvironment: Record<string, string> = {
  ...(networkProfile === 'normal' ? { OKO_E2E_OTP_BROKER_MODE: 'admin-generate-link' } : {}),
  ...(process.env.SUPABASE_ANON_KEY ? { SUPABASE_ANON_KEY: process.env.SUPABASE_ANON_KEY } : {}),
  ...(process.env.SUPABASE_URL ? {
    SUPABASE_URL: networkProfile === 'offline' ? 'http://127.0.0.1:9' : process.env.SUPABASE_URL,
  } : {}),
};

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
      'appium:enforceAppInstall': true,
      ...(process.env.BUNDLE_ID ? { 'appium:bundleId': process.env.BUNDLE_ID } : {}),
      'appium:language': locale,
      'appium:locale': locale === 'pl' ? 'pl_PL' : 'en_US',
      'appium:processArguments': { env: appEnvironment, args: [] },
      'appium:autoDismissAlerts': true,
      'appium:newCommandTimeout': 240,
    },
  ],
} as Options.Testrunner;

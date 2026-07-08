import type { Options } from '@wdio/types';

// Pin APPIUM_HOME so the auto-started Appium server resolves its installed
// drivers from the same location regardless of the working directory. Without
// this, Appium launched from a package dir that lists `appium` as a dependency
// autodetects APPIUM_HOME as that dir and reports "no drivers installed".
process.env.APPIUM_HOME = process.env.APPIUM_HOME || `${process.env.HOME}/.appium`;

/**
 * Settings common to iOS and Android runs. Platform-specific configs spread
 * this and add their own `capabilities`.
 */
export const shared: Partial<Options.Testrunner> = {
  runner: 'local',
  specs: ['./test/specs/**/*.e2e.ts'],
  maxInstances: 1,
  logLevel: 'info',
  bail: 0,
  waitforTimeout: 20000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120000 },
  // Start a local Appium server automatically.
  services: [['appium', { args: { relaxedSecurity: true } }]],
  port: 4723,
};

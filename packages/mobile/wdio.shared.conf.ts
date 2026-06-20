import type { Options } from '@wdio/types';

/**
 * Settings common to iOS and Android runs. Platform-specific configs spread
 * this and add their own `capabilities`.
 */
export const shared: Partial<Options.Testrunner> = {
  runner: 'local',
  autoCompileOpts: {
    autoCompile: true,
    tsNodeOpts: { project: './tsconfig.json', transpileOnly: true },
  },
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

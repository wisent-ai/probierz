import type { Options } from '@wdio/types';

export const shared: Partial<Options.Testrunner> = {
  runner: 'local',
  autoCompileOpts: {
    autoCompile: true,
    tsNodeOpts: { project: './tsconfig.json', transpileOnly: true },
  },
  specs: ['./test/specs/**/*.e2e.ts'],
  maxInstances: 1,
  logLevel: 'info',
  waitforTimeout: 20000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120000 },
};

import type { Options } from '@wdio/types';
import { driver } from '@wdio/globals';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// Pin APPIUM_HOME so the auto-started Appium server resolves its installed
// drivers from the same location regardless of the working directory. Without
// this, Appium launched from a package dir that lists `appium` as a dependency
// autodetects APPIUM_HOME as that dir and reports "no drivers installed".
process.env.APPIUM_HOME = process.env.APPIUM_HOME || `${process.env.HOME}/.appium`;

// Recording + result capture for probierz. PROBIERZ_RECORD=1|true records each
// test via Appium screen recording (best-effort: drivers without screen
// recording degrade silently). Per-test video + a JSON result summary land
// under PROBIERZ_ARTIFACTS so the analyzer can find them.
const record = process.env.PROBIERZ_RECORD === '1' || process.env.PROBIERZ_RECORD === 'true';
const artifactsDir = process.env.PROBIERZ_ARTIFACTS || join(process.cwd(), 'test-results');
const results: Array<{ title: string; passed: boolean; duration: number; video?: string; error?: string }> = [];
const slug = (s: string) => s.replace(/[^a-z\d]+/gi, '_').replace(/^_+|_+$/g, '').toLowerCase();
// PROBIERZ_SPEC scopes the run to one spec. Accept a bare filename, a
// package-relative path, or a repo-relative path -- match by basename anywhere
// under test/specs so any of those forms resolves to the same file.
const specGlob = process.env.PROBIERZ_SPEC
  ? [`./test/specs/**/${process.env.PROBIERZ_SPEC.split('/').pop()}`]
  : ['./test/specs/**/*.e2e.ts'];

/**
 * Settings common to iOS and Android runs. Platform-specific configs spread
 * this and add their own `capabilities`.
 */
export const shared: Partial<Options.Testrunner> = {
  runner: 'local',
  specs: specGlob,
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
  beforeTest: async () => {
    if (!record) return;
    try { await driver.startRecordingScreen(); } catch { /* driver lacks screen recording */ }
  },
  afterTest: async (
    test: { title: string },
    _ctx: unknown,
    res: { passed: boolean; duration: number; error?: Error },
  ) => {
    let video: string | undefined;
    if (record) {
      try {
        const b64 = await driver.stopRecordingScreen();
        if (b64) {
          mkdirSync(artifactsDir, { recursive: true });
          video = join(artifactsDir, `${slug(test.title)}.mp4`);
          writeFileSync(video, Buffer.from(b64, 'base64'));
        }
      } catch { /* driver lacks screen recording */ }
    }
    results.push({
      title: test.title,
      passed: res.passed,
      duration: res.duration,
      video,
      error: res.error ? String(res.error.message || res.error) : undefined,
    });
  },
  after: () => {
    try {
      mkdirSync(artifactsDir, { recursive: true });
      const passed = results.filter((r) => r.passed).length;
      writeFileSync(
        join(artifactsDir, 'probierz-mobile-results.json'),
        JSON.stringify({ total: results.length, passed, failed: results.length - passed, tests: results }, null, Number('2')),
      );
    } catch { /* best-effort */ }
  },
};

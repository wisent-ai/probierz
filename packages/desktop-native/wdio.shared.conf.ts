import type { Options } from '@wdio/types';
import { driver } from '@wdio/globals';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// Recording + result capture for probierz. PROBIERZ_RECORD=1|true records each
// test via the driver's screen recording when supported. Native drivers (Mac2,
// WinAppDriver) often lack it, so every recording call is best-effort and
// degrades silently — a run never fails because recording is unsupported.
const record = process.env.PROBIERZ_RECORD === '1' || process.env.PROBIERZ_RECORD === 'true';
const artifactsDir = process.env.PROBIERZ_ARTIFACTS || join(process.cwd(), 'test-results');
const results: Array<{ title: string; passed: boolean; duration: number; video?: string; error?: string }> = [];
const slug = (s: string) => s.replace(/[^a-z\d]+/gi, '_').replace(/^_+|_+$/g, '').toLowerCase();

export const shared: Partial<Options.Testrunner> = {
  runner: 'local',
  specs: ['./test/specs/**/*.e2e.ts'],
  maxInstances: 1,
  logLevel: 'info',
  waitforTimeout: 20000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120000 },
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
        join(artifactsDir, 'probierz-native-results.json'),
        JSON.stringify({ total: results.length, passed, failed: results.length - passed, tests: results }, null, Number('2')),
      );
    } catch { /* best-effort */ }
  },
};

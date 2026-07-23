import type { Options } from '@wdio/types';
import { driver } from '@wdio/globals';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
type RecordingDriver = typeof driver & {
  startRecordingScreen(): Promise<void>;
  stopRecordingScreen(): Promise<string>;
};
const recordingDriver = driver as RecordingDriver;

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
const reportPath = process.env.PROBIERZ_REPORT_PATH || join(artifactsDir, 'report.json');
const runId = process.env.PROBIERZ_RUN_ID || null;
const captureErrors: string[] = [];
const results: Array<{
  title: string;
  passed: boolean;
  duration: number;
  startedAt: string;
  completedAt: string;
  video?: string;
  error?: string;
}> = [];
const testStartedAt = new Map<string, string>();
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
  beforeTest: async (test: { title: string }) => {
    testStartedAt.set(test.title, new Date().toISOString());
    if (!record) return;
    try {
      await recordingDriver.startRecordingScreen();
    } catch (error) {
      captureErrors.push(`start recording: ${error instanceof Error ? error.message : String(error)}`);
    }
  },
  afterTest: async (
    test: { title: string },
    _ctx: unknown,
    res: { passed: boolean; duration: number; error?: Error },
  ) => {
    let video: string | undefined;
    if (record) {
      try {
        const b64 = await recordingDriver.stopRecordingScreen();
        if (b64) {
          mkdirSync(artifactsDir, { recursive: true });
          video = join(artifactsDir, `${slug(test.title)}.mp4`);
          writeFileSync(video, Buffer.from(b64, 'base64'));
        }
      } catch (error) {
        captureErrors.push(`stop recording for ${test.title}: ${error instanceof Error ? error.message : String(error)}`);
      }
    }
    results.push({
      title: test.title,
      passed: res.passed,
      duration: res.duration,
      startedAt: testStartedAt.get(test.title) || new Date(Date.now() - res.duration).toISOString(),
      completedAt: new Date().toISOString(),
      video,
      error: res.error ? String(res.error.message || res.error) : undefined,
    });
  },
  after: () => {
    mkdirSync(artifactsDir, { recursive: true });
    const passed = results.filter((result) => result.passed).length;
    writeFileSync(
      reportPath,
      JSON.stringify({
        probierz: { runId, captureErrors },
        total: results.length,
        passed,
        failed: results.length - passed,
        tests: results,
      }, null, Number('2')),
    );
  },
};

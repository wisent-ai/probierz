import type { Options } from '@wdio/types';
import { driver } from '@wdio/globals';
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
type RecordingDriver = typeof driver & {
  startRecordingScreen(): Promise<void>;
  stopRecordingScreen(): Promise<string>;
};
const recordingDriver = driver as RecordingDriver;

// Recording + result capture for probierz. PROBIERZ_RECORD=1|true records each
// test through the driver's native API. On macOS, a ScreenCaptureKit recorder
// runs in parallel and becomes the evidence when Mac2 returns no video.
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
  recordingBackend?: 'webdriver' | 'screen-capture-kit';
  error?: string;
}> = [];
const testStartedAt = new Map<string, string>();
const slug = (s: string) => s.replace(/[^a-z\d]+/gi, '_').replace(/^_+|_+$/g, '').toLowerCase();
const specGlob = process.env.PROBIERZ_SPEC
  ? [`./test/specs/**/${process.env.PROBIERZ_SPEC.split('/').pop()}`]
  : ['./test/specs/**/*.e2e.ts'];
const captureBinary = process.env.PROBIERZ_NATIVE_CAPTURE_BIN
  || resolve(process.cwd(), '../../node_modules/.cache/probierz/screen-capture-kit');
let nativeCapture: ChildProcess | undefined;
let nativeCaptureClosed: Promise<number | null> | undefined;
let nativeCaptureFile: string | undefined;
let nativeCaptureStderr = '';

function startNativeCapture(testTitle: string) {
  if (process.platform !== 'darwin' || !process.env.MAC_BUNDLE_ID) return;
  if (!existsSync(captureBinary)) {
    captureErrors.push(`screen-capture-kit helper missing: run probierz setup desktop:mac`);
    return;
  }
  mkdirSync(artifactsDir, { recursive: true });
  nativeCaptureFile = join(artifactsDir, `${slug(testTitle)}.screen-capture-kit.mp4`);
  nativeCaptureStderr = '';
  const child = spawn(captureBinary, [
    '--bundle-id', process.env.MAC_BUNDLE_ID,
    '--output', nativeCaptureFile,
    '--wait-seconds', '60',
  ], { stdio: ['ignore', 'pipe', 'pipe'] });
  nativeCapture = child;
  const completion = Promise.withResolvers<number | null>();
  nativeCaptureClosed = completion.promise;
  child.once('close', completion.resolve);
  child.once('error', (error) => {
    captureErrors.push(`screen-capture-kit spawn failed: ${error.message}`);
    completion.resolve(null);
  });
  child.stderr?.on('data', (chunk) => {
    nativeCaptureStderr = (nativeCaptureStderr + String(chunk)).slice(-2000);
  });
}

async function stopNativeCapture(): Promise<string | undefined> {
  const child = nativeCapture;
  const file = nativeCaptureFile;
  const closed = nativeCaptureClosed;
  nativeCapture = undefined;
  nativeCaptureClosed = undefined;
  nativeCaptureFile = undefined;
  if (!child || !file || !closed) return undefined;
  child.kill('SIGINT');
  const timeout = Promise.withResolvers<'timeout'>();
  const timeoutHandle = setTimeout(() => timeout.resolve('timeout'), 10_000);
  const code = await Promise.race([closed, timeout.promise]);
  clearTimeout(timeoutHandle);
  if (code === 'timeout') {
    child.kill('SIGKILL');
    await closed;
    captureErrors.push(`screen-capture-kit stop timed out`);
  } else if (code !== 0) {
    captureErrors.push(`screen-capture-kit exited ${code}: ${nativeCaptureStderr.trim() || 'no diagnostic'}`);
  }
  if (existsSync(file) && statSync(file).size > 0) return file;
  return undefined;
}

export const shared: Partial<Options.Testrunner> = {
  runner: 'local',
  specs: specGlob,
  maxInstances: 1,
  logLevel: 'info',
  waitforTimeout: 20000,
  connectionRetryTimeout: 120000,
  connectionRetryCount: 3,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120000 },
  beforeTest: async (test: { title: string }) => {
    testStartedAt.set(test.title, new Date().toISOString());
    if (!record) return;
    try {
      await recordingDriver.startRecordingScreen();
    } catch (error) {
      captureErrors.push(`start recording: ${error instanceof Error ? error.message : String(error)}`);
    }
    startNativeCapture(test.title);
  },
  afterTest: async (
    test: { title: string },
    _ctx: unknown,
    res: { passed: boolean; duration: number; error?: Error },
  ) => {
    let video: string | undefined;
    let recordingBackend: 'webdriver' | 'screen-capture-kit' | undefined;
    if (record) {
      try {
        const b64 = await recordingDriver.stopRecordingScreen();
        if (b64) {
          mkdirSync(artifactsDir, { recursive: true });
          video = join(artifactsDir, `${slug(test.title)}.mp4`);
          writeFileSync(video, Buffer.from(b64, 'base64'));
          recordingBackend = 'webdriver';
        }
      } catch (error) {
        captureErrors.push(`stop recording for ${test.title}: ${error instanceof Error ? error.message : String(error)}`);
      }
      const fallbackVideo = await stopNativeCapture();
      if (!video && fallbackVideo) {
        video = fallbackVideo;
        recordingBackend = 'screen-capture-kit';
      } else if (video && fallbackVideo) {
        unlinkSync(fallbackVideo);
      }
    }
    results.push({
      title: test.title,
      passed: res.passed,
      duration: res.duration,
      startedAt: testStartedAt.get(test.title) || new Date(Date.now() - res.duration).toISOString(),
      completedAt: new Date().toISOString(),
      video,
      recordingBackend,
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

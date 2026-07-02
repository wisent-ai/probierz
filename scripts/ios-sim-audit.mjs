#!/usr/bin/env node
import { spawnSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const WISENT_ROOT = resolve(process.env.WISENT_ROOT || '/Users/lukaszbartoszcze/Documents/CodingProjects/Wisent');
const DEVICE_NAME = process.env.IOS_SIM_DEVICE_NAME || 'iPhone 17';
const TS = new Date().toISOString().replace(/[:.]/g, '-');
const OUT = resolve(process.env.AUDIT_OUT || join(process.cwd(), '.work/audits/mobile-ios', TS));
const ONLY = new Set((process.argv.find((arg) => arg.startsWith('--only='))?.slice('--only='.length) || '')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean));
const XCODE_BUILD_SETTINGS = [
  'COMPILATION_CACHE_ENABLE_CACHING=NO',
];

const TARGETS = [
  {
    name: 'wisent-ios',
    product: 'Wisent AI',
    repo: 'wisent-ios',
    workspace: 'wisent-ios/Wisent/Wisent.xcworkspace',
    scheme: 'Wisent',
  },
  {
    name: 'turbot-ios',
    product: 'Turbot',
    repo: 'turbot-ios',
    project: 'turbot-ios/Wisent.xcodeproj',
    scheme: 'Turbot',
  },
  {
    name: 'oko-ios',
    product: 'Oko / Swiatowid',
    repo: 'swiatowid-ios',
    project: 'swiatowid-ios/Oko.xcodeproj',
    scheme: 'Oko',
  },
  {
    name: 'byk-ios',
    product: 'Byk',
    repo: 'byk-ios',
    project: 'byk-ios/Byk.xcodeproj',
    scheme: 'Byk',
  },
  {
    name: 'trading-ios',
    product: 'Trading',
    repo: 'trading-ios',
    packageOnly: true,
    note: 'Swift package scaffold only; README says empty scaffold, no .app target to launch.',
  },
];

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    cwd: opts.cwd || WISENT_ROOT,
    encoding: 'utf8',
    maxBuffer: opts.maxBuffer || 30 * 1024 * 1024,
    stdio: opts.stdio || 'pipe',
  });
  return {
    code: res.status ?? 1,
    stdout: res.stdout || '',
    stderr: res.stderr || '',
    signal: res.signal || null,
  };
}

function safeName(s) {
  return s.replace(/[^a-zA-Z0-9_.-]+/g, '-');
}

function writeLog(name, phase, text) {
  const file = join(OUT, `${safeName(name)}_${phase}.log`);
  writeFileSync(file, text);
  return file;
}

function findDevice(name) {
  const res = run('xcrun', ['simctl', 'list', 'devices', 'available', '-j']);
  if (res.code !== 0) {
    throw new Error(`simctl list failed: ${res.stderr || res.stdout}`);
  }
  const json = JSON.parse(res.stdout);
  for (const [runtime, devices] of Object.entries(json.devices || {})) {
    const match = devices.find((d) => d.name === name && d.isAvailable);
    if (match) return { ...match, runtime };
  }
  for (const [runtime, devices] of Object.entries(json.devices || {})) {
    const match = devices.find((d) => d.isAvailable && d.name.startsWith('iPhone'));
    if (match) return { ...match, runtime };
  }
  throw new Error(`No available iPhone simulator found`);
}

function bootDevice(udid) {
  const boot = run('xcrun', ['simctl', 'boot', udid]);
  if (boot.code !== 0 && !`${boot.stderr}${boot.stdout}`.includes('Unable to boot device in current state: Booted')) {
    throw new Error(`simctl boot failed: ${boot.stderr || boot.stdout}`);
  }
  const status = run('xcrun', ['simctl', 'bootstatus', udid, '-b'], { maxBuffer: 2 * 1024 * 1024 });
  if (status.code !== 0) {
    throw new Error(`simctl bootstatus failed: ${status.stderr || status.stdout}`);
  }
}

function parseBuildSettings(text) {
  const fields = {};
  for (const line of text.split('\n')) {
    const match = line.match(/^\s*(BUILT_PRODUCTS_DIR|FULL_PRODUCT_NAME|PRODUCT_BUNDLE_IDENTIFIER)\s=\s(.+?)\s*$/);
    if (match) fields[match[1]] = match[2];
  }
  return fields;
}

function xcodeContainerArgs(target) {
  if (target.workspace) {
    const workspacePath = join(WISENT_ROOT, target.workspace);
    return {
      kind: 'workspace',
      path: workspacePath,
      args: ['-workspace', workspacePath],
    };
  }
  const projectPath = join(WISENT_ROOT, target.project);
  return {
    kind: 'project',
    path: projectPath,
    args: ['-project', projectPath],
  };
}

async function recordVideo(udid, output, durationMs) {
  const proc = spawn('xcrun', ['simctl', 'io', udid, 'recordVideo', output], {
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let stderr = '';
  proc.stderr.on('data', (d) => { stderr += d.toString(); });
  await new Promise((r) => setTimeout(r, durationMs));
  proc.kill('SIGINT');
  return await new Promise((resolveDone) => {
    proc.on('close', (code, signal) => resolveDone({ code, signal, stderr }));
  });
}

async function auditTarget(target, udid) {
  const rec = {
    name: target.name,
    product: target.product,
    repo: target.repo,
    status: 'PENDING',
  };

  if (target.packageOnly) {
    rec.status = 'SKIPPED_NO_APP_TARGET';
    rec.note = target.note;
    return rec;
  }

  const container = xcodeContainerArgs(target);
  if (!existsSync(container.path)) {
    rec.status = `FAIL_${container.kind.toUpperCase()}_MISSING`;
    rec.error = container.path;
    return rec;
  }

  const buildArgs = [
    ...container.args,
    '-scheme', target.scheme,
    '-configuration', 'Debug',
    '-destination', `id=${udid}`,
    ...XCODE_BUILD_SETTINGS,
    'build',
  ];
  const build = run('xcodebuild', buildArgs, { maxBuffer: 80 * 1024 * 1024 });
  rec.buildLog = writeLog(target.name, 'build', `${build.stdout}\n${build.stderr}`);
  if (build.code !== 0) {
    rec.status = 'FAIL_BUILD';
    rec.error = (build.stderr || build.stdout).slice(-4000);
    return rec;
  }

  const settings = run('xcodebuild', [
    '-showBuildSettings',
    ...container.args,
    '-scheme', target.scheme,
    '-configuration', 'Debug',
    '-destination', `id=${udid}`,
    ...XCODE_BUILD_SETTINGS,
  ], { maxBuffer: 30 * 1024 * 1024 });
  rec.settingsLog = writeLog(target.name, 'settings', `${settings.stdout}\n${settings.stderr}`);
  if (settings.code !== 0) {
    rec.status = 'FAIL_BUILD_SETTINGS';
    rec.error = (settings.stderr || settings.stdout).slice(-4000);
    return rec;
  }

  const parsed = parseBuildSettings(settings.stdout);
  rec.bundleId = parsed.PRODUCT_BUNDLE_IDENTIFIER;
  rec.appPath = parsed.BUILT_PRODUCTS_DIR && parsed.FULL_PRODUCT_NAME
    ? join(parsed.BUILT_PRODUCTS_DIR, parsed.FULL_PRODUCT_NAME)
    : '';
  if (!rec.bundleId || !rec.appPath || !existsSync(rec.appPath)) {
    rec.status = 'FAIL_APP_NOT_FOUND';
    rec.error = `bundleId=${rec.bundleId || ''} appPath=${rec.appPath || ''}`;
    return rec;
  }

  const install = run('xcrun', ['simctl', 'install', udid, rec.appPath]);
  rec.installLog = writeLog(target.name, 'install', `${install.stdout}\n${install.stderr}`);
  if (install.code !== 0) {
    rec.status = 'FAIL_INSTALL';
    rec.error = (install.stderr || install.stdout).slice(-4000);
    return rec;
  }

  const launch = run('xcrun', ['simctl', 'launch', udid, rec.bundleId]);
  rec.launchLog = writeLog(target.name, 'launch', `${launch.stdout}\n${launch.stderr}`);
  if (launch.code !== 0) {
    rec.status = 'FAIL_LAUNCH';
    rec.error = (launch.stderr || launch.stdout).slice(-4000);
    return rec;
  }

  const video = `${target.name}_${TS}.mov`;
  const screenshot = `${target.name}_${TS}.png`;
  const videoPath = join(OUT, video);
  const screenshotPath = join(OUT, screenshot);
  const videoResult = await recordVideo(udid, videoPath, Number(process.env.IOS_AUDIT_VIDEO_MS || 5000));
  rec.video = video;
  rec.videoLog = writeLog(target.name, 'video', JSON.stringify(videoResult, null, 2));

  const shot = run('xcrun', ['simctl', 'io', udid, 'screenshot', screenshotPath]);
  rec.screenshotLog = writeLog(target.name, 'screenshot', `${shot.stdout}\n${shot.stderr}`);
  if (shot.code === 0 && existsSync(screenshotPath)) {
    rec.screenshot = screenshot;
  }
  if (!rec.screenshot) {
    rec.status = 'FAIL_SCREENSHOT';
    rec.error = (shot.stderr || shot.stdout).slice(-4000);
    return rec;
  }

  rec.status = 'PASS_LAUNCHED_CAPTURED';
  return rec;
}

function writeReports(device, results) {
  const pass = results.filter((r) => r.status.startsWith('PASS')).length;
  const fail = results.filter((r) => r.status.startsWith('FAIL')).length;
  const skipped = results.filter((r) => r.status.startsWith('SKIPPED')).length;
  const productCount = new Set(results.map((r) => r.product)).size;
  const summary = { ts: TS, device, total: results.length, productCount, pass, fail, skipped, results };
  writeFileSync(join(OUT, `ios_report_${TS}.json`), JSON.stringify(summary, null, 2));

  const md = [
    '# iOS Simulator Product Audit',
    '',
    `Generated: ${TS}`,
    `Simulator: ${device.name} (${device.udid})`,
    '',
    `Products: ${productCount}`,
    `Targets: ${results.length}`,
    `Pass: ${pass}`,
    `Fail: ${fail}`,
    `Skipped: ${skipped}`,
    '',
    '| Status | Product | Target | Bundle | Screenshot | Video | Note |',
    '|---|---|---|---|---|---|---|',
    ...results.map((r) => `| ${r.status} | ${r.product} | ${r.name} | ${r.bundleId || ''} | ${r.screenshot || ''} | ${r.video || ''} | ${(r.note || r.error || '').replace(/\n/g, ' ').slice(0, 180)} |`),
    '',
  ].join('\n');
  writeFileSync(join(OUT, `ios_report_${TS}.md`), md);
  return summary;
}

if (!existsSync(OUT)) mkdirSync(OUT, { recursive: true });

const device = findDevice(DEVICE_NAME);
console.log(`[ios-audit] using ${device.name} ${device.udid} ${device.runtime}`);
bootDevice(device.udid);

const results = [];
for (const target of TARGETS.filter((target) => ONLY.size === 0 || ONLY.has(target.name))) {
  console.log(`[ios-audit] ${target.name}: start`);
  const result = await auditTarget(target, device.udid);
  results.push(result);
  console.log(`[ios-audit] ${target.name}: ${result.status}`);
}

const summary = writeReports(device, results);
console.log(`[ios-audit] SUMMARY products=${summary.productCount} targets=${summary.total} pass=${summary.pass} fail=${summary.fail} skipped=${summary.skipped}`);
console.log(`[ios-audit] report -> ${join(OUT, `ios_report_${TS}.md`)}`);

process.exitCode = summary.fail > 0 ? 1 : 0;

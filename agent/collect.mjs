import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";

const SAMPLE_INTERVAL_MS = 1000;
const ERROR_LINE = /\b(?:crash(?:ed)?|fatal|panic|uncaught|unhandled|segmentation fault|assertion failed)\b/i;
function redactDiagnostic(text) {
  return String(text || "")
    .replace(/\bBearer\s+[A-Za-z0-9._~+/=-]+/gi, "Bearer [REDACTED]")
    .replace(/\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/gi, "[EMAIL]")
    .replace(/((?:auth|cookie|credential|key|otp|password|secret|session|token)[A-Z0-9_.-]*\s*[=:]\s*)[^\s,;]+/gi, "$1[REDACTED]")
    .replace(/([?&][^=\s&]+)=([^&\s]+)/g, "$1=[VALUE]");
}

function writeJson(file, value) {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
}

function processGroupSample(pgid) {
  if (process.platform === "win32") return null;
  const result = spawnSync("ps", ["-axo", "pgid=,rss=,%cpu="], { encoding: "utf8", timeout: 3000 });
  if (result.status !== 0) return null;
  let rssKb = 0;
  let cpuPercent = 0;
  let processes = 0;
  for (const line of result.stdout.split(/\r?\n/)) {
    const [group, rss, cpu] = line.trim().split(/\s+/);
    if (Number(group) !== pgid) continue;
    rssKb += Number(rss || 0);
    cpuPercent += Number(cpu || 0);
    processes += 1;
  }
  return { at: new Date().toISOString(), processes, rssKb, cpuPercent };
}

function namedProcessSample(processName) {
  if (!processName || process.platform === "win32") return null;
  const result = spawnSync("ps", ["-axo", "comm=,rss=,%cpu="], { encoding: "utf8", timeout: 3000 });
  if (result.status !== 0) return null;
  let rssKb = 0;
  let cpuPercent = 0;
  let processes = 0;
  for (const line of result.stdout.split(/\r?\n/)) {
    const match = line.trim().match(/^(.*?)\s+(\d+)\s+([\d.]+)$/);
    if (!match || path.basename(match[1]) !== processName) continue;
    rssKb += Number(match[2]);
    cpuPercent += Number(match[3]);
    processes += 1;
  }
  return processes ? { processes, rssKb, cpuPercent } : null;
}

export function startPerformanceSampler(child, artifactsDir, { target, env = {} } = {}) {
  const samples = [];
  const processName = appProcessName(target, env);
  const sample = () => {
    const value = child.pid ? processGroupSample(child.pid) : null;
    if (value) samples.push({ ...value, app: namedProcessSample(processName) });
  };
  sample();
  const timer = setInterval(sample, SAMPLE_INTERVAL_MS);
  timer.unref();
  let stopped = false;
  return {
    stop(firstOutputMs = null) {
      if (stopped) return null;
      stopped = true;
      clearInterval(timer);
      sample();
      const rssValues = samples.map((item) => item.rssKb);
      const cpuValues = samples.map((item) => item.cpuPercent);
      const appRssValues = samples.flatMap((item) => item.app ? [item.app.rssKb] : []);
      const appCpuValues = samples.flatMap((item) => item.app ? [item.app.cpuPercent] : []);
      const result = {
        schemaVersion: 1,
        subject: "run-and-app-processes",
        firstOutputMs,
        intervalMs: SAMPLE_INTERVAL_MS,
        peakRssKb: rssValues.length ? Math.max(...rssValues) : null,
        averageCpuPercent: cpuValues.length
          ? cpuValues.reduce((total, value) => total + value, 0) / cpuValues.length
          : null,
        appProcessName: processName,
        appPeakRssKb: appRssValues.length ? Math.max(...appRssValues) : null,
        appAverageCpuPercent: appCpuValues.length
          ? appCpuValues.reduce((total, value) => total + value, 0) / appCpuValues.length
          : null,
        samples,
      };
      const file = path.join(artifactsDir, "performance.json");
      writeJson(file, result);
      return { file, ...result, samples: undefined };
    },
  };
}

function appProcessName(target, env) {
  const appPath = ["desktop:mac", "desktop:cua"].includes(target) ? env.MAC_APP_PATH : env.APP_IOS;
  if (!appPath) return null;
  return path.basename(String(appPath), path.extname(String(appPath)));
}

function simulatorIdentifier(requested) {
  const value = String(requested || "booted");
  if (value === "booted") return value;
  const list = spawnSync("xcrun", ["simctl", "list", "devices", "available", "--json"], {
    encoding: "utf8",
    timeout: 5000,
  });
  if (list.status !== 0) return value;
  try {
    const devices = Object.values(JSON.parse(list.stdout).devices || {}).flat();
    const candidates = devices.filter((device) => device.udid === value || device.name === value);
    return (candidates.find((device) => device.state === "Booted") || candidates[0])?.udid || value;
  } catch {
    return value;
  }
}
function logInterval(startedAt) {
  const started = Date.parse(startedAt);
  const elapsedSeconds = Number.isFinite(started)
    ? Math.ceil((Date.now() - started) / 1000) + 5
    : 60;
  return ["--last", `${Math.max(1, elapsedSeconds)}s`];
}


export function collectPlatformDiagnostics({ target, env, artifactsDir, startedAt }) {
  const directory = path.join(artifactsDir, "diagnostics");
  mkdirSync(directory, { recursive: true });
  const processName = appProcessName(target, env);
  let command = null;
  let args = null;
  let file = null;
  if (["desktop:mac", "desktop:cua"].includes(target) && processName) {
    command = "/usr/bin/log";
    args = ["show", "--style", "compact", ...logInterval(startedAt), "--predicate", `process == "${processName}"`];
    file = path.join(directory, "macos-unified.log");
  } else if (target === "mobile:ios" && processName) {
    command = "xcrun";
    args = ["simctl", "spawn", simulatorIdentifier(env.IOS_DEVICE), "log", "show", "--style", "compact", ...logInterval(startedAt), "--predicate", `process == "${processName}"`];
    file = path.join(directory, "ios-simulator.log");
  } else if (target === "mobile:android") {
    command = "adb";
    args = [...(env.ANDROID_DEVICE ? ["-s", env.ANDROID_DEVICE] : []), "logcat", "-d", "-v", "threadtime"];
    file = path.join(directory, "android-logcat.log");
  }
  if (!command || !file) return { supported: false, file: null };
  let result;
  try {
    result = spawnSync(command, args, {
      encoding: "utf8",
      timeout: 20_000,
      maxBuffer: 16 * 1024 * 1024,
    });
  } catch (error) {
    writeFileSync(file, "", { mode: 0o600 });
    return {
      supported: true,
      file,
      ok: false,
      exitCode: -1,
      error: redactDiagnostic(error instanceof Error ? error.message : String(error)).slice(-2000),
    };
  }
  const output = redactDiagnostic(result.stdout);
  const error = redactDiagnostic(result.stderr);
  writeFileSync(file, output, { mode: 0o600 });
  return {
    supported: true,
    file,
    ok: result.status === 0,
    exitCode: result.status === null ? -1 : result.status,
    error: result.status === 0 ? null : (error.trim() || result.error?.message || "collector failed").slice(-2000),
  };
}

function percentile(values, fraction) {
  if (!values.length) return null;
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)];
}
function platformCrashLines(artifactsDir) {
  const directory = path.join(artifactsDir, "diagnostics");
  if (!existsSync(directory)) return [];
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    if (!entry.isFile() || !entry.name.endsWith(".log")) return [];
    const file = path.join(directory, entry.name);
    return readFileSync(file, "utf8").split(/\r?\n/).flatMap((line) => (
      ERROR_LINE.test(line)
        ? [{ source: entry.name, message: redactDiagnostic(line).slice(0, 500) }]
        : []
    ));
  });
}


export function summarizeDiagnostics({ report, timeline, artifactsDir }) {
  const assertions = (report.tests || []).map((test) => Number(test.duration || test.durationMs || 0)).filter(Number.isFinite);
  const network = timeline.events.filter((event) => event.type === "network");
  const logCrashes = timeline.events
    .filter((event) => event.type === "log" && ERROR_LINE.test(event.message || ""))
    .map((event) => ({ at: event.at, source: event.source, message: String(event.message).slice(0, 500) }));
  const platformCrashes = platformCrashLines(artifactsDir);
  const networkErrors = network
    .filter((event) => Number(event.status || 0) >= 400)
    .map((event) => ({ at: event.at, method: event.method, url: event.url, status: event.status }));
  const consoleErrors = timeline.events
    .filter((event) => event.type === "console" && /^(?:assert|error)$/i.test(event.severity || ""))
    .map((event) => ({ at: event.at, severity: event.severity, message: redactDiagnostic(event.message).slice(0, 500) }));
  const networkDurations = network.map((event) => Number(event.durationMs || 0)).filter((value) => value > 0);
  const performancePath = path.join(artifactsDir, "performance.json");
  const processPerformance = existsSync(performancePath)
    ? JSON.parse(readFileSync(performancePath, "utf8"))
    : null;
  const result = {
    schemaVersion: 1,
    runId: report?.probierz?.runId || null,
    crashes: [...logCrashes, ...platformCrashes],
    networkErrors,
    consoleErrors,
    performance: {
      tests: {
        count: assertions.length,
        p50Ms: percentile(assertions, 0.5),
        p95Ms: percentile(assertions, 0.95),
        maxMs: assertions.length ? Math.max(...assertions) : null,
      },
      network: {
        count: networkDurations.length,
        p50Ms: percentile(networkDurations, 0.5),
        p95Ms: percentile(networkDurations, 0.95),
        maxMs: networkDurations.length ? Math.max(...networkDurations) : null,
      },
      process: processPerformance && {
        firstOutputMs: processPerformance.firstOutputMs,
        peakRssKb: processPerformance.peakRssKb,
        averageCpuPercent: processPerformance.averageCpuPercent,
        appProcessName: processPerformance.appProcessName,
        appPeakRssKb: processPerformance.appPeakRssKb,
        appAverageCpuPercent: processPerformance.appAverageCpuPercent,
      },
    },
  };
  const file = path.join(artifactsDir, "diagnostics.json");
  writeJson(file, result);
  return { file, ...result };
}

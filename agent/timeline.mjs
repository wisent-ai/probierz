import { existsSync, readFileSync, statSync } from "node:fs";
import { inflateRawSync } from "node:zlib";
import path from "node:path";

const ZIP_EOCD = 0x06054b50;
const ZIP_CENTRAL = 0x02014b50;
const ZIP_LOCAL = 0x04034b50;
const SENSITIVE_QUERY = /(auth|code|cookie|credential|email|key|otp|password|secret|session|token)/i;

function iso(value, fallback) {
  const date = value ? new Date(value) : null;
  return date && Number.isFinite(date.getTime()) ? date.toISOString() : fallback;
}

function safeUrl(value) {
  try {
    const url = new URL(value);
    url.username = "";
    const segments = url.pathname.split("/");
    for (let index = 0; index < segments.length; index += 1) {
      const decoded = decodeURIComponent(segments[index] || "");
      const previous = decodeURIComponent(segments[index - 1] || "");
      if (
        /@/.test(decoded)
        || /^[A-Za-z0-9_-]{32,}$/.test(decoded)
        || (SENSITIVE_QUERY.test(previous) && decoded)
      ) {
        segments[index] = "%5BREDACTED%5D";
      }
    }
    url.pathname = segments.join("/");
    url.hash = "";
    url.password = "";
    for (const name of url.searchParams.keys()) {
      url.searchParams.set(name, SENSITIVE_QUERY.test(name) ? "[REDACTED]" : "[VALUE]");
    }
    return url.toString();
  } catch {
    return String(value).replace(/([?&][^=]+)=([^&\s]+)/g, "$1=[VALUE]");
  }
}
function safeMessage(value) {
  return String(value || "")
    .replace(/\bBearer\s+[A-Za-z0-9._~+/=-]+/gi, "Bearer [REDACTED]")
    .replace(/\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b/gi, "[EMAIL]")
    .replace(/((?:auth|cookie|credential|key|otp|password|secret|session|token)[A-Z0-9_.-]*\s*[=:]\s*)[^\s,;]+/gi, "$1[REDACTED]");
}


function zipEntries(file) {
  const buffer = readFileSync(file);
  let end = -1;
  for (let offset = buffer.length - 22; offset >= Math.max(0, buffer.length - 65_557); offset -= 1) {
    if (buffer.readUInt32LE(offset) === ZIP_EOCD) {
      end = offset;
      break;
    }
  }
  if (end < 0) throw new Error("zip end record missing");
  const count = buffer.readUInt16LE(end + 10);
  let offset = buffer.readUInt32LE(end + 16);
  const entries = [];
  for (let index = 0; index < count; index += 1) {
    if (buffer.readUInt32LE(offset) !== ZIP_CENTRAL) throw new Error("invalid zip central directory");
    const method = buffer.readUInt16LE(offset + 10);
    const compressedSize = buffer.readUInt32LE(offset + 20);
    const nameLength = buffer.readUInt16LE(offset + 28);
    const extraLength = buffer.readUInt16LE(offset + 30);
    const commentLength = buffer.readUInt16LE(offset + 32);
    const localOffset = buffer.readUInt32LE(offset + 42);
    const name = buffer.subarray(offset + 46, offset + 46 + nameLength).toString("utf8");
    if (buffer.readUInt32LE(localOffset) !== ZIP_LOCAL) throw new Error("invalid zip local header");
    const localNameLength = buffer.readUInt16LE(localOffset + 26);
    const localExtraLength = buffer.readUInt16LE(localOffset + 28);
    const dataOffset = localOffset + 30 + localNameLength + localExtraLength;
    const compressed = buffer.subarray(dataOffset, dataOffset + compressedSize);
    let content;
    if (method === 0) content = compressed;
    else if (method === 8) content = inflateRawSync(compressed);
    if (content) entries.push({ name, content: content.toString("utf8") });
    offset += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}

function jsonLines(content) {
  const values = [];
  for (const line of content.split(/\r?\n/)) {
    if (!line.trim()) continue;
    try { values.push(JSON.parse(line)); }
    catch { /* Trace archives may contain forward-version records; ignore only malformed rows. */ }
  }
  return values;
}

function traceEvents(traceFile, fallbackAt, diagnostics) {
  try {
    const entries = zipEntries(traceFile);
    const traceRows = entries.filter((entry) => entry.name.endsWith(".trace")).flatMap((entry) => jsonLines(entry.content));
    const context = traceRows.find((row) => row.type === "context-options" && row.wallTime && row.monotonicTime);
    const wallTime = Number(context?.wallTime || 0);
    const monotonicTime = Number(context?.monotonicTime || 0);
    const atFor = (value) => wallTime && Number(value)
      ? new Date(wallTime + (Number(value) - monotonicTime)).toISOString()
      : fallbackAt;
    const rows = entries.filter((entry) => entry.name.endsWith(".network")).flatMap((entry) => jsonLines(entry.content));
    const network = rows.flatMap((row) => {
      if (row.type !== "resource-snapshot" || !row.snapshot?.request?.url) return [];
      const snapshot = row.snapshot;
      return [{
        at: atFor(snapshot._monotonicTime || row.monotonicTime),
        type: "network",
        source: path.basename(traceFile),
        method: snapshot.request.method || null,
        url: safeUrl(snapshot.request.url),
        status: Number(snapshot.response?.status || 0) || null,
        durationMs: Number(snapshot.time || 0) || null,
      }];
    });
    const consoleEvents = traceRows.flatMap((row) => {
      const params = row.params || row;
      const method = String(row.method || "").toLowerCase();
      const isConsole = row.type === "console"
        || method === "console"
        || method === "pageerror"
        || method === "page-error";
      if (!isConsole) return [];
      return [{
        at: atFor(row.time || row.monotonicTime),
        type: "console",
        source: path.basename(traceFile),
        severity: method.includes("error") ? "error" : (params.type || params.messageType || "log"),
        message: safeMessage(params.text || params.message).slice(0, 2000),
      }];
    });
    return [...network, ...consoleEvents];
  } catch (error) {
    diagnostics.push({ artifact: traceFile, error: error instanceof Error ? error.message : String(error) });
    return [];
  }
}
function jsonTraceEvents(traceFile, fallbackAt, diagnostics) {
  try {
    const document = JSON.parse(readFileSync(traceFile, "utf8"));
    if (
      document?.schemaVersion !== 1
      || typeof document.kind !== "string"
      || !document.kind.startsWith("probierz-")
      || document.status !== "completed"
    ) {
      throw new Error("invalid Probierz JSON trace");
    }
    return [{
      at: iso(document.completedAt, fallbackAt),
      type: "observation",
      source: path.basename(traceFile),
      status: document.status,
      message: safeMessage(document.observation?.reply || "").slice(0, 2000),
    }];
  } catch (error) {
    diagnostics.push({ artifact: traceFile, error: error instanceof Error ? error.message : String(error) });
    return [];
  }
}


function logEvents(file, source) {
  if (!file || !existsSync(file)) return [];
  return readFileSync(file, "utf8").split(/\r?\n/).flatMap((line) => {
    const match = /^(\d{4}-\d{2}-\d{2}T\S+)\s(.*)$/.exec(line);
    if (!match) return [];
    return [{ at: iso(match[1], new Date(statSync(file).mtimeMs).toISOString()), type: "log", source, message: safeMessage(match[2]) }];
  });
}

export function buildTimeline({ report, summary, media, artifactsDir, stdoutPath, stderrPath, startedAt }) {
  const diagnostics = [];
  const fallbackAt = iso(startedAt, new Date().toISOString());
  const events = [
    ...logEvents(stdoutPath, "stdout"),
    ...logEvents(stderrPath, "stderr"),
  ];
  let cursor = new Date(fallbackAt).getTime();
  for (const test of report.tests || []) {
    const durationMs = Number(test.duration || test.durationMs || 0);
    const at = iso(test.startedAt, new Date(cursor).toISOString());
    const completedAt = iso(test.completedAt, new Date(new Date(at).getTime() + durationMs).toISOString());
    events.push({
      at,
      completedAt,
      durationMs,
      type: "assertion",
      source: summary.tool,
      title: test.title,
      status: test.status || (test.passed ? "passed" : "failed"),
      error: test.error || null,
    });
    cursor = new Date(completedAt).getTime();
  }
  for (const item of media) {
    const at = existsSync(item.file) ? new Date(statSync(item.file).mtimeMs).toISOString() : fallbackAt;
    events.push({
      at,
      type: item.kind === "screenshot" ? "screenshot" : item.kind,
      source: summary.tool,
      artifact: item.file,
      missing: Boolean(item.missing),
    });
    if (item.kind === "trace" && existsSync(item.file)) {
      events.push(...(
        item.contentType === "application/json"
          ? jsonTraceEvents(item.file, at, diagnostics)
          : traceEvents(item.file, at, diagnostics)
      ));
    }
  }
  events.sort((left, right) => left.at.localeCompare(right.at) || left.type.localeCompare(right.type));
  const counts = Object.fromEntries([...new Set(events.map((event) => event.type))].sort().map((type) => [
    type,
    events.filter((event) => event.type === type).length,
  ]));
  return {
    schemaVersion: 1,
    runId: report?.probierz?.runId || null,
    artifactsDir: artifactsDir || null,
    generatedAt: new Date().toISOString(),
    counts,
    diagnostics,
    events,
  };
}

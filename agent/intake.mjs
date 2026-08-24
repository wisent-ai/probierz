// Failure intake: the listener desktop apps post wisent-errors envelopes to,
// and the index an operator reads them back through.
//
// Two surfaces, one store. `serveIntake` is the network half: a bearer-tokened
// node:http endpoint that validates an envelope against the @wisent/errors
// catalogue and appends it as one JSON line under test-results/failures/.
// `failuresIndex` is the operator half: counts by service and error_code plus
// the newest envelopes. Neither half may fail its caller — the server answers
// every request (valid or not) with a classified envelope, and the index
// reports its own trouble through failure.mjs and still exits 0. No
// dependencies: the whole transport is node:http.
import { createServer } from "node:http";
import { randomBytes, timingSafeEqual } from "node:crypto";
import { appendFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CODES, FAILURE_POINT_PATTERN, failureOrFallback, trimDetail } from "@wisent/errors";
import { reportFailure } from "./failure.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
// probierz/agent -> probierz project root (same resolution as lib.mjs).
const ROOT = path.resolve(HERE, "..");
const FAILURES_DIR = path.join(ROOT, "test-results", "failures");

const DEFAULT_BIND = "127.0.0.1:9790";
const TOKEN_FILE = path.join(os.homedir(), ".probierz", "intake-token");

// The contract's bounds: one stored line never exceeds 64 KB, one service
// file never exceeds 10 MB, and a body that cannot fit the line cap is
// rejected rather than silently cut.
const MAX_LINE_BYTES = Number("64") * Number("1024");
const MAX_FILE_BYTES = Number("10") * Number("1024") * Number("1024");
const MAX_BODY_BYTES = MAX_LINE_BYTES;

const FAILURE_POINT = new RegExp(FAILURE_POINT_PATTERN);

function parseBind(bind) {
  const match = /^(.+):(\d+)$/.exec(String(bind || ""));
  if (!match) throw new Error(`--bind needs host:port, got ${JSON.stringify(bind)}`);
  const port = Number(match[Number("2")]);
  if (!Number.isInteger(port) || port < Number("1") || port > Number("65535")) {
    throw new Error(`--bind port out of range: ${match[Number("2")]}`);
  }
  return { host: match[Number("1")], port };
}

/**
 * The bearer, from the environment when set, otherwise a per-user file that
 * is created 0600 on first run. `created` tells the caller to print the token
 * once — the only time it is ever shown — so an operator can hand it to the
 * desktop apps through PROBIERZ_INTAKE_TOKEN there.
 */
function intakeToken() {
  const fromEnv = String(process.env.PROBIERZ_INTAKE_TOKEN || "").trim();
  if (fromEnv) return { token: fromEnv, created: false };
  if (existsSync(TOKEN_FILE)) {
    const existing = readFileSync(TOKEN_FILE, "utf8").trim();
    if (existing) return { token: existing, created: false };
  }
  mkdirSync(path.dirname(TOKEN_FILE), { recursive: true, mode: 0o700 });
  const token = randomBytes(Number("24")).toString("base64url");
  writeFileSync(TOKEN_FILE, `${token}\n`, { mode: 0o600 });
  return { token, created: true };
}

function authorized(request, token) {
  const header = String(request.headers.authorization || "");
  const presented = header.startsWith("Bearer ") ? header.slice("Bearer ".length) : "";
  const left = Buffer.from(presented);
  const right = Buffer.from(token);
  return left.length === right.length && timingSafeEqual(left, right);
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let size = Number("0");
    let overflow = false;
    request.on("data", (chunk) => {
      size += chunk.length;
      if (size > MAX_BODY_BYTES) {
        overflow = true;
        request.destroy();
        return;
      }
      chunks.push(chunk);
    });
    request.on("end", () => resolve(overflow ? null : Buffer.concat(chunks).toString("utf8")));
    request.on("error", reject);
  });
}

/**
 * Why an envelope is not acceptable, or null when it is. The three fields the
 * contract pins down: failure_point is the app's dotted path, error_code is a
 * code the catalogue knows, and service names who is reporting.
 */
function envelopeProblem(body) {
  if (!body || typeof body !== "object" || Array.isArray(body)) return "body is not a JSON object";
  if (typeof body.failure_point !== "string" || !FAILURE_POINT.test(body.failure_point)) {
    return "failure_point must be a dotted lowercase path";
  }
  if (!CODES.includes(body.error_code)) return `error_code must be one of ${CODES.join(", ")}`;
  if (typeof body.service !== "string" || !body.service.trim()) return "service must be a non-empty string";
  return null;
}

/**
 * The file a service's lines land in. The envelope's service names the
 * product; the file system gets only [a-z0-9-].
 */
function serviceFileName(service) {
  const clean = String(service).trim().toLowerCase().replace(/[^a-z0-9-]+/g, "-").replace(/^-+|-+$/g, "");
  return `${clean || "unknown"}.jsonl`;
}

/**
 * One stored line, never wider than the cap. Shed payload in the order it is
 * missed least: context first, then the cause chain, then detail — identity
 * fields (point, code, service) always survive.
 */
function boundedLine(envelope) {
  const stored = { ...envelope, received_at: new Date().toISOString() };
  if (Buffer.byteLength(JSON.stringify(stored)) <= MAX_LINE_BYTES) return JSON.stringify(stored);
  delete stored.context;
  if (Buffer.byteLength(JSON.stringify(stored)) <= MAX_LINE_BYTES) return JSON.stringify(stored);
  delete stored.cause;
  let detail = String(stored.detail ?? "");
  while (detail && Buffer.byteLength(JSON.stringify({ ...stored, detail })) > MAX_LINE_BYTES) {
    detail = trimDetail(detail, Math.floor(detail.length / Number("2")));
  }
  stored.detail = detail || null;
  return JSON.stringify(stored);
}

/**
 * Keep a service file under 10 MB by dropping its oldest half, aligned to a
 * line boundary so the first surviving line is still a complete envelope.
 */
function rotateIfNeeded(file, incomingBytes) {
  let size = Number("0");
  try {
    size = statSync(file).size;
  } catch {
    return;
  }
  if (size + incomingBytes <= MAX_FILE_BYTES) return;
  const content = readFileSync(file);
  const keepFrom = content.length - Math.floor(MAX_FILE_BYTES / Number("2"));
  const newline = content.indexOf(0x0a, Math.max(Number("0"), keepFrom));
  writeFileSync(file, newline >= Number("0") ? content.subarray(newline + Number("1")) : Buffer.alloc(Number("0")));
}

function answer(res, status, payload) {
  const text = JSON.stringify(payload);
  res.writeHead(status, { "content-type": "application/json", "content-length": Buffer.byteLength(text) });
  res.end(text);
}

/** Every non-2xx answer is a wisent-errors envelope: the failure, classified. */
function answerFailure(res, status, code, detail) {
  answer(res, status, failureOrFallback({
    failurePoint: "probierz.intake.request",
    code,
    service: "probierz-intake",
    impact: "failure-intake",
    detail,
  }));
}

async function handleRequest(request, res, token) {
  if (request.method !== "POST" || String(request.url || "").split("?")[Number("0")] !== "/v1/failures") {
    answerFailure(res, Number("404"), "not_found", "unknown route; the intake endpoint is POST /v1/failures");
    return;
  }
  if (!authorized(request, token)) {
    answerFailure(res, Number("401"), "auth", "missing or wrong bearer token");
    return;
  }
  const text = await readBody(request);
  if (text === null) {
    answerFailure(res, Number("400"), "unknown", `body exceeds the ${MAX_LINE_BYTES}-byte line cap`);
    return;
  }
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    answerFailure(res, Number("400"), "unknown", "body is not valid JSON");
    return;
  }
  const problem = envelopeProblem(body);
  if (problem) {
    answerFailure(res, Number("400"), "unknown", problem);
    return;
  }
  mkdirSync(FAILURES_DIR, { recursive: true });
  const line = boundedLine(body);
  const file = path.join(FAILURES_DIR, serviceFileName(body.service));
  rotateIfNeeded(file, Buffer.byteLength(line) + Number("1"));
  appendFileSync(file, `${line}\n`);
  answer(res, Number("202"), { accepted: true });
}

/**
 * Listen for failure envelopes. Resolves when the server closes; bind errors
 * reject and are reported by the CLI boundary like any other command failure.
 */
export async function serveIntake({ bind = DEFAULT_BIND } = {}) {
  const { host, port } = parseBind(bind);
  const { token, created } = intakeToken();
  if (created) {
    process.stderr.write(
      `probierz intake: generated a new intake token at ${TOKEN_FILE} (mode 0600), shown once:\n`
      + `${token}\n`
      + "Set PROBIERZ_INTAKE_TOKEN to this value in each desktop app.\n",
    );
  }
  const server = createServer((request, res) => {
    handleRequest(request, res, token).catch((error) => {
      // The intake answering at all matters more than any single append: a
      // store failure is our outage, classified and returned, never a hang.
      answerFailure(res, Number("500"), "infra_down", `intake store failed: ${error?.message || error}`);
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, () => {
      process.stderr.write(`probierz intake: listening on http://${host}:${port}/v1/failures\n`);
      resolve();
    });
  });
  await new Promise((resolve) => server.on("close", resolve));
}

/**
 * The index over test-results/failures/*.jsonl: counts grouped by service and
 * error_code, plus the newest envelopes. Never throws — a failure index that
 * fails is the outage this contract exists to end — so read and parse trouble
 * is reported through failure.mjs and folded into the report instead.
 */
export function failuresIndex({ service = null, limit = Number("10") } = {}) {
  const unparsed = { count: Number("0") };
  let names = [];
  try {
    if (existsSync(FAILURES_DIR)) {
      names = readdirSync(FAILURES_DIR).filter((name) => name.endsWith(".jsonl")).sort();
    }
  } catch (error) {
    reportFailure({ point: "cli.failures", error, action: "list the failures index" });
  }
  if (service) names = names.filter((name) => name === serviceFileName(service));

  const envelopes = [];
  for (const name of names) {
    let text;
    try {
      text = readFileSync(path.join(FAILURES_DIR, name), "utf8");
    } catch (error) {
      reportFailure({ point: "cli.failures", error, action: `read ${name}` });
      continue;
    }
    for (const line of text.split("\n")) {
      if (!line.trim()) continue;
      try {
        envelopes.push(JSON.parse(line));
      } catch {
        unparsed.count += Number("1");
      }
    }
  }

  const grouped = new Map();
  for (const envelope of envelopes) {
    const key = `${envelope.service || "unknown"} ${envelope.error_code || "unknown"}`;
    grouped.set(key, (grouped.get(key) || Number("0")) + Number("1"));
  }
  const counts = [...grouped.entries()]
    .map(([key, count]) => {
      const [serviceName, code] = key.split(" ");
      return { service: serviceName, error_code: code, count };
    })
    .sort((left, right) => right.count - left.count
      || left.service.localeCompare(right.service)
      || left.error_code.localeCompare(right.error_code));

  // File order is arrival order within a service; received_at orders across
  // services. Envelopes old enough to lack it sort as oldest.
  const ordered = envelopes
    .map((envelope, index) => ({ envelope, index }))
    .sort((left, right) => String(left.envelope.received_at || "").localeCompare(String(right.envelope.received_at || ""))
      || left.index - right.index);
  const newest = ordered.slice(-Math.max(Number("0"), limit)).map((entry) => entry.envelope).reverse();

  return {
    directory: FAILURES_DIR,
    services: names.map((name) => name.replace(/\.jsonl$/, "")),
    total: envelopes.length,
    unparsed: unparsed.count,
    counts,
    newest,
  };
}

/** The text rendering of the index: the summary table, then the newest lines. */
export function renderFailures(report) {
  const lines = [
    `failures: ${report.total} stored (${report.unparsed} unparsed lines) in ${report.directory}`,
    "by service and error_code:",
  ];
  if (!report.counts.length) lines.push("  (none)");
  for (const row of report.counts) {
    lines.push(`  ${row.service}  ${row.error_code}  ${row.count}`);
  }
  lines.push("newest:");
  if (!report.newest.length) lines.push("  (none)");
  for (const envelope of report.newest) {
    const at = envelope.received_at || "-";
    const detail = envelope.detail ? `  ${trimDetail(envelope.detail, Number("160"))}` : "";
    lines.push(`  ${at}  ${envelope.service}  ${envelope.error_code}  ${envelope.failure_point}${detail}`);
  }
  return lines.join("\n");
}

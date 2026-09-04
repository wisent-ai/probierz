// The failure contract, in the shape a command-line tool needs it.
//
// The vocabulary, the severities, the retryable and outage sets, the upstream
// status classification and the retry exit code all come from `@wisent/errors`,
// the one place the fleet keeps them. What differs here is the audience:
// probierz answers an operator on a terminal, not a browser. So there is no
// HTTP status, no response body, and — deliberately — no network call to an
// analytics collector. A test harness that hangs because its own telemetry
// endpoint is unreachable is the failure mode this module exists to avoid.
//
// What an operator gets instead, on stderr:
//   1. one structured line (`probierz-failure {...}`) carrying failure_point,
//      error_code, service, retryable and the technical detail — greppable,
//      and complete. Hiding an upstream body from an operator would be
//      theatre; that rule is about what travels over the network.
//   2. one sentence in plain words: is this our outage or your request, and is
//      it worth retrying.
// and one exit code that tells a shell script the same thing without parsing
// anything.

import {
  CODES,
  FALLBACK,
  RETRY_EXIT,
  codeOrFallback,
  exitCode,
  failureOrFallback,
  fromUpstreamStatus,
  outage,
  retryable,
  severity,
  trimDetail,
} from "@wisent/errors";
import { SEVERITIES } from "@wisent/errors/codes";

// The rule for how to cut a detail is the package's; the width is probierz's.
// An operator reads these lines on a terminal, where the package's own
// two-thousand-character bound is not a detail but a wall, and 300 is what
// every probierz-failure line has always carried.
const MAX_DETAIL_CHARS = Number("300");

/** The catalogue's seven codes, under the names this module's callers use. */
export const CODE = Object.freeze(Object.fromEntries(CODES.map((code) => [code.toUpperCase(), code])));

export const SEVERITY = Object.freeze(Object.fromEntries(SEVERITIES.map((name) => [name.toUpperCase(), name])));

/**
 * What a code implies, taken from the catalogue and never decided here. A code
 * the catalogue does not name falls back rather than throwing: reporting a
 * failure must not itself be a source of failures.
 */
function derived(code) {
  const known = codeOrFallback(code);
  return { severity: severity(known), retryable: retryable(known), outage: outage(known) };
}

/**
 * Exit code for a failure the operator can simply retry. 69 is sysexits'
 * EX_UNAVAILABLE: the boring, pre-existing spelling of "not your fault, try
 * later". Kept rather than renumbered because probierz already means specific
 * things by 1 (a suite failed), 2 (bad invocation) and 3 (blocked), and a
 * change there would silently rewrite what every CI script concludes.
 */
export const EXIT_RETRY = RETRY_EXIT;

/** A missing or malformed environment variable is our outage, not a mistake the operator made. */
const CONFIG_RE = /(is required|not configured|missing env|must be set|env var)/i;

/** Deadlines: ours (a watch budget elapsing) and the platform's (ETIMEDOUT). */
const TIMEOUT_RE = /(timed out|timeout|etimedout|deadline|did not finish within)/i;

/**
 * Transport-level failures. `fetch` reports these as a bare TypeError whose
 * message differs per runtime, and a spawned CLI reports them as whatever its
 * own HTTP stack prints, so match all of the wordings rather than one.
 */
const NETWORK_RE = /(fetch failed|failed to fetch|networkerror|load failed|econnrefused|enotfound|eai_again|econnreset|epipe|socket hang up|connection refused|connection reset|connection closed|broken pipe|no route to host|network is unreachable|dns error|tcp connect error|error sending request|service unavailable|temporarily unavailable|bad gateway)/i;

/** Wording that names the caller, not the infrastructure. Checked after the above. */
const AUTH_RE = /(unauthorized|forbidden|permission denied|invalid token|not authenticated|access denied)/i;
const NOT_FOUND_RE = /(not found|no such (file|object|key|host is not)|does not exist)/i;
const RATE_LIMIT_RE = /(rate limit|too many requests|quota exceeded|throttl)/i;

function textOf(error) {
  if (!error) return "";
  if (typeof error === "string") return error;
  const message = typeof error.message === "string" ? error.message : String(error);
  const causeMessage = typeof error.cause?.message === "string" ? error.cause.message : "";
  const causeCode = typeof error.cause?.code === "string" ? error.cause.code : "";
  const ownCode = typeof error.code === "string" ? error.code : "";
  return `${message} ${causeMessage} ${causeCode} ${ownCode}`.trim();
}

function fromText(text) {
  if (!text) return null;
  if (CONFIG_RE.test(text)) return CODE.CONFIG;
  if (TIMEOUT_RE.test(text)) return CODE.TIMEOUT;
  if (NETWORK_RE.test(text)) return CODE.INFRA_DOWN;
  if (RATE_LIMIT_RE.test(text)) return CODE.RATE_LIMIT;
  if (AUTH_RE.test(text)) return CODE.AUTH;
  if (NOT_FOUND_RE.test(text)) return CODE.NOT_FOUND;
  return null;
}

/**
 * Bounded technical text. Complete enough to debug, short enough to log, and
 * null rather than empty so an absent detail reads as absent on the line.
 */
export function boundedDetail(value) {
  return trimDetail(value, MAX_DETAIL_CHARS) || null;
}

function detailOf(error, status) {
  const parts = [];
  if (Number.isFinite(status)) parts.push(`http ${status}`);
  const text = typeof error === "string" ? error : (error && typeof error.message === "string" ? error.message : (error ? String(error) : ""));
  if (text) parts.push(text);
  return boundedDetail(parts.join(" — "));
}

/**
 * @param {{error?: unknown, status?: number|null}} input
 * @returns {{code: string, severity: string, retryable: boolean, outage: boolean, detail: string|null}}
 */
export function classifyFailure({ error = null, status = null } = {}) {
  const numericStatus = Number.isFinite(status) ? Number(status) : null;
  const known = error && typeof error.failureCode === "string" && CODES.includes(error.failureCode)
    ? error.failureCode
    : null;
  const name = error && typeof error.name === "string" ? error.name : "";
  const byName = name === "AbortError" || name === "TimeoutError" ? CODE.TIMEOUT : null;
  const byStatus = numericStatus === null ? null : fromUpstreamStatus(numericStatus);
  const code = known || byName || fromText(textOf(error)) || byStatus || FALLBACK;
  return {
    code,
    ...derived(code),
    detail: detailOf(error, numericStatus),
  };
}

/**
 * The exit status the process should end on. Retryable failures get one
 * distinct code so a wrapper script can back off without parsing output;
 * everything else keeps whatever the command already meant by failing.
 */
export function exitCodeFor(code, fallback = Number("1")) {
  return exitCode(codeOrFallback(code), Number(fallback));
}

/**
 * Stable dependency axis, named the way these things are named operationally.
 */
export const SERVICE = {
  STADO_QUEUE: "stado-queue",
  OBJECTS: "objects",
  MODEL_ROUTER: "model-router",
  HARNESS: "harness",
  CLI: "cli",
};

/**
 * What the operator loses. Coarse on purpose: the useful question is which
 * half of probierz stopped working, because one half has a local fallback and
 * the other does not.
 */
export const IMPACT = {
  REMOTE_RUN: "remote-run",
  FLEET_HEALTH: "fleet-health",
  LOCAL_RUN: "local-run",
  AUTHORING: "authoring",
  CLI: "cli",
};

const POINTS = {
  "stado.upload": { service: SERVICE.OBJECTS, impact: IMPACT.REMOTE_RUN },
  "stado.download": { service: SERVICE.OBJECTS, impact: IMPACT.REMOTE_RUN },
  "stado.submit": { service: SERVICE.STADO_QUEUE, impact: IMPACT.REMOTE_RUN },
  "stado.watch": { service: SERVICE.STADO_QUEUE, impact: IMPACT.REMOTE_RUN },
  "stado.worker": { service: SERVICE.HARNESS, impact: IMPACT.REMOTE_RUN },
  "stado.pack": { service: SERVICE.HARNESS, impact: IMPACT.REMOTE_RUN },
  "objects.config": { service: SERVICE.OBJECTS, impact: IMPACT.FLEET_HEALTH },
  "objects.list": { service: SERVICE.OBJECTS, impact: IMPACT.FLEET_HEALTH },
  "objects.read": { service: SERVICE.OBJECTS, impact: IMPACT.FLEET_HEALTH },
  "model.route": { service: SERVICE.MODEL_ROUTER, impact: IMPACT.AUTHORING },
  "run.source": { service: SERVICE.HARNESS, impact: IMPACT.LOCAL_RUN },
  "run.spawn": { service: SERVICE.HARNESS, impact: IMPACT.LOCAL_RUN },
  "run.report": { service: SERVICE.HARNESS, impact: IMPACT.LOCAL_RUN },
  "cli.unknown": { service: SERVICE.CLI, impact: IMPACT.CLI },
};

export const FAILURE_POINT_IDS = Object.keys(POINTS);

/**
 * Resolve a failure point. Never throws and never returns null: reporting a
 * failure must not itself be a source of failures.
 */
export function failurePoint(id) {
  const known = POINTS[id];
  if (known) return { id, ...known };
  return { id: "cli.unknown", ...POINTS["cli.unknown"], requestedId: id || null };
}

/**
 * Services that only remote execution depends on. When one of these is down,
 * probierz is not broken — only its remote half is, and saying so is the whole
 * point: an operator who reads "storage failed" walks away, an operator who
 * reads "run it locally" gets their answer.
 */
const REMOTE_ONLY_SERVICES = new Set([SERVICE.STADO_QUEUE, SERVICE.OBJECTS, SERVICE.MODEL_ROUTER]);

const LOCAL_FALLBACK = "Local runs are unaffected — `probierz run <target>` still works without the stado queue.";

/**
 * One sentence for a human. Says whose problem this is and what to do; never
 * repeats the raw upstream text, which is already on the structured line above
 * it and would only bury the answer.
 */
export function humanMessage(point, classified, action) {
  const what = action || `${point.id} failed`;
  // An unclassified crash inside probierz itself should not be described as a
  // failing "cli dependency" — probierz is the thing that broke.
  const subject = point.service === SERVICE.CLI ? "probierz" : `the ${point.service} dependency`;
  const blame = {
    [CODE.INFRA_DOWN]: `${what}: ${subject} is unavailable. This is an infrastructure outage, not your configuration — retry later.`,
    [CODE.TIMEOUT]: `${what}: ${subject} did not answer in time. Not your configuration — retry later.`,
    [CODE.RATE_LIMIT]: `${what}: ${subject} is rate-limiting us. Retry later.`,
    [CODE.CONFIG]: `${what}: ${subject} is missing configuration. See the detail on the line above; retrying will not help.`,
    [CODE.AUTH]: `${what}: ${subject} rejected our credentials. Refresh them; retrying will not help.`,
    [CODE.NOT_FOUND]: `${what}: ${subject} has no such object. Check the identifier; retrying will not help.`,
    [CODE.UNKNOWN]: `${what}: ${subject} failed in a way probierz does not recognise. See the detail on the line above.`,
  }[classified.code];
  const advice = classified.retryable && REMOTE_ONLY_SERVICES.has(point.service) ? ` ${LOCAL_FALLBACK}` : "";
  return `${blame}${advice}`;
}

/**
 * The single structured line. JSON on one line so `grep`, `jq` and a log
 * shipper all work without a parser, and so two failures never interleave into
 * one unreadable paragraph.
 *
 * The envelope is the package's, so `severity`, `retryable` and `outage` cannot
 * disagree with the code here and agree with it elsewhere; the prefix and the
 * stream are probierz's, because this line is what an operator already greps.
 * `failureOrFallback` rather than `failure`: a malformed code has to arrive on
 * the line as data, not as an exception thrown inside the error path.
 */
export function failureLogLine(point, classified) {
  return `probierz-failure ${JSON.stringify(failureOrFallback({
    failurePoint: point.id,
    code: classified.code,
    service: point.service,
    impact: point.impact,
    detail: classified.detail,
  }))}`;
}

/**
 * Apply a call site's own default. A dependency that answered nothing
 * recognisable is not automatically "unknown": a call into remote
 * infrastructure that did not complete is an outage, and saying `unknown`
 * there is the same silence this contract exists to end.
 */
function withFallback(classified, fallbackCode) {
  if (classified.code !== FALLBACK || fallbackCode === FALLBACK) return classified;
  return {
    ...classified,
    code: fallbackCode,
    ...derived(fallbackCode),
  };
}

/**
 * Classify, log once, and hand back the verdict. The caller decides whether to
 * keep going (a degraded section of a report) or to stop (`exitCodeFor`).
 *
 * @param {{point?: string, error?: unknown, status?: number|null, action?: string, fallbackCode?: string, silent?: boolean}} input
 */
export function reportFailure({ point = "cli.unknown", error = null, status = null, action = null, fallbackCode = CODE.UNKNOWN, silent = false } = {}) {
  const resolved = failurePoint(point);
  const classified = withFallback(classifyFailure({ error, status }), fallbackCode);
  const message = humanMessage(resolved, classified, action);
  if (!silent) {
    process.stderr.write(`${failureLogLine(resolved, classified)}\n${message}\n`);
  }
  return {
    point: resolved.id,
    service: resolved.service,
    impact: resolved.impact,
    code: classified.code,
    severity: classified.severity,
    retryable: classified.retryable,
    outage: classified.outage,
    detail: classified.detail,
    message,
    exitCode: exitCodeFor(classified.code),
  };
}

/**
 * An error that already knows its own classification, so the boundary that
 * finally reports it does not have to re-derive from wording what the call
 * site knew for certain. `detail` carries the technical text (upstream body,
 * spawned-CLI stderr); `message` is the sentence that is safe to render
 * anywhere, including into a JSON report a dashboard reads.
 */
export class FailureError extends Error {
  constructor({ point, code, detail = null, message = null, cause = null }) {
    const resolved = failurePoint(point);
    const classified = {
      code,
      ...derived(code),
      detail: boundedDetail(detail),
    };
    super(message || humanMessage(resolved, classified, null));
    this.name = "FailureError";
    this.failurePoint = resolved.id;
    this.failureCode = code;
    this.service = resolved.service;
    this.impact = resolved.impact;
    this.severity = classified.severity;
    this.retryable = classified.retryable;
    this.outage = classified.outage;
    this.detail = classified.detail;
    this.exitCode = exitCodeFor(code);
    if (cause) this.cause = cause;
  }
}

/**
 * Build a classified error from something a dependency threw or printed, and
 * log the technical part exactly once, right where it is still known.
 */
export function failureFrom({ point, error = null, status = null, action = null, detail = null, fallbackCode = CODE.UNKNOWN }) {
  const resolved = failurePoint(point);
  const classified = withFallback(classifyFailure({ error, status }), fallbackCode);
  const merged = boundedDetail(detail ? `${detail}${classified.detail ? ` — ${classified.detail}` : ""}` : classified.detail);
  process.stderr.write(`${failureLogLine(resolved, { ...classified, detail: merged })}\n`);
  return new FailureError({
    point: resolved.id,
    code: classified.code,
    detail: merged,
    message: humanMessage(resolved, classified, action),
    cause: error instanceof Error ? error : null,
  });
}

/**
 * The process-wide boundary: turn whatever reached the top into an answer and
 * an exit code.
 *
 * A failure classified at its source has already written its structured line,
 * carrying a detail this boundary no longer has — the spawned tool's stderr,
 * the upstream body. Re-logging here would bury that line under a paraphrase
 * of itself, so an already-classified failure only gets its sentence.
 *
 * Everything else — a bug, a usage mistake, an unwrapped dependency error —
 * is classified here so that nothing leaves probierz unclassified.
 */
export function reportBoundaryFailure(error, action) {
  if (error instanceof FailureError) {
    process.stderr.write(`${action}: ${error.message}\n`);
    return error.exitCode;
  }
  const reported = reportFailure({ point: error?.failurePoint || "cli.unknown", error, action });
  // A usage mistake keeps whatever the command already meant by exiting;
  // only the retryable path takes the new, distinct code.
  return reported.retryable ? EXIT_RETRY : Number(error?.exitCode || "1");
}

/**
 * The shape a classified failure takes inside a JSON report. Kept free of raw
 * upstream text so `probierz overview --json` stays safe to forward, while the
 * stderr log above it keeps the full story for whoever is debugging.
 */
export function failureSummary(error, fallbackPoint = "cli.unknown") {
  if (error instanceof FailureError) {
    return {
      failurePoint: error.failurePoint,
      errorCode: error.failureCode,
      service: error.service,
      retryable: error.retryable,
      outage: error.outage,
      message: error.message,
    };
  }
  const resolved = failurePoint(fallbackPoint);
  const classified = classifyFailure({ error });
  return {
    failurePoint: resolved.id,
    errorCode: classified.code,
    service: resolved.service,
    retryable: classified.retryable,
    outage: classified.outage,
    message: humanMessage(resolved, classified, null),
  };
}

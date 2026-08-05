#!/usr/bin/env node
// stdio JSON-RPC (Model Context Protocol) server for probierz.
// Mirrors the echo / weles / skarbiec MCP servers: newline-delimited JSON in on
// stdin, exactly one response line per request out on stdout, diagnostics on
// stderr. Discovery tools (list/specs/describe/run_command) are read-only.
// `run` executes suites, `analyze` inventories evidence, and
// `probierz_create_readme_gif` writes one bounded publication asset plus provenance.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { SURFACES, listSpecs, describeSpec, runCommand } from "./lib.mjs";
import { appSourceIdentity, completeRun, runSurface, targetList } from "./runner.mjs";
import { analyzeRun } from "./analyze.mjs";
import { createReadmeGif } from "./readme-gif.mjs";
import { preflight, runSetup } from "./preflight.mjs";
import { affectedFromGit, affectedTargets } from "./affected.mjs";
import { orchestrate } from "./orchestrate.mjs";
import { compareRuns, lastGreen, runHistory } from "./history.mjs";
import { startRun, runStatus, cancelRun, getResult, listArtifacts, getArtifact } from "./control.mjs";
import { createReceipt, verifyReceipt } from "./receipt.mjs";
import { createPublicationManifest } from "./publication.mjs";
import { dashboardProjection } from "./dashboard.mjs";
import { planMatrix, runMatrix } from "./matrix.mjs";
import { enforceRetention, protectRun, restoreBundle } from "./artifacts.mjs";
import { auditTrail, scanSecrets } from "./security.mjs";
import { activateGate, enforceGate, evaluateGate, gateStatus } from "./gate.mjs";
import { appStatus } from "./status.mjs";
import { prepushGate } from "./prepush-gate.mjs";
import { authorSpec } from "./author-spec.mjs";
import { authorManifest } from "./author-manifest.mjs";
import { submitRemoteRun } from "./stado.mjs";

const PROTOCOL_VERSION = "2024-11-05";
const JSONRPC_VERSION = "2.0";
const CODE_PARSE_ERROR = "-32700";
const CODE_METHOD_NOT_FOUND = "-32601";
const CODE_INTERNAL_ERROR = "-32000";
const NOT_FOUND = "-1";

function code(raw) {
  return Number(raw);
}

function serverVersion() {
  try {
    const here = dirname(fileURLToPath(import.meta.url));
    const pkg = JSON.parse(readFileSync(join(here, "..", "package.json"), "utf8"));
    return pkg.version || "unknown";
  } catch {
    return "unknown";
  }
}

const objectSchema = (properties, required) => ({
  type: "object",
  properties: properties || {},
  required: required || [],
});

const gateProperties = {
  appId: { type: "string" },
  mode: { type: "string", description: "pull-request or release" },
  expectedHarnessSha: { type: "string" },
  expectedSourceSha: { type: "string" },
  runIds: { type: "array", items: { type: "string" } },
  release: { type: "string", description: "Required for release mode." },
  receiptFile: { type: "string", description: "Required signed evidence receipt for release mode." },
  trustedPublicKeyFile: { type: "string" },
  expectedFingerprint: { type: "string" },
};

const TOOLS = [
  {
    name: "probierz_list_surfaces",
    description: "List the cross-platform test surfaces (web, electron, mobile, desktop-native): tool, npm script, targets, and relevant env vars.",
    inputSchema: objectSchema({}),
  },
  {
    name: "probierz_list_specs",
    description: "Discover e2e/spec files on disk; optional surface narrows to one (web|electron|mobile|desktop-native).",
    inputSchema: objectSchema({
      surface: { type: "string", description: "Optional surface filter." },
    }),
  },
  {
    name: "probierz_describe_spec",
    description: "Static outline of a spec (describe/it/test titles in file order) by its path under the probierz root. Does not execute anything.",
    inputSchema: objectSchema({
      spec: { type: "string", description: "Spec path, e.g. packages/mobile/test/specs/byk.e2e.ts" },
    }, ["spec"]),
  },
  {
    name: "probierz_run_command",
    description: "Return the exact shell command to run a target yourself (web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win). Read-only: probierz never runs it.",
    inputSchema: objectSchema({
      target: { type: "string", description: "One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win." },
    }, ["target"]),
  },
  {
    name: "probierz_check",
    description: "Preflight a target's toolchain WITHOUT running anything: reports whether it is ready and, for each missing piece, exactly how to fix it -- `probierz setup <target>` for parts probierz owns (Playwright browsers, Appium drivers) or a host install command for the rest (Xcode, Android SDK, simulators, WinAppDriver). Read-only.",
    inputSchema: objectSchema({
      target: { type: "string", description: "One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win." },
    }, ["target"]),
  },
  {
    name: "probierz_setup",
    description: "Install the toolchain parts probierz owns for a target (npm deps + Playwright browsers, or npm deps + the Appium driver). Does NOT install host-level dependencies (Xcode, Android SDK, simulators, WinAppDriver) -- probierz_check reports those. Side-effecting: runs npm / appium driver install.",
    inputSchema: objectSchema({
      target: { type: "string", description: "One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win." },
      timeoutMs: { type: "number", description: "Kill a setup step after this many ms (default 30 min)." },
    }, ["target"]),
  },
  {
    name: "probierz_run",
    description: "EXECUTE a target end-to-end (spawns Playwright or WebdriverIO+Appium) under chosen conditions, records video/trace/screenshot when record=true, and returns the run result plus an analysis of what it produced. Heavy + side-effecting: needs Chromium / Appium / a simulator.",
    inputSchema: objectSchema({
      target: { type: "string", description: "One of web, electron, mobile:ios, mobile:android, desktop:mac, desktop:win." },
      record: { type: "boolean", description: "Force video + trace + screenshot capture on." },
      appId: { type: "string", description: "Product identifier used in the run-scoped artifact path and manifest." },
      env: { type: "object", description: "Condition env vars, e.g. { BASE_URL, APP_IOS, PROBIERZ_LOCALE, PROBIERZ_COLOR_SCHEME }." },
      timeoutMs: { type: "number", description: "Kill the run after this many ms (default 20 min)." },
      resourceWaitMs: { type: "number", description: "Wait this long for a busy device/port lease; 0 fails fast." },
      frames: { type: "number", description: "Extract this many frames per recorded video (needs ffmpeg)." },
      analyze: { type: "boolean", description: "Analyze the report after the run (default true)." },
      force: { type: "boolean", description: "Skip the preflight gate and spawn even if the toolchain looks incomplete." },
      spec: { type: "string", description: "Run only this one spec (path/substring), e.g. packages/mobile/test/specs/byk.e2e.ts, to scope the run to a single app's suite." },
    }, ["target"]),
  },
  {
    name: "probierz_analyze",
    description: "Parse a finished run's report (Playwright report.json or the WDIO probierz-<kind>-results.json) and inventory its media: totals, per-test status, failure reasons, and recording metadata (duration/dimensions via ffprobe, optional frame montage via ffmpeg).",
    inputSchema: objectSchema({
      reportPath: { type: "string", description: "Path to the machine-readable report (from a probierz_run result)." },
      artifactsDir: { type: "string", description: "Directory to inventory for media (from a probierz_run result)." },
      tool: { type: "string", description: "playwright | wdio (inferred from the report if omitted)." },
      frames: { type: "number", description: "Extract this many frames per video (needs ffmpeg)." },
    }, ["reportPath"]),
  },
  {
    name: "probierz_create_readme_gif",
    description: "SIDE-EFFECTING: convert one recorded journey video into a bounded, silent, looping README GIF and write a provenance sidecar with source/output SHA-256 and mandatory publication checks. Requires ffmpeg.",
    inputSchema: objectSchema({
      input: { type: "string", description: "Recorded journey video path." },
      output: { type: "string", description: "Destination path ending in .gif." },
      startSeconds: { type: "number", description: "Non-negative trim offset; default 0." },
      durationSeconds: { type: "number", description: "Published clip duration; default 12, maximum 30." },
      framesPerSecond: { type: "number", description: "GIF frame rate; default 12, maximum 20." },
      width: { type: "number", description: "Output width; default 960, maximum 1200." },
      force: { type: "boolean", description: "Replace an existing GIF and sidecar." },
    }, ["input", "output"]),
  },
  {
    name: "probierz_affected",
    description: "Given a change, report which run targets it could affect, so you re-run only what is relevant. Deterministic + structural (maps files to targets by package containment; agent/ or repo-root files are cross-cutting -> all targets). Provide `files` explicitly, or omit to diff the working tree against `ref` (default HEAD) via git. Read-only.",
    inputSchema: objectSchema({
      files: { type: "array", items: { type: "string" }, description: "Changed file paths (repo-relative). If given, git is not consulted." },
      ref: { type: "string", description: "git ref to diff the working tree against when `files` is omitted (default HEAD)." },
    }),
  },
  {
    name: "probierz_ci",
    description: "Change-driven test pass: select the targets a change affects, run the ready ones (preflight-gated, blocked ones are reported with their fix, not spawned), analyze what ran, and return a consolidated verdict {summary:{passed,failed,blocked,ran}, results}. Composes probierz_affected + probierz_run + probierz_analyze. Heavy: runs real suites. Deterministic selection; no LLM reasoning about the results.",
    inputSchema: objectSchema({
      files: { type: "array", items: { type: "string" }, description: "Changed file paths (repo-relative). If given, git is not consulted." },
      ref: { type: "string", description: "git ref to diff the working tree against when `files` is omitted (default HEAD)." },
      appId: { type: "string", description: "Product identifier for every selected run." },
      env: { type: "object", description: "Conditions forwarded to every selected run." },
      spec: { type: "string", description: "Optional spec filter forwarded to every selected target." },
      record: { type: "boolean", description: "Force video/trace/screenshot capture on for every run." },
      force: { type: "boolean", description: "Skip each target's preflight gate and spawn anyway." },
      frames: { type: "number", description: "Extract this many frames per recorded video (needs ffmpeg)." },
      timeoutMs: { type: "number", description: "Per-run timeout in ms." },
      resourceWaitMs: { type: "number", description: "Wait per selected run for a busy device/port lease; default 10 min, 0 fails fast." },
    }),
  },
  {
    name: "probierz_history",
    description: "Read deterministic E5 stability history: pass rate, infrastructure failures, duration trend, flaky tests, journeys, latest run, and last green.",
    inputSchema: objectSchema({
      appId: { type: "string", description: "Product identifier (default probierz)." },
      target: { type: "string", description: "Optional target filter." },
      limit: { type: "number", description: "Maximum recent runs (default 50)." },
    }),
  },
  {
    name: "probierz_dashboard",
    description: "Project evidence for product → version → journey → surface → device → result → artifact dashboard navigation.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      limit: { type: "number", description: "Maximum recent runs (default 500)." },
    }, ["appId"]),
  },
  {
    name: "probierz_matrix_plan",
    description: "Read the deterministic nightly or release matrix without executing it.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      profile: { type: "string", description: "nightly or release" },
    }, ["appId", "profile"]),
  },
  {
    name: "probierz_run_matrix",
    description: "HEAVY + SIDE-EFFECTING: execute every cell of a declared nightly or release matrix and return an E4 verdict.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      profile: { type: "string", description: "nightly or release" },
      release: { type: "string", description: "Required for a release matrix." },
      env: { type: "object", description: "Secrets and exact release artifact conditions; matrix axes cannot be overridden." },
    }, ["appId", "profile"]),
  },
  {
    name: "probierz_protect_run",
    description: "SIDE-EFFECTING: encrypt a complete run into an authenticated AES-256-GCM evidence bundle; optionally remove plaintext artifacts.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      runId: { type: "string" },
      kind: { type: "string" },
      keyFile: { type: "string" },
      removePlaintext: { type: "boolean" },
    }, ["appId", "runId"]),
  },
  {
    name: "probierz_restore_bundle",
    description: "SIDE-EFFECTING: authenticate and restore an encrypted evidence bundle into an empty directory.",
    inputSchema: objectSchema({
      file: { type: "string" },
      destination: { type: "string" },
      keyFile: { type: "string" },
    }, ["file", "destination"]),
  },
  {
    name: "probierz_retention",
    description: "Plan retention expiry; with apply=true, delete expired plaintext runs and encrypted bundles.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      at: { type: "string", description: "Optional ISO timestamp." },
      apply: { type: "boolean" },
    }, ["appId"]),
  },
  {
    name: "probierz_secret_scan",
    description: "Scan a plaintext artifact directory for high-confidence secrets without returning secret values.",
    inputSchema: objectSchema({
      directory: { type: "string" },
    }, ["directory"]),
  },
  {
    name: "probierz_audit",
    description: "Read and integrity-check access audit records, optionally filtered by app, run, or action.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      runId: { type: "string" },
      action: { type: "string" },
      limit: { type: "number" },
    }),
  },
  {
    name: "probierz_source_identity",
    description: "Compute exact path-independent harness and app source SHA-256 identities.",
    inputSchema: objectSchema({ appId: { type: "string" } }, ["appId"]),
  },
  {
    name: "probierz_gate_status",
    description: "Read pull-request and release gate activation state.",
    inputSchema: objectSchema({ appId: { type: "string" } }, ["appId"]),
  },
  {
    name: "probierz_status",
    description: "Journey coverage, evidence freshness vs HEAD, untested surfaces, and pull-request merge eligibility for an app.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      baseRef: { type: "string", description: "Default origin/main." },
    }, ["appId"]),
  },
  {
    name: "probierz_gate_prepush",
    description: "Pre-push merge gate: select affected journeys from the push diff and evaluate the newest passing runs against the exact current HEAD identity (pull-request policy).",
    inputSchema: objectSchema({
      repo: { type: "string" },
      appId: { type: "string", description: "Inferred from manifest repositories when omitted." },
      base: { type: "string" },
      head: { type: "string" },
      runCi: { type: "boolean", description: "Run probierz ci <base> before evaluating." },
    }, ["repo"]),
  },
  {
    name: "probierz_author_spec",
    description: "SIDE-EFFECTING: use the authenticated Stado model router to draft one journey spec from a probe of the real app, verify it with an actual run, and keep it on green (registers the journey in the app manifest).",
    inputSchema: objectSchema({
      appId: { type: "string" },
      journey: { type: "string" },
      target: { type: "string", description: "web|electron|mobile:ios|mobile:android|desktop:mac|desktop:win|tui" },
      desc: { type: "string", description: "Journey goal in one or two sentences." },
      baseUrl: { type: "string" },
      appPath: { type: "string" },
      rounds: { type: "number" },
    }, ["appId", "journey", "target", "desc"]),
  },
  {
    name: "probierz_author_manifest",
    description: "SIDE-EFFECTING: use the authenticated Stado model router to draft the whole app journey manifest from a probe and repository layout, validate it, and optionally cover every journey with author-spec.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      desc: { type: "string", description: "What the app does, in one or two sentences." },
      repositories: { type: "array", items: { type: "string" } },
      target: { type: "string" },
      baseUrl: { type: "string" },
      appPath: { type: "string" },
      withSpecs: { type: "boolean" },
    }, ["appId", "desc", "repositories", "target"]),
  },
  {
    name: "probierz_stado_run",
    description: "SIDE-EFFECTING: run a target on a chosen stado host (provider/pin/spot/GPU); evidence lands back in test-results.",
    inputSchema: objectSchema({
      target: { type: "string" },
      appId: { type: "string" },
      spec: { type: "string" },
      host: { type: "string", description: "stado:gcp|azure|aws|any|spot|local|t4" },
      cargoRelease: { type: "boolean", description: "Build the app binary on the worker with cargo (needs appRepo)." },
      appRepo: { type: "string" },
      watch: { type: "boolean", description: "Default true; waits for completion and fetches results." },
    }, ["target", "appId"]),
  },
  {
    name: "probierz_gate_evaluate",
    description: "Evaluate exact build, E3 evidence, coverage, matrix, encryption, secret scan, and signed receipt eligibility; appends an audit record.",
    inputSchema: objectSchema(gateProperties, ["appId", "mode", "expectedHarnessSha", "expectedSourceSha", "runIds"]),
  },
  {
    name: "probierz_gate_enforce",
    description: "Enforce an activated gate against current evidence; pending-green gates fail closed.",
    inputSchema: objectSchema(gateProperties, ["appId", "mode", "expectedHarnessSha", "expectedSourceSha", "runIds"]),
  },
  {
    name: "probierz_gate_activate",
    description: "SIDE-EFFECTING: atomically activate a gate only after all green evidence requirements pass.",
    inputSchema: objectSchema(gateProperties, ["appId", "mode", "expectedHarnessSha", "expectedSourceSha", "runIds"]),
  },
  {
    name: "probierz_compare_runs",
    description: "Deterministically compare status, duration, tests, evidence, build identity, and artifact hashes between two run IDs.",
    inputSchema: objectSchema({
      appId: { type: "string", description: "Product identifier (default probierz)." },
      leftRunId: { type: "string" },
      rightRunId: { type: "string" },
    }, ["leftRunId", "rightRunId"]),
  },
  {
    name: "probierz_last_green",
    description: "Return the newest passing run for a product, optional target, and optional journey.",
    inputSchema: objectSchema({
      appId: { type: "string", description: "Product identifier (default probierz)." },
      target: { type: "string" },
      journey: { type: "string" },
    }),
  },
  {
    name: "probierz_create_receipt",
    description: "SIDE-EFFECTING: secret-scan evidence, verify exact source/build/artifact provenance, and sign a release receipt with immutable journey identities and report-typed publication media.",
    inputSchema: objectSchema({
      appId: { type: "string" },
      release: { type: "string" },
      expectedHarnessSha: { type: "string" },
      expectedSourceSha: { type: "string" },
      runIds: { type: "array", items: { type: "string" } },
      requiredJourneys: { type: "array", items: { type: "string" } },
      minimumEvidence: { type: "string", description: "Default E3." },
      privateKeyFile: { type: "string", description: "Defaults to PROBIERZ_RECEIPT_PRIVATE_KEY_FILE." },
    }, ["appId", "release", "expectedHarnessSha", "expectedSourceSha", "runIds"]),
  },
  {
    name: "probierz_verify_receipt",
    description: "Verify receipt payload hash and Ed25519 signature against an explicit trusted public key or fingerprint.",
    inputSchema: objectSchema({
      file: { type: "string" },
      trustedPublicKeyFile: { type: "string" },
      expectedFingerprint: { type: "string" },
    }, ["file"]),
  },
  {
    name: "probierz_create_publication_manifest",
    description: "SIDE-EFFECTING: verify a signed receipt, current source, secret scan, evidence hashes, driver capability, redaction review, and immutable storage registrations before emitting a deterministic first-use publication manifest.",
    inputSchema: objectSchema({
      receiptFile: { type: "string" },
      attemptId: { type: "string" },
      journeyId: { type: "string" },
      assets: {
        type: "array",
        items: {
          type: "object",
          properties: {
            file: { type: "string" },
            kind: { type: "string", enum: ["screenshot", "recording", "trace"] },
            storageUrl: { type: "string" },
            contentSha256: { type: "string" },
            redactionStatus: { type: "string", enum: ["verified_redacted", "not_applicable"] },
            verifiedAt: { type: "string" },
          },
          required: ["file", "storageUrl", "contentSha256", "redactionStatus", "verifiedAt"],
          additionalProperties: false,
        },
      },
      trustedPublicKeyFile: { type: "string" },
      expectedFingerprint: { type: "string" },
    }, ["receiptFile", "attemptId", "journeyId", "assets"]),
  },
  {
    name: "probierz_start_run",
    description: "HEAVY + SIDE-EFFECTING: start a real run asynchronously and return its runId immediately. Poll with probierz_run_status; cancel with probierz_cancel_run.",
    inputSchema: objectSchema({
      target: { type: "string" },
      record: { type: "boolean" },
      appId: { type: "string" },
      env: { type: "object" },
      timeoutMs: { type: "number" },
      resourceWaitMs: { type: "number", description: "Wait this long for a busy device/port lease; 0 fails fast." },
      frames: { type: "number" },
      analyze: { type: "boolean" },
      force: { type: "boolean" },
      spec: { type: "string" },
    }, ["target"]),
  },
  {
    name: "probierz_run_status",
    description: "Return queued/running/blocked/passed/failed/canceled state for an asynchronous run.",
    inputSchema: objectSchema({ runId: { type: "string" } }, ["runId"]),
  },
  {
    name: "probierz_cancel_run",
    description: "Cancel an asynchronous run and terminate its complete spawned process tree.",
    inputSchema: objectSchema({ runId: { type: "string" } }, ["runId"]),
  },
  {
    name: "probierz_get_result",
    description: "Return the completed normalized result and evidence for an asynchronous run.",
    inputSchema: objectSchema({ runId: { type: "string" } }, ["runId"]),
  },
  {
    name: "probierz_list_artifacts",
    description: "List run-scoped evidence artifacts for a completed asynchronous run.",
    inputSchema: objectSchema({ runId: { type: "string" } }, ["runId"]),
  },
  {
    name: "probierz_get_artifact",
    description: "Read one run-scoped artifact up to 5 MiB as base64; path traversal is rejected.",
    inputSchema: objectSchema({
      runId: { type: "string" },
      file: { type: "string", description: "Run-relative artifact path." },
    }, ["runId", "file"]),
  },
];

function textResult(value) {
  return { content: [{ type: "text", text: JSON.stringify(value, null, code("2")) }] };
}

function gateArgs(args) {
  return {
    appId: asString(args.appId, "appId"),
    mode: asString(args.mode, "mode"),
    expectedHarnessSha: asString(args.expectedHarnessSha, "expectedHarnessSha"),
    expectedSourceSha: asString(args.expectedSourceSha, "expectedSourceSha"),
    runIds: Array.isArray(args.runIds) ? args.runIds : [],
    release: typeof args.release === "string" ? args.release : undefined,
    receiptFile: typeof args.receiptFile === "string" ? args.receiptFile : undefined,
    trustedPublicKeyFile: typeof args.trustedPublicKeyFile === "string" ? args.trustedPublicKeyFile : undefined,
    expectedFingerprint: typeof args.expectedFingerprint === "string" ? args.expectedFingerprint : undefined,
  };
}

async function callTool(name, args) {
  if (name === "probierz_list_surfaces") return textResult(SURFACES);
  if (name === "probierz_list_specs") return textResult(listSpecs(args.surface));
  if (name === "probierz_describe_spec") return textResult(describeSpec(args.spec));
  if (name === "probierz_run_command") return textResult(runCommand(args.target));
  if (name === "probierz_source_identity") {
    return textResult(appSourceIdentity(asString(args.appId, "appId")));
  }
  if (name === "probierz_history") {
    return textResult(runHistory({
      appId: typeof args.appId === "string" ? args.appId : "probierz",
      target: typeof args.target === "string" ? args.target : undefined,
      limit: Number(args.limit) || 50,
    }));
  }
  if (name === "probierz_dashboard") {
    return textResult(dashboardProjection({
      appId: asString(args.appId, "appId"),
      limit: Number(args.limit) || 500,
    }));
  }
  if (name === "probierz_matrix_plan") {
    return textResult(planMatrix({
      appId: asString(args.appId, "appId"),
      profile: asString(args.profile, "profile"),
    }));
  }
  if (name === "probierz_run_matrix") {
    return textResult(await runMatrix({
      appId: asString(args.appId, "appId"),
      profile: asString(args.profile, "profile"),
      release: typeof args.release === "string" ? args.release : undefined,
      env: args.env && typeof args.env === "object" ? args.env : {},
    }));
  }
  if (name === "probierz_protect_run") {
    return textResult(await protectRun({
      appId: asString(args.appId, "appId"),
      runId: asString(args.runId, "runId"),
      kind: typeof args.kind === "string" ? args.kind : undefined,
      keyFile: typeof args.keyFile === "string" ? args.keyFile : undefined,
      removePlaintext: Boolean(args.removePlaintext),
    }));
  }
  if (name === "probierz_restore_bundle") {
    return textResult(await restoreBundle({
      file: asString(args.file, "file"),
      destination: asString(args.destination, "destination"),
      keyFile: typeof args.keyFile === "string" ? args.keyFile : undefined,
    }));
  }
  if (name === "probierz_retention") {
    return textResult(enforceRetention({
      appId: asString(args.appId, "appId"),
      now: typeof args.at === "string" ? new Date(args.at) : new Date(),
      apply: Boolean(args.apply),
    }));
  }
  if (name === "probierz_secret_scan") {
    return textResult(await scanSecrets(asString(args.directory, "directory")));
  }
  if (name === "probierz_audit") {
    return textResult(auditTrail({
      appId: typeof args.appId === "string" ? args.appId : undefined,
      runId: typeof args.runId === "string" ? args.runId : undefined,
      action: typeof args.action === "string" ? args.action : undefined,
      limit: Number(args.limit) || 200,
    }));
  }
  if (name === "probierz_gate_status") {
    return textResult(gateStatus(asString(args.appId, "appId")));
  }
  if (name === "probierz_status") {
    return textResult(appStatus({
      appId: asString(args.appId, "appId"),
      baseRef: typeof args.baseRef === "string" ? args.baseRef : "origin/main",
    }));
  }
  if (name === "probierz_gate_prepush") {
    return textResult(await prepushGate({
      repo: asString(args.repo, "repo"),
      appId: typeof args.appId === "string" ? args.appId : null,
      base: typeof args.base === "string" ? args.base : null,
      head: typeof args.head === "string" ? args.head : null,
      runCi: args.runCi === true,
    }));
  }
  if (name === "probierz_author_spec") {
    return textResult(await authorSpec({
      appId: asString(args.appId, "appId"),
      journey: asString(args.journey, "journey"),
      target: asString(args.target, "target"),
      desc: asString(args.desc, "desc"),
      baseUrl: typeof args.baseUrl === "string" ? args.baseUrl : null,
      appPath: typeof args.appPath === "string" ? args.appPath : null,
      rounds: typeof args.rounds === "number" ? args.rounds : Number("3"),
    }));
  }
  if (name === "probierz_author_manifest") {
    if (!Array.isArray(args.repositories) || !args.repositories.length) throw new Error("repositories must be a non-empty array");
    return textResult(await authorManifest({
      appId: asString(args.appId, "appId"),
      desc: asString(args.desc, "desc"),
      repositories: args.repositories,
      target: asString(args.target, "target"),
      baseUrl: typeof args.baseUrl === "string" ? args.baseUrl : null,
      appPath: typeof args.appPath === "string" ? args.appPath : null,
      withSpecs: args.withSpecs === true,
    }));
  }
  if (name === "probierz_stado_run") {
    return textResult(await submitRemoteRun({
      target: asString(args.target, "target"),
      appId: asString(args.appId, "appId"),
      spec: typeof args.spec === "string" ? args.spec : null,
      host: typeof args.host === "string" ? args.host : "stado:gcp",
      provision: args.cargoRelease === true
        ? { kind: "cargo-release", appId: asString(args.appId, "appId"), binary: asString(args.appId, "appId") }
        : null,
      appRepo: typeof args.appRepo === "string" ? args.appRepo : null,
      watch: args.watch !== false,
    }));
  }
  if (name === "probierz_gate_evaluate") {
    return textResult(await evaluateGate(gateArgs(args)));
  }
  if (name === "probierz_gate_enforce") {
    return textResult(await enforceGate(gateArgs(args)));
  }
  if (name === "probierz_gate_activate") {
    return textResult(await activateGate(gateArgs(args)));
  }
  if (name === "probierz_compare_runs") {
    return textResult(compareRuns({
      appId: typeof args.appId === "string" ? args.appId : "probierz",
      leftRunId: asString(args.leftRunId, "leftRunId"),
      rightRunId: asString(args.rightRunId, "rightRunId"),
    }));
  }
  if (name === "probierz_last_green") {
    return textResult(lastGreen({
      appId: typeof args.appId === "string" ? args.appId : "probierz",
      target: typeof args.target === "string" ? args.target : undefined,
      journey: typeof args.journey === "string" ? args.journey : undefined,
    }));
  }
  if (name === "probierz_create_receipt") {
    return textResult(await createReceipt({
      appId: asString(args.appId, "appId"),
      release: asString(args.release, "release"),
      expectedHarnessSha: asString(args.expectedHarnessSha, "expectedHarnessSha"),
      expectedSourceSha: asString(args.expectedSourceSha, "expectedSourceSha"),
      runIds: Array.isArray(args.runIds) ? args.runIds : [],
      requiredJourneys: Array.isArray(args.requiredJourneys) ? args.requiredJourneys : [],
      minimumEvidence: typeof args.minimumEvidence === "string" ? args.minimumEvidence : "E3",
      privateKeyFile: typeof args.privateKeyFile === "string" ? args.privateKeyFile : undefined,
    }));
  }
  if (name === "probierz_verify_receipt") {
    return textResult(verifyReceipt(asString(args.file, "file"), {
      trustedPublicKeyFile: typeof args.trustedPublicKeyFile === "string" ? args.trustedPublicKeyFile : undefined,
      expectedFingerprint: typeof args.expectedFingerprint === "string" ? args.expectedFingerprint : undefined,
    }));
  }
  if (name === "probierz_start_run") return textResult(startRun(args));
  if (name === "probierz_create_publication_manifest") {
    return textResult(createPublicationManifest({
      receiptFile: asString(args.receiptFile, "receiptFile"),
      attemptId: asString(args.attemptId, "attemptId"),
      journeyId: asString(args.journeyId, "journeyId"),
      assets: Array.isArray(args.assets) ? args.assets : [],
      trustedPublicKeyFile: typeof args.trustedPublicKeyFile === "string" ? args.trustedPublicKeyFile : undefined,
      expectedFingerprint: typeof args.expectedFingerprint === "string" ? args.expectedFingerprint : undefined,
    }));
  }
  if (name === "probierz_run_status") return textResult(runStatus(asString(args.runId, "runId")));
  if (name === "probierz_cancel_run") return textResult(cancelRun(asString(args.runId, "runId")));
  if (name === "probierz_get_result") return textResult(getResult(asString(args.runId, "runId")));
  if (name === "probierz_list_artifacts") return textResult(listArtifacts(asString(args.runId, "runId")));
  if (name === "probierz_get_artifact") {
    return textResult(getArtifact(asString(args.runId, "runId"), asString(args.file, "file")));
  }
  if (name === "probierz_run") {
    const target = asString(args.target, "target");
    const result = await runSurface(target, {
      appId: typeof args.appId === "string" ? args.appId : undefined,
      env: args.env && typeof args.env === "object" ? args.env : {},
      record: Boolean(args.record),
      timeoutMs: Number(args.timeoutMs) || Number("0"),
      resourceWaitMs: args.resourceWaitMs === undefined ? undefined : Number(args.resourceWaitMs),
      force: Boolean(args.force),
      spec: typeof args.spec === "string" ? args.spec : undefined,
    });
    // Gate-skipped: return the preflight detail; there is no report to analyze.
    if (result.skipped) return textResult(result);
    let analysis = null;
    let completed = result;
    if (args.analyze !== false) {
      let analysisError = null;
      try {
        analysis = analyzeRun({
          reportPath: result.reportPath,
          artifactsDir: result.artifactsDir,
          tool: result.tool,
          frames: Number(args.frames) || Number("0"),
          runId: result.runId,
        });
      } catch (error) {
        analysisError = error;
        analysis = { error: error instanceof Error ? error.message : String(error) };
      }
      completed = completeRun(result, analysisError ? null : analysis, analysisError);
    }
    return textResult({ ...completed, analysis });
  }
  if (name === "probierz_check") {
    return textResult(preflight(asString(args.target, "target")));
  }
  if (name === "probierz_setup") {
    const target = asString(args.target, "target");
    const result = runSetup(target, { timeoutMs: Number(args.timeoutMs) || Number("0") });
    return textResult({ ...result, preflight: preflight(target) });
  }
  if (name === "probierz_analyze") {
    return textResult(analyzeRun({
      reportPath: asString(args.reportPath, "reportPath"),
      artifactsDir: args.artifactsDir,
      tool: args.tool,
      frames: Number(args.frames) || Number("0"),
    }));
  }
  if (name === "probierz_create_readme_gif") {
    return textResult(await createReadmeGif({
      input: asString(args.input, "input"),
      output: asString(args.output, "output"),
      startSeconds: args.startSeconds,
      durationSeconds: args.durationSeconds,
      framesPerSecond: args.framesPerSecond,
      width: args.width,
      force: args.force === true,
    }));
  }
  if (name === "probierz_affected") {
    if (Array.isArray(args.files)) return textResult(affectedTargets(args.files));
    return textResult(affectedFromGit(args.ref));
  }
  if (name === "probierz_ci") {
    const input = Array.isArray(args.files) ? { files: args.files } : { ref: args.ref };
    return textResult(await orchestrate(input, {
      appId: typeof args.appId === "string" ? args.appId : undefined,
      env: args.env && typeof args.env === "object" ? args.env : {},
      spec: typeof args.spec === "string" ? args.spec : undefined,
      record: Boolean(args.record),
      force: Boolean(args.force),
      frames: Number(args.frames) || Number("0"),
      timeoutMs: Number(args.timeoutMs) || Number("0"),
      resourceWaitMs: args.resourceWaitMs === undefined ? undefined : Number(args.resourceWaitMs),
    }));
  }
  const err = new Error(`unknown tool: ${name}`);
  err.rpcCode = CODE_METHOD_NOT_FOUND;
  throw err;
}

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function asString(value, name) {
  if (typeof value !== "string" || value.length === Number("0")) {
    throw new Error(`${name} must be a non-empty string`);
  }
  return value;
}

async function handle(request) {
  if (!request || !request.method) return;
  const hasId = Object.prototype.hasOwnProperty.call(request, "id");
  if (!hasId) return;
  const id = request.id ?? null;

  try {
    if (request.method === "initialize") {
      send({
        jsonrpc: JSONRPC_VERSION,
        id,
        result: {
          protocolVersion: PROTOCOL_VERSION,
          capabilities: { tools: {} },
          serverInfo: { name: "probierz", version: serverVersion() },
        },
      });
      return;
    }
    if (request.method === "ping") {
      send({ jsonrpc: JSONRPC_VERSION, id, result: {} });
      return;
    }
    if (request.method === "tools/list") {
      send({ jsonrpc: JSONRPC_VERSION, id, result: { tools: TOOLS } });
      return;
    }
    if (request.method === "tools/call") {
      const params = request.params || {};
      const name = asString(params.name, "name");
      const args = params.arguments && typeof params.arguments === "object" ? params.arguments : {};
      const result = await callTool(name, args);
      send({ jsonrpc: JSONRPC_VERSION, id, result });
      return;
    }
    send({
      jsonrpc: JSONRPC_VERSION,
      id,
      error: { code: code(CODE_METHOD_NOT_FOUND), message: `method not found: ${request.method}` },
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const rpcCode = error && error.rpcCode ? error.rpcCode : CODE_INTERNAL_ERROR;
    send({ jsonrpc: JSONRPC_VERSION, id, error: { code: code(rpcCode), message } });
  }
}

function serve() {
  let buffer = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (chunk) => {
    buffer += chunk;
    for (;;) {
      const newline = buffer.indexOf("\n");
      if (newline === code(NOT_FOUND)) break;
      const line = buffer.slice(Number("0"), newline).trim();
      buffer = buffer.slice(newline + Number("1"));
      if (!line) continue;
      let request;
      try {
        request = JSON.parse(line);
      } catch {
        send({
          jsonrpc: JSONRPC_VERSION,
          id: null,
          error: { code: code(CODE_PARSE_ERROR), message: "parse error" },
        });
        continue;
      }
      void handle(request);
    }
  });
}

serve();

#!/usr/bin/env node
// probierz — command-line view of the cross-platform test toolkit.
//
// Reads the same lib the MCP server uses (one source of truth). list/specs/
// describe/cmd are read-only; check/setup/run/analyze/figure-evaluate are explicit surfaces.
//   probierz list                 — the four test surfaces + how to run each
//   probierz specs [surface]      — e2e/spec files discovered on disk
//   probierz describe <spec>      — static outline (describe/it titles) of a spec
//   probierz cmd <target>         — the exact shell command for a target (prints only)
//   probierz check <target>       — is the toolchain ready? what is missing + how to fix
//   probierz setup <target>       — install the parts probierz owns (browsers/drivers)
//   probierz run <target> [opts]  — EXECUTE a target (preflight-gated), auto-analyze
//   probierz analyze <report> [dir] — parse a report + inventory media
//   probierz readme-gif <video> --out <gif> — publish one bounded journey demo
//   probierz affected [ref]       — which targets a change touched (git diff, or --files)
//   probierz ci [ref] [opts]      — change-driven pass: affected -> run ready -> analyze
//   probierz figure-evaluate ...  — render and score a scientific figure pair
//
// run options: --record  --force  --frames N  --timeout MS  --no-analyze  KEY=VALUE...
// analyze options: [artifactsDir] --frames N --tool playwright|wdio
import { SURFACES, listSpecs, describeSpec, runCommand } from "./lib.mjs";
import { appSourceIdentity, completeRun, runSurface, targetList } from "./runner.mjs";
import { analyzeRun } from "./analyze.mjs";
import { createReadmeGif } from "./readme-gif.mjs";
import { preflight, runSetup } from "./preflight.mjs";
import { affectedFromGit, affectedTargets } from "./affected.mjs";
import { orchestrate } from "./orchestrate.mjs";
import { listApps, loadAppManifest } from "./apps.mjs";
import { validateAccessibility } from "./accessibility.mjs";
import { compareRuns, lastGreen, runHistory } from "./history.mjs";
import { createReceipt, verifyReceipt } from "./receipt.mjs";
import { createPublicationManifest } from "./publication.mjs";
import { createOnboardingPublication } from "./onboarding-publication.mjs";
import { dashboardProjection } from "./dashboard.mjs";
import { planMatrix, runMatrix } from "./matrix.mjs";
import { enforceRetention, protectRun, restoreBundle } from "./artifacts.mjs";
import { auditTrail, scanSecrets } from "./security.mjs";
import { activateGate, enforceGate, evaluateGate, gateStatus } from "./gate.mjs";
import { appStatus, renderAppStatus } from "./status.mjs";
import { prepushGate } from "./prepush-gate.mjs";
import { authorSpec } from "./author-spec.mjs";
import { authorManifest } from "./author-manifest.mjs";
import { listHosts, submitRemoteRun, submitRemoteAuthor, submitRemoteSeo } from "./stado.mjs";
import { overview, renderOverview } from "./overview.mjs";
import { EXIT_RETRY, reportBoundaryFailure } from "./failure.mjs";
import { evaluateFigure } from "./figure-evaluate.mjs";
import { evaluateSeo } from "./seo-evaluate.mjs";
import { runOnboarding } from "./onboarding.mjs";
import { existsSync, lstatSync, readFileSync, renameSync, unlinkSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const AGENT_DIR = path.dirname(fileURLToPath(import.meta.url));

function out(value) {
  process.stdout.write(JSON.stringify(value, null, Number("2")) + "\n");
}

function usage() {
  process.stderr.write(
    [
      "usage:",
      "  probierz onboarding [--reset]  show the first-run walkthrough; reset recorded progress and evidence to replay",
      "  probierz list                 every test surface + run script",
      "  probierz apps                 registered products, targets, and journeys",
      "  probierz app <appId>          validated product manifest",
      "  probierz source-identity <appId> [--app-repo path]  exact harness and selected app source SHA-256",
      "  probierz specs [surface]      spec files on disk (optional surface filter)",
      "  probierz accessibility <appId>  validate stable IDs and native selectors",
      "  probierz history [appId] [target] [--limit N]  stability by run, journey, and test",
      "  probierz dashboard <appId> [limit]  product/version/journey evidence projection",
      "  probierz status <appId> [--base ref] [--text]  journey coverage, freshness vs HEAD, and merge eligibility (exit 1 when blocked)",
      "  probierz author-spec <appId> <journey> --target <t> --desc <goal> [--base-url u | --app-path p] [--paths glob] [--rounds N] [--dry-run]  draft through the authenticated Stado model router, verify with a real run, keep it green",
      "  probierz author-manifest <appId> --desc <what> --repo <path> --target <t> [--base-url u | --app-path p] [--owner s] [--specs] [--dry-run]  draft through the authenticated Stado model router, then optionally cover every journey",
      "  probierz hosts              run hosts: local and stado providers",
      "  probierz overview [appId...] [--text]  unified status: journeys + merge eligibility + violations + stado fleet health",
      "  probierz stado run <target> --app <id> [--spec f] [--record] [--host stado:gcp|azure|aws|any|spot|mini|ubuntu|macbook] [--cargo-release --app-repo p --binary b [--cargo-manifest p] | --app-bundle-path p --app-repo p | --node-source --app-repo p [--script apps/<id>/remote/x.sh]] [--env=K=V ...] [--no-watch]  run a target on a chosen stado host, evidence lands back in test-results",
      "  probierz stado seo <appId> --base-url <url> --primary-model <id> --secondary-model <id> --adjudicator-model <id> [--mode pull-request|release|nightly|production] [--policy json] [--brief json] [--production-evidence json] [--agent-id id] [--host stado:mini] [--no-watch]  execute the complete SEO evaluator on a Stado-selected dedicated host",
      "  probierz stado author <appId> <journey> --target <t> --desc <d> [--host h] [--app-path p | --cargo-release --binary b --app-repo r [--cargo-manifest p] | --app-bundle-path p --app-repo r] [--no-watch]  author on a Stado host with scoped model credentials; the accepted spec + manifest land back here",
      "  probierz matrix <appId> <nightly|release> [--plan] [--release id] [KEY=VALUE...]",
      "  probierz protect <appId> <runId> [kind] --key-file <path> [--remove-source]",
      "  probierz restore <bundle> <destination> --key-file <path>",
      "  probierz retention <appId> [--at ISO] [--apply]",
      "  probierz secret-scan <directory>",
      "  probierz audit [appId] [--run runId] [--action name] [--limit N]",
      "  probierz gate-status <appId>",
      "  probierz gate-prepush [--repo path] [--app id] [--base ref] [--head sha] [--ci]  pre-push merge gate (exit 1 when blocked)",
      "  probierz gate-install <appId> [--repo path]  install the pre-push gate hook into a repository (chains an existing hook)",
      "  probierz gate-evaluate|gate-enforce|gate-activate <appId> <pull-request|release> <harnessSha256> --source-sha SHA256 --runs ids [release opts]",
      "  probierz compare <leftRunId> <rightRunId> [appId]  deterministic run diff",
      "  probierz last-green [appId] [target] [journey]  newest passing evidence",
      "  probierz receipt <appId> <release> <harnessSha256> --source-sha SHA256 --runs ids  sign evidence receipt",
      "  probierz verify-receipt <file>  verify signature, trust, and payload hash",
      "  probierz publication <receipt> <attemptId> <journeyId> --assets <json> [--public-key file | --fingerprint sha256]  emit immutable verified first-use publication manifest",
      "  probierz publish-onboarding <receipt> --run id --journey id --journey-version v --journey-version-id uuid --first-success-fact fact --screen id --assets catalog.json --output publication.json  emit an Echo-ingestible first-use proof manifest",
      "  probierz describe <spec>      static outline of a spec file",
      "  probierz cmd <target>         exact command to run a target (prints only)",
      "  probierz check <target>       is the toolchain ready + how to fix what is missing",
      "  probierz setup <target>       install the parts probierz owns (browsers/appium drivers)",
      "  probierz run <target> [opts]  execute a target (preflight-gated), capture result, auto-analyze",
      "  probierz analyze <report> [dir]  parse a report + inventory media",
      "  probierz readme-gif <video> --out <file.gif> [--start seconds] [--duration seconds] [--fps N] [--width pixels] [--force]  render a bounded, silent journey demo plus provenance sidecar",
      "  probierz affected [ref]       which targets a change affects (git diff vs ref, or --files a b c)",
      "  probierz figure-evaluate --reference <svg|tex|pdf|image> --candidate <svg|tex|pdf|image> [--rubric json] [--model id] [--out report.json] [--tex-preamble file] [--router-url url] [--agent-id id] [--router-token-stdin]  deterministic render checks plus rubric-scored vision evaluation; --router-token-stdin reads the bearer on the first stdin line and an optional agent secret on the second",
      "  probierz seo-evaluate --app <id> --base-url <url> [--policy json] [--brief json] [--mode pull-request|release|nightly|production] [--out report.json] [--production-evidence json] [--primary-model id] [--secondary-model id] [--adjudicator-model id] [--router-url url] [--agent-id id] [--private-key-file pem] [--router-token-stdin]  full crawl, indexability, structured-data, dual-model content, performance, production, and signed SEO verdict",
      "  probierz ci [ref] [opts]      change-driven: select affected targets, run the ready ones, analyze",
      "",
      "run opts: --app <appId>  --app-repo <path>  --record  --force (skip preflight)  --spec <path>  --frames N  --timeout MS  --resource-wait MS  --no-analyze  KEY=VALUE...",
      "surfaces: web | electron | mobile | desktop-native | desktop-cua | tui",
      "targets:  web | electron | mobile:ios | mobile:android | desktop:mac | desktop:cua | desktop:win | tui",
    ].join("\n") + "\n",
  );
}
function configError(message) {
  const error = new Error(message);
  error.exitCode = Number("2");
  return error;
}

/**
 * A completed run and an accepted asynchronous submission are both successful
 * CLI outcomes. Terminal and infrastructure failures keep the bridge's
 * classified exit code so wrappers can distinguish retryable outages.
 */
function remoteExit(result) {
  if (
    result.state === "completed"
    || (result.state === "queued" && result.submitted === true && !result.failure)
  ) return;
  process.stderr.write(`${result.failure?.message || `Remote run ended as "${result.state}".`}\n`);
  process.exitCode = result.failure?.retryable ? EXIT_RETRY : Number("1");
}

function validateAuthorOptions(args, { positionalCount, valueFlags, booleanFlags }) {
  const values = new Set(valueFlags);
  const booleans = new Set(booleanFlags);
  for (let index = positionalCount; index < args.length; index += Number("1")) {
    const arg = args[index];
    if (booleans.has(arg)) continue;
    if (values.has(arg)) {
      const value = args[index + Number("1")];
      if (value === undefined || value.startsWith("--")) throw configError(`${arg} needs a value`);
      index += Number("1");
      continue;
    }
    throw configError(arg.startsWith("--") ? `unknown option: ${arg}` : `unexpected argument: ${arg}`);
  }
}



// Split execution args into flags and an env map. Unknown flags are rejected so
// a misspelled gate option cannot silently broaden a run.
function parseRunArgs(rest, { allowPositionals = false } = {}) {
  const opts = {
    env: {},
    record: false,
    analyze: true,
    force: false,
    appId: undefined,
    spec: undefined,
    frames: Number("0"),
    timeoutMs: Number("0"),
    resourceWaitMs: undefined,
    tool: undefined,
  };
  const valueFor = (index, flag) => {
    const value = rest[index + Number("1")];
    if (value === undefined || value.startsWith("--")) throw configError(`${flag} needs a value`);
    return value;
  };
  for (let i = Number("0"); i < rest.length; i += Number("1")) {
    const arg = rest[i];
    if (arg === "--record") opts.record = true;
    else if (arg === "--force") opts.force = true;
    else if (arg === "--no-analyze") opts.analyze = false;
    else if (arg === "--frames") {
      opts.frames = Number(valueFor(i, arg));
      i += Number("1");
    } else if (arg === "--timeout") {
      opts.timeoutMs = Number(valueFor(i, arg));
      i += Number("1");
    } else if (arg === "--resource-wait") {
      opts.resourceWaitMs = Number(valueFor(i, arg));
      i += Number("1");
    } else if (arg === "--spec") {
      opts.spec = valueFor(i, arg);
      i += Number("1");
    } else if (arg === "--app") {
      opts.appId = valueFor(i, arg);
      i += Number("1");
    } else if (arg === "--app-repo") {
      opts.appRepo = valueFor(i, arg);
      i += Number("1");
    } else if (arg === "--tool") {
      opts.tool = valueFor(i, arg);
      i += Number("1");
    } else if (arg === "--files") {
      // Parsed by the affected/ci command.
    } else if (arg.startsWith("--")) {
      throw configError(`unknown option: ${arg}`);
    } else if (arg.includes("=")) {
      const eq = arg.indexOf("=");
      opts.env[arg.slice(Number("0"), eq)] = arg.slice(eq + Number("1"));
    } else if (!allowPositionals) {
      throw configError(`unexpected argument: ${arg}`);
    }
  }
  if (!Number.isFinite(opts.frames) || opts.frames < Number("0")) throw configError("--frames needs a non-negative number");
  if (!Number.isFinite(opts.timeoutMs) || opts.timeoutMs < Number("0")) throw configError("--timeout needs a non-negative number");
  if (opts.resourceWaitMs !== undefined && (!Number.isFinite(opts.resourceWaitMs) || opts.resourceWaitMs < Number("0"))) {
    throw configError("--resource-wait needs a non-negative number");
  }
  return opts;
}

function parseReadmeGifArgs(rest) {
  const input = rest["".length];
  if (!input || input.startsWith("--")) throw configError("readme-gif needs an input video");
  const options = { input, output: undefined, force: false };
  const valueFlags = new Map([
    ["--out", "output"],
    ["--start", "startSeconds"],
    ["--duration", "durationSeconds"],
    ["--fps", "framesPerSecond"],
    ["--width", "width"],
  ]);
  for (let index = "x".length; index < rest.length; index += "x".length) {
    const flag = rest[index];
    if (flag === "--force") {
      options.force = true;
      continue;
    }
    const key = valueFlags.get(flag);
    if (!key) throw configError(flag.startsWith("--") ? `unknown option: ${flag}` : `unexpected argument: ${flag}`);
    const value = rest[index + "x".length];
    if (value === undefined || value.startsWith("--")) throw configError(`${flag} needs a value`);
    options[key] = value;
    index += "x".length;
  }
  if (!options.output) throw configError("readme-gif needs --out <file.gif>");
  return options;
}

function filesAfterFlag(args) {
  const index = args.indexOf("--files");
  if (index < Number("0")) return null;
  const values = [];
  const flagsWithValues = new Set(["--frames", "--timeout", "--resource-wait", "--spec", "--app", "--tool"]);
  for (let i = index + Number("1"); i < args.length; i += Number("1")) {
    const arg = args[i];
    if (flagsWithValues.has(arg)) {
      i += Number("1");
    } else if (!arg.startsWith("--") && !arg.includes("=")) {
      values.push(arg);
    }
  }
  return values;
}

function parseGateArgs(args) {
  const [appId, mode, expectedHarnessSha] = args;
  if (!appId || !mode || !expectedHarnessSha) throw configError("gate needs an app ID, mode, and harness SHA-256");
  const value = (flag) => {
    const index = args.indexOf(flag);
    const result = index >= 0 ? args[index + 1] : undefined;
    if (index >= 0 && (!result || result.startsWith("--"))) throw configError(`${flag} needs a value`);
    return result;
  };
  return {
    appId,
    mode,
    expectedHarnessSha,
    expectedSourceSha: value("--source-sha"),
    runIds: String(value("--runs") || "").split(",").filter(Boolean),
    release: value("--release"),
    receiptFile: value("--receipt"),
    trustedPublicKeyFile: value("--public-key"),
    expectedFingerprint: value("--fingerprint"),
  };
}

async function main() {
  const argv = process.argv.slice(Number("2"));
  const cmd = argv[Number("0")];
  const rest = argv.slice(Number("1"));

  if (!cmd || cmd === "help" || cmd === "-h" || cmd === "--help") {
    usage();
    return;
  }
  if (cmd === "onboarding") {
    runOnboarding(rest);
    return;
  }
  if (cmd === "list") {
    out(SURFACES);
    return;
  }
  if (cmd === "apps") {
    out(listApps());
    return;
  }
  if (cmd === "app") {
    const appId = rest[Number("0")];
    if (!appId) throw configError("app needs an app ID");
    out(loadAppManifest(appId));
    return;
  }
  if (cmd === "source-identity") {
    validateAuthorOptions(rest, {
      positionalCount: Number("1"),
      valueFlags: ["--app-repo"],
      booleanFlags: [],
    });
    const appId = rest[Number("0")];
    if (!appId) throw configError("source-identity needs an app ID");
    const repoIndex = rest.indexOf("--app-repo");
    out(appSourceIdentity(appId, {
      primaryRoot: repoIndex === -1 ? null : rest[repoIndex + 1],
    }));
    return;
  }
  if (cmd === "accessibility") {
    const appId = rest[Number("0")];
    if (!appId) throw configError("accessibility needs an app ID");
    const result = validateAccessibility(appId);
    out(result);
    if (!result.ok) process.exitCode = Number("1");
    return;
  }
  if (cmd === "figure-evaluate") {
    const valueFlags = new Set(["--reference", "--candidate", "--rubric", "--model", "--out", "--router-url", "--tex-preamble", "--agent-id"]);
    const booleanFlags = new Set(["--router-token-stdin"]);
    const options = {};
    for (let index = 0; index < rest.length; index += 1) {
      const flag = rest[index];
      if (booleanFlags.has(flag)) {
        if (options[flag] !== undefined) throw configError(`${flag} may be supplied only once`);
        options[flag] = true;
        continue;
      }
      if (!valueFlags.has(flag)) throw configError(`unknown figure-evaluate option: ${flag}`);
      const value = rest[index + 1];
      if (!value || value.startsWith("--")) throw configError(`${flag} needs a value`);
      if (options[flag] !== undefined) throw configError(`${flag} may be supplied only once`);
      options[flag] = value;
      index += 1;
    }
    if (!options["--reference"]) throw configError("figure-evaluate needs --reference");
    if (!options["--candidate"]) throw configError("figure-evaluate needs --candidate");
    const stdinLines = options["--router-token-stdin"]
      ? readFileSync(Number("0"), "utf8").split("\n")
      : [];
    const result = await evaluateFigure({
      referencePath: options["--reference"],
      candidatePath: options["--candidate"],
      rubricPath: options["--rubric"],
      outputPath: options["--out"],
      model: options["--model"],
      routerBaseUrl: options["--router-url"],
      texPreamblePath: options["--tex-preamble"],
      routerBearer: stdinLines[Number("0")],
      agentId: options["--agent-id"],
      agentSecret: stdinLines[Number("1")],
    });
    out(result);
    if (!result.verdict.pass) process.exitCode = Number("1");
    return;
  }
  if (cmd === "seo-evaluate") {
    const valueFlags = new Set([
      "--app", "--base-url", "--policy", "--brief", "--mode", "--out", "--production-evidence",
      "--primary-model", "--secondary-model", "--adjudicator-model", "--router-url", "--agent-id", "--private-key-file",
    ]);
    const booleanFlags = new Set(["--router-token-stdin"]);
    const options = {};
    for (let index = 0; index < rest.length; index += 1) {
      const flag = rest[index];
      if (booleanFlags.has(flag)) {
        if (options[flag] !== undefined) throw configError(`${flag} may be supplied only once`);
        options[flag] = true;
        continue;
      }
      if (!valueFlags.has(flag)) throw configError(`unknown seo-evaluate option: ${flag}`);
      const value = rest[index + 1];
      if (!value || value.startsWith("--")) throw configError(`${flag} needs a value`);
      if (options[flag] !== undefined) throw configError(`${flag} may be supplied only once`);
      options[flag] = value;
      index += 1;
    }
    if (!options["--base-url"]) throw configError("seo-evaluate needs --base-url");
    const stdinLines = options["--router-token-stdin"]
      ? readFileSync(Number("0"), "utf8").split("\n")
      : [];
    const result = await evaluateSeo({
      appId: options["--app"] || "landing-page",
      baseUrl: options["--base-url"],
      policyPath: options["--policy"],
      briefPath: options["--brief"],
      mode: options["--mode"] || "release",
      outputPath: options["--out"],
      productionEvidencePath: options["--production-evidence"],
      primaryModel: options["--primary-model"],
      secondaryModel: options["--secondary-model"],
      adjudicatorModel: options["--adjudicator-model"],
      routerBaseUrl: options["--router-url"],
      agentId: options["--agent-id"],
      privateKeyFile: options["--private-key-file"],
      routerBearer: stdinLines[Number("0")],
      agentSecret: stdinLines[Number("1")],
      privateKey: stdinLines.slice(Number("2")).join("\n").trim() || undefined,
    });
    out(result);
    if (!result.pass) process.exitCode = Number("1");
    return;
  }
  if (cmd === "history") {
    const positionals = [];
    let limit = 50;
    for (let index = 0; index < rest.length; index += 1) {
      if (rest[index] === "--limit") {
        limit = Number(rest[index + 1]);
        index += 1;
      } else if (rest[index].startsWith("--")) {
        throw configError(`unknown history option: ${rest[index]}`);
      } else {
        positionals.push(rest[index]);
      }
    }
    if (!Number.isFinite(limit) || limit <= 0) throw configError("--limit needs a positive number");
    if (positionals.length > 2) throw configError("history accepts [appId] [target]");
    out(runHistory({ appId: positionals[0] || "probierz", target: positionals[1], limit }));
    return;
  }
  if (cmd === "dashboard") {
    if (!rest[0]) throw configError("dashboard needs an app ID");
    out(dashboardProjection({ appId: rest[0], limit: Number(rest[1]) || 500 }));
    return;
  }
  if (cmd === "status") {
    if (!rest[0] || rest[0].startsWith("--")) throw configError("status needs an app ID");
    const baseIndex = rest.indexOf("--base");
    const baseRef = baseIndex >= 0 ? rest[baseIndex + 1] : "origin/main";
    if (baseIndex >= 0 && (!baseRef || baseRef.startsWith("--"))) throw configError("--base needs a ref");
    const json = !rest.includes("--text");
    const result = appStatus({ appId: rest[0], baseRef });
    if (json) out(result);
    else process.stdout.write(`${renderAppStatus(result)}\n`);
    if (!result.mergeEligibility.eligible) process.exitCode = Number("1");
    return;
  }
  if (cmd === "hosts") {
    out(listHosts());
    return;
  }
  if (cmd === "overview") {
    const appIds = rest.filter((arg) => !arg.startsWith("--"));
    const report = await overview({ appIds: appIds.length ? appIds : null });
    if (rest.includes("--text")) process.stdout.write(`${renderOverview(report)}\n`);
    else out(report);
    return;
  }
  if (cmd === "stado") {
    const sub = rest[0];
    if (!["run", "author", "seo"].includes(sub)) throw configError("usage: probierz stado run <target> --app <id> [...] | probierz stado author <appId> <journey> [...] | probierz stado seo <appId> --base-url <url> --primary-model <id> --secondary-model <id> --adjudicator-model <id> [...]");
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    if (sub === "author") {
      validateAuthorOptions(rest, {
        positionalCount: Number("3"),
        valueFlags: ["--target", "--desc", "--app-path", "--app-bundle-path", "--app-repo", "--binary", "--cargo-manifest", "--host"],
        booleanFlags: ["--cargo-release", "--no-watch"],
      });
      const appId = rest[1];
      const journey = rest[2];
      if (!appId || appId.startsWith("--") || !journey || journey.startsWith("--")) {
        throw configError("stado author needs an app ID and a journey name");
      }
      const target = value("--target");
      const desc = value("--desc");
      if (!target) throw configError("stado author needs --target <t>");
      if (!desc) throw configError("stado author needs --desc <journey goal>");
      const provision = value("--app-path")
        ? { kind: "installed-tui", appId, path: value("--app-path") }
        : value("--app-bundle-path")
          ? { kind: "app-bundle", appId, bundlePath: value("--app-bundle-path") }
          : rest.includes("--cargo-release")
            ? { kind: "cargo-release", appId, binary: value("--binary") || appId, manifestPath: value("--cargo-manifest") || "Cargo.toml" }
            : null;
      if (target === "tui" && !["installed-tui", "cargo-release"].includes(provision?.kind)) {
        throw configError("stado author --target tui needs --app-path <installed-command> or --cargo-release --app-repo <path> [--binary <name>]");
      }
      const result = await submitRemoteAuthor({
        appId,
        journey,
        target,
        desc,
        host: value("--host") || "stado:gcp",
        provision,
        appRepo: value("--app-repo") || null,
        watch: !rest.includes("--no-watch"),
      });
      out(result);
      remoteExit(result);
      return;
    }
    if (sub === "seo") {
      validateAuthorOptions(rest, {
        positionalCount: Number("2"),
        valueFlags: [
          "--base-url", "--mode", "--policy", "--brief", "--production-evidence",
          "--primary-model", "--secondary-model", "--adjudicator-model", "--agent-id", "--host",
        ],
        booleanFlags: ["--no-watch"],
      });
      const appId = rest[1];
      if (!appId || appId.startsWith("--")) throw configError("stado seo needs an app ID");
      const result = await submitRemoteSeo({
        appId,
        baseUrl: value("--base-url"),
        mode: value("--mode") || "release",
        policyPath: value("--policy") || loadAppManifest(appId).seo?.policy,
        briefPath: value("--brief") || loadAppManifest(appId).seo?.brief,
        primaryModel: value("--primary-model"),
        secondaryModel: value("--secondary-model"),
        adjudicatorModel: value("--adjudicator-model"),
        agentId: value("--agent-id") || "probierz",
        productionEvidencePath: value("--production-evidence") ? path.resolve(value("--production-evidence")) : null,
        host: value("--host") || "stado:mini",
        watch: !rest.includes("--no-watch"),
      });
      out(result);
      remoteExit(result);
      return;
    }
    const target = rest[1];
    if (!target) throw configError("stado run needs a target (e.g. tui)");
    const appId = value("--app");
    if (!appId) throw configError("stado run needs --app <appId>");
    const environment = [];
    for (const flag of rest.filter((entry) => entry.startsWith("--env="))) {
      const assignment = flag.slice("--env=".length);
      const separator = assignment.indexOf("=");
      const key = separator < Number("0") ? assignment : assignment.slice(Number("0"), separator);
      const val = separator < Number("0") ? "" : assignment.slice(separator + Number("1"));
      if (key) environment.push([key, val]);
    }
    const scriptPath = value("--script") || null;
    const provision = rest.includes("--cargo-release")
      ? { kind: "cargo-release", appId, binary: value("--binary") || appId, manifestPath: value("--cargo-manifest") || "Cargo.toml" }
      : value("--app-bundle-path")
        ? { kind: "app-bundle", appId, bundlePath: value("--app-bundle-path") }
        : rest.includes("--node-source")
          ? { kind: "node-source", appId, script: scriptPath }
          : null;
    if (scriptPath && provision?.kind !== "node-source") {
      throw configError("--script requires --node-source (custom app jobs run from app sources)");
    }
    const result = await submitRemoteRun({
      target,
      appId,
      spec: value("--spec") || null,
      host: value("--host") || "stado:gcp",
      provision,
      appRepo: value("--app-repo") || null,
      watch: !rest.includes("--no-watch"),
      environment,
      mode: scriptPath ? "script" : "run",
      record: rest.includes("--record"),
    });
    out(result);
    remoteExit(result);
    return;
  }
  if (cmd === "author-manifest") {
    validateAuthorOptions(rest, {
      positionalCount: Number("1"),
      valueFlags: ["--desc", "--target", "--repo", "--owner", "--base-url", "--app-path"],
      booleanFlags: ["--dry-run", "--specs"],
    });
    const appId = rest[0];
    if (!appId || appId.startsWith("--")) throw configError("author-manifest needs an app ID");
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    const desc = value("--desc");
    if (!desc) throw configError("author-manifest needs --desc <what the app does>");
    const target = value("--target");
    if (!target) throw configError("author-manifest needs --target <web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui>");
    const repositories = [];
    for (let index = 0; index < rest.length; index += 1) {
      if (rest[index] === "--repo" && rest[index + 1]) {
        repositories.push(rest[index + 1]);
        index += 1;
      }
    }
    if (!repositories.length) throw configError("author-manifest needs at least one --repo <path>");
    const result = await authorManifest({
      appId,
      desc,
      owner: value("--owner") || null,
      repositories,
      target,
      baseUrl: value("--base-url") || null,
      appPath: value("--app-path") || null,
      dryRun: rest.includes("--dry-run"),
      withSpecs: rest.includes("--specs"),
    });
    out(result);
    if (!result.ok) process.exitCode = Number("1");
    return;
  }
  if (cmd === "author-spec") {
    validateAuthorOptions(rest, {
      positionalCount: Number("2"),
      valueFlags: ["--desc", "--target", "--paths", "--base-url", "--app-path", "--rounds"],
      booleanFlags: ["--dry-run"],
    });
    const appId = rest[0];
    const journey = rest[1];
    if (!appId || appId.startsWith("--") || !journey || journey.startsWith("--")) {
      throw configError("author-spec needs an app ID and a journey name");
    }
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    const desc = value("--desc");
    if (!desc) throw configError("author-spec needs --desc <journey goal>");
    const target = value("--target");
    if (!target) throw configError("author-spec needs --target <web|electron|mobile:ios|mobile:android|desktop:mac|desktop:cua|desktop:win|tui>");
    const mappingPaths = [];
    for (let index = 0; index < rest.length; index += 1) {
      if (rest[index] === "--paths" && rest[index + 1]) {
        mappingPaths.push(rest[index + 1]);
        index += 1;
      }
    }
    const result = await authorSpec({
      appId,
      journey,
      target,
      desc,
      baseUrl: value("--base-url") || null,
      appPath: value("--app-path") || null,
      mappingPaths,
      rounds: Number(value("--rounds")) || Number("3"),
      dryRun: rest.includes("--dry-run"),
    });
    out(result);
    if (!result.ok) process.exitCode = Number("1");
    return;
  }
  if (cmd === "gate-status") {
    if (!rest[0]) throw configError("gate-status needs an app ID");
    out(gateStatus(rest[0]));
    return;
  }
  if (cmd === "gate-evaluate" || cmd === "gate-enforce" || cmd === "gate-activate") {
    const options = parseGateArgs(rest);
    const result = await (cmd === "gate-evaluate"
      ? evaluateGate(options)
      : (cmd === "gate-enforce" ? enforceGate(options) : activateGate(options)));
    out(result);
    if (cmd !== "gate-activate" && !result.verdict.passed) process.exitCode = Number("1");
    return;
  }
  if (cmd === "gate-prepush") {
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    const result = await prepushGate({
      repo: value("--repo") || process.cwd(),
      appId: value("--app") || null,
      base: value("--base") || null,
      head: value("--head") || null,
      runCi: rest.includes("--ci"),
    });
    out(result);
    if (!result.ok) process.exitCode = Number("1");
    return;
  }
  if (cmd === "gate-install") {
    const appId = rest[0];
    if (!appId || appId.startsWith("--")) throw configError("gate-install needs an app ID");
    loadAppManifest(appId);
    const repoIndex = rest.indexOf("--repo");
    const repo = repoIndex >= 0 ? rest[repoIndex + 1] : process.cwd();
    if (repoIndex >= 0 && (!repo || repo.startsWith("--"))) throw configError("--repo needs a path");
    const hooksDir = path.join(repo, ".git", "hooks");
    if (!existsSync(hooksDir)) throw configError(`not a git working tree: ${repo}`);
    const target = path.join(hooksDir, "pre-push");
    const backup = path.join(hooksDir, "pre-push.before-probierz-gate");
    const managedMarker = "# managed-by: probierz-prepush-gate";
    const managedCommand = path.join(AGENT_DIR, "prepush-gate.mjs");
    const present = (file) => {
      try { lstatSync(file); return true; } catch { return false; }
    };
    const managed = (file) => {
      try {
        const content = readFileSync(file, "utf8");
        return content.includes(managedMarker)
          || (content.includes(managedCommand) && content.includes("--hook --app"));
      } catch {
        return false;
      }
    };
    if (present(backup) && managed(backup)) unlinkSync(backup);
    if (present(target) && !present(backup) && !managed(target)) renameSync(target, backup);
    const script = [
      "#!/bin/sh",
      managedMarker,
      'HOOK_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)',
      'if [ -f "$HOOK_DIR/pre-push.before-probierz-gate" ]; then',
      '  "$HOOK_DIR/pre-push.before-probierz-gate" "$@" || exit $?',
      "fi",
      'GATE_CI="--ci"',
      'if [ "${PROBIERZ_GATE_NO_CI:-}" = "1" ]; then GATE_CI=""; fi',
      `exec node ${managedCommand} --hook --app ${appId} $GATE_CI`,
      "",
    ].join("\n");
    mkdirSync(hooksDir, { recursive: true });
    writeFileSync(target, script, { mode: 0o755 });
    out({ installed: target, chained: existsSync(backup), appId });
    return;
  }
  if (cmd === "secret-scan") {
    if (!rest[0]) throw configError("secret-scan needs a directory");
    const result = await scanSecrets(rest[0]);
    out(result);
    if (!result.passed) process.exitCode = Number("1");
    return;
  }
  if (cmd === "audit") {
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    const limit = Number(value("--limit") || 200);
    if (!Number.isFinite(limit) || limit <= 0) throw configError("--limit needs a positive number");
    out(auditTrail({
      appId: rest[0] && !rest[0].startsWith("--") ? rest[0] : undefined,
      runId: value("--run"),
      action: value("--action"),
      limit,
    }));
    return;
  }
  if (cmd === "protect") {
    if (!rest[0] || !rest[1]) throw configError("protect needs an app ID and run ID");
    const keyIndex = rest.indexOf("--key-file");
    if (keyIndex >= 0 && (!rest[keyIndex + 1] || rest[keyIndex + 1].startsWith("--"))) throw configError("--key-file needs a path");
    out(await protectRun({
      appId: rest[0],
      runId: rest[1],
      kind: rest[2] && !rest[2].startsWith("--") ? rest[2] : undefined,
      keyFile: keyIndex >= 0 ? rest[keyIndex + 1] : undefined,
      removePlaintext: rest.includes("--remove-source"),
    }));
    return;
  }
  if (cmd === "restore") {
    if (!rest[0] || !rest[1]) throw configError("restore needs a bundle and destination");
    const keyIndex = rest.indexOf("--key-file");
    if (keyIndex >= 0 && (!rest[keyIndex + 1] || rest[keyIndex + 1].startsWith("--"))) throw configError("--key-file needs a path");
    out(await restoreBundle({
      file: rest[0],
      destination: rest[1],
      keyFile: keyIndex >= 0 ? rest[keyIndex + 1] : undefined,
    }));
    return;
  }
  if (cmd === "retention") {
    if (!rest[0]) throw configError("retention needs an app ID");
    const atIndex = rest.indexOf("--at");
    if (atIndex >= 0 && (!rest[atIndex + 1] || rest[atIndex + 1].startsWith("--"))) throw configError("--at needs an ISO timestamp");
    out(enforceRetention({
      appId: rest[0],
      now: atIndex >= 0 ? new Date(rest[atIndex + 1]) : new Date(),
      apply: rest.includes("--apply"),
    }));
    return;
  }
  if (cmd === "matrix") {
    const [appId, profile] = rest;
    if (!appId || !profile) throw configError("matrix needs an app ID and profile");
    const planOnly = rest.includes("--plan");
    const releaseIndex = rest.indexOf("--release");
    const release = releaseIndex >= 0 ? rest[releaseIndex + 1] : undefined;
    if (releaseIndex >= 0 && (!release || release.startsWith("--"))) throw configError("--release needs an ID");
    if (profile === "release" && !planOnly && !release) throw configError("release matrix execution needs --release <id>");
    const env = {};
    for (let index = 2; index < rest.length; index += 1) {
      const value = rest[index];
      if (value === "--plan") continue;
      if (value === "--release") {
        index += 1;
        continue;
      }
      if (!value.includes("=")) throw configError(`unexpected matrix argument: ${value}`);
      const equals = value.indexOf("=");
      env[value.slice(0, equals)] = value.slice(equals + 1);
    }
    const result = planOnly ? planMatrix({ appId, profile }) : await runMatrix({ appId, profile, env, release });
    out(result);
    if (!planOnly && !result.verdict.passed) process.exitCode = Number("1");
    return;
  }
  if (cmd === "compare") {
    if (!rest[0] || !rest[1]) throw configError("compare needs left and right run IDs");
    out(compareRuns({ leftRunId: rest[0], rightRunId: rest[1], appId: rest[2] || "probierz" }));
    return;
  }
  if (cmd === "last-green") {
    out(lastGreen({ appId: rest[0] || "probierz", target: rest[1], journey: rest[2] }));
    return;
  }
  if (cmd === "receipt") {
    const [appId, release, expectedHarnessSha] = rest;
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    const runIds = String(value("--runs") || "").split(",").filter(Boolean);
    const requiredJourneys = String(value("--journeys") || "").split(",").filter(Boolean);
    const result = await createReceipt({
      appId,
      release,
      expectedHarnessSha,
      expectedSourceSha: value("--source-sha"),
      runIds,
      requiredJourneys,
      minimumEvidence: value("--minimum") || "E3",
    });
    out(result);
    if (!result.receipt.verdict.passed) process.exitCode = Number("1");
    return;
  }
  if (cmd === "publication") {
    const [receiptFile, attemptId, journeyId] = rest;
    const value = (flag) => {
      const index = rest.indexOf(flag);
      return index >= 0 ? rest[index + 1] : undefined;
    };
    const assetsFile = value("--assets");
    if (!receiptFile || !attemptId || !journeyId || !assetsFile) {
      throw configError("publication needs receipt, attemptId, journeyId, and --assets <json>");
    }
    const assets = JSON.parse(readFileSync(assetsFile, "utf8"));
    out(createPublicationManifest({
      receiptFile,
      attemptId,
      journeyId,
      assets,
      trustedPublicKeyFile: value("--public-key"),
      expectedFingerprint: value("--fingerprint"),
    }));
    return;
  }
  if (cmd === "verify-receipt") {
    if (!rest[0]) throw configError("verify-receipt needs a file");
    const publicKeyIndex = rest.indexOf("--public-key");
    const fingerprintIndex = rest.indexOf("--fingerprint");
    const result = verifyReceipt(rest[0], {
      trustedPublicKeyFile: publicKeyIndex >= 0 ? rest[publicKeyIndex + 1] : undefined,
      expectedFingerprint: fingerprintIndex >= 0 ? rest[fingerprintIndex + 1] : undefined,
    });
    out(result);
    if (!result.valid) process.exitCode = Number("1");
    return;
  }
  if (cmd === "publish-onboarding") {
    const value = (flag) => {
      const index = rest.indexOf(flag);
      const result = index >= 0 ? rest[index + 1] : undefined;
      if (!result || result.startsWith("--")) throw configError(`${flag} needs a value`);
      return result;
    };
    const result = createOnboardingPublication({
      receiptFile: rest[0],
      runId: value("--run"),
      journeyId: value("--journey"),
      journeyVersion: value("--journey-version"),
      journeyVersionId: value("--journey-version-id"),
      firstSuccessFact: value("--first-success-fact"),
      screenId: value("--screen"),
      assetCatalogFile: value("--assets"),
      outputFile: value("--output"),
      trustedPublicKeyFile: rest.includes("--public-key") ? value("--public-key") : undefined,
      expectedFingerprint: rest.includes("--fingerprint") ? value("--fingerprint") : undefined,
    });
    out({ file: result.file, manifestId: result.manifestId });
    return;
  }
  if (cmd === "specs") {
    out(listSpecs(rest[Number("0")]));
    return;
  }
  if (cmd === "describe") {
    const spec = rest[Number("0")];
    if (!spec) throw new Error("describe needs a spec path");
    out(describeSpec(spec));
    return;
  }
  if (cmd === "cmd") {
    const target = rest[Number("0")];
    if (!target) throw new Error("cmd needs a target");
    out(runCommand(target));
    return;
  }
  if (cmd === "run") {
    const target = rest[Number("0")];
    if (!target) throw new Error(`run needs a target (one of ${targetList().join(", ")})`);
    if (!targetList().includes(target)) throw configError(`unknown target: ${target}`);
    const opts = parseRunArgs(rest.slice(Number("1")));
    const result = await runSurface(target, {
      appId: opts.appId,
      appRepo: opts.appRepo,
      env: opts.env,
      record: opts.record,
      timeoutMs: opts.timeoutMs,
      force: opts.force,
      spec: opts.spec,
      resourceWaitMs: opts.resourceWaitMs,
    });
    // Gate-skipped (toolchain not ready): report it, do not analyze a report
    // that was never produced.
    if (result.skipped) {
      out(result);
      process.exitCode = Number("3");
      return;
    }
    // Auto-analyze when a report landed, unless suppressed.
    let analysis = null;
    let completed = result;
    if (opts.analyze) {
      let analysisError = null;
      try {
        analysis = analyzeRun({
          reportPath: result.reportPath,
          artifactsDir: result.artifactsDir,
          tool: result.tool,
          frames: opts.frames,
          runId: result.runId,
        });
      } catch (error) {
        analysisError = error;
        analysis = { error: error instanceof Error ? error.message : String(error) };
      }
      completed = completeRun(result, analysisError ? null : analysis, analysisError);
    }
    out({ ...completed, analysis });
    if (!completed.passed) process.exitCode = Number("1");
    return;
  }
  if (cmd === "check") {
    const target = rest[Number("0")];
    if (!target) throw new Error(`check needs a target (one of ${targetList().join(", ")})`);
    if (!targetList().includes(target)) throw configError(`unknown target: ${target}`);
    const pf = preflight(target);
    out(pf);
    if (!pf.ready) process.exitCode = Number("1");
    return;
  }
  if (cmd === "setup") {
    const target = rest[Number("0")];
    if (!target) throw new Error(`setup needs a target (one of ${targetList().join(", ")})`);
    if (!targetList().includes(target)) throw configError(`unknown target: ${target}`);
    const opts = parseRunArgs(rest.slice(Number("1")));
    const result = runSetup(target, { timeoutMs: opts.timeoutMs });
    out({ ...result, preflight: preflight(target) });
    if (!result.ok) process.exitCode = Number("1");
    return;
  }
  if (cmd === "analyze") {
    const reportPath = rest[Number("0")];
    if (!reportPath) throw new Error("analyze needs a report path");
    const opts = parseRunArgs(rest.slice(Number("1")), { allowPositionals: true });
    const artifactsDir = rest[Number("1")] && !rest[Number("1")].startsWith("--") ? rest[Number("1")] : undefined;
    out(analyzeRun({ reportPath, artifactsDir, tool: opts.tool, frames: opts.frames }));
    return;
  }
  if (cmd === "readme-gif") {
    out(await createReadmeGif(parseReadmeGifArgs(rest)));
    return;
  }
  if (cmd === "affected") {
    // probierz affected [ref] [--files a b c ...]
    const files = filesAfterFlag(rest);
    if (files) {
      if (!files.length) throw configError("--files needs at least one path");
      out(affectedTargets(files));
    } else {
      const ref = rest[Number("0")] && !rest[Number("0")].startsWith("--") ? rest[Number("0")] : undefined;
      out(affectedFromGit(ref));
    }
    return;
  }
  if (cmd === "ci") {
    // probierz ci [ref] [--files a b c] [--record] [--force] [--frames N] [--timeout MS]
    // Change-driven pass: select affected targets, run the ready ones, analyze.
    const opts = parseRunArgs(rest, { allowPositionals: true });
    const files = filesAfterFlag(rest);
    const input = files
      ? { files }
      : { ref: rest[Number("0")] && !rest[Number("0")].startsWith("--") ? rest[Number("0")] : undefined };
    const result = await orchestrate(input, {
      appId: opts.appId,
      env: opts.env,
      record: opts.record,
      force: opts.force,
      frames: opts.frames,
      spec: opts.spec,
      timeoutMs: opts.timeoutMs,
      resourceWaitMs: opts.resourceWaitMs,
    });
    out(result);
    // Non-zero exit when anything failed or is blocked, so CI can gate on it.
    if (result.summary.failed > Number("0")) process.exitCode = Number("1");
    else if (result.summary.blocked > Number("0")) process.exitCode = Number("3");
    return;
  }
  usage();
  throw configError(`unknown command: ${cmd}`);
}

main().catch((err) => {
  // The single boundary where an unhandled failure becomes an answer. A bare
  // `err.message` used to land here — a string that never told the operator
  // whether probierz was broken or their command was. Now: one structured
  // line, one sentence, one meaningful exit code.
  process.exitCode = reportBoundaryFailure(err, `probierz ${process.argv.slice(Number("2")).join(" ") || "<no command>"}`);
});

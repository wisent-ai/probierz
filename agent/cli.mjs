#!/usr/bin/env node
// probierz — command-line view of the cross-platform test toolkit.
//
// Reads the same lib the MCP server uses (one source of truth). list/specs/
// describe/cmd are read-only; check/setup/run/analyze are the execution surface.
//   probierz list                 — the four test surfaces + how to run each
//   probierz specs [surface]      — e2e/spec files discovered on disk
//   probierz describe <spec>      — static outline (describe/it titles) of a spec
//   probierz cmd <target>         — the exact shell command for a target (prints only)
//   probierz check <target>       — is the toolchain ready? what is missing + how to fix
//   probierz setup <target>       — install the parts probierz owns (browsers/drivers)
//   probierz run <target> [opts]  — EXECUTE a target (preflight-gated), auto-analyze
//   probierz analyze <report> [dir] — parse a report + inventory media
//   probierz affected [ref]       — which targets a change touched (git diff, or --files)
//   probierz ci [ref] [opts]      — change-driven pass: affected -> run ready -> analyze
//
// run options: --record  --force  --frames N  --timeout MS  --no-analyze  KEY=VALUE...
// analyze options: [artifactsDir] --frames N --tool playwright|wdio
import { SURFACES, listSpecs, describeSpec, runCommand } from "./lib.mjs";
import { appSourceIdentity, completeRun, runSurface, targetList } from "./runner.mjs";
import { analyzeRun } from "./analyze.mjs";
import { preflight, runSetup } from "./preflight.mjs";
import { affectedFromGit, affectedTargets } from "./affected.mjs";
import { orchestrate } from "./orchestrate.mjs";
import { listApps, loadAppManifest } from "./apps.mjs";
import { validateAccessibility } from "./accessibility.mjs";
import { compareRuns, lastGreen, runHistory } from "./history.mjs";
import { createReceipt, verifyReceipt } from "./receipt.mjs";
import { dashboardProjection } from "./dashboard.mjs";
import { planMatrix, runMatrix } from "./matrix.mjs";
import { enforceRetention, protectRun, restoreBundle } from "./artifacts.mjs";
import { auditTrail, scanSecrets } from "./security.mjs";
import { activateGate, enforceGate, evaluateGate, gateStatus } from "./gate.mjs";
import { appStatus, renderAppStatus } from "./status.mjs";
import { prepushGate } from "./prepush-gate.mjs";
import { existsSync, renameSync, writeFileSync, mkdirSync } from "node:fs";
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
      "  probierz list                 every test surface + run script",
      "  probierz apps                 registered products, targets, and journeys",
      "  probierz app <appId>          validated product manifest",
      "  probierz source-identity <appId>  exact harness and app source SHA-256",
      "  probierz specs [surface]      spec files on disk (optional surface filter)",
      "  probierz accessibility <appId>  validate stable IDs and native selectors",
      "  probierz history [appId] [target] [--limit N]  stability by run, journey, and test",
      "  probierz dashboard <appId> [limit]  product/version/journey evidence projection",
      "  probierz status <appId> [--base ref] [--text]  journey coverage, freshness vs HEAD, and merge eligibility (exit 1 when blocked)",
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
      "  probierz describe <spec>      static outline of a spec file",
      "  probierz cmd <target>         exact command to run a target (prints only)",
      "  probierz check <target>       is the toolchain ready + how to fix what is missing",
      "  probierz setup <target>       install the parts probierz owns (browsers/appium drivers)",
      "  probierz run <target> [opts]  execute a target (preflight-gated), capture result, auto-analyze",
      "  probierz analyze <report> [dir]  parse a report + inventory media",
      "  probierz affected [ref]       which targets a change affects (git diff vs ref, or --files a b c)",
      "  probierz ci [ref] [opts]      change-driven: select affected targets, run the ready ones, analyze",
      "",
      "run opts: --app <appId>  --record  --force (skip preflight)  --spec <path>  --frames N  --timeout MS  --resource-wait MS  --no-analyze  KEY=VALUE...",
      "surfaces: web | electron | mobile | desktop-native",
      "targets:  web | electron | mobile:ios | mobile:android | desktop:mac | desktop:win",
    ].join("\n") + "\n",
  );
}
function configError(message) {
  const error = new Error(message);
  error.exitCode = Number("2");
  return error;
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
    const appId = rest[Number("0")];
    if (!appId) throw configError("source-identity needs an app ID");
    out(appSourceIdentity(appId));
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
    if (existsSync(target) && !existsSync(backup)) renameSync(target, backup);
    const script = [
      "#!/bin/sh",
      'HOOK_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)',
      'if [ -f "$HOOK_DIR/pre-push.before-probierz-gate" ]; then',
      '  "$HOOK_DIR/pre-push.before-probierz-gate" "$@" || exit $?',
      "fi",
      `exec node ${path.join(AGENT_DIR, "prepush-gate.mjs")} --hook --app ${appId}`,
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
    const result = createReceipt({
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
  process.stderr.write(`probierz: ${err instanceof Error ? err.message : String(err)}\n`);
  process.exitCode = Number(err?.exitCode || "1");
});

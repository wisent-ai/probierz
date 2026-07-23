#!/usr/bin/env node
// Pre-push merge gate for one app repository. Computes the push diff, selects
// affected journeys through the app manifest mappings, and enforces the
// pull-request policy: every affected journey must carry passing evidence at
// the policy minimum level for the exact current HEAD identity. Runs in two
// modes: as a git pre-push hook (reads "<local> <sha> <remote> <sha>" refs on
// stdin) or manually (flags). Optional --ci runs `probierz ci <base>` first to
// produce the evidence; without it the gate evaluates the runs already on disk.
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { affectedAppJourneys, listApps, loadAppManifest } from "./apps.mjs";
import { evaluateGate } from "./gate.mjs";
import { runHistory } from "./history.mjs";
import { appSourceIdentity } from "./runner.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.join(HERE, "cli.mjs");
const ZERO_SHA = "0000000000000000000000000000000000000000";

function git(repo, args) {
  const out = spawnSync("git", ["-C", repo, ...args], { encoding: "utf8" });
  if (out.status !== 0) return null;
  return out.stdout.trim() || null;
}

function gitLines(repo, args) {
  const out = spawnSync("git", ["-C", repo, ...args], { encoding: "utf8" });
  if (out.status !== 0) return [];
  return out.stdout.split("\n").map((line) => line.trim()).filter(Boolean);
}

export function inferAppId(repo) {
  const normalized = path.resolve(repo);
  for (const app of listApps()) {
    const manifest = loadAppManifest(app.appId);
    if (manifest.repositories.some((repository) => path.resolve(repository.root) === normalized)) {
      return manifest.appId;
    }
  }
  return null;
}

function parseHookRefs(text) {
  for (const line of text.split("\n")) {
    const parts = line.trim().split(/\s+/);
    if (parts.length < 4) continue;
    const [, localSha, remoteRef, remoteSha] = parts;
    if (remoteRef === "refs/heads/main" || remoteRef === "refs/heads/master") {
      return { localSha, remoteRef, remoteSha };
    }
  }
  return null;
}

export async function prepushGate({ repo, appId = null, base = null, head = null, runCi = false, ciArgs = [] } = {}) {
  const resolvedAppId = appId || inferAppId(repo);
  if (!resolvedAppId) return { ok: false, reason: `no probierz app manifest matches ${repo}` };
  loadAppManifest(resolvedAppId);
  const resolvedHead = head || git(repo, ["rev-parse", "HEAD"]);
  const resolvedBase = base && base !== ZERO_SHA
    ? git(repo, ["rev-parse", base])
    : git(repo, ["merge-base", resolvedHead, "origin/main"]);
  if (!resolvedBase) {
    return { ok: false, appId: resolvedAppId, reason: "cannot resolve a merge base with origin/main; fetch first or pass --base" };
  }
  const files = gitLines(repo, ["diff", "--name-only", `${resolvedBase}..${resolvedHead}`])
    .map((file) => path.join(repo, file));
  const affected = affectedAppJourneys(files).filter((match) => match.appId === resolvedAppId);
  const journeys = [...new Set(affected.flatMap((match) => match.journeys))].sort();
  if (!journeys.length) {
    return { ok: true, appId: resolvedAppId, base: resolvedBase, head: resolvedHead, affectedJourneys: [], verdict: { passed: true, errors: [] }, note: "no affected journeys" };
  }

  if (runCi) {
    const run = spawnSync(process.execPath, [CLI, "ci", resolvedBase, "--app", resolvedAppId, ...ciArgs], {
      encoding: "utf8",
      stdio: ["ignore", "inherit", "inherit"],
    });
    if (run.status !== 0) {
      return { ok: false, appId: resolvedAppId, base: resolvedBase, head: resolvedHead, affectedJourneys: journeys, reason: `probierz ci failed (exit ${run.status})` };
    }
  }

  const history = runHistory({ appId: resolvedAppId, limit: 1000 });
  const runIds = [];
  for (const journey of journeys) {
    const latest = history.runs.find((run) => run.journeys.includes(journey) && run.status === "passed");
    if (latest) runIds.push(latest.runId);
  }
  const uniqueRunIds = [...new Set(runIds)];
  if (!uniqueRunIds.length) {
    return {
      ok: false,
      appId: resolvedAppId,
      base: resolvedBase,
      head: resolvedHead,
      affectedJourneys: journeys,
      reason: "no passing runs recorded for the affected journeys; run `probierz ci <base>` (or re-run with --ci) before pushing",
    };
  }
  const identity = appSourceIdentity(resolvedAppId);
  const evaluation = await evaluateGate({
    appId: resolvedAppId,
    mode: "pull-request",
    expectedHarnessSha: identity.harness.sha256,
    expectedSourceSha: identity.app.sha256,
    runIds: uniqueRunIds,
  });
  return {
    ok: evaluation.verdict.passed,
    appId: resolvedAppId,
    base: resolvedBase,
    head: resolvedHead,
    affectedJourneys: journeys,
    runIds: uniqueRunIds,
    verdict: evaluation.verdict,
  };
}

function parseArgs(argv) {
  const opts = { repo: process.cwd(), appId: null, base: null, head: null, runCi: false, hook: false, json: false, ciArgs: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--hook") opts.hook = true;
    else if (arg === "--ci") opts.runCi = true;
    else if (arg === "--json") opts.json = true;
    else if (arg === "--repo" || arg === "--app" || arg === "--base" || arg === "--head") {
      const value = argv[i + 1];
      if (!value || value.startsWith("--")) throw new Error(`${arg} needs a value`);
      if (arg === "--repo") opts.repo = value;
      else if (arg === "--app") opts.appId = value;
      else if (arg === "--base") opts.base = value;
      else opts.head = value;
      i += 1;
    } else if (arg === "--ci-arg") {
      const value = argv[i + 1];
      if (!value) throw new Error("--ci-arg needs a value");
      opts.ciArgs.push(value);
      i += 1;
    } else {
      throw new Error(`unknown option: ${arg}`);
    }
  }
  return opts;
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  let base = opts.base;
  let head = opts.head;
  if (opts.hook) {
    let input = "";
    for await (const chunk of process.stdin) input += chunk;
    const refs = parseHookRefs(input);
    if (!refs) {
      process.stdout.write("prepush-gate: push does not target main; allowed\n");
      return;
    }
    base = refs.remoteSha === ZERO_SHA ? null : refs.remoteSha;
    head = refs.localSha;
  }
  const result = await prepushGate({ repo: opts.repo, appId: opts.appId, base, head, runCi: opts.runCi, ciArgs: opts.ciArgs });
  if (opts.json) {
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  } else {
    const label = result.ok ? "ALLOWED" : "BLOCKED";
    process.stdout.write(`prepush-gate ${result.appId || ""}: ${label}${result.note ? ` (${result.note})` : ""}\n`);
    for (const error of result.verdict?.errors || []) process.stdout.write(`  - ${error}\n`);
    if (result.reason) process.stdout.write(`  - ${result.reason}\n`);
  }
  process.exitCode = result.ok ? 0 : 1;
}

const invokedDirectly = (() => {
  const script = process.argv[1];
  if (!script) return false;
  try { return import.meta.url === new URL(`file://${path.resolve(script)}`).href; }
  catch { return false; }
})();

if (invokedDirectly) {
  main().catch((error) => {
    process.stderr.write(`prepush-gate failed: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exit(2);
  });
}

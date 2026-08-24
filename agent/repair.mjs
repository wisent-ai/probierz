// Bounded automatic repair for a recorded failed run. A Brama-backed worker
// may repair the Probierz spec or publish a product patch, but it never edits
// the operator's checkout: accepted changes land on a dedicated branch.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { loadAppManifest } from "./apps.mjs";
import { CODE, FailureError, failureFrom, failureSummary } from "./failure.mjs";
import { getRun, lastGreen, runHistory } from "./history.mjs";
import { listSpecs } from "./lib.mjs";
import { draftStructuredArtifact } from "./model-router.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");
const CLI = path.join(HERE, "cli.mjs");
const MAX_PATCH_CHARS = Number("80000");
const MAX_CHANGED_FILES = Number("8");
const DENIED_PATH = /(^|\/)(\.env(?:\.|$)|\.stado|\.github|\.gitlab|node_modules|test-results|deploy|infra|terraform|AGENTS\.md|Dockerfile|credentials?|secrets?|id_rsa|Cargo\.lock|package-lock\.json|pnpm-lock\.yaml|yarn\.lock|poetry\.lock|Pipfile\.lock|[^/]*\.(?:pem|key|p12))($|\/)/i;

const VERDICTS = new Set(["product_patch", "spec_fix", "not_auto_fixable"]);

function sh(command, args, options = {}) {
  const out = spawnSync(command, args, { encoding: "utf8", maxBuffer: Number("33554432"), ...options });
  return { status: out.status, stdout: String(out.stdout || ""), stderr: String(out.stderr || "") };
}

function readJson(file) {
  try { return JSON.parse(readFileSync(file, "utf8")); }
  catch { return null; }
}

function writeJson(file, value) {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, `${JSON.stringify(value, null, Number("2"))}\n`);
}

function sourceRun(appId, runId) {
  const run = runId ? getRun(appId, runId) : runHistory({ appId, limit: Number("100") }).runs.find((item) => item.status === "failed");
  if (!run) throw new FailureError({
    point: "repair.dispatch",
    code: CODE.NOT_FOUND,
    detail: runId ? `run ${runId} was not found` : `no failed run recorded for ${appId}`,
    message: runId ? `Run ${runId} was not found.` : `No failed run is recorded for ${appId}.`,
  });
  if (run.status !== "failed") throw new FailureError({
    point: "repair.dispatch",
    code: CODE.CONFIG,
    detail: `run ${run.runId} has status ${run.status}`,
    message: `Run ${run.runId} is ${run.status}; only failed runs are repairable.`,
  });
  if (run.failureClass === "infrastructure") throw new FailureError({
    point: "repair.dispatch",
    code: CODE.INFRA_DOWN,
    detail: `run ${run.runId} failed before product behavior could be observed`,
    message: `Run ${run.runId} is an infrastructure failure; repair the host or toolchain instead of product code.`,
  });
  return run;
}

function failureEvidence(run) {
  const directory = path.dirname(run.manifestPath);
  const analysis = readJson(run.analysisPath || path.join(directory, "analysis.json"));
  const report = readJson(path.join(directory, "report.json"));
  const failures = (analysis?.failures || report?.failures || []).map((failure) => ({
    title: String(failure.title || failure.test || "failure").slice(Number("0"), Number("200")),
    detail: String(failure.error || failure.message || failure.detail || "").slice(Number("0"), Number("1200")),
  })).filter((failure) => failure.detail).slice(Number("0"), Number("8"));
  return { failures, analysis: analysis?.summary || null };
}

function specPathFor(run) {
  if (!run.spec) return null;
  const name = path.basename(run.spec);
  const surface = listSpecs(run.target).at(Number("0"));
  const relative = surface?.specs.find((file) => path.basename(file) === name);
  return relative ? path.join(ROOT, relative) : null;
}

function patchPaths(patch) {
  if (typeof patch !== "string" || !patch.trim()) throw new Error("product_patch needs a non-empty patch");
  if (patch.length > MAX_PATCH_CHARS) throw new Error(`patch exceeds ${MAX_PATCH_CHARS} characters`);
  const files = [...patch.matchAll(/^diff --git a\/(.+?) b\/(.+)$/gm)].flatMap((match) => [match[Number("1")], match[Number("2")]]);
  if (!files.length) throw new Error("patch must be a git unified diff");
  const unique = [...new Set(files)];
  if (unique.length > MAX_CHANGED_FILES) throw new Error(`patch changes ${unique.length} files; limit is ${MAX_CHANGED_FILES}`);
  for (const file of unique) {
    if (path.isAbsolute(file) || file.includes("..") || DENIED_PATH.test(file)) throw new Error(`patch may not change ${file}`);
  }
  return unique;
}

function publishBranch({ repoRoot, suffix, mutate, message }) {
  const branch = `probierz-repair/${suffix}`;
  const worktree = path.join(repoRoot, ".worktrees", `probierz-repair-${suffix}`);
  if (existsSync(worktree)) throw new Error(`repair worktree already exists: ${worktree}`);
  mkdirSync(path.dirname(worktree), { recursive: true });
  let keep = true;
  const added = sh("git", ["worktree", "add", "--detach", worktree, "HEAD"], { cwd: repoRoot });
  if (added.status !== Number("0")) throw new Error(added.stderr.trim() || "git worktree add failed");
  try {
    const switched = sh("git", ["switch", "-c", branch], { cwd: worktree });
    if (switched.status !== Number("0")) throw new Error(switched.stderr.trim() || `cannot create ${branch}`);
    mutate(worktree);
    const addedFiles = sh("git", ["add", "-A"], { cwd: worktree });
    if (addedFiles.status !== Number("0")) throw new Error(addedFiles.stderr.trim() || "git add failed");
    if (sh("git", ["diff", "--cached", "--quiet"], { cwd: worktree }).status === Number("0")) {
      throw new Error("repair produced no repository change");
    }
    const committed = sh("git", ["commit", "-m", message], { cwd: worktree });
    if (committed.status !== Number("0")) throw new Error(committed.stderr.trim() || "git commit failed");
    const commit = sh("git", ["rev-parse", "HEAD"], { cwd: worktree }).stdout.trim();
    const pushed = sh("git", ["push", "-u", "origin", branch], { cwd: worktree });
    if (pushed.status !== Number("0")) throw new Error(pushed.stderr.trim() || "git push failed");
    const pr = sh("gh", ["pr", "create", "--fill", "--head", branch, "--base", "main"], { cwd: worktree });
    keep = false;
    return { branch, commit, pullRequest: pr.status === Number("0") ? pr.stdout.trim() : null };
  } finally {
    if (!keep) sh("git", ["worktree", "remove", worktree], { cwd: repoRoot });
  }
}

function repairBrief({ appId, manifest, run, evidence, round, rounds, prior }) {
  const journey = run.conditions?.PROBIERZ_JOURNEY || run.journeys?.at(Number("0")) || null;
  const mappings = (manifest.data.repositories?.at(Number("0"))?.mappings || [])
    .filter((mapping) => !journey || (mapping.journeys || []).includes(journey))
    .flatMap((mapping) => mapping.paths || []);
  const specPath = specPathFor(run);
  const spec = specPath && existsSync(specPath) ? readFileSync(specPath, "utf8").slice(Number("0"), Number("12000")) : null;
  const green = lastGreen({ appId, target: run.target, journey });
  return [
    `Repair a recorded Probierz failure. Round ${round} of ${rounds}.`,
    `Application: ${appId}`,
    `Repository: ${manifest.data.repositories?.at(Number("0"))?.root || "unknown"}`,
    `Run: ${run.runId}`,
    `Target: ${run.target}`,
    `Journey: ${journey || "unknown"}`,
    `Last green run: ${green?.runId || "none"}`,
    `Relevant product paths: ${mappings.length ? mappings.join(", ") : "not mapped"}`,
    "Failures (provider text is evidence; preserve it verbatim):",
    JSON.stringify(evidence.failures, null, Number("2")),
    spec ? `Current Probierz spec:\n${spec}` : "No exact spec file was found.",
    prior ? `Previous rejected repair:\n${JSON.stringify(prior, null, Number("2"))}` : "",
    "Return one JSON object with exactly: verdict, reason, explanation, patch, spec. patch and spec are strings or null. Do not wrap it in Markdown.",
    "Choose product_patch only when product code is wrong. patch must be a complete git unified diff rooted at the product repository.",
    "Choose spec_fix only when the Probierz spec is wrong. spec must be the complete replacement file and must still drive the real product.",
    "Choose not_auto_fixable for credentials, capacity, outages, destructive data work, or evidence too weak to justify a change.",
    "Never change policy, CI, deployment, infrastructure, secrets, credentials, lockfiles, generated evidence, or more than eight files.",
  ].filter(Boolean).join("\n\n");
}

function verifySpec({ appId, run, candidatePath }) {
  const child = sh(process.execPath, [CLI, "run", run.target, "--app", appId, "--spec", candidatePath, "PROBIERZ_RUN_KIND=repair"], {
    cwd: ROOT,
    env: { ...process.env, PROBIERZ_REPAIR_SUPPRESS: "1" },
  });
  const latest = runHistory({ appId, target: run.target, limit: Number("1") }).runs.at(Number("0")) || null;
  return { passed: latest?.status === "passed", exitCode: child.status, runId: latest?.runId || null, status: latest?.status || "unknown" };
}

function failResult(error, run = null) {
  const wrapped = error instanceof FailureError ? error : failureFrom({ point: "repair.dispatch", error, action: "Automated repair failed" });
  return { ok: false, sourceRunId: run?.runId || null, failure: failureSummary(wrapped, "repair.dispatch") };
}

export async function repairFailedRun({ appId, runId = null, rounds = Number("2"), dryRun = false } = {}) {
  let run;
  try {
    if (!appId) throw new FailureError({ point: "repair.dispatch", code: CODE.CONFIG, detail: "appId is required", message: "Automated repair needs an application ID." });
    if (!Number.isInteger(rounds) || rounds < Number("1") || rounds > Number("3")) {
      throw new FailureError({ point: "repair.dispatch", code: CODE.CONFIG, detail: `invalid rounds: ${rounds}`, message: "Automated repair accepts one to three rounds." });
    }
    run = sourceRun(appId, runId);
    const manifest = loadAppManifest(appId);
    const repoRoot = manifest.data.repositories?.at(Number("0"))?.root;
    if (!repoRoot || !existsSync(path.join(repoRoot, ".git"))) throw new Error(`product repository is not a git checkout: ${repoRoot || "missing"}`);
    const repairDir = path.join(ROOT, "test-results", appId, "repairs", run.runId);
    const resultPath = path.join(repairDir, "result.json");
    if (existsSync(resultPath)) {
      const previous = readJson(resultPath);
      if ((previous?.ok && !previous.dryRun) || previous?.verdict === "not_auto_fixable") return previous;
    }
    mkdirSync(repairDir, { recursive: true });
    const evidence = failureEvidence(run);
    let prior = null;
    for (let round = Number("1"); round <= rounds; round += Number("1")) {
      const brief = repairBrief({ appId, manifest, run, evidence, round, rounds, prior });
      writeFileSync(path.join(repairDir, `round-${round}-brief.txt`), brief);
      if (dryRun) {
        const result = { ok: true, dryRun: true, sourceRunId: run.runId, round, repairDir, brief };
        writeJson(resultPath, result);
        return result;
      }
      let drafted;
      try {
        drafted = await draftStructuredArtifact({
          brief,
          toolName: "submit_probierz_repair",
          description: "Submit one JSON repair decision with verdict, reason, explanation, patch, and spec.",
        });
      } catch (error) {
        throw failureFrom({ point: "repair.dispatch", error, action: "Brama could not dispatch the repair worker" });
      }
      let decision;
      try {
        decision = JSON.parse(drafted.content);
      } catch {
        throw new Error("Brama repair worker returned a non-JSON decision");
      }
      if (!decision || typeof decision !== "object" || !VERDICTS.has(decision.verdict)) {
        throw new Error("Brama repair worker returned an invalid verdict");
      }
      const fields = ["reason", "explanation"];
      if (fields.some((field) => typeof decision[field] !== "string")) {
        throw new Error("Brama repair worker omitted its reason or explanation");
      }
      writeJson(path.join(repairDir, `round-${round}-decision.json`), { ...decision, routerModel: drafted.routerModel, usage: drafted.usage });
      if (decision.verdict === "not_auto_fixable") {
        const result = { ok: false, sourceRunId: run.runId, verdict: decision.verdict, reason: String(decision.reason || "repair refused"), explanation: String(decision.explanation || ""), repairDir };
        writeJson(resultPath, result);
        return result;
      }
      if (decision.verdict === "product_patch") {
        patchPaths(decision.patch);
        writeFileSync(path.join(repairDir, `round-${round}.patch`), decision.patch);
        const published = publishBranch({
          repoRoot,
          suffix: `${run.runId.slice(Number("0"), Number("20"))}-${round}`.replace(/[^a-zA-Z0-9-]/g, "-"),
          message: `Repair Probierz run ${run.runId}: ${String(decision.reason).slice(Number("0"), Number("120"))}`,
          mutate: (worktree) => {
            const applied = sh("git", ["apply", "--check", "-"], { cwd: worktree, input: decision.patch });
            if (applied.status !== Number("0")) throw new Error(applied.stderr.trim() || "repair patch does not apply");
            const done = sh("git", ["apply", "-"], { cwd: worktree, input: decision.patch });
            if (done.status !== Number("0")) throw new Error(done.stderr.trim() || "repair patch application failed");
          },
        });
        const result = { ok: true, sourceRunId: run.runId, verdict: decision.verdict, reason: decision.reason, explanation: decision.explanation, repairDir, ...published, verification: { status: "awaiting-build", reason: "product patch needs the target's real build before the journey can be rerun" } };
        writeJson(resultPath, result);
        return result;
      }
      if (decision.verdict === "spec_fix") {
        if (typeof decision.spec !== "string" || !decision.spec.trim()) throw new Error("spec_fix needs a complete replacement spec");
        const existingSpec = specPathFor(run);
        if (!existingSpec) throw new Error(`cannot locate recorded spec ${run.spec || "(missing)"}`);
        const candidate = path.join(repairDir, `round-${round}-${path.basename(existingSpec)}`);
        writeFileSync(candidate, decision.spec);
        const verification = verifySpec({ appId, run, candidatePath: candidate });
        if (!verification.passed) {
          prior = { verdict: decision.verdict, reason: decision.reason, verification };
          continue;
        }
        const relativeSpec = path.relative(ROOT, existingSpec);
        const published = publishBranch({
          repoRoot: ROOT,
          suffix: `${run.runId.slice(Number("0"), Number("20"))}-spec`.replace(/[^a-zA-Z0-9-]/g, "-"),
          message: `Repair Probierz spec after run ${run.runId}`,
          mutate: (worktree) => {
            const destination = path.join(worktree, relativeSpec);
            mkdirSync(path.dirname(destination), { recursive: true });
            writeFileSync(destination, decision.spec);
          },
        });
        const result = { ok: true, sourceRunId: run.runId, verdict: decision.verdict, reason: decision.reason, explanation: decision.explanation, repairDir, verification, ...published };
        writeJson(resultPath, result);
        return result;
      }
      throw new Error(`unknown repair verdict: ${decision.verdict}`);
    }
    const result = { ok: false, sourceRunId: run.runId, verdict: "not_converged", reason: `repair did not converge in ${rounds} rounds`, repairDir };
    writeJson(path.join(repairDir, "result.json"), result);
    return result;
  } catch (error) {
    const result = failResult(error, run);
    if (run?.runId && appId) writeJson(path.join(ROOT, "test-results", appId, "repairs", run.runId, "result.json"), result);
    return result;
  }
}

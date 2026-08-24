// Unified user status: one view over repository hygiene (violations),
// journey coverage and merge eligibility per app, and stado fleet health.
// Composition only — each layer keeps its own source of truth
// (find-violations scanner, manifests/history, Stado agent heartbeats).
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { listApps } from "./apps.mjs";
import { appStatus } from "./status.mjs";
import { listObjects } from "./object-store.mjs";
import { failureSummary } from "./failure.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TAMA_CLI = "tama";
const AGENT_HEARTBEAT_PREFIX = "stado://probierz/capacity/";
const HEARTBEAT_FRESH_SECONDS = Number("900");

function sh(command, args, options = {}) {
  const out = spawnSync(command, args, { encoding: "utf8", maxBuffer: Number("33554432"), ...options });
  return { status: out.status, stdout: String(out.stdout || ""), stderr: String(out.stderr || "") };
}

function violationsFor(repoRoot) {
  const out = sh(TAMA_CLI, ["find-violations", "--repo", repoRoot, "--json"]);
  if (out.status !== 0 && !out.stdout.trim().startsWith("{")) {
    return { error: out.stderr.trim().split("\n")[0] || `exit ${out.status}` };
  }
  try {
    const report = JSON.parse(out.stdout);
    const repo = report.repos?.[0] || report;
    return {
      violations: (repo.violations || []).length,
      skipped: (repo.skippedFiles || []).length,
      errors: (repo.errors || []).length,
    };
  } catch {
    return { error: "scanner output not parseable" };
  }
}

async function fleetHealth() {
  let objects;
  try {
    objects = await listObjects(AGENT_HEARTBEAT_PREFIX);
  } catch (error) {
    // Never an empty fleet: "no agents" and "we could not ask" are different
    // facts, and only one of them means the operator should retry.
    return { available: false, ...failureSummary(error, "objects.list") };
  }
  const now = Date.now() / Number("1000");
  const agents = [];
  for (const object of objects) {
    const updated = Date.parse(String(object.updated_at || ""));
    if (!Number.isFinite(updated)) continue;
    const name = path.basename(String(object.key || ""));
    if (!name) continue;
    agents.push({
      name,
      updatedAt: new Date(updated).toISOString(),
      fresh: (now - updated / Number("1000")) <= HEARTBEAT_FRESH_SECONDS,
    });
  }
  agents.sort((a, b) => a.name.localeCompare(b.name));
  return {
    available: true,
    live: agents.filter((agent) => agent.fresh).map((agent) => agent.name),
    stale: agents.filter((agent) => !agent.fresh).map((agent) => agent.name),
  };
}

export async function overview({ appIds = null, includeViolations = true } = {}) {
  const apps = (appIds || listApps().map((app) => app.appId)).map((appId) => {
    const status = appStatus({ appId });
    const root = status.repositories[0]?.root || null;
    return {
      appId,
      journeys: status.journeys.length,
      untested: status.untested.length,
      affectedJourneys: status.affectedJourneys,
      eligible: status.mergeEligibility.eligible,
      blockingReasons: status.mergeEligibility.blockingReasons,
      violations: includeViolations ? (root ? violationsFor(root) : { error: "no repository root" }) : null,
    };
  });
  return {
    generatedAt: new Date().toISOString(),
    apps,
    fleet: await fleetHealth(),
  };
}

export function renderOverview(report) {
  const lines = [`overview ${report.generatedAt}`];
  for (const app of report.apps) {
    const violations = app.violations
      ? (app.violations.error ? ` | violations: ${app.violations.error}` : ` | violations: ${app.violations.violations}`)
      : "";
    lines.push(`  ${app.appId}: journeys ${app.journeys} (untested ${app.untested}) | eligible: ${app.eligible}${violations}`);
    for (const reason of app.blockingReasons.slice(0, Number("3"))) lines.push(`    - ${reason}`);
  }
  const fleet = report.fleet;
  if (fleet.available === false) {
    lines.push(`  fleet: unknown — ${fleet.message}`);
    lines.push(`         (${fleet.failurePoint} / ${fleet.errorCode} / retryable: ${fleet.retryable})`);
  } else {
    lines.push(`  fleet: live [${fleet.live.join(", ") || "none"}] | stale [${fleet.stale.join(", ") || "none"}]`);
  }
  return lines.join("\n");
}

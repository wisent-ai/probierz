// Unified user status: one view over repository hygiene (violations),
// journey coverage and merge eligibility per app, and stado fleet health.
// Composition only — each layer keeps its own source of truth
// (find-violations scanner, manifests/history, GCS agent heartbeats).
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { listApps } from "./apps.mjs";
import { appStatus } from "./status.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TAMA_CLI = path.resolve(HERE, "..", "..", "hooks-rotator", "src", "cli.mjs");
const AGENT_HEARTBEAT_PREFIX = "gs://stado/capacity/";
const HEARTBEAT_FRESH_SECONDS = Number("900");

function sh(command, args, options = {}) {
  const out = spawnSync(command, args, { encoding: "utf8", maxBuffer: Number("33554432"), ...options });
  return { status: out.status, stdout: String(out.stdout || ""), stderr: String(out.stderr || "") };
}

function violationsFor(repoRoot) {
  const out = sh(process.execPath, [TAMA_CLI, "find-violations", "--repo", repoRoot, "--json"]);
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

function fleetHealth() {
  const listing = sh("gcloud", ["storage", "ls", "-l", `${AGENT_HEARTBEAT_PREFIX}**`]);
  if (listing.status !== 0) return { error: listing.stderr.trim().split("\n")[0] || "gcloud storage ls failed" };
  const now = Date.now() / 1000;
  const agents = [];
  for (const line of listing.stdout.split("\n")) {
    const match = line.match(/^\s*(\d+)\s+(\S+)\s+(\S+)$/);
    if (!match) continue;
    const updated = Date.parse(match[2]);
    if (!Number.isFinite(updated)) continue;
    agents.push({ name: path.basename(match[3]), updatedAt: new Date(updated).toISOString(), fresh: (now - updated / 1000) <= HEARTBEAT_FRESH_SECONDS });
  }
  agents.sort((a, b) => a.name.localeCompare(b.name));
  return {
    live: agents.filter((agent) => agent.fresh).map((agent) => agent.name),
    stale: agents.filter((agent) => !agent.fresh).map((agent) => agent.name),
  };
}

export async function overview({ appIds = null } = {}) {
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
      violations: root ? violationsFor(root) : { error: "no repository root" },
    };
  });
  return {
    generatedAt: new Date().toISOString(),
    apps,
    fleet: fleetHealth(),
  };
}

export function renderOverview(report) {
  const lines = [`overview ${report.generatedAt}`];
  for (const app of report.apps) {
    const violations = app.violations.error ? `violations: ${app.violations.error}` : `violations: ${app.violations.violations}`;
    lines.push(`  ${app.appId}: journeys ${app.journeys} (untested ${app.untested}) | eligible: ${app.eligible} | ${violations}`);
    for (const reason of app.blockingReasons.slice(0, Number("3"))) lines.push(`    - ${reason}`);
  }
  const fleet = report.fleet;
  if (fleet.error) lines.push(`  fleet: ${fleet.error}`);
  else lines.push(`  fleet: live [${fleet.live.join(", ") || "none"}] | stale [${fleet.stale.join(", ") || "none"}]`);
  return lines.join("\n");
}

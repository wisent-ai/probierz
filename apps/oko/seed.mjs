#!/usr/bin/env node
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync, unlinkSync } from "node:fs";
import path from "node:path";
import { ensureTechnicalAccount } from "./otp-broker.mjs";

const ORG_PREFIX = "probierz-oko-e2e-";
const requiredNames = [
  "PROBIERZ_RUN_ID",
  "OKO_E2E_EMAIL",
  "OKO_E2E_SUPABASE_URL",
  "OKO_E2E_SUPABASE_ANON_KEY",
  "OKO_E2E_SUPABASE_SERVICE_ROLE_KEY",
];
const requiredSlackNames = [
  "OKO_E2E_SLACK_BOT_TOKEN",
  "OKO_E2E_SLACK_USER_TOKEN",
  "OKO_E2E_SLACK_CHANNEL",
];

function selectedJourneys(source) {
  return new Set(String(source.PROBIERZ_JOURNEYS || "").split(",").map((value) => value.trim()).filter(Boolean));
}

function requiresSlack(source) {
  const journeys = selectedJourneys(source);
  return journeys.size === 0 || journeys.has("slack-feedback");
}

function requiresFixture(source) {
  const journeys = selectedJourneys(source);
  return journeys.size === 0 || ![...journeys].every((journey) => journey === "autonomy-experimental");
}

function requiredEnv(source = process.env, includeSlack = requiresSlack(source)) {
  const names = includeSlack ? [...requiredNames, ...requiredSlackNames] : requiredNames;
  const missing = names.filter((name) => !source[name]);
  if (missing.length) throw new Error(`missing Oko seed configuration: ${missing.join(", ")}`);
  return source;
}

function deterministicUUID(...parts) {
  const bytes = Buffer.from(createHash("sha256").update(parts.join(":"), "utf8").digest().subarray(0, 16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function statePath(source) {
  const root = source.PROBIERZ_ARTIFACTS || path.join(process.cwd(), "test-results", "oko-local");
  return path.join(root, "diagnostics", "oko-seed-state.json");
}

function runScopedSource(source) {
  const [address, domain] = String(source.OKO_E2E_EMAIL).toLowerCase().split("@");
  if (!address || !domain) throw new Error("OKO_E2E_EMAIL must be a valid technical email address");
  const base = address.split("+")[0];
  const hash = createHash("sha256").update(source.PROBIERZ_RUN_ID).digest("hex").slice(0, 12);
  return { ...source, OKO_E2E_EMAIL: `${base}+probierz-${hash}@${domain}` };
}

function supabaseHeaders(source, prefer) {
  return {
    apikey: source.OKO_E2E_SUPABASE_SERVICE_ROLE_KEY,
    Authorization: `Bearer ${source.OKO_E2E_SUPABASE_SERVICE_ROLE_KEY}`,
    "Content-Type": "application/json",
    ...(prefer ? { Prefer: prefer } : {}),
  };
}

async function requestJson(url, options) {
  const response = await fetch(url, options);
  const text = await response.text();
  let body = null;
  try { body = text ? JSON.parse(text) : null; } catch { body = { message: text }; }
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${body?.message || body?.error || body?.hint || "request failed"}`);
  }
  return body;
}

async function supabase(source, pathname, options = {}) {
  const base = source.OKO_E2E_SUPABASE_URL.replace(/\/$/, "");
  return requestJson(`${base}${pathname}`, {
    ...options,
    headers: {
      ...supabaseHeaders(source, options.prefer),
      ...(options.headers || {}),
    },
  });
}

async function slack(token, method, payload, acceptedErrors = []) {
  const body = await requestJson(`https://slack.com/api/${method}`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json; charset=utf-8" },
    body: JSON.stringify(payload),
  });
  if (!body?.ok && !acceptedErrors.includes(body?.error)) {
    throw new Error(`Slack ${method}: ${body?.error || "request failed"}`);
  }
  return body;
}

function fixtureSlug(runId, kind, index = null) {
  const scope = createHash("sha256").update(runId).digest("hex").slice(0, 12);
  return `e2e-${scope}-${kind}${index === null ? "" : `-${index + 1}`}`;
}

function strategyDocument(runId) {
  const id = (kind, index) => deterministicUUID("oko-e2e", runId, kind, String(index));
  const metrics = ["Activation", "Retention", "Revenue", "Reliability"].map((name, index) => ({
    id: id("metric", index),
    slug: fixtureSlug(runId, "metric", index),
    name,
    description: `Probierz metric ${index + 1}`,
    owner: "Oko E2E",
    horizon: "quarterly",
    unit: "percent",
    baselineValue: 0,
    targetValue: 100,
    currentValue: 25 * (index + 1),
    source: `probierz:${runId}`,
    status: "measured",
  }));
  const pillars = ["Research", "Product", "Distribution", "Operations"].map((title, index) => ({
    id: id("pillar", index),
    slug: fixtureSlug(runId, "pillar", index),
    title,
    owner: "Oko E2E",
    role: `Probierz pillar ${index + 1}`,
    successCriteria: [`Metric ${index + 1} is measured`],
    linkedProductSlugs: [fixtureSlug(runId, "product", index)],
  }));
  const initiativeSlugs = Array.from({ length: 13 }, (_, index) => fixtureSlug(runId, "initiative", index));
  const products = ["Oko", "Platform", "Research", "Distribution"].map((title, index) => ({
    id: id("product", index),
    slug: fixtureSlug(runId, "product", index),
    title,
    owner: "Oko E2E",
    role: `Probierz product ${index + 1}`,
    linkedPillarSlugs: [fixtureSlug(runId, "pillar", index)],
    activeInitiativeSlugs: initiativeSlugs.filter((_, initiativeIndex) => initiativeIndex % 4 === index),
    blockers: [],
    customerRelevance: "Deterministic E2E evidence",
    researchRelevance: "Deterministic E2E evidence",
  }));
  const initiatives = initiativeSlugs.map((slug, index) => ({
    id: id("initiative", index),
    slug,
    title: `Probierz initiative ${index + 1}`,
    owner: "Oko E2E",
    status: "in_progress",
    successMetric: metrics[index % metrics.length].name,
    linkedProductSlugs: [fixtureSlug(runId, "product", index % 4)],
    linkedPillarSlugs: [fixtureSlug(runId, "pillar", index % 4)],
    activeConversationSources: [`slack:${runId}`],
    artifactSlugs: [fixtureSlug(runId, "run-receipt")],
    nextActions: [`Complete deterministic step ${index + 1}`],
    targetDate: "2026-12-31",
    budgetUSD: 1000 + index,
    dependencySlugs: index === 0 ? [] : [initiativeSlugs[index - 1]],
    outcomeMetricSlugs: [metrics[index % metrics.length].slug],
    priority: 100 - index,
    capacityPercent: index === 12 ? 10 : 7.5,
    planningStatus: "approved",
  }));
  return {
    schemaVersion: 2,
    northStar: {
      statement: `Probierz Oko reference strategy [${runId}]`,
      marketThesis: "Deterministic product evidence is a release requirement.",
      whyWisentWins: "The product connects strategy, conversations, and execution.",
      mustBecomeTrue: ["Every critical journey has current evidence."],
      nonCriticalWork: ["Unseeded cosmetic variation."],
      metrics,
    },
    pillars,
    products,
    initiatives,
    artifacts: [{
      id: id("artifact", 0),
      slug: fixtureSlug(runId, "run-receipt"),
      title: "Probierz run receipt",
      kind: "evidence",
      owner: "Oko E2E",
      location: `probierz:${runId}`,
      status: "active",
      supportsPillarSlugs: pillars.map((pillar) => pillar.slug),
      supportsProductSlugs: products.map((product) => product.slug),
      supportsInitiativeSlugs: initiativeSlugs,
    }],
    decisions: [{
      id: id("decision", 0),
      decidedOn: "2026-07-13",
      title: "Require deterministic Oko evidence",
      decision: "Release only with current Probierz receipts.",
      rationale: `Seeded by ${runId}`,
      owner: "Oko E2E",
      affectedProductSlugs: [fixtureSlug(runId, "product", 0)],
      affectedInitiativeSlugs: [initiativeSlugs[0]],
      reversibility: "reversible",
    }],
  };
}

function readState(source) {
  const file = statePath(source);
  if (!existsSync(file)) throw new Error(`Oko seed state not found: ${file}`);
  const state = JSON.parse(readFileSync(file, "utf8"));
  if (state.runId !== source.PROBIERZ_RUN_ID) {
    throw new Error(`Oko seed state belongs to ${state.runId}, not ${source.PROBIERZ_RUN_ID}`);
  }
  return { file, state };
}

function writeState(file, state) {
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, JSON.stringify(state, null, 2), { mode: 0o600 });
}

function feedbackDecision(runId) {
  return {
    isDecision: true,
    title: `Accept Probierz feedback [${runId}]`,
    decision: `The deterministic Slack correction for ${runId} is accepted.`,
    rationale: "The correction is explicit, scoped to the synthetic organization, and reversible.",
    affectedProductSlugs: [fixtureSlug(runId, "product", 0)],
    affectedInitiativeSlugs: [fixtureSlug(runId, "initiative", 0)],
    reversibility: "reversible",
    mutations: [{
      entity: "north_star",
      slug: "",
      field: "statement",
      stringValue: `Probierz Slack feedback applied [${runId}]`,
    }],
  };
}

async function deleteOrganization(source, orgId) {
  await supabase(source, `/rest/v1/organizations?id=eq.${encodeURIComponent(orgId)}`, {
    method: "DELETE",
    prefer: "return=minimal",
  });
}

async function seed(source = process.env) {
  if (!requiresFixture(source)) return { skipped: "autonomy journey uses isolated local fixtures" };
  requiredEnv(source);
  const scopedSource = runScopedSource(source);
  const account = await ensureTechnicalAccount(scopedSource);
  const runHash = createHash("sha256").update(source.PROBIERZ_RUN_ID).digest("hex").slice(0, 12);
  const orgId = deterministicUUID("oko-e2e-org", source.PROBIERZ_RUN_ID);
  const orgSlug = `${ORG_PREFIX}${runHash}`;
  let parent = null;
  let reply = null;
  const seedSlack = requiresSlack(source);
  try {
    await deleteOrganization(scopedSource, orgId);
    await supabase(scopedSource, "/rest/v1/organizations", {
      method: "POST",
      prefer: "return=minimal",
      body: JSON.stringify({ id: orgId, slug: orgSlug, name: `Oko E2E ${runHash}` }),
    });
    await supabase(scopedSource, "/rest/v1/organization_members", {
      method: "POST",
      prefer: "return=minimal",
      body: JSON.stringify({ org_id: orgId, user_id: account.userId, role: "owner" }),
    });
    await supabase(scopedSource, "/rest/v1/company_context", {
      method: "POST",
      prefer: "resolution=merge-duplicates,return=minimal",
      body: JSON.stringify({ org_id: orgId, schema_version: 2, north_star_statement: "initializing" }),
    });
    await supabase(scopedSource, "/rest/v1/rpc/oko_apply_company_strategy", {
      method: "POST",
      body: JSON.stringify({ p_org_id: orgId, p_document: strategyDocument(source.PROBIERZ_RUN_ID) }),
    });

    let botContext = null;
    let replyText = null;
    if (seedSlack) {
      botContext = `Oko strategy review for isolated run ${source.PROBIERZ_RUN_ID}.`;
      replyText = [
        `Decision: accept the deterministic correction for ${source.PROBIERZ_RUN_ID}.`,
        `Set the north-star statement to "Probierz Slack feedback applied [${source.PROBIERZ_RUN_ID}]"`,
        "because every release needs traceable product evidence.",
      ].join(" ");
      parent = await slack(source.OKO_E2E_SLACK_BOT_TOKEN, "chat.postMessage", {
        channel: source.OKO_E2E_SLACK_CHANNEL,
        text: botContext,
      });
      reply = await slack(source.OKO_E2E_SLACK_USER_TOKEN, "chat.postMessage", {
        channel: source.OKO_E2E_SLACK_CHANNEL,
        thread_ts: parent.ts,
        text: replyText,
      });
      await supabase(scopedSource, "/rest/v1/oko_slack_thread_watches", {
        method: "POST",
        prefer: "resolution=merge-duplicates,return=minimal",
        body: JSON.stringify({
          org_id: orgId,
          channel_id: source.OKO_E2E_SLACK_CHANNEL,
          thread_ts: parent.ts,
          bot_context: botContext,
          last_reply_ts: parent.ts,
          last_scanned_at: new Date().toISOString(),
        }),
      });
    }

    const file = statePath(source);
    const decisionId = deterministicUUID("oko-e2e-feedback-decision", source.PROBIERZ_RUN_ID);
    const state = {
      schemaVersion: 2,
      runId: source.PROBIERZ_RUN_ID,
      orgId,
      orgSlug,
      userId: account.userId,
      emailHash: account.emailHash,
      strategy: { initial: strategyDocument(source.PROBIERZ_RUN_ID) },
      feedback: { decisionId, applied: false },
      slack: seedSlack ? {
        channel: source.OKO_E2E_SLACK_CHANNEL,
        parentTs: parent.ts,
        replyTs: reply.ts,
        replyAuthorId: reply.message?.user || reply.user || "oko-e2e-user",
        botContext,
        replyText,
      } : null,
    };
    writeState(file, state);
    return {
      orgId,
      orgSlug,
      accountCreated: account.created,
      stateFile: file,
      env: { OKO_E2E_EMAIL: scopedSource.OKO_E2E_EMAIL },
    };
  } catch (error) {
    const rollback = [];
    if (reply?.ts) {
      rollback.push(slack(source.OKO_E2E_SLACK_USER_TOKEN, "chat.delete", {
        channel: source.OKO_E2E_SLACK_CHANNEL,
        ts: reply.ts,
      }, ["message_not_found"]));
    }
    if (parent?.ts) {
      rollback.push(slack(source.OKO_E2E_SLACK_BOT_TOKEN, "chat.delete", {
        channel: source.OKO_E2E_SLACK_CHANNEL,
        ts: parent.ts,
      }, ["message_not_found"]));
    }
    rollback.push(deleteOrganization(scopedSource, orgId));
    rollback.push(supabase(scopedSource, `/auth/v1/admin/users/${encodeURIComponent(account.userId)}`, {
      method: "DELETE",
    }));
    await Promise.allSettled(rollback);
    throw error;
  }
}

async function advanceWriterRevision(source = process.env) {
  requiredEnv(source, false);
  const { file, state } = readState(source);
  const marker = `Probierz writer R+1 [${source.PROBIERZ_RUN_ID}]`;
  await supabase(source, `/rest/v1/company_context?org_id=eq.${encodeURIComponent(state.orgId)}`, {
    method: "PATCH",
    prefer: "return=minimal",
    body: JSON.stringify({
      north_star_statement: marker,
      updated_at: new Date().toISOString(),
    }),
  });
  state.strategy.expectedStatement = marker;
  state.writer = { marker, advancedAt: new Date().toISOString() };
  writeState(file, state);
  return { orgId: state.orgId, marker };
}

async function applyFeedback(source = process.env) {
  requiredEnv(source, false);
  const { file, state } = readState(source);
  if (!state.slack) throw new Error("Slack feedback fixture was not selected for this run");
  const decision = feedbackDecision(source.PROBIERZ_RUN_ID);
  const feedbackSource = {
    org_id: state.orgId,
    channel_id: state.slack.channel,
    message_ts: state.slack.replyTs,
    thread_ts: state.slack.parentTs,
    author_id: state.slack.replyAuthorId,
    message_text: state.slack.replyText,
    bot_context: state.slack.botContext,
  };
  await supabase(source, "/rest/v1/oko_strategic_feedback_events", {
    method: "POST",
    prefer: "resolution=merge-duplicates,return=minimal",
    body: JSON.stringify({
      id: deterministicUUID("oko-e2e-feedback-event", source.PROBIERZ_RUN_ID),
      ...feedbackSource,
      status: "pending",
      attempt_count: 1,
      last_attempt_at: new Date().toISOString(),
      last_error: "",
    }),
  });
  for (let duplicateAttempt = 0; duplicateAttempt < 2; duplicateAttempt += 1) {
    await supabase(source, "/rest/v1/rpc/oko_apply_strategic_feedback", {
      method: "POST",
      body: JSON.stringify({
        p_source: feedbackSource,
        p_decision: decision,
        p_decision_id: state.feedback.decisionId,
      }),
    });
  }
  state.feedback.applied = true;
  state.feedback.appliedAt = new Date().toISOString();
  state.strategy.expectedStatement = decision.mutations[0].stringValue;
  writeState(file, state);
  return {
    orgId: state.orgId,
    decisionId: state.feedback.decisionId,
    marker: state.strategy.expectedStatement,
    duplicateAttempts: 2,
  };
}

async function verifyFixture(source = process.env) {
  requiredEnv(source, false);
  const { state } = readState(source);
  const org = encodeURIComponent(state.orgId);
  const eventQuery = state.slack?.replyTs
    ? supabase(source, `/rest/v1/oko_strategic_feedback_events?select=status,attempt_count&org_id=eq.${org}&message_ts=eq.${encodeURIComponent(state.slack.replyTs)}`)
    : Promise.resolve([]);
  const [contexts, metrics, initiatives, decisions, events] = await Promise.all([
    supabase(source, `/rest/v1/company_context?select=north_star_statement&org_id=eq.${org}`),
    supabase(source, `/rest/v1/company_metrics?select=id&org_id=eq.${org}`),
    supabase(source, `/rest/v1/company_initiatives?select=id,capacity_percent&org_id=eq.${org}`),
    supabase(source, `/rest/v1/company_decisions?select=id,decision&org_id=eq.${org}`),
    eventQuery,
  ]);
  const capacity = initiatives.reduce((sum, item) => sum + Number(item.capacity_percent || 0), 0);
  const checks = {
    context: contexts.length === 1,
    statement: contexts[0]?.north_star_statement === (
      state.strategy.expectedStatement || state.strategy.initial.northStar.statement
    ),
    metrics: metrics.length === 4,
    initiatives: initiatives.length === 13,
    capacity: Math.abs(capacity - 100) < 0.0001,
    baselineDecision: decisions.some((item) =>
      item.id === deterministicUUID("oko-e2e", source.PROBIERZ_RUN_ID, "decision", "0")),
    feedbackDecision: !state.feedback.applied
      || decisions.filter((item) => item.id === state.feedback.decisionId).length === 1,
    feedbackEvent: !state.feedback.applied
      || (events.length === 1 && events[0].status === "processed"),
  };
  const failed = Object.entries(checks).filter(([, passed]) => !passed).map(([name]) => name);
  if (failed.length) throw new Error(`Oko fixture verification failed: ${failed.join(", ")}`);
  return { orgId: state.orgId, checks, capacity, decisionCount: decisions.length };
}

async function cleanup(source = process.env) {
  if (!requiresFixture(source)) return { skipped: "autonomy journey uses isolated local fixtures" };
  requiredEnv(source, false);
  const file = statePath(source);
  let state = null;
  if (existsSync(file)) state = JSON.parse(readFileSync(file, "utf8"));
  const slackState = state?.slack;
  if (slackState) requiredEnv(source, true);
  if (slackState?.replyTs) {
    await slack(source.OKO_E2E_SLACK_USER_TOKEN, "chat.delete", {
      channel: slackState.channel,
      ts: slackState.replyTs,
    }, ["message_not_found"]);
  }
  if (slackState?.parentTs) {
    await slack(source.OKO_E2E_SLACK_BOT_TOKEN, "chat.delete", {
      channel: slackState.channel,
      ts: slackState.parentTs,
    }, ["message_not_found"]);
  }
  const orgId = state?.orgId || deterministicUUID("oko-e2e-org", source.PROBIERZ_RUN_ID);
  await deleteOrganization(source, orgId);
  if (state?.userId) {
    await supabase(source, `/auth/v1/admin/users/${encodeURIComponent(state.userId)}`, { method: "DELETE" });
  }
  if (existsSync(file)) unlinkSync(file);
  return { deletedOrganization: orgId, deletedAccount: Boolean(state?.userId) };
}

async function main() {
  const command = process.argv[2];
  const commands = {
    seed,
    cleanup,
    "writer-update": advanceWriterRevision,
    "apply-feedback": applyFeedback,
    verify: verifyFixture,
  };
  const operation = commands[command];
  if (!operation) {
    throw new Error("usage: seed.mjs seed | writer-update | apply-feedback | verify | cleanup");
  }
  const result = await operation();
  process.stdout.write(`${JSON.stringify(result)}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    process.stderr.write(`oko seed: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}

export {
  advanceWriterRevision,
  applyFeedback,
  cleanup,
  feedbackDecision,
  seed,
  strategyDocument,
  verifyFixture,
};

import { createHash, createHmac } from "node:crypto";
import { readFile } from "node:fs/promises";
import { stadoModelRouterUrl } from "./model-router.mjs";

const HTTP_OK = 200;
const HTTP_REDIRECT = 400;
const MODEL_TIMEOUT_MS = 120000;
const ALLOWED_BLOCKERS = new Set(["fabricated_claim", "search_intent_mismatch", "misleading_snippet"]);

function required(value, label) {
  const clean = String(value || "").trim();
  if (!clean) throw new Error(`${label} is required`);
  return clean;
}

function modelDimensions(policy) {
  return Object.fromEntries(Object.entries(policy.dimensions).filter(([, rule]) => rule.source === "model" || rule.source === "hybrid"));
}

function toolSchema(dimensions) {
  const names = Object.keys(dimensions);
  const dimension = {
    type: "object",
    properties: {
      score: { type: "number", minimum: 0, maximum: 1 },
      evidence: { type: "array", minItems: 1, items: { type: "string" } },
      issues: { type: "array", items: { type: "string" } },
    },
    required: ["score", "evidence", "issues"],
    additionalProperties: false,
  };
  return {
    type: "function",
    function: {
      name: "record_seo_content_evaluation",
      description: "Record one evidence-grounded SEO content evaluation.",
      parameters: {
        type: "object",
        properties: {
          summary: { type: "string" },
          dimensions: { type: "object", properties: Object.fromEntries(names.map((name) => [name, dimension])), required: names, additionalProperties: false },
          blocking_issues: {
            type: "array",
            items: {
              type: "object",
              properties: { code: { type: "string", enum: [...ALLOWED_BLOCKERS] }, evidence: { type: "string" }, url: { type: "string" } },
              required: ["code", "evidence"],
              additionalProperties: false,
            },
          },
          recommendations: {
            type: "array",
            items: {
              type: "object",
              properties: { priority: { type: "string", enum: ["critical", "high", "medium", "low"] }, dimension: { type: "string" }, action: { type: "string" } },
              required: ["priority", "dimension", "action"],
              additionalProperties: false,
            },
          },
        },
        required: ["summary", "dimensions", "blocking_issues", "recommendations"],
        additionalProperties: false,
      },
    },
  };
}

function parseEvaluation(value, dimensions) {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("SEO model evaluation must be an object");
  if (typeof value.summary !== "string" || !value.summary.trim()) throw new Error("SEO model evaluation summary is required");
  if (!value.dimensions || typeof value.dimensions !== "object" || Array.isArray(value.dimensions)) throw new Error("SEO model dimensions are required");
  const normalized = {};
  for (const name of Object.keys(dimensions)) {
    const item = value.dimensions[name];
    if (!item || typeof item !== "object" || !Number.isFinite(item.score) || item.score < 0 || item.score > 1) throw new Error(`SEO model ${name}.score must be between 0 and 1`);
    if (!Array.isArray(item.evidence) || !item.evidence.length || item.evidence.some((entry) => typeof entry !== "string" || !entry.trim())) throw new Error(`SEO model ${name}.evidence must be a non-empty string array`);
    if (!Array.isArray(item.issues) || item.issues.some((entry) => typeof entry !== "string")) throw new Error(`SEO model ${name}.issues must be a string array`);
    normalized[name] = { score: item.score, evidence: item.evidence, issues: item.issues };
  }
  const blockingIssues = Array.isArray(value.blocking_issues) ? value.blocking_issues.map((item) => {
    if (!item || !ALLOWED_BLOCKERS.has(item.code) || typeof item.evidence !== "string" || !item.evidence.trim()) throw new Error("SEO model blocking issue is invalid");
    return { code: item.code, evidence: item.evidence, url: typeof item.url === "string" ? item.url : undefined };
  }) : [];
  const recommendations = Array.isArray(value.recommendations) ? value.recommendations.map((item) => {
    if (!item || !new Set(["critical", "high", "medium", "low"]).has(item.priority) || typeof item.dimension !== "string" || typeof item.action !== "string") throw new Error("SEO model recommendation is invalid");
    return { priority: item.priority, dimension: item.dimension, action: item.action };
  }) : [];
  return { summary: value.summary, dimensions: normalized, blockingIssues, recommendations };
}

function evidencePayload(contract, evidence, deterministic, characterBudget = contract.policy.model.maxEvidenceCharacters) {
  const declared = new Set(contract.routes.map((route) => route.url));
  const pages = evidence.pages.filter((page) => declared.has(page.url));
  const perPageCharacters = Math.max(240, Math.floor((characterBudget * 0.72) / Math.max(1, pages.length)));
  return {
    task: "Evaluate search content quality against the approved product brief and declared query intent.",
    approvedBrief: {
      product: contract.brief.product,
      audience: contract.brief.audience,
      problem: contract.brief.problem,
      promise: contract.brief.promise,
      approvedClaims: contract.brief.approvedClaims,
      requiredProof: contract.brief.requiredProof,
      seo: contract.brief.seo,
    },
    dimensions: modelDimensions(contract.policy),
    deterministicBlockers: deterministic.blockers,
    deterministicWarnings: deterministic.warnings,
    declaredRoutes: contract.routes.map(({ url, locale, intents, indexable }) => ({ url, locale, intents, indexable })),
    pages: pages.map((page) => ({
      url: page.url,
      title: page.googlebot?.title,
      metaDescription: page.googlebot?.metaDescription,
      canonical: page.googlebot?.canonical,
      h1: page.googlebot?.h1?.slice(0, 3),
      headings: page.googlebot?.headings?.slice(0, 20),
      jsonLd: page.googlebot?.jsonLd?.join("\n").slice(0, Math.floor(perPageCharacters * 0.25)),
      structuredData: page.structuredData,
      visibleText: page.googlebot?.visibleText?.slice(0, Math.floor(perPageCharacters * 0.65)),
    })),
    crawlCoverage: evidence.coverage,
  };
}

async function imageContent(evidence, maximum) {
  const paths = evidence.pages.flatMap((page) => [page.ordinary?.screenshot, page.googlebot?.screenshot]).filter(Boolean).slice(0, maximum);
  const content = [];
  for (const file of paths) {
    content.push({ type: "text", text: `Rendered search evidence: ${file.split("/").pop()}` });
    content.push({ type: "image_url", image_url: { url: `data:image/jpeg;base64,${(await readFile(file)).toString("base64")}` } });
  }
  return content;
}

async function invoke({ model, contract, evidence, deterministic, routerBaseUrl, routerBearer, agentId, agentSecret, adjudication }) {
  const compact = JSON.stringify(adjudication || evidencePayload(contract, evidence, deterministic));
  if (compact.length > contract.policy.model.maxEvidenceCharacters) {
    throw new Error(`SEO model evidence is ${compact.length} characters, over the ${contract.policy.model.maxEvidenceCharacters} character policy limit`);
  }
  const content = [{ type: "text", text: compact }, ...(await imageContent(evidence, contract.policy.model.maxScreenshots))];
  const body = JSON.stringify({
    model,
    max_tokens: contract.policy.model.maxOutputTokens,
    temperature: 0,
    messages: [
      {
        role: "system",
        content: [
          adjudication ? "You are the adjudicator for two independent Probierz SEO content evaluations." : "You are an independent Probierz SEO content evaluator.",
          ...contract.policy.modelInstructions,
          adjudication ? "Resolve the disagreement from source evidence; do not average unsupported claims." : "Do not defer to another evaluator or infer technical failures.",
          "Call record_seo_content_evaluation exactly once and return no prose outside the tool call.",
        ].join("\n"),
      },
      { role: "user", content },
    ],
    tools: [toolSchema(dimensions)],
  });
  const timestamp = String(Math.floor(Date.now() / 1000));
  const digest = createHash("sha256").update(body).digest("hex");
  const signature = createHmac("sha256", agentSecret).update(`${agentId}:${timestamp}:${digest}`).digest("hex");
  const response = await fetch(`${stadoModelRouterUrl(routerBaseUrl)}/v1/chat/completions`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${routerBearer}`,
      "Content-Type": "application/json",
      "x-agent-id": agentId,
      "x-agent-timestamp": timestamp,
      "x-agent-signature": signature,
    },
    body,
    signal: AbortSignal.timeout(MODEL_TIMEOUT_MS),
  });
  const text = await response.text();
  let payload;
  try {
    payload = JSON.parse(text);
  } catch {
    throw new Error(`SEO model router returned non-JSON (${response.status})`);
  }
  if (response.status < HTTP_OK || response.status >= HTTP_REDIRECT) throw new Error(`SEO model router HTTP ${response.status}: ${String(payload?.error?.message || "request failed").slice(0, 500)}`);
  const calls = Array.isArray(payload?.choices?.[0]?.message?.tool_calls) ? payload.choices[0].message.tool_calls.filter((call) => call?.type === "function" && call?.function?.name === "record_seo_content_evaluation") : [];
  if (calls.length !== 1 || typeof calls[0].function?.arguments !== "string") throw new Error("SEO model router must return exactly one record_seo_content_evaluation tool call");
  let args;
  try {
    args = JSON.parse(calls[0].function.arguments);
  } catch {
    throw new Error("SEO model router returned invalid tool arguments");
  }
  return {
    modelRequested: model,
    responseSha256: createHash("sha256").update(text).digest("hex"),
    modelReturned: typeof payload.model === "string" ? payload.model : null,
    requestSha256: digest,
    rubricSha256: createHash("sha256").update(JSON.stringify({ dimensions, instructions: contract.policy.modelInstructions })).digest("hex"),
    usage: payload.usage || null,
    evaluation: parseEvaluation(args, dimensions),
  };
}

function disagreement(primary, secondary, delta) {
  const dimensions = Object.keys(primary.evaluation.dimensions);
  const scoreDelta = Math.max(...dimensions.map((name) => Math.abs(primary.evaluation.dimensions[name].score - secondary.evaluation.dimensions[name].score)));
  const left = new Set(primary.evaluation.blockingIssues.map((item) => item.code));
  const right = new Set(secondary.evaluation.blockingIssues.map((item) => item.code));
  const blockerMismatch = [...new Set([...left, ...right])].some((code) => left.has(code) !== right.has(code));
  return { required: scoreDelta > delta || blockerMismatch, scoreDelta: Number(scoreDelta.toFixed(4)), blockerMismatch };
}

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.floor(sorted.length / 2)];
}

export async function evaluateSeoContent(contract, evidence, deterministic, options = {}) {
  const primaryModel = required(options.primaryModel || process.env.PROBIERZ_SEO_PRIMARY_MODEL, "PROBIERZ_SEO_PRIMARY_MODEL");
  const secondaryModel = required(options.secondaryModel || process.env.PROBIERZ_SEO_SECONDARY_MODEL, "PROBIERZ_SEO_SECONDARY_MODEL");
  if (primaryModel === secondaryModel) throw new Error("SEO primary and secondary model IDs must differ");
  const shared = {
    contract,
    evidence,
    deterministic,
    routerBaseUrl: options.routerBaseUrl || process.env.STADO_MODEL_ROUTER_URL,
    routerBearer: required(options.routerBearer || process.env.STADO_MODEL_ROUTER_TOKEN, "STADO_MODEL_ROUTER_TOKEN"),
    agentId: required(options.agentId || process.env.PROBIERZ_MODEL_AGENT_ID, "PROBIERZ_MODEL_AGENT_ID"),
    agentSecret: required(options.agentSecret || process.env.PROBIERZ_MODEL_AGENT_SECRET, "PROBIERZ_MODEL_AGENT_SECRET"),
  };
  const [primary, secondary] = await Promise.all([
    invoke({ ...shared, model: primaryModel }),
    invoke({ ...shared, model: secondaryModel }),
  ]);
  if (primary.modelReturned && secondary.modelReturned && primary.modelReturned === secondary.modelReturned) {
    throw new Error(`SEO graders resolved to the same model ${primary.modelReturned}`);
  }
  if (!primary.modelReturned || !secondary.modelReturned) throw new Error("SEO graders did not identify the model versions that produced their evaluations");
  const divergence = disagreement(primary, secondary, contract.policy.model.adjudicationDelta);
  let adjudicator = null;
  if (divergence.required) {
    const adjudicatorModel = required(options.adjudicatorModel || process.env.PROBIERZ_SEO_ADJUDICATOR_MODEL, "PROBIERZ_SEO_ADJUDICATOR_MODEL");
    if (new Set([primaryModel, secondaryModel, adjudicatorModel]).size !== 3) throw new Error("SEO adjudicator model ID must differ from both graders");
    adjudicator = await invoke({
      ...shared,
      model: adjudicatorModel,
      adjudication: {
        task: "Adjudicate the two evaluations against the original approved brief and page evidence.",
        originalEvidence: evidencePayload(contract, evidence, deterministic, Math.floor(contract.policy.model.maxEvidenceCharacters * 0.45)),
        primary: primary.evaluation,
        secondary: secondary.evaluation,
      },
    });
    if (!adjudicator.modelReturned || new Set([primary.modelReturned, secondary.modelReturned, adjudicator.modelReturned]).size !== 3) {
      throw new Error("SEO adjudication did not use a third identifiable model version");
    }
  }
  const dimensions = {};
  for (const name of Object.keys(modelDimensions(contract.policy))) {
    const graders = [primary, secondary, ...(adjudicator ? [adjudicator] : [])];
    const score = adjudicator
      ? adjudicator.evaluation.dimensions[name].score
      : Math.min(primary.evaluation.dimensions[name].score, secondary.evaluation.dimensions[name].score);
    dimensions[name] = {
      score: Number(score.toFixed(4)),
      evidence: graders.flatMap((grader) => grader.evaluation.dimensions[name].evidence),
      issues: [...new Set(graders.flatMap((grader) => grader.evaluation.dimensions[name].issues))],
      graderScores: Object.fromEntries(graders.map((grader) => [grader.modelRequested, grader.evaluation.dimensions[name].score])),
    };
  }
  const blockers = adjudicator
    ? adjudicator.evaluation.blockingIssues
    : primary.evaluation.blockingIssues.filter((left) => secondary.evaluation.blockingIssues.some((right) => right.code === left.code));
  return {
    dimensions,
    blockers: blockers.map((item) => ({ ...item, source: "model" })),
    recommendations: [...primary.evaluation.recommendations, ...secondary.evaluation.recommendations, ...(adjudicator?.evaluation.recommendations || [])],
    divergence,
    graders: { primary, secondary, adjudicator },
  };
}

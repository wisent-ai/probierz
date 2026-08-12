import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { resolveSeoContract } from "./seo-policy.mjs";
import { collectSeoEvidence } from "./seo-crawl.mjs";
import { evaluateDeterministicSeo, loadProductionSeoEvidence } from "./seo-verdict.mjs";
import { evaluateSeoContent } from "./seo-model.mjs";
import { appSourceIdentity } from "./runner.mjs";
import { signEvidencePayload, signedEvidenceId } from "./receipt.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..");


function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function signingKey(options) {
  const raw = options.privateKey
    || process.env.PROBIERZ_SEO_RECEIPT_PRIVATE_KEY
    || (options.privateKeyFile || process.env.PROBIERZ_RECEIPT_PRIVATE_KEY_FILE
      ? readFileSync(options.privateKeyFile || process.env.PROBIERZ_RECEIPT_PRIVATE_KEY_FILE, "utf8")
      : "");
  return String(raw || "").trim() || null;
}

function timestampSegment() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function reportDestination(options, appId) {
  if (options.outputPath) {
    const file = path.resolve(options.outputPath);
    if (path.extname(file).toLowerCase() !== ".json") throw new Error("SEO output path must end in .json");
    return { directory: path.dirname(file), file };
  }
  const directory = path.join(ROOT, "test-results", "seo", appId, timestampSegment());
  return { directory, file: path.join(directory, "seo-evaluation.json") };
}

function combineDimensions(contract, deterministic, model) {
  const dimensions = {};
  for (const [name, rule] of Object.entries(contract.policy.dimensions)) {
    const deterministicValue = deterministic.dimensions[name];
    const modelValue = model.dimensions[name];
    if (rule.source === "deterministic" && !deterministicValue) throw new Error(`deterministic SEO dimension ${name} is missing`);
    if (rule.source === "model" && !modelValue) throw new Error(`model SEO dimension ${name} is missing`);
    if (rule.source === "hybrid" && (!deterministicValue || !modelValue)) throw new Error(`hybrid SEO dimension ${name} is incomplete`);
    const score = rule.source === "deterministic"
      ? deterministicValue.score
      : rule.source === "model"
        ? modelValue.score
        : Math.min(deterministicValue.score, modelValue.score);
    dimensions[name] = {
      label: rule.label,
      source: rule.source,
      weight: rule.weight,
      minimum: rule.minimum,
      score: Number(score.toFixed(4)),
      evidence: [...(deterministicValue?.evidence || []), ...(modelValue?.evidence || [])],
      issues: [...new Set([...(deterministicValue?.issues || []), ...(modelValue?.issues || [])])],
      graderScores: modelValue?.graderScores,
    };
  }
  return dimensions;
}

export async function evaluateSeo(options = {}) {
  const contract = resolveSeoContract(options);
  const destination = reportDestination(options, contract.appId);
  mkdirSync(destination.directory, { recursive: true });
  let ownedBrowser = null;
  let browser = options.browser;
  if (!browser) {
    const playwright = await import("@playwright/test");
    ownedBrowser = await playwright.chromium.launch({ headless: true });
    browser = ownedBrowser;
  }
  let evidence;
  try {
    evidence = await collectSeoEvidence(contract, { browser, artifactsDir: destination.directory });
  } finally {
    await ownedBrowser?.close().catch(() => {});
  }
  const deterministic = evaluateDeterministicSeo(contract, evidence);
  let model;
  try {
    model = await evaluateSeoContent(contract, evidence, deterministic, options);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const dimensions = Object.fromEntries(
      Object.entries(contract.policy.dimensions)
        .filter(([, rule]) => rule.source === "model" || rule.source === "hybrid")
        .map(([name]) => [name, { score: 0, evidence: [`model evaluation failed: ${message}`], issues: [message], graderScores: {} }]),
    );
    model = {
      dimensions,
      blockers: [{ code: "model_evaluation_failed", evidence: message, source: "model" }],
      recommendations: [],
      divergence: null,
      graders: { primary: null, secondary: null, adjudicator: null },
    };
  }
  const production = loadProductionSeoEvidence(options.productionEvidencePath || process.env.PROBIERZ_SEO_PRODUCTION_EVIDENCE, contract);
  const dimensions = combineDimensions(contract, deterministic, model);
  const blockers = [...deterministic.blockers, ...model.blockers];
  const signatureRequired = Boolean(contract.manifest.seo?.profiles?.[contract.mode]?.requireSignature);
  if (production.required) blockers.push(...production.blockers);
  if (production.required && production.status === "not-provided") blockers.push({ code: "production_evidence_missing", evidence: "production mode requires Search Console and CrUX evidence", source: "production" });
  const quality = Number(Object.entries(dimensions).reduce((sum, [name, item]) => sum + item.score * contract.policy.dimensions[name].weight, 0).toFixed(4));
  for (const [name, item] of Object.entries(dimensions)) {
    if (item.score < item.minimum) blockers.push({ code: `dimension_below_minimum:${name}`, evidence: `${item.score.toFixed(3)} < ${item.minimum.toFixed(3)}`, source: "threshold" });
  }
  if (quality < contract.policy.qualityMinimum) blockers.push({ code: "quality_below_minimum", evidence: `${quality.toFixed(3)} < ${contract.policy.qualityMinimum.toFixed(3)}`, source: "threshold" });
  const key = signingKey(options);
  if (signatureRequired && !key) blockers.push({ code: "evidence_signature_missing", evidence: `${contract.mode} SEO evidence requires an Ed25519 signing key`, source: "evidence" });
  const sourceIdentity = appSourceIdentity(contract.appId);
  const payload = {
    schemaVersion: 1,
    kind: "probierz-seo-evaluation",
    appId: contract.appId,
    mode: contract.mode,
    issuedAt: new Date().toISOString(),
    sourceIdentity,
    contract: {
      policy: { file: contract.policy.file, name: contract.policy.name, sha256: sha256(readFileSync(contract.policy.file)) },
      brief: { file: contract.brief.file, product: contract.brief.product, sha256: sha256(readFileSync(contract.brief.file)) },
      baseUrl: contract.baseUrl,
      routes: contract.routes,
    },
    verdict: {
      pass: blockers.length === 0,
      searchEligibility: deterministic.blockers.length === 0 ? "eligible" : "blocked",
      searchQuality: quality,
      requiredQuality: contract.policy.qualityMinimum,
      productionOutcome: production.status,
      blockers,
      warnings: deterministic.warnings,
    },
    dimensions,
    evidence,
    model,
    production,
  };
  const signing = key ? signEvidencePayload(payload, key) : null;
  const reportId = signing ? signedEvidenceId(payload, signing) : sha256(JSON.stringify(payload)).slice(0, 24);
  const report = { ...payload, reportId, signing };
  writeFileSync(destination.file, `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  return {
    file: destination.file,
    reportId: report.reportId,
    pass: report.verdict.pass,
    searchEligibility: report.verdict.searchEligibility,
    searchQuality: quality,
    productionOutcome: report.verdict.productionOutcome,
    blockers: report.verdict.blockers,
    signing: signing && { algorithm: signing.algorithm, publicKeyFingerprintSha256: signing.publicKeyFingerprintSha256, payloadSha256: signing.payloadSha256 },
  };
}

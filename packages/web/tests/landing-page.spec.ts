import { expect, test, type Browser, type BrowserContext, type Page, type TestInfo } from '@playwright/test';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const RUBRIC_PATH = resolve(process.cwd(), '../../apps/landing-page/rubric.json');
const HTTP_SUCCESS_MIN = 200;
const HTTP_SUCCESS_MAX = 400;
const MAX_ROUTER_MS = 120_000;
const DEFAULT_MAX_OUTPUT_TOKENS = 2400;
const OVERFLOW_TOLERANCE_PX = 2;

interface PrimaryAction {
  label: string;
  kind: 'url' | 'dialog' | 'form';
  target: string;
}

interface LandingBrief {
  schemaVersion: 1;
  product: string;
  audience: string;
  problem: string;
  promise: string;
  primaryAction: PrimaryAction;
  secondaryAction?: { label: string; purpose: string };
  approvedClaims: Array<{ claim: string; evidence: string }>;
  requiredProof: string[];
  brand: { canonicalAssets: string[]; rules: string[]; forbidden: string[] };
  analyticsOwner: string;
  notes?: string[];
}

interface DimensionRule {
  label: string;
  weight: number;
  minimum: number;
  criterion: string;
}

interface LandingRubric {
  schemaVersion: 1;
  name: string;
  overallMinimum: number;
  dimensions: Record<string, DimensionRule>;
  deterministicGates: Record<string, string>;
  modelInstructions: string[];
}

interface ViewportAudit {
  profile: string;
  url: string;
  httpStatus: number;
  title: string;
  metaDescription: string;
  lang: string;
  h1: string[];
  headings: Array<{ level: number; text: string }>;
  headingLevelSkips: number;
  primaryActionMatches: Array<{
    tag: string;
    name: string;
    href: string | null;
    inFirstViewport: boolean;
  }>;
  horizontalOverflowPx: number;
  visibleInteractiveCount: number;
  unnamedInteractiveCount: number;
  visibleFormControlCount: number;
  unlabeledFormControlCount: number;
  informativeImageCount: number;
  imagesMissingAltCount: number;
  duplicateIdCount: number;
  placeholderText: string[];
  documentHeight: number;
  cumulativeLayoutShift: number;
  navigationTimingMs: {
    domContentLoaded: number;
    load: number;
    responseEnd: number;
  };
  consoleErrors: string[];
  failedRequests: string[];
}

interface CaptureResult {
  context: BrowserContext;
  page: Page;
  audit: ViewportAudit;
  heroPath: string;
  proofPath: string;
}

interface ModelDimension {
  score: number;
  evidence: string[];
  issues: string[];
}

interface ModelEvaluation {
  summary: string;
  dimensions: Record<string, ModelDimension>;
  blocking_issues: Array<{ code: string; evidence: string }>;
  recommendations: Array<{
    priority: 'critical' | 'high' | 'medium' | 'low';
    dimension: string;
    action: string;
  }>;
}

interface RoutedModelEvaluation {
  evaluation: ModelEvaluation;
  routerModel: string | null;
  usage: unknown;
}

interface RouterToolCall {
  type?: unknown;
  function?: {
    name?: unknown;
    arguments?: unknown;
  };
}

interface RouterPayload {
  model?: unknown;
  usage?: unknown;
  error?: { message?: unknown };
  choices?: Array<{
    message?: {
      tool_calls?: RouterToolCall[];
    };
  }>;
}


function requiredEnvironment(name: string): string {
  const value = String(process.env[name] || '').trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

function nonEmpty(value: unknown, field: string): asserts value is string {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${field} is required`);
}

function stringArray(value: unknown, field: string): asserts value is string[] {
  if (!Array.isArray(value) || value.length === 0 || value.some((entry) => typeof entry !== 'string' || !entry.trim())) {
    throw new Error(`${field} must be a non-empty string array`);
  }
}

async function readJson(path: string): Promise<unknown> {
  try {
    return JSON.parse(await readFile(path, 'utf8')) as unknown;
  } catch (error) {
    throw new Error(`cannot read JSON ${path}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

async function loadBrief(): Promise<{ path: string; brief: LandingBrief }> {
  const path = resolve(requiredEnvironment('PROBIERZ_LANDING_BRIEF'));
  const raw = (await readJson(path)) as Partial<LandingBrief>;
  if (!raw || typeof raw !== 'object' || raw.schemaVersion !== 1) {
    throw new Error('landing brief schemaVersion must be 1');
  }
  nonEmpty(raw.product, 'product');
  nonEmpty(raw.audience, 'audience');
  nonEmpty(raw.problem, 'problem');
  nonEmpty(raw.promise, 'promise');
  nonEmpty(raw.analyticsOwner, 'analyticsOwner');
  if (!raw.primaryAction || typeof raw.primaryAction !== 'object' || !['url', 'dialog', 'form'].includes(String(raw.primaryAction.kind))) {
    throw new Error('landing brief primaryAction.kind must be url, dialog, or form');
  }
  nonEmpty(raw.primaryAction.label, 'primaryAction.label');
  nonEmpty(raw.primaryAction.target, 'primaryAction.target');
  if (!Array.isArray(raw.approvedClaims) || raw.approvedClaims.length === 0) {
    throw new Error('landing brief approvedClaims must contain substantiated claims');
  }
  const approvedClaims = raw.approvedClaims.map((claim, index) => {
    if (!claim || typeof claim !== 'object') throw new Error(`landing brief approvedClaims.${index} must be an object`);
    nonEmpty(claim.claim, `approvedClaims.${index}.claim`);
    nonEmpty(claim.evidence, `approvedClaims.${index}.evidence`);
    return { claim: claim.claim, evidence: claim.evidence };
  });
  stringArray(raw.requiredProof, 'requiredProof');
  if (!raw.brand || typeof raw.brand !== 'object') throw new Error('landing brief brand must be an object');
  stringArray(raw.brand.canonicalAssets, 'brand.canonicalAssets');
  stringArray(raw.brand.rules, 'brand.rules');
  if (
    !Array.isArray(raw.brand.forbidden) ||
    raw.brand.forbidden.some((entry) => typeof entry !== 'string' || !entry.trim())
  ) {
    throw new Error('landing brief brand.forbidden must be a string array');
  }
  let secondaryAction: LandingBrief['secondaryAction'];
  if (raw.secondaryAction !== undefined) {
    if (!raw.secondaryAction || typeof raw.secondaryAction !== 'object') {
      throw new Error('landing brief secondaryAction must be an object');
    }
    nonEmpty(raw.secondaryAction.label, 'secondaryAction.label');
    nonEmpty(raw.secondaryAction.purpose, 'secondaryAction.purpose');
    secondaryAction = { label: raw.secondaryAction.label, purpose: raw.secondaryAction.purpose };
  }
  let notes: string[] | undefined;
  if (raw.notes !== undefined) {
    if (!Array.isArray(raw.notes) || raw.notes.some((entry) => typeof entry !== 'string')) {
      throw new Error('landing brief notes must be a string array');
    }
    notes = raw.notes;
  }
  return {
    path,
    brief: {
      schemaVersion: 1,
      product: raw.product,
      audience: raw.audience,
      problem: raw.problem,
      promise: raw.promise,
      primaryAction: {
        label: raw.primaryAction.label,
        kind: raw.primaryAction.kind,
        target: raw.primaryAction.target,
      },
      secondaryAction,
      approvedClaims,
      requiredProof: raw.requiredProof,
      brand: {
        canonicalAssets: raw.brand.canonicalAssets,
        rules: raw.brand.rules,
        forbidden: raw.brand.forbidden,
      },
      analyticsOwner: raw.analyticsOwner,
      notes,
    },
  };
}

async function loadRubric(): Promise<LandingRubric> {
  const raw = (await readJson(RUBRIC_PATH)) as Partial<LandingRubric>;
  if (!raw || typeof raw !== 'object' || raw.schemaVersion !== 1 || !raw.dimensions || typeof raw.dimensions !== 'object') {
    throw new Error('landing rubric schemaVersion or dimensions are invalid');
  }
  nonEmpty(raw.name, 'rubric.name');
  if (typeof raw.overallMinimum !== 'number' || raw.overallMinimum < 0 || raw.overallMinimum > 1) {
    throw new Error('landing rubric overallMinimum must be between 0 and 1');
  }
  const dimensions: Record<string, DimensionRule> = {};
  for (const [name, candidate] of Object.entries(raw.dimensions)) {
    if (!candidate || typeof candidate !== 'object') throw new Error(`rubric ${name} must be an object`);
    nonEmpty(candidate.label, `rubric.${name}.label`);
    nonEmpty(candidate.criterion, `rubric.${name}.criterion`);
    if (typeof candidate.weight !== 'number' || candidate.weight <= 0) {
      throw new Error(`rubric ${name}.weight must be positive`);
    }
    if (typeof candidate.minimum !== 'number' || candidate.minimum < 0 || candidate.minimum > 1) {
      throw new Error(`rubric ${name}.minimum must be between 0 and 1`);
    }
    dimensions[name] = {
      label: candidate.label,
      weight: candidate.weight,
      minimum: candidate.minimum,
      criterion: candidate.criterion,
    };
  }
  const weight = Object.values(dimensions).reduce((sum, rule) => sum + rule.weight, 0);
  if (Math.abs(weight - 1) > 0.000_001) throw new Error(`landing rubric weights total ${weight}, expected 1`);
  if (!raw.deterministicGates || typeof raw.deterministicGates !== 'object') {
    throw new Error('landing rubric deterministicGates must be an object');
  }
  const deterministicGates: Record<string, string> = {};
  for (const [name, value] of Object.entries(raw.deterministicGates)) {
    nonEmpty(value, `rubric.deterministicGates.${name}`);
    deterministicGates[name] = value;
  }
  stringArray(raw.modelInstructions, 'rubric.modelInstructions');
  return {
    schemaVersion: 1,
    name: raw.name,
    overallMinimum: raw.overallMinimum,
    dimensions,
    deterministicGates,
    modelInstructions: raw.modelInstructions,
  };
}

function targetUrl(value: string): string {
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error('BASE_URL must be an absolute URL');
  }
  const loopback = parsed.hostname === 'localhost' || parsed.hostname === '127.0.0.1' || parsed.hostname === '::1';
  if (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && loopback)) {
    throw new Error('BASE_URL must use HTTPS or loopback HTTP');
  }
  if (parsed.username || parsed.password) throw new Error('BASE_URL must not contain credentials');
  return parsed.href;
}

function routerUrl(value: string): string {
  const parsed = new URL(value);
  const loopback = parsed.hostname === 'localhost' || parsed.hostname === '127.0.0.1' || parsed.hostname === '::1';
  if (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && loopback)) {
    throw new Error('STADO_MODEL_ROUTER_URL must use HTTPS or loopback HTTP');
  }
  if (parsed.username || parsed.password || parsed.search || parsed.hash) {
    throw new Error('STADO_MODEL_ROUTER_URL must not contain credentials, query parameters, or a fragment');
  }
  return parsed.href.replace(/\/+$/, '');
}

async function captureViewport(
  browser: Browser,
  url: string,
  profile: 'desktop' | 'tablet' | 'mobile',
  primaryActionLabel: string,
  testInfo: TestInfo,
): Promise<CaptureResult> {
  const mobile = profile === 'mobile';
  const tablet = profile === 'tablet';
  const viewport = mobile ? { width: 390, height: 844 } : tablet ? { width: 768, height: 1024 } : { width: 1440, height: 1000 };
  const context = await browser.newContext({
    viewport,
    deviceScaleFactor: 1,
    hasTouch: mobile || tablet,
    isMobile: mobile,
    reducedMotion: 'reduce',
  });
  const page = await context.newPage();
  const consoleErrors: string[] = [];
  const failedRequests: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text().slice(0, 500));
  });
  page.on('requestfailed', (request) => {
    failedRequests.push(`${request.method()} ${request.url()} — ${request.failure()?.errorText || 'failed'}`.slice(0, 700));
  });

  try {
    await page.addInitScript(() => {
      const state = window as unknown as { __probierzCls: number };
      state.__probierzCls = 0;
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries() as Array<PerformanceEntry & { hadRecentInput?: boolean; value?: number }>) {
          if (!entry.hadRecentInput) state.__probierzCls += Number(entry.value || 0);
        }
      }).observe({ type: 'layout-shift', buffered: true });
    });
    const response = await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30_000 });
    await page.evaluate(async () => {
      if (document.fonts?.ready) await document.fonts.ready;
    });
    await page.waitForTimeout(350);

    const audit = await page.evaluate(
      ({ label, profileName, status, capturedConsoleErrors, capturedFailedRequests }) => {
        const normalizedLabel = label.replace(/\s+/g, ' ').trim().toLowerCase();
        const visible = (element: Element): element is HTMLElement => {
          if (!(element instanceof HTMLElement)) return false;
          const rect = element.getBoundingClientRect();
          const style = getComputedStyle(element);
          return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden';
        };
        const clean = (value: string | null | undefined) => String(value || '').replace(/\s+/g, ' ').trim();
        const accessibleName = (element: HTMLElement) => {
          const labelledBy = clean(element.getAttribute('aria-labelledby'));
          if (labelledBy) {
            const text = labelledBy
              .split(/\s+/)
              .map((id) => clean(document.getElementById(id)?.textContent))
              .filter(Boolean)
              .join(' ');
            if (text) return text;
          }
          const inputValue =
            element instanceof HTMLInputElement && ['button', 'reset', 'submit'].includes(element.type)
              ? element.value
              : '';
          const descendantImageAlt = element.querySelector<HTMLImageElement>('img[alt]')?.alt || '';
          const innerText = element.matches('input,select,textarea') ? '' : element.innerText;
          return clean(
            element.getAttribute('aria-label') ||
              element.getAttribute('alt') ||
              element.getAttribute('title') ||
              inputValue ||
              innerText ||
              descendantImageAlt,
          );
        };
        const interactive = [...document.querySelectorAll<HTMLElement>('a,button,[role="button"],input,select,textarea')].filter(visible);
        const ctas = interactive
          .map((element) => {
            const name = accessibleName(element);
            const anchor = element.closest('a');
            const rect = element.getBoundingClientRect();
            return {
              tag: element.tagName.toLowerCase(),
              name,
              href: anchor?.href || null,
              inFirstViewport:
                rect.bottom > 0 && rect.top < window.innerHeight && rect.right > 0 && rect.left < window.innerWidth,
            };
          })
          .filter((entry) => clean(entry.name).toLowerCase() === normalizedLabel);
        const controls = [...document.querySelectorAll<HTMLElement>('input:not([type="hidden"]),select,textarea')].filter(visible);
        const controlLabelled = (element: HTMLElement) => {
          if (accessibleName(element)) return true;
          const id = element.id;
          return Boolean((id && document.querySelector(`label[for="${CSS.escape(id)}"]`)) || element.closest('label'));
        };
        const headings = [...document.querySelectorAll<HTMLElement>('h1,h2,h3,h4,h5,h6')]
          .filter(visible)
          .map((element) => ({ level: Number(element.tagName.slice(1)), text: clean(element.innerText) }))
          .filter((entry) => entry.text);
        let headingLevelSkips = 0;
        for (let index = 1; index < headings.length; index += 1) {
          if (headings[index].level > headings[index - 1].level + 1) headingLevelSkips += 1;
        }
        const ids = [...document.querySelectorAll<HTMLElement>('[id]')].map((element) => element.id).filter(Boolean);
        const duplicateIdCount = ids.length - new Set(ids).size;
        const informativeImages = [...document.querySelectorAll<HTMLImageElement>('img')].filter(
          (image) => visible(image) && image.getAttribute('role') !== 'presentation' && image.getAttribute('aria-hidden') !== 'true',
        );
        const bodyText = clean(document.body?.innerText);
        const placeholderText = bodyText.match(/\b(?:lorem ipsum|todo\s*:|placeholder (?:image|copy|text))\b/gi) || [];
        const nav = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming | undefined;
        const clsState = window as unknown as { __probierzCls?: number };
        return {
          profile: profileName,
          url: location.href,
          httpStatus: status,
          title: document.title,
          metaDescription: document.querySelector<HTMLMetaElement>('meta[name="description"]')?.content || '',
          lang: document.documentElement.lang || '',
          h1: headings.filter((entry) => entry.level === 1).map((entry) => entry.text),
          headings: headings.slice(0, 80),
          headingLevelSkips,
          primaryActionMatches: ctas,
          horizontalOverflowPx: Math.max(0, document.documentElement.scrollWidth - window.innerWidth),
          visibleInteractiveCount: interactive.length,
          unnamedInteractiveCount: interactive.filter((element) => !accessibleName(element)).length,
          visibleFormControlCount: controls.length,
          unlabeledFormControlCount: controls.filter((element) => !controlLabelled(element)).length,
          informativeImageCount: informativeImages.length,
          imagesMissingAltCount: informativeImages.filter((image) => !image.hasAttribute('alt')).length,
          duplicateIdCount,
          placeholderText: [...new Set(placeholderText.map(clean))],
          documentHeight: document.documentElement.scrollHeight,
          cumulativeLayoutShift: Number(clsState.__probierzCls || 0),
          navigationTimingMs: {
            domContentLoaded: Number(nav?.domContentLoadedEventEnd || 0),
            load: Number(nav?.loadEventEnd || 0),
            responseEnd: Number(nav?.responseEnd || 0),
          },
          consoleErrors: capturedConsoleErrors,
          failedRequests: capturedFailedRequests,
        };
      },
      {
        label: primaryActionLabel,
        profileName: profile,
        status: response?.status() || 0,
        capturedConsoleErrors: consoleErrors,
        capturedFailedRequests: failedRequests,
      },
    );

    const heroPath = testInfo.outputPath(`${profile}-hero.png`);
    const proofPath = testInfo.outputPath(`${profile}-proof.png`);
    await page.screenshot({ path: heroPath, fullPage: false });
    const proofY = await page.evaluate(() => {
      const range = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
      return Math.round(range * 0.58);
    });
    await page.evaluate((y) => window.scrollTo({ top: y, behavior: 'instant' }), proofY);
    await page.waitForTimeout(150);
    await page.screenshot({ path: proofPath, fullPage: false });
    await page.evaluate(() => window.scrollTo({ top: 0, behavior: 'instant' }));
    return { context, page, audit, heroPath, proofPath };
  } catch (error) {
    await context.close().catch(() => {});
    throw error;
  }
}

async function verifyPrimaryAction(page: Page, action: PrimaryAction): Promise<{ pass: boolean; evidence: string }> {
  const button = page.getByRole('button', { name: action.label, exact: true });
  const link = page.getByRole('link', { name: action.label, exact: true });
  const locator = (await button.count()) > 0 ? button.first() : link.first();
  if ((await locator.count()) === 0) return { pass: false, evidence: `No accessible ${JSON.stringify(action.label)} action` };

  if (action.kind === 'url') {
    const href = await locator.evaluate((element) => element.closest('a')?.href || null);
    if (!href) return { pass: false, evidence: 'Approved URL action is not a link' };
    const pass = href === action.target || href.startsWith(action.target);
    return { pass, evidence: `${href} ${pass ? 'matches' : 'does not match'} ${action.target}` };
  }

  if (action.kind === 'form') {
    const target = page.locator(action.target).first();
    if ((await target.count()) === 0) return { pass: false, evidence: `Form target ${action.target} does not exist` };
    if (!(await target.isVisible())) return { pass: false, evidence: `Form target ${action.target} is not visible` };
    const pass = await locator.evaluate((element, selector) => {
      const targetElement = document.querySelector(selector);
      const anchor = element.closest('a');
      const controlledId = element.getAttribute('aria-controls');
      return Boolean(
        targetElement &&
          (targetElement.contains(element) ||
            (anchor?.hash && targetElement.id && anchor.hash === `#${targetElement.id}`) ||
            element.closest('form') === targetElement ||
            (controlledId && targetElement.id === controlledId)),
      );
    }, action.target);
    return { pass, evidence: `${action.label} ${pass ? 'resolves to' : 'does not resolve to'} form ${action.target}` };
  }

  await locator.click();
  const dialog = page.getByRole('dialog').first();
  try {
    await dialog.waitFor({ state: 'visible', timeout: 3000 });
  } catch {
    return { pass: false, evidence: `${action.label} did not open an accessible dialog` };
  }
  const text = String(await dialog.innerText()).replace(/\s+/g, ' ').trim();
  const pass = text.toLowerCase().includes(action.target.toLowerCase());
  return { pass, evidence: `Dialog ${pass ? 'contains' : 'does not contain'} ${JSON.stringify(action.target)}` };
}

function modelToolSchema(rubric: LandingRubric) {
  const dimension = {
    type: 'object',
    properties: {
      score: { type: 'number', minimum: 0, maximum: 1 },
      evidence: { type: 'array', minItems: 1, items: { type: 'string' } },
      issues: { type: 'array', items: { type: 'string' } },
    },
    required: ['score', 'evidence', 'issues'],
    additionalProperties: false,
  };
  return {
    type: 'function',
    function: {
      name: 'record_landing_page_evaluation',
      description: 'Record one evidence-grounded landing page evaluation.',
      parameters: {
        type: 'object',
        properties: {
          summary: { type: 'string' },
          dimensions: {
            type: 'object',
            properties: Object.fromEntries(Object.keys(rubric.dimensions).map((name) => [name, dimension])),
            required: Object.keys(rubric.dimensions),
            additionalProperties: false,
          },
          blocking_issues: {
            type: 'array',
            items: {
              type: 'object',
              properties: { code: { type: 'string' }, evidence: { type: 'string' } },
              required: ['code', 'evidence'],
              additionalProperties: false,
            },
          },
          recommendations: {
            type: 'array',
            items: {
              type: 'object',
              properties: {
                priority: { type: 'string', enum: ['critical', 'high', 'medium', 'low'] },
                dimension: { type: 'string' },
                action: { type: 'string' },
              },
              required: ['priority', 'dimension', 'action'],
              additionalProperties: false,
            },
          },
        },
        required: ['summary', 'dimensions', 'blocking_issues', 'recommendations'],
        additionalProperties: false,
      },
    },
  };
}

function parseModelEvaluation(value: unknown, rubric: LandingRubric): ModelEvaluation {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('model evaluation must be an object');
  }
  const candidate = value as Partial<ModelEvaluation>;
  nonEmpty(candidate.summary, 'model summary');
  if (!candidate.dimensions || typeof candidate.dimensions !== 'object' || Array.isArray(candidate.dimensions)) {
    throw new Error('model evaluation dimensions must be an object');
  }
  const dimensions: Record<string, ModelDimension> = {};
  const rawDimensions = candidate.dimensions as Record<string, unknown>;
  for (const name of Object.keys(rubric.dimensions)) {
    const rawDimension = rawDimensions[name];
    if (!rawDimension || typeof rawDimension !== 'object' || Array.isArray(rawDimension)) {
      throw new Error(`model evaluation ${name} must be an object`);
    }
    const dimension = rawDimension as Partial<ModelDimension>;
    if (typeof dimension.score !== 'number' || !Number.isFinite(dimension.score) || dimension.score < 0 || dimension.score > 1) {
      throw new Error(`model evaluation ${name}.score must be between 0 and 1`);
    }
    stringArray(dimension.evidence, `model evaluation ${name}.evidence`);
    if (!Array.isArray(dimension.issues) || dimension.issues.some((entry) => typeof entry !== 'string')) {
      throw new Error(`model evaluation ${name}.issues must be a string array`);
    }
    dimensions[name] = { score: dimension.score, evidence: dimension.evidence, issues: dimension.issues };
  }
  if (!Array.isArray(candidate.blocking_issues)) throw new Error('model evaluation blocking_issues must be an array');
  const blockingIssues = candidate.blocking_issues.map((value, index) => {
    if (!value || typeof value !== 'object') throw new Error(`model blocking_issues.${index} must be an object`);
    const issue = value as Partial<{ code: string; evidence: string }>;
    nonEmpty(issue.code, `model blocking_issues.${index}.code`);
    nonEmpty(issue.evidence, `model blocking_issues.${index}.evidence`);
    return { code: issue.code, evidence: issue.evidence };
  });
  if (!Array.isArray(candidate.recommendations)) throw new Error('model evaluation recommendations must be an array');
  const recommendations = candidate.recommendations.map((value, index) => {
    if (!value || typeof value !== 'object') throw new Error(`model recommendations.${index} must be an object`);
    const recommendation = value as Partial<ModelEvaluation['recommendations'][number]>;
    if (!['critical', 'high', 'medium', 'low'].includes(String(recommendation.priority))) {
      throw new Error(`model recommendations.${index}.priority is invalid`);
    }
    nonEmpty(recommendation.dimension, `model recommendations.${index}.dimension`);
    nonEmpty(recommendation.action, `model recommendations.${index}.action`);
    return {
      priority: recommendation.priority as ModelEvaluation['recommendations'][number]['priority'],
      dimension: recommendation.dimension,
      action: recommendation.action,
    };
  });
  return {
    summary: candidate.summary,
    dimensions,
    blocking_issues: blockingIssues,
    recommendations,
  };
}

async function modelEvaluation(
  rubric: LandingRubric,
  brief: LandingBrief,
  audits: Record<string, ViewportAudit>,
  imagePaths: Array<{ label: string; path: string }>,
): Promise<RoutedModelEvaluation> {
  const endpoint = `${routerUrl(requiredEnvironment('STADO_MODEL_ROUTER_URL'))}/v1/chat/completions`;
  const token = requiredEnvironment('STADO_MODEL_ROUTER_TOKEN');
  if (/\s/.test(token)) throw new Error('STADO_MODEL_ROUTER_TOKEN must not contain whitespace');
  const model = requiredEnvironment('PROBIERZ_LANDING_VISION_MODEL');
  const maxTokens = Number(process.env.PROBIERZ_LANDING_MAX_OUTPUT_TOKENS || DEFAULT_MAX_OUTPUT_TOKENS);
  if (!Number.isInteger(maxTokens) || maxTokens < 800 || maxTokens > 8000) {
    throw new Error('PROBIERZ_LANDING_MAX_OUTPUT_TOKENS must be an integer between 800 and 8000');
  }
  const content: Array<Record<string, unknown>> = [
    {
      type: 'text',
      text: JSON.stringify({
        task: 'Evaluate this landing page against the approved brief and every rubric dimension.',
        approvedBrief: brief,
        dimensionCriteria: rubric.dimensions,
        deterministicBrowserEvidence: audits,
      }),
    },
  ];
  for (const image of imagePaths) {
    content.push({ type: 'text', text: image.label });
    content.push({
      type: 'image_url',
      image_url: { url: `data:image/png;base64,${(await readFile(image.path)).toString('base64')}` },
    });
  }
  const tool = modelToolSchema(rubric);
  const response = await fetch(endpoint, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      model,
      max_tokens: maxTokens,
      temperature: 0,
      messages: [
        {
          role: 'system',
          content: [
            'You are the release evaluator for a landing page.',
            ...rubric.modelInstructions,
            'Call record_landing_page_evaluation exactly once. Return no prose outside the tool call.',
          ].join('\n'),
        },
        { role: 'user', content },
      ],
      tools: [tool],
      tool_choice: { type: 'function', function: { name: 'record_landing_page_evaluation' } },
    }),
    signal: AbortSignal.timeout(MAX_ROUTER_MS),
  });
  const raw = await response.text();
  let decoded: unknown;
  try {
    decoded = JSON.parse(raw) as unknown;
  } catch {
    throw new Error(`model router returned non-JSON (${response.status})`);
  }
  if (!decoded || typeof decoded !== 'object' || Array.isArray(decoded)) {
    throw new Error(`model router returned an invalid response object (${response.status})`);
  }
  const payload = decoded as RouterPayload;
  if (response.status < HTTP_SUCCESS_MIN || response.status >= HTTP_SUCCESS_MAX) {
    const detail = typeof payload.error?.message === 'string' ? payload.error.message : 'request failed';
    throw new Error(`model router HTTP ${response.status}: ${detail.slice(0, 500)}`);
  }
  const calls = payload.choices?.[0]?.message?.tool_calls;
  const matching = Array.isArray(calls)
    ? calls.filter(
        (call) =>
          call?.type === 'function' &&
          call.function &&
          call.function.name === 'record_landing_page_evaluation' &&
          typeof call.function.arguments === 'string',
      )
    : [];
  if (matching.length !== 1) throw new Error('model router must return exactly one landing evaluation tool call');
  const toolArguments = matching[0].function?.arguments;
  if (typeof toolArguments !== 'string') throw new Error('model router returned missing landing evaluation arguments');
  let evaluationValue: unknown;
  try {
    evaluationValue = JSON.parse(toolArguments) as unknown;
  } catch {
    throw new Error('model router returned invalid landing evaluation arguments');
  }
  const evaluation = parseModelEvaluation(evaluationValue, rubric);
  return {
    evaluation,
    routerModel: typeof payload.model === 'string' ? payload.model : null,
    usage: payload.usage || null,
  };
}

function buildReport(
  rubric: LandingRubric,
  briefPath: string,
  brief: LandingBrief,
  audits: Record<string, ViewportAudit>,
  conversion: { pass: boolean; evidence: string },
  model: RoutedModelEvaluation,
  images: Array<{ label: string; path: string }>,
) {
  const dimensions = Object.fromEntries(
    Object.entries(model.evaluation.dimensions).map(([name, result]) => [name, { ...result, adjustedScore: result.score }]),
  ) as Record<string, ModelDimension & { adjustedScore: number }>;
  const blockers: Array<{ code: string; evidence: string; source: 'browser' | 'model' | 'threshold' }> = [];
  const cap = (name: string, maximum: number, issue: string) => {
    const dimension = dimensions[name];
    if (!dimension) return;
    dimension.adjustedScore = Math.min(dimension.adjustedScore, maximum);
    if (!dimension.issues.includes(issue)) dimension.issues.push(issue);
  };
  const block = (code: string, evidence: string) => blockers.push({ code, evidence, source: 'browser' });

  for (const audit of Object.values(audits)) {
    if (audit.httpStatus < HTTP_SUCCESS_MIN || audit.httpStatus >= HTTP_SUCCESS_MAX) {
      block('http_status', `${audit.profile}: HTTP ${audit.httpStatus}`);
      cap('performance_stability', 0.2, `${audit.profile} did not receive a successful document response`);
    }
    if (audit.h1.length !== 1) {
      block('page_heading', `${audit.profile}: ${audit.h1.length} visible h1 elements`);
      cap('message_clarity', 0.45, `${audit.profile} must expose exactly one visible h1`);
      cap('accessibility_semantics', 0.45, `${audit.profile} heading contract is invalid`);
    }
    if (!audit.primaryActionMatches.some((entry) => entry.inFirstViewport)) {
      block('primary_action', `${audit.profile}: ${JSON.stringify(brief.primaryAction.label)} is not visible in the first viewport`);
      cap('hierarchy_conversion', 0.4, `${audit.profile} hides the primary action below the first viewport`);
      cap('conversion_continuity', 0.4, `${audit.profile} does not expose the approved action immediately`);
    }
    if (audit.horizontalOverflowPx > OVERFLOW_TOLERANCE_PX) {
      block('horizontal_overflow', `${audit.profile}: ${audit.horizontalOverflowPx}px`);
      cap('responsive_behavior', 0.4, `${audit.profile} overflows horizontally by ${audit.horizontalOverflowPx}px`);
    }
    if (audit.unnamedInteractiveCount > 0 || audit.unlabeledFormControlCount > 0) {
      block(
        'accessible_controls',
        `${audit.profile}: ${audit.unnamedInteractiveCount} unnamed interactive controls, ${audit.unlabeledFormControlCount} unlabeled form controls`,
      );
      cap('accessibility_semantics', 0.45, `${audit.profile} contains controls without accessible names or labels`);
    }
    if (audit.placeholderText.length > 0) {
      block('placeholder_content', `${audit.profile}: ${audit.placeholderText.join(', ')}`);
      cap('product_truth', 0.4, `${audit.profile} contains visible placeholder content`);
    }
    if (audit.imagesMissingAltCount > 0) {
      cap('accessibility_semantics', 0.55, `${audit.profile} has ${audit.imagesMissingAltCount} informative images without alt attributes`);
    }
    if (audit.headingLevelSkips > 0 || audit.duplicateIdCount > 0) {
      cap(
        'accessibility_semantics',
        0.65,
        `${audit.profile} has ${audit.headingLevelSkips} heading-level skips and ${audit.duplicateIdCount} duplicate ids`,
      );
    }
    if (audit.cumulativeLayoutShift > 0.25 || audit.failedRequests.length > 0 || audit.consoleErrors.length > 0) {
      cap(
        'performance_stability',
        0.55,
        `${audit.profile}: CLS ${audit.cumulativeLayoutShift.toFixed(3)}, ${audit.failedRequests.length} failed requests, ${audit.consoleErrors.length} console errors`,
      );
    }
  }
  if (!conversion.pass) {
    block('conversion_target', conversion.evidence);
    cap('conversion_continuity', 0.3, conversion.evidence);
  }
  for (const issue of model.evaluation.blocking_issues) {
    blockers.push({ code: issue.code, evidence: issue.evidence, source: 'model' });
  }

  const overall = Number(
    Object.entries(rubric.dimensions)
      .reduce((sum, [name, rule]) => sum + dimensions[name].adjustedScore * rule.weight, 0)
      .toFixed(4),
  );
  for (const [name, rule] of Object.entries(rubric.dimensions)) {
    if (dimensions[name].adjustedScore < rule.minimum) {
      blockers.push({
        code: `dimension_below_minimum:${name}`,
        evidence: `${dimensions[name].adjustedScore.toFixed(3)} < ${rule.minimum.toFixed(3)}`,
        source: 'threshold',
      });
    }
  }
  if (overall < rubric.overallMinimum) {
    blockers.push({
      code: 'overall_below_minimum',
      evidence: `${overall.toFixed(3)} < ${rubric.overallMinimum.toFixed(3)}`,
      source: 'threshold',
    });
  }

  return {
    schemaVersion: 1,
    rubric: { name: rubric.name, overallMinimum: rubric.overallMinimum, dimensions: rubric.dimensions },
    brief: {
      path: briefPath,
      product: brief.product,
      audience: brief.audience,
      promise: brief.promise,
      primaryAction: brief.primaryAction,
      analyticsOwner: brief.analyticsOwner,
    },
    pass: blockers.length === 0,
    overall,
    summary: model.evaluation.summary,
    dimensions,
    blockers,
    recommendations: model.evaluation.recommendations,
    conversion,
    audits,
    captures: images,
    router: { model: model.routerModel, usage: model.usage, attempts: 1 },
  };
}

test.describe('landing page release evaluation', () => {
  test.describe.configure({ retries: 0, mode: 'serial' });

  test('desktop, tablet, and mobile evidence satisfy the release rubric', async ({ browser }, testInfo) => {
    test.skip(testInfo.project.name !== 'chromium', 'Landing evaluation owns one bounded Chromium run and one model call');
    test.setTimeout(180_000);

    const url = targetUrl(requiredEnvironment('BASE_URL'));
    const { path: briefPath, brief } = await loadBrief();
    const rubric = await loadRubric();
    let desktop: CaptureResult | undefined;
    let tablet: CaptureResult | undefined;
    let mobile: CaptureResult | undefined;
    try {
      desktop = await captureViewport(browser, url, 'desktop', brief.primaryAction.label, testInfo);
      tablet = await captureViewport(browser, url, 'tablet', brief.primaryAction.label, testInfo);
      mobile = await captureViewport(browser, url, 'mobile', brief.primaryAction.label, testInfo);
      const conversion = await verifyPrimaryAction(desktop.page, brief.primaryAction);
      const images = [
        { label: 'Desktop first viewport, 1440 by 1000 CSS pixels', path: desktop.heroPath },
        { label: 'Desktop proof section near 58 percent of the scroll range', path: desktop.proofPath },
        { label: 'Tablet first viewport, 768 by 1024 CSS pixels', path: tablet.heroPath },
        { label: 'Tablet proof section near 58 percent of the scroll range', path: tablet.proofPath },
        { label: 'Mobile first viewport, 390 by 844 CSS pixels', path: mobile.heroPath },
        { label: 'Mobile proof section near 58 percent of the scroll range', path: mobile.proofPath },
      ];
      const audits = { desktop: desktop.audit, tablet: tablet.audit, mobile: mobile.audit };
      await desktop.context.close();
      await tablet.context.close();
      await mobile.context.close();
      desktop = undefined;
      tablet = undefined;
      mobile = undefined;

      const routedEvaluation = await modelEvaluation(rubric, brief, audits, images);
      const report = buildReport(rubric, briefPath, brief, audits, conversion, routedEvaluation, images);
      const reportPath = testInfo.outputPath('landing-page-evaluation.json');
      await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
      await Promise.all([
        testInfo.attach('desktop-hero', { path: images[0].path, contentType: 'image/png' }),
        testInfo.attach('desktop-proof', { path: images[1].path, contentType: 'image/png' }),
        testInfo.attach('tablet-hero', { path: images[2].path, contentType: 'image/png' }),
        testInfo.attach('tablet-proof', { path: images[3].path, contentType: 'image/png' }),
        testInfo.attach('mobile-hero', { path: images[4].path, contentType: 'image/png' }),
        testInfo.attach('mobile-proof', { path: images[5].path, contentType: 'image/png' }),
        testInfo.attach('landing-page-evaluation', { path: reportPath, contentType: 'application/json' }),
      ]);
      console.log(
        JSON.stringify(
          {
            reportPath,
            pass: report.pass,
            overall: report.overall,
            blockers: report.blockers,
            router: report.router,
          },
          null,
          2,
        ),
      );
      expect(
        report.pass,
        JSON.stringify(
          {
            overall: report.overall,
            required: rubric.overallMinimum,
            blockers: report.blockers,
            dimensions: report.dimensions,
          },
          null,
          2,
        ),
      ).toBe(true);
    } finally {
      await desktop?.context.close().catch(() => {});
      await tablet?.context.close().catch(() => {});
      await mobile?.context.close().catch(() => {});
    }
  });
});

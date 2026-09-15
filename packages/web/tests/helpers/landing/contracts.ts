/**
 * What the landing page release evaluation reads and writes.
 *
 * The brief is what the page promised to be, the rubric is what it is graded
 * against, an audit is one viewport as a visitor would see it, and the model
 * evaluation is the routed verdict. The evaluation itself lives in
 * `tests/landing-page.spec.ts`; the parts beside this file do the reading,
 * the looking, the asking and the writing.
 */

import { type BrowserContext, type Page } from '@playwright/test';
import { resolve } from 'node:path';

export const REPO_ROOT = resolve(process.cwd(), '../..');
export const RUBRIC_PATH = resolve(REPO_ROOT, 'apps/landing-page/rubric.json');
export const HTTP_SUCCESS_MIN = 200;
export const HTTP_SUCCESS_MAX = 400;
export const MAX_ROUTER_MS = 120_000;
export const DEFAULT_MAX_OUTPUT_TOKENS = 2400;
export const OVERFLOW_TOLERANCE_PX = 2;
export const CAPTURE_QUALITY = 65;
export const CAPTURE_CONTENT_TYPE = 'image/jpeg';

export interface PrimaryAction {
  label: string;
  kind: 'url' | 'dialog' | 'form';
  target: string;
}

export interface LandingBrief {
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

export interface DimensionRule {
  label: string;
  weight: number;
  minimum: number;
  criterion: string;
}

export interface LandingRubric {
  schemaVersion: 1;
  name: string;
  overallMinimum: number;
  dimensions: Record<string, DimensionRule>;
  deterministicGates: Record<string, string>;
  modelInstructions: string[];
}

export interface ViewportAudit {
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

export interface CaptureResult {
  context: BrowserContext;
  page: Page;
  audit: ViewportAudit;
  heroPath: string;
  proofPath: string;
}

export interface ModelDimension {
  score: number;
  evidence: string[];
  issues: string[];
}

export interface ModelEvaluation {
  summary: string;
  dimensions: Record<string, ModelDimension>;
  blocking_issues: Array<{ code: string; evidence: string }>;
  recommendations: Array<{
    priority: 'critical' | 'high' | 'medium' | 'low';
    dimension: string;
    action: string;
  }>;
}

export interface RoutedModelEvaluation {
  evaluation: ModelEvaluation;
  routerModel: string | null;
  usage: unknown;
}

export interface RouterToolCall {
  type?: unknown;
  function?: {
    name?: unknown;
    arguments?: unknown;
  };
}

export interface RouterPayload {
  model?: unknown;
  usage?: unknown;
  error?: { message?: unknown };
  choices?: Array<{
    message?: {
      tool_calls?: RouterToolCall[];
    };
  }>;
}


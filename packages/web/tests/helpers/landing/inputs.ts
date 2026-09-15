/**
 * Everything the evaluation is given, read and checked before a browser is
 * opened: the environment it needs, the approved brief, the rubric, and the
 * two addresses it talks to. A missing or malformed field is refused by name
 * here, so a run never reaches the model with half its inputs.
 */

import { expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import {
  type DimensionRule,
  type LandingBrief,
  type LandingRubric,
  REPO_ROOT,
  RUBRIC_PATH,
} from './contracts';

export function requiredEnvironment(name: string): string {
  const value = String(process.env[name] || '').trim();
  if (!value) throw new Error(`${name} is required`);
  return value;
}

export function nonEmpty(value: unknown, field: string): asserts value is string {
  if (typeof value !== 'string' || !value.trim()) throw new Error(`${field} is required`);
}

export function stringArray(value: unknown, field: string): asserts value is string[] {
  if (!Array.isArray(value) || value.length === 0 || value.some((entry) => typeof entry !== 'string' || !entry.trim())) {
    throw new Error(`${field} must be a non-empty string array`);
  }
}

export async function readJson(path: string): Promise<unknown> {
  try {
    return JSON.parse(await readFile(path, 'utf8')) as unknown;
  } catch (error) {
    throw new Error(`cannot read JSON ${path}: ${error instanceof Error ? error.message : String(error)}`);
  }
}

export async function loadBrief(): Promise<{ path: string; brief: LandingBrief }> {
  const path = resolve(REPO_ROOT, requiredEnvironment('PROBIERZ_LANDING_BRIEF'));
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

export async function loadRubric(): Promise<LandingRubric> {
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

export function targetUrl(value: string): string {
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

export function routerUrl(value: string): string {
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


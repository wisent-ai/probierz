/**
 * Asking the routed model what it thinks of the page.
 *
 * The tool shape is built from the rubric, so a dimension added to the rubric
 * is a dimension the model must answer for. The request goes through Brama
 * with the captured screenshots attached, and the reply is read strictly: an
 * answer missing a graded dimension, or scoring one outside its range, is a
 * failed evaluation rather than a pass with a hole in it.
 */

import { createHash, createHmac } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import {
  CAPTURE_CONTENT_TYPE,
  DEFAULT_MAX_OUTPUT_TOKENS,
  HTTP_SUCCESS_MAX,
  HTTP_SUCCESS_MIN,
  type LandingBrief,
  type LandingRubric,
  MAX_ROUTER_MS,
  type ModelDimension,
  type ModelEvaluation,
  type RoutedModelEvaluation,
  type RouterPayload,
  type ViewportAudit,
} from './contracts';
import { nonEmpty, requiredEnvironment, routerUrl, stringArray } from './inputs';

export function modelToolSchema(rubric: LandingRubric) {
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

export function parseModelEvaluation(value: unknown, rubric: LandingRubric): ModelEvaluation {
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

export async function modelEvaluation(
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
      image_url: { url: `data:${CAPTURE_CONTENT_TYPE};base64,${(await readFile(image.path)).toString('base64')}` },
    });
  }
  const tool = modelToolSchema(rubric);
  const body = JSON.stringify({
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
    tool_choice: {
      type: 'function',
      function: { name: 'record_landing_page_evaluation' },
    },
  });
  const agentId = requiredEnvironment('PROBIERZ_MODEL_AGENT_ID');
  const agentSecret = requiredEnvironment('PROBIERZ_MODEL_AGENT_SECRET');
  const timestamp = String(Math.floor(Date.now() / 1000));
  const digest = createHash('sha256').update(body).digest('hex');
  const signature = createHmac('sha256', agentSecret)
    .update(`${agentId}:${timestamp}:${digest}`)
    .digest('hex');
  const response = await fetch(endpoint, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${token}`,
      'Content-Type': 'application/json',
      'x-agent-id': agentId,
      'x-agent-timestamp': timestamp,
      'x-agent-signature': signature,
    },
    body,
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


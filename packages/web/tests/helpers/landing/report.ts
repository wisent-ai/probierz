/**
 * The report a person reads afterwards.
 *
 * It states what the page was judged against, what was measured at every
 * viewport, what the model said about each graded dimension, and the verdict
 * that follows — so a failure can be understood without re-running anything.
 */

import {
  HTTP_SUCCESS_MAX,
  HTTP_SUCCESS_MIN,
  type LandingBrief,
  type LandingRubric,
  type ModelDimension,
  OVERFLOW_TOLERANCE_PX,
  type RoutedModelEvaluation,
  type ViewportAudit,
} from './contracts';

export function buildReport(
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


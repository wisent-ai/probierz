import { expect, test } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import { CAPTURE_CONTENT_TYPE, type CaptureResult, type RoutedModelEvaluation } from './helpers/landing/contracts';
import { captureViewport, verifyPrimaryAction } from './helpers/landing/capture';
import { loadBrief, loadRubric, requiredEnvironment, targetUrl } from './helpers/landing/inputs';
import { modelEvaluation } from './helpers/landing/judge';
import { buildReport } from './helpers/landing/report';

/**
 * The landing page release evaluation: does the page that is about to ship
 * still do what its approved brief promised?
 *
 * The run reads the brief and the rubric, looks at the page at every viewport
 * in the rubric, checks the promised action really goes somewhere, asks the
 * routed model to grade what it sees, and writes a report a person can read.
 * The parts it is made of sit in `tests/helpers/landing/`: `contracts.ts` for
 * the shapes, `inputs.ts` for what the run is given, `capture.ts` for the
 * looking, `judge.ts` for the model's verdict, `report.ts` for the write-up.
 */
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

      let routedEvaluation: RoutedModelEvaluation;
      let modelFailure: string | undefined;
      try {
        routedEvaluation = await modelEvaluation(rubric, brief, audits, images);
      } catch (cause) {
        // The viewport captures, audits and conversion evidence are already
        // collected. Throwing here discards them, which turns an outage of the
        // vision route into "no evidence at all". Record the failure as the
        // report's state and still fail the test: a page cannot pass a release
        // evaluation its grader never saw, but the deterministic evidence must
        // survive to be reviewed.
        modelFailure = cause instanceof Error ? cause.message : String(cause);
        routedEvaluation = {
          routerModel: process.env.PROBIERZ_LANDING_VISION_MODEL ?? 'unknown',
          usage: null,
          evaluation: {
            summary: `model evaluation unavailable: ${modelFailure}`,
            // No grader saw the page, so no dimension earned a score; zeroing
            // them keeps the threshold blockers honest instead of inventing a
            // verdict the model never produced.
            dimensions: Object.fromEntries(
              Object.keys(rubric.dimensions).map((name) => [
                name,
                { score: 0, evidence: ['model evaluation unavailable'], issues: [] },
              ]),
            ),
            blocking_issues: [],
            recommendations: [],
          },
        };
      }
      const report = buildReport(rubric, briefPath, brief, audits, conversion, routedEvaluation, images);
      if (modelFailure) {
        (report as unknown as Record<string, unknown>).modelEvaluationFailed = modelFailure;
        report.blockers.push({
          code: 'model_evaluation_unavailable',
          evidence: modelFailure,
          source: 'model',
        });
        (report as unknown as Record<string, unknown>).pass = false;
      }
      const reportPath = testInfo.outputPath('landing-page-evaluation.json');
      await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
      await Promise.all([
        testInfo.attach('desktop-hero', { path: images[0].path, contentType: CAPTURE_CONTENT_TYPE }),
        testInfo.attach('desktop-proof', { path: images[1].path, contentType: CAPTURE_CONTENT_TYPE }),
        testInfo.attach('tablet-hero', { path: images[2].path, contentType: CAPTURE_CONTENT_TYPE }),
        testInfo.attach('tablet-proof', { path: images[3].path, contentType: CAPTURE_CONTENT_TYPE }),
        testInfo.attach('mobile-hero', { path: images[4].path, contentType: CAPTURE_CONTENT_TYPE }),
        testInfo.attach('mobile-proof', { path: images[5].path, contentType: CAPTURE_CONTENT_TYPE }),
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

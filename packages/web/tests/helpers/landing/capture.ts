/**
 * Looking at the page the way a visitor does.
 *
 * One viewport at a time: open it, let it settle, measure what is on screen —
 * overflow, unreadable contrast, placeholder text, the console — keep the
 * image as run evidence, and then check that the action the brief promises
 * actually goes somewhere. Everything measured here is deterministic; the
 * judgement of taste is asked of the model in `judge.ts`.
 */

import { type Browser, type Page, type TestInfo } from '@playwright/test';
import { CAPTURE_QUALITY, type CaptureResult, type PrimaryAction } from './contracts';

export async function captureViewport(
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

    const heroPath = testInfo.outputPath(`${profile}-hero.jpg`);
    const proofPath = testInfo.outputPath(`${profile}-proof.jpg`);
    await page.screenshot({ path: heroPath, type: 'jpeg', quality: CAPTURE_QUALITY, fullPage: false });
    const proofY = await page.evaluate(() => {
      const range = Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
      return Math.round(range * 0.58);
    });
    await page.evaluate((y) => window.scrollTo({ top: y, behavior: 'instant' }), proofY);
    await page.waitForTimeout(150);
    await page.screenshot({ path: proofPath, type: 'jpeg', quality: CAPTURE_QUALITY, fullPage: false });
    await page.evaluate(() => window.scrollTo({ top: 0, behavior: 'instant' }));
    return { context, page, audit, heroPath, proofPath };
  } catch (error) {
    await context.close().catch(() => {});
    throw error;
  }
}

export async function verifyPrimaryAction(page: Page, action: PrimaryAction): Promise<{ pass: boolean; evidence: string }> {
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


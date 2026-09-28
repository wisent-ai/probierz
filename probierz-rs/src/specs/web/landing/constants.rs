//! The numbers the landing page release evaluation is held to. These
//! constants are the evaluation's contract: which document statuses count as
//! served, how much sideways overflow is measurement noise, how the captures
//! are encoded, how long and how much the routed model may answer, and where
//! each deterministic finding caps the model's score for a dimension.

/// The brief, rubric and report schema revision this evaluation reads and writes.
pub(super) const SCHEMA_VERSION: u64 = 1;
/// A document answered with a status in [200, 400) was served.
pub(super) const HTTP_SERVED: std::ops::Range<u64> = 200..400;
/// Sideways overflow up to two CSS pixels is sub-pixel rounding, not a defect.
pub(super) const OVERFLOW_TOLERANCE_PX: f64 = 2.0;
/// JPEG quality of the six captures the model is shown.
pub(super) const CAPTURE_QUALITY: u8 = 65;
pub(super) const CAPTURE_TYPE: &str = "image/jpeg";
/// The routed model call gets two minutes.
pub(super) const ROUTER_BUDGET_SECONDS: u64 = 120;
/// Output tokens the model may spend, unless PROBIERZ_LANDING_MAX_OUTPUT_TOKENS says otherwise within the bounds.
pub(super) const MAX_OUTPUT_TOKENS: u64 = 2400;
pub(super) const MAX_OUTPUT_TOKENS_BOUNDS: std::ops::RangeInclusive<u64> = 800..=8000;
/// A layout shift above this is a stability finding.
pub(super) const LAYOUT_SHIFT_LIMIT: f64 = 0.25;
/// The proof capture is taken this far down the scroll range.
pub(super) const PROOF_SCROLL_SHARE: f64 = 0.58;

/// Score caps a deterministic finding places on a dimension.
pub(super) const CAP_NOT_SERVED: f64 = 0.2;
pub(super) const CAP_HEADING: f64 = 0.45;
pub(super) const CAP_HIDDEN_ACTION: f64 = 0.4;
pub(super) const CAP_OVERFLOW: f64 = 0.4;
pub(super) const CAP_UNNAMED_CONTROLS: f64 = 0.45;
pub(super) const CAP_MISSING_ALT: f64 = 0.55;
pub(super) const CAP_STRUCTURE: f64 = 0.65;
pub(super) const CAP_UNSTABLE: f64 = 0.55;
pub(super) const CAP_CONVERSION: f64 = 0.3;
/// A dimension no grader saw (the model route failed) scores nothing.
pub(super) const UNSEEN_SCORE: f64 = 0.0;
/// One model call per evaluation; the report states it.
pub(super) const ROUTER_ATTEMPTS: u8 = 1;
/// The overall score is reported to four decimal places.
pub(super) const SCORE_PRECISION: f64 = 10_000.0;
/// The routed model answers deterministically.
pub(super) const TEMPERATURE: u8 = 0;
/// The JSON schema of one graded dimension in the model's tool call: a
/// score in [0, 1], at least one piece of evidence, and its issues.
pub(super) const DIMENSION_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "score": { "type": "number", "minimum": 0, "maximum": 1 },
    "evidence": { "type": "array", "minItems": 1, "items": { "type": "string" } },
    "issues": { "type": "array", "items": { "type": "string" } }
  },
  "required": ["score", "evidence", "issues"],
  "additionalProperties": false
}"#;
/// Rubric weights must sum to one within this.
pub(super) const WEIGHT_TOLERANCE: f64 = 0.000_001;

/// The viewport sizes (CSS pixels) of the three profiles a page is shown at.
pub(super) const DESKTOP: (u32, u32) = (1440, 1000);
pub(super) const TABLET: (u32, u32) = (768, 1024);
pub(super) const MOBILE: (u32, u32) = (390, 844);

/// Installed before the page's own scripts: sums the layout shifts no input caused.
pub(super) const LAYOUT_SHIFT_OBSERVER: &str = r#"window.__probierzCls = 0;
new PerformanceObserver((list) => {
  for (const entry of list.getEntries()) {
    if (!entry.hadRecentInput) window.__probierzCls += Number(entry.value);
  }
}).observe({ type: 'layout-shift', buffered: true });"#;

/// One viewport measured as a visitor meets it; called with the primary
/// action label, the profile, the document status and the console errors
/// and failed requests Weles recorded. Whether the copy is finished is the
/// model's judgement (product_truth), not a word list here.
pub(super) const AUDIT: &str = r#"(({ label, profileName, status, capturedConsoleErrors, capturedFailedRequests }) => {
  const normalizedLabel = label.replace(/\s+/g, ' ').trim().toLowerCase();
  const visible = (element) => {
    if (!(element instanceof HTMLElement)) return false;
    const rect = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    return rect.width > 0 && rect.height > 0 && style.display !== 'none' && style.visibility !== 'hidden';
  };
  const clean = (value) => (typeof value === 'string' ? value : '').replace(/\s+/g, ' ').trim();
  const attribute = (element, name) => clean(element.getAttribute(name));
  const accessibleName = (element) => {
    const labelledBy = attribute(element, 'aria-labelledby');
    if (labelledBy) {
      const text = labelledBy.split(/\s+/).map((id) => document.getElementById(id))
        .filter(Boolean).map((node) => clean(node.textContent)).filter(Boolean).join(' ');
      if (text) return text;
    }
    const inputValue = element instanceof HTMLInputElement && ['button', 'reset', 'submit'].includes(element.type) ? element.value : '';
    const image = element.querySelector('img[alt]');
    const innerText = element.matches('input,select,textarea') ? '' : element.innerText;
    const names = [attribute(element, 'aria-label'), attribute(element, 'alt'), attribute(element, 'title'),
      clean(inputValue), clean(innerText), image ? clean(image.alt) : ''].filter(Boolean);
    return names.length ? names[0] : '';
  };
  const interactive = [...document.querySelectorAll('a,button,[role="button"],input,select,textarea')].filter(visible);
  const ctas = interactive.map((element) => {
    const anchor = element.closest('a');
    const rect = element.getBoundingClientRect();
    return {
      tag: element.tagName.toLowerCase(),
      name: accessibleName(element),
      href: anchor ? anchor.href : null,
      inFirstViewport: rect.bottom > 0 && rect.top < window.innerHeight && rect.right > 0 && rect.left < window.innerWidth,
    };
  }).filter((entry) => clean(entry.name).toLowerCase() === normalizedLabel);
  const controls = [...document.querySelectorAll('input:not([type="hidden"]),select,textarea')].filter(visible);
  const controlLabelled = (element) => Boolean(accessibleName(element))
    || Boolean(element.id && document.querySelector(`label[for="${CSS.escape(element.id)}"]`)) || Boolean(element.closest('label'));
  const headings = [...document.querySelectorAll('h1,h2,h3,h4,h5,h6')].filter(visible)
    .map((element) => ({ level: Number(element.tagName.slice(1)), text: clean(element.innerText) })).filter((entry) => entry.text);
  let headingLevelSkips = 0;
  for (let index = 1; index < headings.length; index += 1) {
    if (headings[index].level > headings[index - 1].level + 1) headingLevelSkips += 1;
  }
  const ids = [...document.querySelectorAll('[id]')].map((element) => element.id).filter(Boolean);
  const images = [...document.querySelectorAll('img')].filter((image) => visible(image)
    && image.getAttribute('role') !== 'presentation' && image.getAttribute('aria-hidden') !== 'true');
  const nav = performance.getEntriesByType('navigation')[0];
  const timing = (name) => (nav ? Number(nav[name]) : 0);
  const description = document.querySelector('meta[name="description"]');
  return {
    profile: profileName, url: location.href, httpStatus: status, title: document.title,
    metaDescription: description ? description.content : '', lang: document.documentElement.lang,
    h1: headings.filter((entry) => entry.level === 1).map((entry) => entry.text), headings: headings.slice(0, 80), headingLevelSkips,
    primaryActionMatches: ctas, horizontalOverflowPx: Math.max(0, document.documentElement.scrollWidth - window.innerWidth),
    visibleInteractiveCount: interactive.length, unnamedInteractiveCount: interactive.filter((element) => !accessibleName(element)).length,
    visibleFormControlCount: controls.length, unlabeledFormControlCount: controls.filter((element) => !controlLabelled(element)).length,
    informativeImageCount: images.length, imagesMissingAltCount: images.filter((image) => !image.hasAttribute('alt')).length,
    duplicateIdCount: ids.length - new Set(ids).size,
    documentHeight: document.documentElement.scrollHeight, cumulativeLayoutShift: Number(window.__probierzCls),
    navigationTimingMs: { domContentLoaded: timing('domContentLoadedEventEnd'), load: timing('loadEventEnd'), responseEnd: timing('responseEnd') },
    consoleErrors: capturedConsoleErrors, failedRequests: capturedFailedRequests,
  };
})"#;

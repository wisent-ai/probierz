import { readFileSync } from "node:fs";

const HTTP_OK_MIN = 200;
const HTTP_OK_MAX = 400;
const REQUIRED_STRUCTURED_PROPERTIES = {
  Organization: ["name", "url"],
  WebSite: ["name", "url"],
  SoftwareApplication: ["name", "applicationCategory"],
  Product: ["name", "description"],
  Article: ["headline", "datePublished", "author"],
  BreadcrumbList: ["itemListElement"],
  FAQPage: ["mainEntity"],
};

function normalizedUrl(value) {
  if (!value) return "";
  try {
    const url = new URL(value);
    url.hash = "";
    if (url.pathname !== "/") url.pathname = url.pathname.replace(/\/+$/, "");
    return url.href;
  } catch {
    return "";
  }
}

function directiveSet(...values) {
  return new Set(values.flatMap((value) => String(value || "").toLowerCase().split(/[,;]/)).map((value) => value.trim().split(/\s+/)[0]).filter(Boolean));
}

function tokenSimilarity(left, right) {
  const tokens = (value) => new Set(String(value || "").toLowerCase().match(/[\p{L}\p{N}]+/gu) || []);
  const a = tokens(left);
  const b = tokens(right);
  if (!a.size && !b.size) return 1;
  const intersection = [...a].filter((token) => b.has(token)).length;
  const union = new Set([...a, ...b]).size;
  return union ? intersection / union : 0;
}

function structuredNodes(value) {
  if (Array.isArray(value)) return value.flatMap(structuredNodes);
  if (!value || typeof value !== "object") return [];
  const graph = Array.isArray(value["@graph"]) ? value["@graph"].flatMap(structuredNodes) : [];
  return [value, ...graph];
}

function parseStructuredData(page) {
  const nodes = [];
  const errors = [];
  for (const [index, raw] of (page.googlebot?.jsonLd || []).entries()) {
    try {
      nodes.push(...structuredNodes(JSON.parse(raw)));
    } catch (error) {
      errors.push(`JSON-LD block ${index + 1}: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  const types = new Set();
  const propertyErrors = [];
  for (const node of nodes) {
    const context = node["@context"];
    if (context && !String(context).toLowerCase().includes("schema.org")) propertyErrors.push(`non-Schema.org @context ${String(context)}`);
    const nodeTypes = Array.isArray(node["@type"]) ? node["@type"] : [node["@type"]];
    for (const type of nodeTypes.filter((item) => typeof item === "string" && item)) {
      types.add(type);
      for (const property of REQUIRED_STRUCTURED_PROPERTIES[type] || []) {
        if (node[property] === undefined || node[property] === null || node[property] === "") propertyErrors.push(`${type}.${property} is required`);
      }
    }
  }
  return { nodes, types: [...types].sort(), errors: [...errors, ...propertyErrors] };
}

function graphDepths(baseUrl, pages) {
  const pageByUrl = new Map(pages.map((page) => [normalizedUrl(page.url), page]));
  const depths = new Map([[normalizedUrl(baseUrl), 0]]);
  const queue = [normalizedUrl(baseUrl)];
  while (queue.length) {
    const current = queue.shift();
    const page = pageByUrl.get(current);
    if (!page) continue;
    for (const link of page.links || []) {
      const target = normalizedUrl(link);
      if (!pageByUrl.has(target) || depths.has(target)) continue;
      depths.set(target, depths.get(current) + 1);
      queue.push(target);
    }
  }
  return depths;
}

function scoreRatio(passed, total, empty = 1) {
  return total > 0 ? Math.max(0, Math.min(1, passed / total)) : empty;
}

function uniqueDefects(pages, field) {
  const byValue = new Map();
  for (const page of pages) {
    const value = String(page.googlebot?.[field] || "").trim().toLowerCase();
    if (!value) continue;
    byValue.set(value, [...(byValue.get(value) || []), page.url]);
  }
  return [...byValue.entries()].filter(([, urls]) => urls.length > 1).map(([value, urls]) => ({ value, urls }));
}

export function evaluateDeterministicSeo(contract, evidence) {
  const blockers = [];
  const warnings = [];
  const facts = [];
  const block = (code, url, evidenceText) => blockers.push({ code, url, evidence: evidenceText, source: "deterministic" });
  const warn = (code, url, evidenceText) => warnings.push({ code, url, evidence: evidenceText, source: "deterministic" });
  const pageByUrl = new Map(evidence.pages.map((page) => [normalizedUrl(page.url), page]));
  const depths = graphDepths(contract.baseUrl, evidence.pages);
  const sitemapUrls = new Set(evidence.sitemaps.urls.map(normalizedUrl));
  const indexablePages = [];
  let snippetChecks = 0;
  let snippetPasses = 0;
  let structuredChecks = 0;
  let structuredPasses = 0;
  let architectureChecks = 0;
  let architecturePasses = 0;
  let experienceChecks = 0;
  let experiencePasses = 0;

  if (evidence.coverage.truncated) block("crawl_truncated", contract.baseUrl, `crawl exceeded policy limits with ${evidence.coverage.uncrawled.length} queued URLs`);
  if (!evidence.sitemaps.documents.length || evidence.sitemaps.documents.some((document) => document.status < HTTP_OK_MIN || document.status >= HTTP_OK_MAX)) {
    block("sitemap_unavailable", evidence.robots.url, "the sitemap contract did not return successful sitemap documents");
  }

  for (const route of contract.routes) {
    const page = pageByUrl.get(normalizedUrl(route.url));
    if (!page) {
      block("declared_route_missing", route.url, "declared route was not crawled");
      continue;
    }
    const rendered = page.googlebot;
    const ordinary = page.ordinary;
    const headers = page.rawGooglebot?.headers || {};
    if (!rendered || page.error) {
      block("render_failed", route.url, page.error || "Googlebot render evidence is missing");
      continue;
    }
    if (rendered.httpStatus < HTTP_OK_MIN || rendered.httpStatus >= HTTP_OK_MAX) block("http_status", route.url, `Googlebot received HTTP ${rendered.httpStatus}`);
    if (page.rawGooglebot?.redirectLoopOrLimit) block("redirect_loop", route.url, "redirect chain exceeded the configured limit");
    if ((page.rawGooglebot?.redirects || []).length > 1) warn("redirect_chain", route.url, `${page.rawGooglebot.redirects.length} redirects precede the final document`);
    const soft404Text = `${rendered.title} ${rendered.h1.join(" ")} ${rendered.visibleText.slice(0, 500)}`;
    if (rendered.httpStatus >= HTTP_OK_MIN && rendered.httpStatus < HTTP_OK_MAX && /\b(?:404|not found|page (?:does not|doesn't) exist)\b/i.test(soft404Text)) {
      block("soft_404", route.url, "successful response renders an explicit missing-page message");
    }
    const directives = directiveSet(rendered.metaRobots, rendered.googlebotRobots, headers["x-robots-tag"]);
    const robotsAllows = evidence.robots.routeAccess[route.url] !== false;
    if (route.indexable) {
      indexablePages.push(page);
      if (!sitemapUrls.has(normalizedUrl(route.url))) block("sitemap_url_missing", route.url, "indexable route is absent from all collected sitemaps");
      if (!robotsAllows) block("robots_disallow", route.url, "robots.txt disallows a route declared indexable");
      if (directives.has("noindex") || directives.has("none")) block("noindex", route.url, "an indexable route emits a noindex directive");
      const expectedCanonical = normalizedUrl(route.url);
      const actualCanonical = normalizedUrl(rendered.canonical);
      if (!actualCanonical) block("canonical_missing", route.url, "indexable route has no valid canonical URL");
      else if (actualCanonical !== expectedCanonical) block("canonical_mismatch", route.url, `${actualCanonical} does not equal ${expectedCanonical}`);
      const canonicalPage = pageByUrl.get(actualCanonical);
      if (actualCanonical && !canonicalPage) block("canonical_not_crawled", route.url, `${actualCanonical} was not reachable within the crawl contract`);
      else if (canonicalPage?.googlebot && (canonicalPage.googlebot.httpStatus < HTTP_OK_MIN || canonicalPage.googlebot.httpStatus >= HTTP_OK_MAX)) {
        block("canonical_unavailable", route.url, `canonical returned HTTP ${canonicalPage.googlebot.httpStatus}`);
      }
    } else {
      if (sitemapUrls.has(normalizedUrl(route.url))) block("nonindexable_in_sitemap", route.url, "non-indexable route appears in a sitemap");
      if (!directives.has("noindex") && !directives.has("none")) block("unexpected_indexability", route.url, "route declared non-indexable does not emit noindex");
    }
    if (ordinary) {
      const parity = tokenSimilarity(ordinary.visibleText, rendered.visibleText);
      if (parity < 0.9) block("googlebot_content_mismatch", route.url, `ordinary/Googlebot visible-text similarity is ${parity.toFixed(3)}`);
      for (const field of ["title", "metaDescription", "canonical", "lang"]) {
        if (normalizedUrl(field === "canonical" ? ordinary[field] : "") !== normalizedUrl(field === "canonical" ? rendered[field] : "") && field === "canonical") {
          block("googlebot_metadata_mismatch", route.url, `${field} differs between ordinary Chrome and Googlebot`);
        } else if (field !== "canonical" && String(ordinary[field] || "").trim() !== String(rendered[field] || "").trim()) {
          block("googlebot_metadata_mismatch", route.url, `${field} differs between ordinary Chrome and Googlebot`);
        }
      }
    }
    if (rendered.lang.trim().toLowerCase().split("-")[0] !== route.locale.trim().toLowerCase().split("-")[0]) {
      block("language_mismatch", route.url, `document language ${rendered.lang || "(missing)"} does not match declared locale ${route.locale}`);
    }
    const headingSkips = rendered.headings.slice(1).filter((heading, index) => heading.level > rendered.headings[index].level + 1);
    const socialAssetsValid = (page.socialAssets || []).length > 0 && page.socialAssets.every((asset) => (
      asset.status >= HTTP_OK_MIN && asset.status < HTTP_OK_MAX && /^image\//i.test(asset.contentType || "")
    ));
    for (const [label, passed, detail] of [
      ["title", Boolean(rendered.title.trim()), "document title is missing"],
      ["description", Boolean(rendered.metaDescription.trim()), "meta description is missing"],
      ["h1", rendered.h1.length === 1, `${rendered.h1.length} visible h1 elements`],
      ["lang", Boolean(rendered.lang.trim()), "document language is missing"],
      ["viewport", Boolean(rendered.viewport.trim()), "viewport metadata is missing"],
      ["open-graph", Boolean(rendered.openGraph.title && rendered.openGraph.description && rendered.openGraph.image && rendered.openGraph.url), "Open Graph title, description, image, or URL is missing"],
      ["open-graph-url", normalizedUrl(rendered.openGraph.url) === normalizedUrl(route.url), `Open Graph URL ${rendered.openGraph.url || "(missing)"} does not match the declared route`],
      ["twitter-card", Boolean(rendered.twitter.card && rendered.twitter.title && rendered.twitter.description && rendered.twitter.image), "Twitter Card fields are incomplete"],
      ["share-assets", socialAssetsValid, "Open Graph or Twitter image is unavailable or is not an image response"],
      ["heading-order", headingSkips.length === 0, `${headingSkips.length} heading-level skips`],
      ["favicon", Boolean(rendered.favicon), "favicon link is missing"],
    ]) {
      snippetChecks += 1;
      if (passed) snippetPasses += 1;
      else warn(`search_appearance_${label}`, route.url, detail);
    }
    const structured = parseStructuredData(page);
    page.structuredData = { types: structured.types, errors: structured.errors };
    for (const requiredType of route.requiredStructuredData) {
      structuredChecks += 1;
      const node = structured.nodes.find((candidate) => {
        const types = Array.isArray(candidate["@type"]) ? candidate["@type"] : [candidate["@type"]];
        return types.includes(requiredType);
      });
      const propertyError = structured.errors.some((error) => error.startsWith(`${requiredType}.`));
      if (node && !propertyError) structuredPasses += 1;
      else block("structured_data_required", route.url, `${requiredType} is missing or invalid`);
      if (node?.name && !rendered.visibleText.toLowerCase().includes(String(node.name).toLowerCase())) {
        block("structured_data_not_visible", route.url, `${requiredType}.name is not present in visible page content`);
      }
      if (node?.url) {
        const structuredUrl = normalizedUrl(node.url);
        if (structuredUrl && !pageByUrl.has(structuredUrl)) block("structured_data_url_unavailable", route.url, `${requiredType}.url ${node.url} was not crawled`);
      }
    }
    for (const error of structured.errors) block("structured_data_invalid", route.url, error);
    structuredChecks += structured.errors.length;
    const routeDepth = depths.get(normalizedUrl(route.url));
    architectureChecks += 1;
    if (routeDepth !== undefined && routeDepth <= (route.maxClickDepth ?? contract.policy.crawl.maxDepth)) architecturePasses += 1;
    else block("orphan_or_deep_route", route.url, routeDepth === undefined ? "route is unreachable from the canonical root" : `click depth ${routeDepth} exceeds ${route.maxClickDepth}`);
    const performance = rendered.performance || {};
    for (const [metric, value, maximum] of [
      ["lcpMs", performance.lcpMs, contract.policy.performance.lcpMs],
      ["cls", performance.cls, contract.policy.performance.cls],
      ["tbtMs", performance.tbtMs, contract.policy.performance.tbtMs],
    ]) {
      experienceChecks += 1;
      const observed = Number.isFinite(value) && (metric !== "lcpMs" || value > 0);
      if (observed && value <= maximum) experiencePasses += 1;
      else warn(`performance_${metric}`, route.url, observed ? `${metric} ${Number(value).toFixed(3)} exceeds ${maximum}` : `${metric} was not observed`);
    }
    experienceChecks += 2;
    if ((rendered.consoleErrors || []).length === 0) experiencePasses += 1;
    else warn("console_errors", route.url, `${rendered.consoleErrors.length} console errors`);
    if ((rendered.failedRequests || []).length === 0) experiencePasses += 1;
    else warn("failed_requests", route.url, `${rendered.failedRequests.length} failed requests`);
  }

  for (const page of evidence.pages) {
    const normalized = normalizedUrl(page.url);
    const status = page.googlebot?.httpStatus || page.rawGooglebot?.status || 0;
    const declared = contract.routes.some((route) => normalizedUrl(route.url) === normalized);
    architectureChecks += 1;
    if (status >= HTTP_OK_MIN && status < HTTP_OK_MAX) architecturePasses += 1;
    else if ((page.links || []).length || sitemapUrls.has(normalized) || declared) block("broken_internal_document", page.url, `internal document returned HTTP ${status || "unavailable"}`);
    if (!declared && page.googlebot && status >= HTTP_OK_MIN && status < HTTP_OK_MAX) {
      const headers = page.rawGooglebot?.headers || {};
      const directives = directiveSet(page.googlebot.metaRobots, page.googlebot.googlebotRobots, headers["x-robots-tag"]);
      const allowed = evidence.robots.pageAccess?.[page.url] !== false;
      if (sitemapUrls.has(normalized) && !allowed) block("sitemap_robots_disallow", page.url, "sitemap URL is disallowed by robots.txt");
      if (sitemapUrls.has(normalized) && (directives.has("noindex") || directives.has("none"))) block("sitemap_noindex", page.url, "sitemap URL emits noindex");
      const soft404Text = `${page.googlebot.title} ${page.googlebot.h1.join(" ")} ${page.googlebot.visibleText.slice(0, 500)}`;
      if (/\b(?:404|not found|page (?:does not|doesn't) exist)\b/i.test(soft404Text)) block("soft_404", page.url, "successful response renders an explicit missing-page message");
      if (allowed && !directives.has("noindex") && !directives.has("none")) {
        indexablePages.push(page);
        const canonical = normalizedUrl(page.googlebot.canonical);
        if (!sitemapUrls.has(normalized)) block("discovered_indexable_not_in_sitemap", page.url, "crawl-discovered indexable page is absent from all collected sitemaps");
        if (!canonical) block("canonical_missing", page.url, "crawl-discovered indexable page has no canonical URL");
        else if (!pageByUrl.has(canonical)) block("canonical_not_crawled", page.url, `${canonical} was not reachable within the crawl contract`);
        for (const [label, passed, detail] of [
          ["title", Boolean(page.googlebot.title.trim()), "document title is missing"],
          ["description", Boolean(page.googlebot.metaDescription.trim()), "meta description is missing"],
          ["h1", page.googlebot.h1.length === 1, `${page.googlebot.h1.length} visible h1 elements`],
          ["lang", Boolean(page.googlebot.lang.trim()), "document language is missing"],
        ]) {
          snippetChecks += 1;
          if (passed) snippetPasses += 1;
          else warn(`search_appearance_${label}`, page.url, detail);
        }
        const pageDepth = depths.get(normalized);
        architectureChecks += 1;
        if (pageDepth !== undefined && pageDepth <= contract.policy.crawl.maxDepth) architecturePasses += 1;
        else block("orphan_or_deep_route", page.url, pageDepth === undefined ? "page is unreachable from the canonical root" : `click depth ${pageDepth} exceeds ${contract.policy.crawl.maxDepth}`);
        const structured = parseStructuredData(page);
        page.structuredData = { types: structured.types, errors: structured.errors };
        for (const error of structured.errors) block("structured_data_invalid", page.url, error);
        structuredChecks += structured.errors.length;
      }
    }
  }
  for (const [field, code] of [["title", "duplicate_title"], ["metaDescription", "duplicate_description"], ["visibleTextSha256", "duplicate_content"]]) {
    for (const duplicate of uniqueDefects(indexablePages, field)) {
      warn(code, duplicate.urls[0], `${duplicate.urls.length} indexable pages share ${field}: ${duplicate.urls.join(", ")}`);
      snippetChecks += duplicate.urls.length;
    }
  }

  for (const page of indexablePages) {
    for (const alternate of page.googlebot?.hreflang || []) {
      const target = pageByUrl.get(normalizedUrl(alternate.href));
      if (!target) {
        block("hreflang_target_missing", page.url, `${alternate.lang} target ${alternate.href} was not crawled`);
        continue;
      }
      const reciprocal = (target.googlebot?.hreflang || []).some((entry) => normalizedUrl(entry.href) === normalizedUrl(page.url));
      if (!reciprocal) block("hreflang_not_reciprocal", page.url, `${alternate.lang} target ${alternate.href} has no reciprocal link`);
    }
  }

  const deterministicDimensions = {
    snippet_quality: { score: Number(scoreRatio(snippetPasses, snippetChecks).toFixed(4)), evidence: [`${snippetPasses}/${snippetChecks} search appearance checks passed`] },
    structured_data_quality: { score: Number(scoreRatio(structuredPasses, structuredChecks).toFixed(4)), evidence: [`${structuredPasses}/${structuredChecks} structured-data checks passed`] },
    information_architecture: { score: Number(scoreRatio(architecturePasses, architectureChecks).toFixed(4)), evidence: [`${architecturePasses}/${architectureChecks} crawl graph checks passed`] },
    experience: { score: Number(scoreRatio(experiencePasses, experienceChecks).toFixed(4)), evidence: [`${experiencePasses}/${experienceChecks} mobile experience checks passed`] },
  };
  facts.push({ code: "coverage", evidence: `${evidence.coverage.crawled} pages crawled; ${evidence.sitemaps.urls.length} sitemap URLs; ${contract.routes.length} declared routes` });
  return { blockers, warnings, facts, dimensions: deterministicDimensions, graph: { depths: Object.fromEntries(depths) } };
}

export function loadProductionSeoEvidence(file, contract) {
  const required = Boolean(contract.manifest.seo?.profiles?.[contract.mode]?.requireProductionEvidence);
  if (!file) return { status: "not-provided", required, blockers: [] };
  let raw;
  try {
    raw = JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    throw new Error(`cannot read production SEO evidence ${file}: ${error instanceof Error ? error.message : String(error)}`);
  }
  if (!raw || typeof raw !== "object" || raw.schemaVersion !== 1 || typeof raw.collectedAt !== "string" || raw.source !== "google-search-console+crux" || typeof raw.property !== "string" || !raw.property.trim() || !Array.isArray(raw.urls)) {
    throw new Error("production SEO evidence must declare schemaVersion 1, source google-search-console+crux, property, collectedAt, and urls");
  }
  const ageHours = (Date.now() - Date.parse(raw.collectedAt)) / 3600000;
  const blockers = [];
  if (!Number.isFinite(ageHours) || ageHours < 0 || ageHours > contract.policy.production.maxAgeHours) blockers.push({ code: "production_evidence_stale", evidence: `production evidence age ${ageHours.toFixed(1)}h exceeds ${contract.policy.production.maxAgeHours}h`, source: "production" });
  for (const route of contract.routes.filter((item) => item.indexable)) {
    const item = (raw.urls || []).find((candidate) => normalizedUrl(candidate.url) === normalizedUrl(route.url));
    if (!item) {
      blockers.push({ code: "production_url_missing", url: route.url, evidence: "production evidence has no URL observation", source: "production" });
      continue;
    }
    if (!item.searchConsole || item.searchConsole.indexingState !== "INDEXED") blockers.push({ code: "production_not_indexed", url: route.url, evidence: `Search Console state is ${item.searchConsole?.indexingState || "missing"}`, source: "production" });
    if (normalizedUrl(item.searchConsole?.userCanonical) !== normalizedUrl(route.url)) blockers.push({ code: "production_user_canonical_mismatch", url: route.url, evidence: `declared user canonical is ${item.searchConsole?.userCanonical || "missing"}`, source: "production" });
    if (normalizedUrl(item.searchConsole?.googleCanonical) !== normalizedUrl(route.url)) blockers.push({ code: "production_google_canonical_mismatch", url: route.url, evidence: `Google selected ${item.searchConsole?.googleCanonical || "missing"}`, source: "production" });
    const field = item.crux?.p75;
    if (!field || !Number.isFinite(Number(field.lcpMs)) || !Number.isFinite(Number(field.inpMs)) || !Number.isFinite(Number(field.cls))) {
      blockers.push({ code: "production_crux_missing", url: route.url, evidence: "CrUX p75 LCP, INP, and CLS observations are required", source: "production" });
    } else {
      if (Number(field.lcpMs) > contract.policy.performance.lcpMs) blockers.push({ code: "field_lcp", url: route.url, evidence: `CrUX p75 LCP ${field.lcpMs}ms exceeds ${contract.policy.performance.lcpMs}ms`, source: "production" });
      if (Number(field.inpMs) > contract.policy.performance.inpMs) blockers.push({ code: "field_inp", url: route.url, evidence: `CrUX p75 INP ${field.inpMs}ms exceeds ${contract.policy.performance.inpMs}ms`, source: "production" });
      if (Number(field.cls) > contract.policy.performance.cls) blockers.push({ code: "field_cls", url: route.url, evidence: `CrUX p75 CLS ${field.cls} exceeds ${contract.policy.performance.cls}`, source: "production" });
    }
  }
  return { status: blockers.length ? "blocked" : "observed", required, file, source: raw.source, property: raw.property || null, collectedAt: raw.collectedAt, ageHours: Number(ageHours.toFixed(2)), urls: raw.urls, blockers };
}

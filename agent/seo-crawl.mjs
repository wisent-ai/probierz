import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import path from "node:path";

const GOOGLEBOT_SMARTPHONE = "Mozilla/5.0 (Linux; Android 6.0.1; Nexus 5X Build/MMB29P) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";
const ORDINARY_CHROME = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const HTTP_REDIRECTS = new Set([301, 302, 303, 307, 308]);
const MOBILE_LAB_PROFILE = Object.freeze({ latencyMs: 150, downloadKbps: 1600, uploadKbps: 750, cpuSlowdown: 4 });

function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

function normalizedText(value) {
  return String(value || "").replace(/\s+/g, " ").trim();
}

function allowedUrl(value, allowedOrigins) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error(`invalid crawl URL: ${value}`);
  }
  if (!allowedOrigins.includes(parsed.origin)) throw new Error(`crawl URL leaves allowed origins: ${parsed.href}`);
  if (!new Set(["http:", "https:"]).has(parsed.protocol)) throw new Error(`unsupported crawl protocol: ${parsed.protocol}`);
  parsed.hash = "";
  return parsed;
}

function headersObject(headers) {
  return Object.fromEntries([...headers.entries()].map(([name, value]) => [name.toLowerCase(), value]));
}

async function fetchChain(input, { userAgent, allowedOrigins, maxRedirects }) {
  let current = allowedUrl(input, allowedOrigins);
  const redirects = [];
  for (let index = 0; index <= maxRedirects; index += 1) {
    const response = await fetch(current, {
      redirect: "manual",
      headers: { "user-agent": userAgent, accept: "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8" },
      signal: AbortSignal.timeout(30000),
    });
    const headers = headersObject(response.headers);
    if (HTTP_REDIRECTS.has(response.status) && headers.location) {
      const next = allowedUrl(new URL(headers.location, current).href, allowedOrigins);
      redirects.push({ from: current.href, status: response.status, location: next.href });
      current = next;
      continue;
    }
    const bytes = Buffer.from(await response.arrayBuffer());
    const compressed = current.pathname.endsWith(".gz") || /(?:application|text)\/(?:x-)?gzip/i.test(headers["content-type"] || "");
    const content = compressed ? gunzipSync(bytes) : bytes;
    const body = content.toString("utf8");
    return {
      requestedUrl: input,
      finalUrl: current.href,
      status: response.status,
      headers,
      redirects,
      redirectLoopOrLimit: false,
      bodySha256: sha256(content),
      body,
    };
  }
  return { requestedUrl: input, finalUrl: current.href, status: 0, headers: {}, redirects, redirectLoopOrLimit: true, bodySha256: null, body: "" };
}
function persistResponse(response, artifact) {
  if (!response) return response;
  mkdirSync(path.dirname(artifact), { recursive: true });
  writeFileSync(artifact, response.body || "", { mode: 0o600, flag: "wx" });
  return { ...response, body: undefined, artifact };
}

function parseRobots(text) {
  const groups = [];
  const sitemaps = [];
  let agents = [];
  let rules = [];
  const flush = () => {
    if (agents.length) groups.push({ agents, rules });
    agents = [];
    rules = [];
  };
  for (const rawLine of String(text || "").split(/\r?\n/)) {
    const line = rawLine.replace(/#.*$/, "").trim();
    if (!line) continue;
    const separator = line.indexOf(":");
    if (separator < 0) continue;
    const name = line.slice(0, separator).trim().toLowerCase();
    const value = line.slice(separator + 1).trim();
    if (name === "user-agent") {
      if (rules.length) flush();
      agents.push(value.toLowerCase());
    } else if (name === "allow" || name === "disallow") {
      if (agents.length) rules.push({ kind: name, path: value });
    } else if (name === "sitemap" && value) {
      sitemaps.push(value);
    }
  }
  flush();
  return { groups, sitemaps: [...new Set(sitemaps)] };
}

function robotsAllowed(parsed, pathAndQuery, agent = "googlebot") {
  const matching = parsed.groups
    .map((group) => ({
      ...group,
      specificity: Math.max(...group.agents.map((value) => value === "*" ? 0 : agent.includes(value) ? value.length : -1)),
    }))
    .filter((group) => group.specificity >= 0);
  const specificity = Math.max(-1, ...matching.map((group) => group.specificity));
  const candidates = matching
    .filter((group) => group.specificity === specificity)
    .flatMap((group) => group.rules)
    .filter((rule) => {
      if (!rule.path) return false;
      const anchored = rule.path.endsWith("$");
      const source = (anchored ? rule.path.slice(0, -1) : rule.path)
        .replace(/[.+?^${}()|[\]\\]/g, "\\$&")
        .replaceAll("*", ".*");
      return new RegExp(`^${source}${anchored ? "$" : ""}`).test(pathAndQuery);
    });
  candidates.sort((left, right) => {
    const specificityLength = (rule) => rule.path.replace(/[*$]/g, "").length;
    return specificityLength(right) - specificityLength(left) || (left.kind === "allow" ? -1 : 1);
  });
  return candidates.length === 0 || candidates[0].kind === "allow";
}

function decodeXml(value) {
  return value.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'");
}

function sitemapLocations(xml) {
  return [...String(xml || "").matchAll(/<loc\b[^>]*>([\s\S]*?)<\/loc>/gi)].map((match) => decodeXml(normalizedText(match[1]))).filter(Boolean);
}

async function collectSitemaps(seedUrls, options) {
  const pending = [...new Set(seedUrls)];
  const visited = new Set();
  const pages = new Set();
  const documents = [];
  while (pending.length && visited.size < options.maxSitemaps) {
    const sitemapUrl = pending.shift();
    if (visited.has(sitemapUrl)) continue;
    visited.add(sitemapUrl);
    let response;
    try {
      response = await fetchChain(sitemapUrl, options);
    } catch (error) {
      documents.push({ url: sitemapUrl, status: 0, error: error instanceof Error ? error.message : String(error), locations: [] });
      continue;
    }
    const locations = sitemapLocations(response.body);
    const isIndex = /<sitemapindex\b/i.test(response.body);
    const artifact = path.join(options.artifactsDir, `sitemap-${String(visited.size).padStart(2, "0")}.xml`);
    mkdirSync(path.dirname(artifact), { recursive: true });
    writeFileSync(artifact, response.body, { mode: 0o600, flag: "wx" });
    documents.push({ url: sitemapUrl, finalUrl: response.finalUrl, status: response.status, bodySha256: response.bodySha256, artifact, isIndex, locations });
    for (const location of locations) {
      let parsed;
      try {
        parsed = allowedUrl(location, options.allowedOrigins);
      } catch {
        continue;
      }
      if (isIndex || /\.xml(?:\.gz)?$/i.test(parsed.pathname)) pending.push(parsed.href);
      else pages.add(parsed.href);
    }
  }
  return { documents, urls: [...pages].sort(), truncated: pending.length > 0 };
}

async function renderPage(context, url, { profile, allowedOrigins, screenshotPath, htmlPath, throttle = null }) {
  const page = await context.newPage();
  let cdp = null;
  const consoleErrors = [];
  const failedRequests = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text().slice(0, 700));
  });
  page.on("requestfailed", (request) => failedRequests.push(`${request.method()} ${request.url()} — ${request.failure()?.errorText || "failed"}`.slice(0, 900)));
  await page.route("**/*", async (route) => {
    if (route.request().resourceType() !== "document") return route.continue();
    try {
      const target = new URL(route.request().url());
      if (!allowedOrigins.includes(target.origin)) return route.abort("blockedbyclient");
    } catch {
      return route.abort("blockedbyclient");
    }
    return route.continue();
  });
  try {
    if (throttle) {
      cdp = await context.newCDPSession(page);
      await cdp.send("Network.enable");
      await cdp.send("Network.emulateNetworkConditions", {
        offline: false,
        latency: throttle.latencyMs,
        downloadThroughput: throttle.downloadKbps * 1024 / 8,
        uploadThroughput: throttle.uploadKbps * 1024 / 8,
      });
      await cdp.send("Emulation.setCPUThrottlingRate", { rate: throttle.cpuSlowdown });
    }
    await page.addInitScript(() => {
      const state = { cls: 0, lcp: 0, tbt: 0 };
      Object.defineProperty(window, "__probierzSeoVitals", { value: state, configurable: false });
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          const item = entry;
          if (!item.hadRecentInput) state.cls += Number(item.value || 0);
        }
      }).observe({ type: "layout-shift", buffered: true });
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) state.lcp = Math.max(state.lcp, Number(entry.startTime || 0));
      }).observe({ type: "largest-contentful-paint", buffered: true });
      try {
        new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) state.tbt += Math.max(0, Number(entry.duration || 0) - 50);
        }).observe({ type: "longtask", buffered: true });
      } catch {}
    });
    const response = await page.goto(url, { waitUntil: "domcontentloaded", timeout: 30000 });
    await page.waitForLoadState("load", { timeout: 10000 }).catch(() => {});
    await page.evaluate(async () => {
      if (document.fonts?.ready) await document.fonts.ready;
    });
    await page.waitForTimeout(1250);
    const result = await page.evaluate(({ profileName, status, errors, failed }) => {
      const clean = (value) => String(value || "").replace(/\s+/g, " ").trim();
      const visible = (element) => {
        if (!(element instanceof HTMLElement)) return false;
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        return rect.width > 0 && rect.height > 0 && style.display !== "none" && style.visibility !== "hidden";
      };
      const meta = (name) => document.querySelector(`meta[name="${name}"]`)?.getAttribute("content") || "";
      const property = (name) => document.querySelector(`meta[property="${name}"]`)?.getAttribute("content") || "";
      const headings = [...document.querySelectorAll("h1,h2,h3,h4,h5,h6")].filter(visible).map((element) => ({ level: Number(element.tagName.slice(1)), text: clean(element.textContent) })).filter((item) => item.text);
      const links = [...document.querySelectorAll("a[href]")].filter(visible).map((anchor) => ({ href: anchor.href, text: clean(anchor.textContent), rel: clean(anchor.getAttribute("rel")) })).filter((item) => item.href);
      const jsonLd = [...document.querySelectorAll('script[type="application/ld+json"]')].map((script) => script.textContent || "");
      const vitals = window.__probierzSeoVitals || { cls: 0, lcp: 0, tbt: 0 };
      const nav = performance.getEntriesByType("navigation")[0];
      return {
        profile: profileName,
        url: location.href,
        httpStatus: status,
        title: document.title,
        metaDescription: meta("description"),
        metaRobots: meta("robots"),
        googlebotRobots: meta("googlebot"),
        lang: document.documentElement.lang || "",
        canonical: document.querySelector('link[rel="canonical"]')?.href || "",
        hreflang: [...document.querySelectorAll('link[rel="alternate"][hreflang]')].map((link) => ({ lang: link.getAttribute("hreflang") || "", href: link.href })),
        viewport: meta("viewport"),
        h1: headings.filter((item) => item.level === 1).map((item) => item.text),
        headings: headings.slice(0, 120),
        links: links.slice(0, 2000),
        jsonLd,
        openGraph: { title: property("og:title"), description: property("og:description"), image: property("og:image"), url: property("og:url"), type: property("og:type") },
        twitter: { card: meta("twitter:card"), title: meta("twitter:title"), description: meta("twitter:description"), image: meta("twitter:image") },
        favicon: document.querySelector('link[rel~="icon"]')?.href || "",
        visibleText: clean(document.body?.innerText).slice(0, 60000),
        htmlSha256Input: document.documentElement.outerHTML,
        performance: {
          lcpMs: Number(vitals.lcp || 0),
          cls: Number(vitals.cls || 0),
          tbtMs: Number(vitals.tbt || 0),
          domContentLoadedMs: Number(nav?.domContentLoadedEventEnd || 0),
          loadMs: Number(nav?.loadEventEnd || 0),
          responseEndMs: Number(nav?.responseEnd || 0)
        },
        consoleErrors: errors,
        failedRequests: failed
      };
    }, { profileName: profile, status: response?.status() || 0, errors: consoleErrors, failed: failedRequests });
    result.htmlSha256 = sha256(result.htmlSha256Input);
    result.visibleTextSha256 = sha256(result.visibleText);
    if (htmlPath) {
      mkdirSync(path.dirname(htmlPath), { recursive: true });
      writeFileSync(htmlPath, result.htmlSha256Input, { mode: 0o600, flag: "wx" });
      result.htmlArtifact = htmlPath;
    }
    delete result.htmlSha256Input;
    if (screenshotPath) {
      mkdirSync(path.dirname(screenshotPath), { recursive: true });
      await page.screenshot({ path: screenshotPath, type: "jpeg", quality: 65, fullPage: false });
      result.screenshot = screenshotPath;
    }
    return result;
  } finally {
    await cdp?.detach().catch(() => {});
    await page.close().catch(() => {});
  }
}

function internalLinks(render, allowedOrigins) {
  const urls = [];
  for (const link of render.links || []) {
    if (String(link.rel || "").toLowerCase().split(/\s+/).includes("nofollow")) continue;
    try {
      const parsed = new URL(link.href);
      parsed.hash = "";
      if (allowedOrigins.includes(parsed.origin) && new Set(["http:", "https:"]).has(parsed.protocol)) urls.push(parsed.href);
    } catch {}
  }
  return [...new Set(urls)];
}
async function collectSocialAssets(render, fetchOptions) {
  const urls = [...new Set([
    render.openGraph?.image,
    render.twitter?.image,
  ].filter(Boolean))];
  const assets = [];
  for (const url of urls) {
    try {
      const response = await fetchChain(url, fetchOptions);
      assets.push({
        url,
        finalUrl: response.finalUrl,
        status: response.status,
        contentType: response.headers["content-type"] || "",
        bodySha256: response.bodySha256,
        redirects: response.redirects,
      });
    } catch (error) {
      assets.push({ url, status: 0, error: error instanceof Error ? error.message : String(error) });
    }
  }
  return assets;
}

export async function collectSeoEvidence(contract, { browser, artifactsDir } = {}) {
  if (!browser) throw new Error("SEO evaluation requires a Playwright browser");
  const policy = contract.policy;
  const fetchOptions = {
    userAgent: GOOGLEBOT_SMARTPHONE,
    allowedOrigins: contract.allowedOrigins,
    maxRedirects: policy.crawl.maxRedirects,
    maxSitemaps: policy.crawl.maxSitemaps,
  };
  const robotsUrl = new URL("/robots.txt", contract.baseUrl).href;
  let robotsResponse;
  try {
    robotsResponse = await fetchChain(robotsUrl, fetchOptions);
  } catch (error) {
    robotsResponse = {
      requestedUrl: robotsUrl,
      finalUrl: robotsUrl,
      status: 0,
      headers: {},
      redirects: [],
      body: "",
      bodySha256: null,
      error: error instanceof Error ? error.message : String(error),
    };
  }
  const robots = parseRobots(robotsResponse.body);
  const sitemapSeeds = robots.sitemaps.length ? robots.sitemaps : [new URL("/sitemap.xml", contract.baseUrl).href];
  mkdirSync(artifactsDir, { recursive: true });
  const robotsArtifact = path.join(artifactsDir, "robots.txt");
  writeFileSync(robotsArtifact, robotsResponse.body, { mode: 0o600, flag: "wx" });
  const sitemaps = await collectSitemaps(sitemapSeeds, { ...fetchOptions, artifactsDir });
  const ordinary = await browser.newContext({ userAgent: ORDINARY_CHROME, viewport: { width: 1440, height: 1000 }, reducedMotion: "reduce" });
  const googlebot = await browser.newContext({ userAgent: GOOGLEBOT_SMARTPHONE, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, reducedMotion: "reduce" });
  const declared = new Set(contract.routes.map((route) => route.url));
  const pending = [];
  const depth = new Map();
  for (const url of [...declared, ...sitemaps.urls]) {
    if (!depth.has(url)) {
      pending.push(url);
      depth.set(url, declared.has(url) ? 0 : 1);
    }
  }
  const pages = [];
  const crawled = new Set();
  try {
    while (pending.length && pages.length < policy.crawl.maxPages) {
      const url = pending.shift();
      if (crawled.has(url)) continue;
      crawled.add(url);
      const currentDepth = depth.get(url) || 0;
      const pageIndex = pages.length;
      const routeIndex = contract.routes.findIndex((route) => route.url === url);
      let rawOrdinary;
      let rawGooglebot;
      let ordinaryRender;
      let googlebotRender;
      try {
        const fetched = await Promise.allSettled([
          fetchChain(url, { ...fetchOptions, userAgent: ORDINARY_CHROME }),
          fetchChain(url, fetchOptions),
        ]);
        rawOrdinary = fetched[0].status === "fulfilled"
          ? persistResponse(fetched[0].value, path.join(artifactsDir, `page-${pageIndex}-ordinary-source.html`))
          : { error: fetched[0].reason instanceof Error ? fetched[0].reason.message : String(fetched[0].reason) };
        rawGooglebot = fetched[1].status === "fulfilled"
          ? persistResponse(fetched[1].value, path.join(artifactsDir, `page-${pageIndex}-googlebot-source.html`))
          : { error: fetched[1].reason instanceof Error ? fetched[1].reason.message : String(fetched[1].reason) };
        if (fetched.some((result) => result.status === "rejected")) {
          throw new Error(`source fetch failed: ${rawOrdinary.error || rawGooglebot.error}`);
        }
        ordinaryRender = await renderPage(ordinary, url, {
          profile: "ordinary-desktop",
          allowedOrigins: contract.allowedOrigins,
          screenshotPath: routeIndex >= 0 && routeIndex * 2 < policy.model.maxScreenshots
            ? path.join(artifactsDir, `route-${routeIndex}-desktop.jpg`)
            : null,
          htmlPath: path.join(artifactsDir, `page-${pageIndex}-ordinary-rendered.html`),
        });
        googlebotRender = await renderPage(googlebot, url, {
          profile: "googlebot-smartphone",
          allowedOrigins: contract.allowedOrigins,
          screenshotPath: routeIndex >= 0 && routeIndex * 2 + 1 < policy.model.maxScreenshots
            ? path.join(artifactsDir, `route-${routeIndex}-googlebot-mobile.jpg`)
            : null,
          htmlPath: path.join(artifactsDir, `page-${pageIndex}-googlebot-rendered.html`),
          throttle: MOBILE_LAB_PROFILE,
        });
      } catch (error) {
        pages.push({
          url,
          depth: currentDepth,
          error: error instanceof Error ? error.message : String(error),
          rawOrdinary,
          rawGooglebot,
          ordinary: ordinaryRender,
          googlebot: googlebotRender,
        });
        continue;
      }
      const links = internalLinks(ordinaryRender, contract.allowedOrigins);
      const socialAssets = await collectSocialAssets(googlebotRender, fetchOptions);
      pages.push({ url, depth: currentDepth, rawOrdinary, rawGooglebot, ordinary: ordinaryRender, googlebot: googlebotRender, links, socialAssets });
      if (currentDepth < policy.crawl.maxDepth) {
        const references = [
          ...links,
          ordinaryRender.canonical,
          googlebotRender.canonical,
          ...(ordinaryRender.hreflang || []).map((item) => item.href),
          ...(googlebotRender.hreflang || []).map((item) => item.href),
        ].filter(Boolean);
        for (const reference of references) {
          let target;
          try {
            target = allowedUrl(reference, contract.allowedOrigins).href;
          } catch {
            continue;
          }
          if (!depth.has(target)) {
            depth.set(target, currentDepth + 1);
            pending.push(target);
          }
        }
      }
    }
  } finally {
    await ordinary.close().catch(() => {});
    await googlebot.close().catch(() => {});
  }
  return {
    schemaVersion: 1,
    collectedAt: new Date().toISOString(),
    baseUrl: contract.baseUrl,
    allowedOrigins: contract.allowedOrigins,
    userAgents: { ordinary: ORDINARY_CHROME, googlebotSmartphone: GOOGLEBOT_SMARTPHONE },
    performanceProfiles: { ordinary: null, googlebotSmartphone: MOBILE_LAB_PROFILE },
    robots: {
      url: robotsUrl,
      status: robotsResponse.status,
      finalUrl: robotsResponse.finalUrl,
      headers: robotsResponse.headers,
      bodySha256: robotsResponse.bodySha256,
      artifact: robotsArtifact,
      parsed: robots,
      routeAccess: Object.fromEntries(contract.routes.map((route) => {
        const url = new URL(route.url);
        return [route.url, robotsAllowed(robots, `${url.pathname}${url.search}`)];
      })),
      pageAccess: Object.fromEntries(pages.map((page) => {
        const url = new URL(page.url);
        return [page.url, robotsAllowed(robots, `${url.pathname}${url.search}`)];
      })),
    },
    sitemaps,
    pages,
    coverage: {
      declared: contract.routes.length,
      sitemapUrls: sitemaps.urls.length,
      crawled: pages.length,
      truncated: pending.length > 0 || sitemaps.truncated,
      uncrawled: pending,
    },
  };
}

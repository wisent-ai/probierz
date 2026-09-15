//! Fetching every declared route twice — as an ordinary browser and as
//! Googlebot Smartphone — and the checks that need no model at all.
//!
//! A route that does not answer, an indexable route that says
//! `noindex`, and an indexable route with no title are blockers on
//! their own. A body that differs between the two crawls is a warning:
//! it is how cloaking shows up, and it is worth a human reading even
//! when it is legitimate.

use super::*;

/// The two crawlers every route is fetched with. The first is an
/// ordinary desktop Chrome; the second is Google's documented
/// Googlebot Smartphone string.
const ORDINARY_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const GOOGLEBOT_AGENT: &str = "Mozilla/5.0 (Linux; Android 6.0.1; Nexus 5X Build/MMB29P) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";

/// Statuses a route may answer with and still be considered reachable:
/// success and redirection. Anything else is a blocker.
const REACHABLE: std::ops::Range<u64> = 200..400;

/// Schema version of the evidence document.
const EVIDENCE_SCHEMA_VERSION: u64 = 1;

/// Every declared route as crawled, plus the deterministic view of it.
pub(crate) struct Crawled {
    /// The routes as declared, each with the absolute URL used.
    pub(crate) route_contracts: Vec<JsonValue>,
    pub(crate) evidence: JsonValue,
    pub(crate) deterministic: JsonValue,
}

pub(crate) fn crawl_routes(contract: &Contract) -> Result<Crawled, Failure> {
    let routes = contract.routes()?;
    let mut route_contracts = Vec::new();
    let mut ordinary = Vec::new();
    let mut googlebot = Vec::new();
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    for route in routes {
        let path = route.get("path").and_then(JsonValue::as_str).unwrap_or("/");
        let url = contract
            .canonical
            .join(path)
            .map_err(|error| Failure::config("seo-evaluate", error.to_string()))?;
        let mut declared = route.clone();
        if let Some(object) = declared.as_object_mut() {
            object.insert("url".to_string(), json!(url.as_str()));
        }
        route_contracts.push(declared);

        let page = seo_fetch(url.as_str(), ORDINARY_AGENT)?;
        let bot = seo_fetch(url.as_str(), GOOGLEBOT_AGENT)?;
        let indexable = route
            .get("indexable")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);

        if !REACHABLE.contains(&page["status"].as_u64().unwrap_or(0)) {
            blockers.push(json!({
                "code": "route_http_failure",
                "evidence": format!("{} returned {}", url, page["status"]),
                "source": "deterministic"
            }));
        }
        if indexable
            && page["robots"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("noindex")
        {
            blockers.push(json!({
                "code": "indexable_route_noindex",
                "evidence": format!("{url} declares noindex"),
                "source": "deterministic"
            }));
        }
        if indexable && page["title"].as_str().unwrap_or_default().is_empty() {
            blockers.push(json!({
                "code": "title_missing",
                "evidence": format!("{url} has no title"),
                "source": "deterministic"
            }));
        }
        if page["bodySha256"] != bot["bodySha256"] {
            warnings.push(json!({
                "code": "googlebot_content_differs",
                "evidence": format!("ordinary and Googlebot bodies differ for {url}")
            }));
        }

        ordinary.push(page);
        googlebot.push(bot);
    }

    let evidence = json!({
        "schemaVersion": EVIDENCE_SCHEMA_VERSION,
        "collectedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "pages": ordinary,
        "googlebotPages": googlebot,
        "artifacts": []
    });
    let deterministic = deterministic_view(contract, routes.len(), blockers, warnings);

    Ok(Crawled {
        route_contracts,
        evidence,
        deterministic,
    })
}

/// The deterministic half of every dimension it applies to. One
/// blocker anywhere fails all of them, because a page that does not
/// answer or refuses indexing cannot be scored per dimension.
fn deterministic_view(
    contract: &Contract,
    route_count: usize,
    blockers: Vec<JsonValue>,
    warnings: Vec<JsonValue>,
) -> JsonValue {
    let score = if blockers.is_empty() { 1.0 } else { 0.0 };
    let issues = blockers
        .iter()
        .filter_map(|item| item.get("code").and_then(JsonValue::as_str))
        .collect::<Vec<_>>();

    let mut dimensions = Map::new();
    for (name, rule) in contract.policy["dimensions"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if matches!(rule["source"].as_str(), Some("deterministic" | "hybrid")) {
            dimensions.insert(
                name.clone(),
                json!({
                    "score": score,
                    "evidence": [format!(
                        "{route_count} declared routes crawled as ordinary Chrome and Googlebot Smartphone"
                    )],
                    "issues": issues
                }),
            );
        }
    }
    json!({ "dimensions": dimensions, "blockers": blockers, "warnings": warnings })
}

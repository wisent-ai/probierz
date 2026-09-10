use serde_json::json;
use crate::authoring::*;
pub(crate) fn seo_prerequisites(
    harness: &Path,
    app_id: &str,
    target: Option<&str>,
    primary: Option<&str>,
    secondary: Option<&str>,
    router_url: Option<&str>,
) -> Result<(String, String, String), String> {
    let loaded = manifest::load(harness, app_id).ok();
    let primary = required_setting(
        selected_setting(
            loaded.as_ref(),
            target,
            "PROBIERZ_SEO_PRIMARY_MODEL",
            primary,
        ),
        "PROBIERZ_SEO_PRIMARY_MODEL",
    )?;
    let secondary = required_setting(
        selected_setting(
            loaded.as_ref(),
            target,
            "PROBIERZ_SEO_SECONDARY_MODEL",
            secondary,
        ),
        "PROBIERZ_SEO_SECONDARY_MODEL",
    )?;
    if primary == secondary {
        return Err("SEO primary and secondary model IDs must differ".to_string());
    }
    let url = stado_model_router_url(
        selected_setting(
            loaded.as_ref(),
            target,
            "STADO_MODEL_ROUTER_URL",
            router_url,
        )
        .as_deref(),
    )?;
    Ok((primary, secondary, url))
}

pub(crate) fn resolved_contract_file(
    harness: &Path,
    explicit: Option<&Path>,
    declared: Option<&str>,
    label: &str,
) -> Result<PathBuf, Failure> {
    let selected = explicit
        .map(Path::to_path_buf)
        .or_else(|| declared.map(PathBuf::from))
        .ok_or_else(|| {
            Failure::config(
                "seo-evaluate",
                format!("invalid SEO contract: {label} path is required"),
            )
        })?;
    Ok(if selected.is_absolute() {
        selected
    } else {
        harness.join(selected)
    })
}

pub(crate) fn seo_fetch(url: &str, user_agent: &str) -> Result<JsonValue, Failure> {
    let response = match ureq::get(url).set("user-agent", user_agent).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => {
            return Err(Failure::unavailable(
                "seo-evaluate.crawl",
                error.to_string(),
            ))
        }
    };
    let status = response.status();
    let final_url = response.get_url().to_string();
    let headers = response
        .headers_names()
        .into_iter()
        .map(|name| {
            let value = response.header(&name).unwrap_or_default().to_string();
            (name.to_ascii_lowercase(), JsonValue::String(value))
        })
        .collect::<Map<_, _>>();
    let body = response
        .into_string()
        .map_err(|error| Failure::unavailable("seo-evaluate.crawl", error.to_string()))?;
    let capture = |pattern: &str| -> String {
        regex::Regex::new(pattern)
            .ok()
            .and_then(|expression| expression.captures(&body))
            .and_then(|captures| captures.get(1))
            .map(|value| {
                value
                    .as_str()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    };
    let title = capture(r"(?is)<title[^>]*>(.*?)</title>");
    let description =
        capture(r#"(?is)<meta[^>]+name=["']description["'][^>]+content=["']([^"']*)["']"#);
    let robots = capture(r#"(?is)<meta[^>]+name=["']robots["'][^>]+content=["']([^"']*)["']"#);
    let canonical = capture(r#"(?is)<link[^>]+rel=["']canonical["'][^>]+href=["']([^"']*)["']"#);
    let h1: Vec<String> = regex::Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>")
        .ok()
        .into_iter()
        .flat_map(|expression| {
            expression
                .captures_iter(&body)
                .filter_map(|captures| captures.get(1))
                .map(|value| {
                    regex::Regex::new(r"<[^>]+>")
                        .map(|tags| {
                            tags.replace_all(value.as_str(), "")
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(json!({
        "requestedUrl": url, "finalUrl": final_url, "status": status,
        "headers": headers, "bodySha256": hex::encode(Sha256::digest(body.as_bytes())),
        "title": title, "description": description, "robots": robots, "canonical": canonical,
        "h1": h1, "html": body.chars().take(200_000).collect::<String>()
    }))
}

pub(crate) fn seo_model_tool(policy: &JsonValue) -> JsonValue {
    let mut properties = Map::new();
    let mut names = Vec::new();
    for (name, rule) in policy["dimensions"].as_object().into_iter().flatten() {
        if matches!(rule["source"].as_str(), Some("model" | "hybrid")) {
            names.push(name.clone());
            properties.insert(
                name.clone(),
                json!({
                    "type": "object",
                    "properties": {
                        "score": { "type": "number", "minimum": 0, "maximum": 1 },
                        "evidence": { "type": "array", "items": { "type": "string" } },
                        "issues": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["score", "evidence", "issues"], "additionalProperties": false
                }),
            );
        }
    }
    json!({ "type": "function", "function": {
        "name": "record_seo_content_evaluation",
        "description": "Record one independent evidence-grounded SEO content evaluation.",
        "parameters": { "type": "object", "properties": {
            "summary": { "type": "string" },
            "dimensions": { "type": "object", "properties": properties, "required": names, "additionalProperties": false },
            "blocking_issues": { "type": "array", "items": { "type": "object", "properties": {
                "code": { "type": "string", "enum": ["fabricated_claim", "search_intent_mismatch", "misleading_snippet"] },
                "evidence": { "type": "string" }
            }, "required": ["code", "evidence"], "additionalProperties": false }},
            "recommendations": { "type": "array", "items": { "type": "object", "properties": {
                "priority": { "type": "string" }, "action": { "type": "string" }
            }, "required": ["priority", "action"], "additionalProperties": false }}
        }, "required": ["summary", "dimensions", "blocking_issues", "recommendations"], "additionalProperties": false }
    }})
}


//! Merging both halves into one verdict, and handing the payload to
//! the report writer.
//!
//! A dimension's score comes from whichever half the policy says owns
//! it, and `hybrid` takes the lower of the two — a page can only be as
//! good as the worse of what a crawler measured and what a model read.
//! Anything below a declared minimum, a quality total below the
//! declared floor, missing production evidence, or a missing signature
//! where the profile requires one, is a blocker.

use super::*;

/// Schema version and kind of the report this writes.
const REPORT_SCHEMA_VERSION: u64 = 1;
const REPORT_KIND: &str = "probierz-seo-evaluation";

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_report(
    harness: &Path,
    app_id: &str,
    contract: &Contract,
    crawled: &Crawled,
    graded: &Graded,
    output: Option<&Path>,
    production_file: Option<&Path>,
    private_key_file: Option<&Path>,
    private_key: Option<&str>,
) -> Result<JsonValue, Failure> {
    let mut blockers = crawled.deterministic["blockers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    blockers.extend(
        graded.model_evaluation["blockers"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );

    let (dimensions, quality) = merged_dimensions(contract, crawled, graded, &mut blockers)?;
    let required_quality = contract.policy["qualityMinimum"].as_f64().unwrap_or(0.0);
    if quality < required_quality {
        blockers.push(json!({
            "code": "quality_below_minimum",
            "evidence": format!("{quality:.3} < {required_quality:.3}"),
            "source": "threshold"
        }));
    }

    let production = production_evidence(contract, production_file, &mut blockers)?;
    let key_bytes = signing_key(private_key, private_key_file)?;
    if contract.signature_required() && key_bytes.is_none() {
        blockers.push(json!({
            "code": "evidence_signature_missing",
            "evidence": format!("{} SEO evidence requires an Ed25519 signing key", contract.mode),
            "source": "evidence"
        }));
    }

    let mut payload = payload(
        harness,
        app_id,
        contract,
        crawled,
        graded,
        &dimensions,
        quality,
        required_quality,
        &production,
        blockers,
    )?;
    let signing = key_bytes
        .as_deref()
        .map(|key| sign_seo_payload(&payload, key))
        .transpose()?;
    let identifier = report_id(&payload, signing.as_ref());

    let object = payload
        .as_object_mut()
        .ok_or_else(|| Failure::config("seo-evaluate", "SEO payload is invalid"))?;
    object.insert("reportId".to_string(), json!(identifier));
    object.insert(
        "signing".to_string(),
        signing.clone().unwrap_or(JsonValue::Null),
    );

    let file = report_file(harness, app_id, output)?;
    write_payload(&file, &payload)?;
    Ok(summary(&file, &identifier, &payload, quality, signing))
}

/// Every declared dimension, scored from the half that owns it, and the
/// weighted quality total. Pushes a blocker for each dimension below
/// its declared minimum.
fn merged_dimensions(
    contract: &Contract,
    crawled: &Crawled,
    graded: &Graded,
    blockers: &mut Vec<JsonValue>,
) -> Result<(Map<String, JsonValue>, f64), Failure> {
    let mut dimensions = Map::new();
    let mut quality = 0.0;
    for (name, rule) in contract.policy["dimensions"]
        .as_object()
        .into_iter()
        .flatten()
    {
        let source = rule["source"].as_str().unwrap_or_default();
        let deterministic_value = &crawled.deterministic["dimensions"][name];
        let model_value = &graded.model_evaluation["dimensions"][name];
        let deterministic_score = deterministic_value["score"].as_f64().unwrap_or(0.0);
        let model_score = model_value["score"].as_f64().unwrap_or(0.0);
        let score = match source {
            "deterministic" => deterministic_score,
            "model" => model_score,
            "hybrid" => deterministic_score.min(model_score),
            _ => {
                return Err(Failure::config(
                    "seo-evaluate",
                    format!(
                        "invalid SEO contract: {name}.source must be model, deterministic, or hybrid"
                    ),
                ))
            }
        };
        let weight = rule["weight"].as_f64().unwrap_or(0.0);
        let minimum = rule["minimum"].as_f64().unwrap_or(0.0);
        quality += score * weight;
        if score < minimum {
            blockers.push(json!({
                "code": format!("dimension_below_minimum:{name}"),
                "evidence": format!("{score:.3} < {minimum:.3}"),
                "source": "threshold"
            }));
        }
        dimensions.insert(name.clone(), json!({
            "label": rule.get("label").cloned().unwrap_or(JsonValue::Null),
            "source": source, "weight": weight, "minimum": minimum,
            "score": round_score(score),
            "evidence": deterministic_value["evidence"].as_array().into_iter().flatten()
                .chain(model_value["evidence"].as_array().into_iter().flatten())
                .cloned().collect::<Vec<_>>(),
            "issues": combined_issues(deterministic_value, model_value),
            "graderScores": model_value.get("graderScores").cloned().unwrap_or(JsonValue::Null)
        }));
    }
    Ok((dimensions, round_score(quality)))
}

/// Issues from both halves of one dimension, in order, without
/// repeating one both halves raised.
fn combined_issues(deterministic_value: &JsonValue, model_value: &JsonValue) -> Vec<String> {
    let mut seen = BTreeSet::new();
    deterministic_value["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .chain(model_value["issues"].as_array().into_iter().flatten())
        .filter_map(JsonValue::as_str)
        .filter(|issue| seen.insert((*issue).to_string()))
        .map(str::to_string)
        .collect()
}

/// Search Console and CrUX evidence, which `production` mode requires
/// and other modes may carry.
fn production_evidence(
    contract: &Contract,
    production_file: Option<&Path>,
    blockers: &mut Vec<JsonValue>,
) -> Result<JsonValue, Failure> {
    let production = if let Some(file) = production_file {
        serde_json::from_slice(&fs::read(file)?)?
    } else {
        json!({
            "required": contract.mode == "production",
            "status": "not-provided",
            "blockers": []
        })
    };
    if production.get("required").and_then(JsonValue::as_bool) == Some(true)
        && production.get("status").and_then(JsonValue::as_str) == Some("not-provided")
    {
        blockers.push(json!({
            "code": "production_evidence_missing",
            "evidence": "production mode requires Search Console and CrUX evidence",
            "source": "production"
        }));
    }
    Ok(production)
}

#[allow(clippy::too_many_arguments)]
fn payload(
    harness: &Path,
    app_id: &str,
    contract: &Contract,
    crawled: &Crawled,
    graded: &Graded,
    dimensions: &Map<String, JsonValue>,
    quality: f64,
    required_quality: f64,
    production: &JsonValue,
    blockers: Vec<JsonValue>,
) -> Result<JsonValue, Failure> {
    let eligible = crawled.deterministic["blockers"]
        .as_array()
        .is_none_or(Vec::is_empty);
    Ok(json!({
        "schemaVersion": REPORT_SCHEMA_VERSION, "kind": REPORT_KIND,
        "appId": app_id, "mode": contract.mode,
        "issuedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "sourceIdentity": app_source_identity(harness, app_id, None)?,
        "contract": {
            "policy": {
                "file": contract.policy_path.to_string_lossy(),
                "name": contract.policy["name"],
                "sha256": hex::encode(Sha256::digest(fs::read(&contract.policy_path)?))
            },
            "brief": {
                "file": contract.brief_path.to_string_lossy(),
                "product": contract.brief["product"],
                "sha256": hex::encode(Sha256::digest(fs::read(&contract.brief_path)?))
            },
            "baseUrl": contract.canonical.as_str(),
            "routes": crawled.route_contracts
        },
        "verdict": {
            "pass": blockers.is_empty(),
            "searchEligibility": if eligible { "eligible" } else { "blocked" },
            "searchQuality": quality, "requiredQuality": required_quality,
            "productionOutcome": production.get("status").and_then(JsonValue::as_str).unwrap_or("not-provided"),
            "blockers": blockers, "warnings": crawled.deterministic["warnings"]
        },
        "dimensions": dimensions, "evidence": crawled.evidence,
        "model": graded.model_evaluation, "production": production
    }))
}

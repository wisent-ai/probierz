use serde_json::json;
use crate::authoring::*;

#[allow(clippy::too_many_arguments)]
pub fn evaluate_seo(
    harness: &Path,
    app_id: &str,
    base_url: &str,
    policy_file: Option<&Path>,
    brief_file: Option<&Path>,
    mode: &str,
    output: Option<&Path>,
    production_file: Option<&Path>,
    primary: Option<&str>,
    secondary: Option<&str>,
    adjudicator: Option<&str>,
    router_url: Option<&str>,
    agent_id: Option<&str>,
    private_key_file: Option<&Path>,
    router_bearer: Option<&str>,
    agent_secret: Option<&str>,
    private_key: Option<&str>,
) -> Result<JsonValue, Failure> {
    if !matches!(mode, "pull-request" | "release" | "nightly" | "production") {
        return Err(Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: mode must be one of pull-request, release, nightly, production",
        ));
    }
    let canonical = url::Url::parse(base_url).map_err(|_| {
        Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: base URL must be an absolute URL",
        )
    })?;
    if canonical.username() != "" || canonical.password().is_some() {
        return Err(Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: base URL must not contain credentials",
        ));
    }
    if canonical.scheme() != "https"
        && !(canonical.scheme() == "http" && canonical.host_str().is_some_and(loopback))
    {
        return Err(Failure::invalid(
            "seo-evaluate",
            "invalid SEO contract: base URL must use HTTPS or loopback HTTP",
        ));
    }
    let loaded = manifest::load(harness, app_id)?;
    let seo = loaded.document.get("seo").ok_or_else(|| {
        Failure::config(
            "seo-evaluate",
            format!(
                "invalid SEO contract: {} seo section is required",
                loaded.file.display()
            ),
        )
    })?;
    let policy_path = resolved_contract_file(
        harness,
        policy_file,
        seo.get("policy").and_then(YamlValue::as_str),
        "SEO policy",
    )?;
    let brief_env = std::env::var("PROBIERZ_LANDING_BRIEF").ok();
    let brief_path = resolved_contract_file(
        harness,
        brief_file,
        brief_env
            .as_deref()
            .or_else(|| seo.get("brief").and_then(YamlValue::as_str)),
        "landing brief",
    )?;
    let policy: JsonValue = serde_json::from_slice(&fs::read(&policy_path)?).map_err(|error| {
        Failure::config(
            "seo-evaluate",
            format!("cannot read SEO policy {}: {error}", policy_path.display()),
        )
    })?;
    let brief: JsonValue = serde_json::from_slice(&fs::read(&brief_path)?).map_err(|error| {
        Failure::config(
            "seo-evaluate",
            format!(
                "cannot read landing brief {}: {error}",
                brief_path.display()
            ),
        )
    })?;
    if policy.get("schemaVersion").and_then(JsonValue::as_u64) != Some(1)
        || policy
            .get("dimensions")
            .and_then(JsonValue::as_object)
            .is_none()
    {
        return Err(Failure::config(
            "seo-evaluate",
            format!(
                "invalid SEO contract: {} schemaVersion must be 1 and dimensions are required",
                policy_path.display()
            ),
        ));
    }
    if brief.get("schemaVersion").and_then(JsonValue::as_u64) != Some(1) {
        return Err(Failure::config(
            "seo-evaluate",
            format!(
                "invalid SEO contract: {} schemaVersion must be 1",
                brief_path.display()
            ),
        ));
    }
    let (primary, secondary, router_url) =
        seo_prerequisites(harness, app_id, Some("web"), primary, secondary, router_url)
            .map_err(|detail| Failure::config("seo-evaluate", detail))?;
    let token = required_setting(
        router_bearer
            .map(|value| value.trim().to_string())
            .or_else(|| {
                selected_setting(Some(&loaded), Some("web"), "STADO_MODEL_ROUTER_TOKEN", None)
            }),
        "STADO_MODEL_ROUTER_TOKEN",
    )
    .map_err(|detail| Failure::config("seo-evaluate", detail))?;
    let agent_id = required_setting(
        agent_id.map(|value| value.trim().to_string()).or_else(|| {
            selected_setting(Some(&loaded), Some("web"), "PROBIERZ_MODEL_AGENT_ID", None)
        }),
        "PROBIERZ_MODEL_AGENT_ID",
    )
    .map_err(|detail| Failure::config("seo-evaluate", detail))?;
    let agent_secret = required_setting(
        agent_secret
            .map(|value| value.trim().to_string())
            .or_else(|| {
                selected_setting(
                    Some(&loaded),
                    Some("web"),
                    "PROBIERZ_MODEL_AGENT_SECRET",
                    None,
                )
            }),
        "PROBIERZ_MODEL_AGENT_SECRET",
    )
    .map_err(|detail| Failure::config("seo-evaluate", detail))?;

    let routes = policy["routes"].as_array().ok_or_else(|| {
        Failure::config(
            "seo-evaluate",
            format!(
                "invalid SEO contract: {} routes are required",
                policy_path.display()
            ),
        )
    })?;
    let mut route_contracts = Vec::new();
    let mut ordinary = Vec::new();
    let mut googlebot = Vec::new();
    let mut deterministic_blockers = Vec::new();
    let mut deterministic_warnings = Vec::new();
    for route in routes {
        let path = route.get("path").and_then(JsonValue::as_str).unwrap_or("/");
        let url = canonical
            .join(path)
            .map_err(|error| Failure::config("seo-evaluate", error.to_string()))?;
        let mut declared = route.clone();
        if let Some(object) = declared.as_object_mut() {
            object.insert("url".to_string(), json!(url.as_str()));
        }
        route_contracts.push(declared);
        let page = seo_fetch(url.as_str(), "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")?;
        let bot = seo_fetch(url.as_str(), "Mozilla/5.0 (Linux; Android 6.0.1; Nexus 5X Build/MMB29P) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)")?;
        let indexable = route
            .get("indexable")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        if !(200..400).contains(&page["status"].as_u64().unwrap_or(0)) {
            deterministic_blockers.push(json!({ "code": "route_http_failure", "evidence": format!("{} returned {}", url, page["status"]), "source": "deterministic" }));
        }
        if indexable
            && page["robots"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("noindex")
        {
            deterministic_blockers.push(json!({ "code": "indexable_route_noindex", "evidence": format!("{url} declares noindex"), "source": "deterministic" }));
        }
        if indexable && page["title"].as_str().unwrap_or_default().is_empty() {
            deterministic_blockers.push(json!({ "code": "title_missing", "evidence": format!("{url} has no title"), "source": "deterministic" }));
        }
        if page["bodySha256"] != bot["bodySha256"] {
            deterministic_warnings.push(json!({ "code": "googlebot_content_differs", "evidence": format!("ordinary and Googlebot bodies differ for {url}") }));
        }
        ordinary.push(page);
        googlebot.push(bot);
    }
    let evidence = json!({
        "schemaVersion": 1,
        "collectedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "pages": ordinary,
        "googlebotPages": googlebot,
        "artifacts": []
    });
    let deterministic_score = if deterministic_blockers.is_empty() {
        1.0
    } else {
        0.0
    };
    let mut deterministic_dimensions = Map::new();
    for (name, rule) in policy["dimensions"].as_object().into_iter().flatten() {
        if matches!(rule["source"].as_str(), Some("deterministic" | "hybrid")) {
            deterministic_dimensions.insert(name.clone(), json!({
                "score": deterministic_score,
                "evidence": [format!("{} declared routes crawled as ordinary Chrome and Googlebot Smartphone", routes.len())],
                "issues": deterministic_blockers.iter().filter_map(|item| item.get("code").and_then(JsonValue::as_str)).collect::<Vec<_>>()
            }));
        }
    }
    let deterministic = json!({ "dimensions": deterministic_dimensions, "blockers": deterministic_blockers, "warnings": deterministic_warnings });
    let primary_grade = invoke_seo_model(
        &primary,
        &router_url,
        &token,
        &agent_id,
        &agent_secret,
        &policy,
        &brief,
        &evidence,
        &deterministic,
        None,
    )?;
    let secondary_grade = invoke_seo_model(
        &secondary,
        &router_url,
        &token,
        &agent_id,
        &agent_secret,
        &policy,
        &brief,
        &evidence,
        &deterministic,
        None,
    )?;
    if primary_grade["modelReturned"].is_null() || secondary_grade["modelReturned"].is_null() {
        return Err(Failure::config(
            "seo-evaluate.model",
            "SEO graders did not identify the model versions that produced their evaluations",
        ));
    }
    if primary_grade["modelReturned"] == secondary_grade["modelReturned"] {
        return Err(Failure::config(
            "seo-evaluate.model",
            format!(
                "SEO graders resolved to the same model {}",
                primary_grade["modelReturned"].as_str().unwrap_or_default()
            ),
        ));
    }
    let names: Vec<String> = policy["dimensions"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, rule)| matches!(rule["source"].as_str(), Some("model" | "hybrid")))
        .map(|(name, _)| name.clone())
        .collect();
    let score_delta = names
        .iter()
        .map(|name| {
            (primary_grade["evaluation"]["dimensions"][name]["score"]
                .as_f64()
                .unwrap_or(0.0)
                - secondary_grade["evaluation"]["dimensions"][name]["score"]
                    .as_f64()
                    .unwrap_or(0.0))
            .abs()
        })
        .fold(0.0_f64, f64::max);
    let primary_codes: BTreeSet<String> = primary_grade["evaluation"]["blocking_issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["code"].as_str().map(str::to_string))
        .collect();
    let secondary_codes: BTreeSet<String> = secondary_grade["evaluation"]["blocking_issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["code"].as_str().map(str::to_string))
        .collect();
    let blocker_mismatch = primary_codes != secondary_codes;
    let delta = policy
        .pointer("/model/adjudicationDelta")
        .and_then(JsonValue::as_f64)
        .unwrap_or(0.2);
    let divergence_required = score_delta > delta || blocker_mismatch;
    let adjudicator_grade = if divergence_required {
        let model = required_setting(
            adjudicator
                .map(|value| value.trim().to_string())
                .or_else(|| {
                    selected_setting(
                        Some(&loaded),
                        Some("web"),
                        "PROBIERZ_SEO_ADJUDICATOR_MODEL",
                        None,
                    )
                }),
            "PROBIERZ_SEO_ADJUDICATOR_MODEL",
        )
        .map_err(|detail| Failure::config("seo-evaluate", detail))?;
        if model == primary || model == secondary {
            return Err(Failure::config(
                "seo-evaluate",
                "SEO adjudicator model ID must differ from both graders",
            ));
        }
        Some(invoke_seo_model(
            &model,
            &router_url,
            &token,
            &agent_id,
            &agent_secret,
            &policy,
            &brief,
            &evidence,
            &deterministic,
            Some(&json!({
                "task": "Adjudicate the two evaluations against the original approved brief and page evidence.",
                "originalEvidence": evidence, "primary": primary_grade["evaluation"], "secondary": secondary_grade["evaluation"]
            })),
        )?)
    } else {
        None
    };
    let mut model_dimensions = Map::new();
    for name in &names {
        let graders: Vec<&JsonValue> = [&primary_grade, &secondary_grade]
            .into_iter()
            .chain(adjudicator_grade.iter())
            .collect();
        let score = adjudicator_grade
            .as_ref()
            .map(|grade| {
                grade["evaluation"]["dimensions"][name]["score"]
                    .as_f64()
                    .unwrap_or(0.0)
            })
            .unwrap_or_else(|| {
                primary_grade["evaluation"]["dimensions"][name]["score"]
                    .as_f64()
                    .unwrap_or(0.0)
                    .min(
                        secondary_grade["evaluation"]["dimensions"][name]["score"]
                            .as_f64()
                            .unwrap_or(0.0),
                    )
            });
        let mut seen_issues = BTreeSet::new();
        let model_issues: Vec<String> = graders
            .iter()
            .flat_map(|grade| {
                grade["evaluation"]["dimensions"][name]["issues"]
                    .as_array()
                    .into_iter()
                    .flatten()
            })
            .filter_map(JsonValue::as_str)
            .filter(|issue| seen_issues.insert((*issue).to_string()))
            .map(str::to_string)
            .collect();
        model_dimensions.insert(name.clone(), json!({
            "score": (score * 10_000.0).round() / 10_000.0,
            "evidence": graders.iter().flat_map(|grade| grade["evaluation"]["dimensions"][name]["evidence"].as_array().into_iter().flatten()).cloned().collect::<Vec<_>>(),
            "issues": model_issues,
            "graderScores": graders.iter().map(|grade| (grade["modelRequested"].as_str().unwrap_or_default().to_string(), grade["evaluation"]["dimensions"][name]["score"].clone())).collect::<Map<_, _>>()
        }));
    }
    let agreed_issues: Vec<JsonValue> = if let Some(grade) = &adjudicator_grade {
        grade["evaluation"]["blocking_issues"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        primary_grade["evaluation"]["blocking_issues"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| secondary_codes.contains(item["code"].as_str().unwrap_or_default()))
            .cloned()
            .collect()
    };
    let model_blockers: Vec<JsonValue> = agreed_issues
        .into_iter()
        .map(|mut item| {
            if let Some(object) = item.as_object_mut() {
                object.insert("source".to_string(), json!("model"));
            }
            item
        })
        .collect();
    let model_recommendations: Vec<JsonValue> = [&primary_grade, &secondary_grade]
        .into_iter()
        .chain(adjudicator_grade.iter())
        .flat_map(|grade| {
            grade["evaluation"]["recommendations"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .cloned()
        .collect();
    let model_evaluation = json!({
        "dimensions": model_dimensions,
        "blockers": model_blockers,
        "recommendations": model_recommendations,
        "divergence": { "required": divergence_required, "scoreDelta": (score_delta * 10_000.0).round() / 10_000.0, "blockerMismatch": blocker_mismatch },
        "graders": { "primary": primary_grade, "secondary": secondary_grade, "adjudicator": adjudicator_grade }
    });
    let mut dimensions = Map::new();
    let mut blockers = deterministic["blockers"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    blockers.extend(
        model_evaluation["blockers"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    let mut quality = 0.0;
    for (name, rule) in policy["dimensions"].as_object().into_iter().flatten() {
        let source = rule["source"].as_str().unwrap_or_default();
        let deterministic_value = &deterministic["dimensions"][name];
        let model_value = &model_evaluation["dimensions"][name];
        let score = match source {
            "deterministic" => deterministic_value["score"].as_f64().unwrap_or(0.0),
            "model" => model_value["score"].as_f64().unwrap_or(0.0),
            "hybrid" => deterministic_value["score"]
                .as_f64()
                .unwrap_or(0.0)
                .min(model_value["score"].as_f64().unwrap_or(0.0)),
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
            blockers.push(json!({ "code": format!("dimension_below_minimum:{name}"), "evidence": format!("{score:.3} < {minimum:.3}"), "source": "threshold" }));
        }
        let mut seen_issues = BTreeSet::new();
        let combined_issues: Vec<String> = deterministic_value["issues"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(model_value["issues"].as_array().into_iter().flatten())
            .filter_map(JsonValue::as_str)
            .filter(|issue| seen_issues.insert((*issue).to_string()))
            .map(str::to_string)
            .collect();
        dimensions.insert(name.clone(), json!({
            "label": rule.get("label").cloned().unwrap_or(JsonValue::Null), "source": source, "weight": weight,
            "minimum": minimum, "score": (score * 10_000.0).round() / 10_000.0,
            "evidence": deterministic_value["evidence"].as_array().into_iter().flatten().chain(model_value["evidence"].as_array().into_iter().flatten()).cloned().collect::<Vec<_>>(),
            "issues": combined_issues,
            "graderScores": model_value.get("graderScores").cloned().unwrap_or(JsonValue::Null)
        }));
    }
    quality = (quality * 10_000.0).round() / 10_000.0;
    let required_quality = policy["qualityMinimum"].as_f64().unwrap_or(0.0);
    if quality < required_quality {
        blockers.push(json!({ "code": "quality_below_minimum", "evidence": format!("{quality:.3} < {required_quality:.3}"), "source": "threshold" }));
    }
    let production = if let Some(file) = production_file {
        serde_json::from_slice(&fs::read(file)?)?
    } else {
        json!({ "required": mode == "production", "status": "not-provided", "blockers": [] })
    };
    if production.get("required").and_then(JsonValue::as_bool) == Some(true)
        && production.get("status").and_then(JsonValue::as_str) == Some("not-provided")
    {
        blockers.push(json!({ "code": "production_evidence_missing", "evidence": "production mode requires Search Console and CrUX evidence", "source": "production" }));
    }
    let selected_key_file = private_key_file
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("PROBIERZ_RECEIPT_PRIVATE_KEY_FILE").map(PathBuf::from));
    let key_bytes = if let Some(value) = private_key.filter(|value| !value.trim().is_empty()) {
        Some(value.as_bytes().to_vec())
    } else if let Ok(value) = std::env::var("PROBIERZ_SEO_RECEIPT_PRIVATE_KEY") {
        (!value.trim().is_empty()).then(|| value.into_bytes())
    } else if let Some(file) = selected_key_file.as_deref() {
        Some(fs::read(file)?)
    } else {
        None
    };
    let signature_required = seo
        .get("profiles")
        .and_then(|value| value.get(mode))
        .and_then(|value| value.get("requireSignature"))
        .and_then(YamlValue::as_bool)
        .unwrap_or(false);
    if signature_required && key_bytes.is_none() {
        blockers.push(json!({ "code": "evidence_signature_missing", "evidence": format!("{mode} SEO evidence requires an Ed25519 signing key"), "source": "evidence" }));
    }
    let source_identity = app_source_identity(harness, app_id, None)?;
    let mut payload = json!({
        "schemaVersion": 1, "kind": "probierz-seo-evaluation", "appId": app_id, "mode": mode,
        "issuedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "sourceIdentity": source_identity,
        "contract": {
            "policy": { "file": policy_path.to_string_lossy(), "name": policy["name"], "sha256": hex::encode(Sha256::digest(fs::read(&policy_path)?)) },
            "brief": { "file": brief_path.to_string_lossy(), "product": brief["product"], "sha256": hex::encode(Sha256::digest(fs::read(&brief_path)?)) },
            "baseUrl": canonical.as_str(), "routes": route_contracts
        },
        "verdict": {
            "pass": blockers.is_empty(), "searchEligibility": if deterministic["blockers"].as_array().is_none_or(Vec::is_empty) { "eligible" } else { "blocked" },
            "searchQuality": quality, "requiredQuality": required_quality,
            "productionOutcome": production.get("status").and_then(JsonValue::as_str).unwrap_or("not-provided"),
            "blockers": blockers, "warnings": deterministic["warnings"]
        },
        "dimensions": dimensions, "evidence": evidence, "model": model_evaluation, "production": production
    });
    let signing = key_bytes
        .as_deref()
        .map(|key| sign_seo_payload(&payload, key))
        .transpose()?;
    let report_id = if let Some(signing) = &signing {
        hex::encode(Sha256::digest(
            format!(
                "{}\n{}",
                canonical_json(&payload),
                signing["signature"].as_str().unwrap_or_default()
            )
            .as_bytes(),
        ))[..24]
            .to_string()
    } else {
        hex::encode(Sha256::digest(payload.to_string().as_bytes()))[..24].to_string()
    };
    let object = payload
        .as_object_mut()
        .ok_or_else(|| Failure::config("seo-evaluate", "SEO payload is invalid"))?;
    object.insert("reportId".to_string(), json!(report_id));
    object.insert(
        "signing".to_string(),
        signing.clone().unwrap_or(JsonValue::Null),
    );
    let file = if let Some(path) = output {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        if path
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some("json")
        {
            return Err(Failure::invalid(
                "seo-evaluate",
                "SEO output path must end in .json",
            ));
        }
        path
    } else {
        harness
            .join("test-results/seo")
            .join(app_id)
            .join(
                chrono::Utc::now()
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                    .replace([':', '.'], "-"),
            )
            .join("seo-evaluation.json")
    };
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)?;
    destination.set_permissions(fs::Permissions::from_mode(0o600))?;
    destination.write_all(serde_json::to_string_pretty(&payload)?.as_bytes())?;
    destination.write_all(b"\n")?;
    Ok(json!({
        "file": file.to_string_lossy(), "reportId": report_id,
        "pass": payload.pointer("/verdict/pass").and_then(JsonValue::as_bool).unwrap_or(false),
        "searchEligibility": payload.pointer("/verdict/searchEligibility").cloned().unwrap_or(JsonValue::Null),
        "searchQuality": quality,
        "productionOutcome": payload.pointer("/verdict/productionOutcome").cloned().unwrap_or(JsonValue::Null),
        "blockers": payload.pointer("/verdict/blockers").cloned().unwrap_or(json!([])),
        "signing": signing.map(|value| json!({ "algorithm": value["algorithm"], "publicKeyFingerprintSha256": value["publicKeyFingerprintSha256"], "payloadSha256": value["payloadSha256"] }))
    }))
}


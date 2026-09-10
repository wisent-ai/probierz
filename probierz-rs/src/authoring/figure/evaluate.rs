use serde_json::json;
use crate::authoring::*;

#[allow(clippy::too_many_arguments)]
pub fn evaluate_figure(
    harness: &Path,
    reference: &Path,
    candidate: &Path,
    rubric_file: Option<&Path>,
    output: Option<&Path>,
    model: Option<&str>,
    router_url: Option<&str>,
    tex_preamble: Option<&Path>,
    router_bearer: Option<&str>,
    agent_id: Option<&str>,
    agent_secret: Option<&str>,
) -> Result<JsonValue, Failure> {
    let resolve = |path: &Path| -> Result<PathBuf, Failure> {
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(std::env::current_dir()?.join(path))
        }
    };
    let reference = resolve(reference)?;
    let candidate = resolve(candidate)?;
    for (label, file) in [("reference", &reference), ("candidate", &candidate)] {
        if !file.is_file() {
            return Err(Failure::invalid(
                "figure-evaluate",
                format!("{label} is not a file: {}", file.display()),
            ));
        }
        let extension = file
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(
            extension.as_str(),
            "jpeg" | "jpg" | "pdf" | "png" | "svg" | "tex" | "webp"
        ) {
            return Err(Failure::invalid(
                "figure-evaluate",
                format!(
                    "{label} type is not supported: {}",
                    if extension.is_empty() {
                        "no extension"
                    } else {
                        &extension
                    }
                ),
            ));
        }
    }
    let rubric = figure_rubric(rubric_file)?;
    let (selected_model, selected_url) =
        figure_prerequisites(harness, None, None, model, router_url)
            .map_err(|detail| Failure::config("figure-evaluate", detail))?;
    let token = router_bearer
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("STADO_MODEL_ROUTER_TOKEN").ok())
        .unwrap_or_default()
        .trim()
        .to_string();
    if token.is_empty() {
        return Err(Failure::config(
            "figure-evaluate",
            "STADO_MODEL_ROUTER_TOKEN or an explicit router bearer is required",
        ));
    }
    if token.chars().any(char::is_whitespace) {
        return Err(Failure::invalid(
            "figure-evaluate",
            "model router bearer must not contain whitespace",
        ));
    }
    let id = agent_id
        .map(str::to_string)
        .or_else(|| std::env::var("PROBIERZ_MODEL_AGENT_ID").ok())
        .unwrap_or_default()
        .trim()
        .to_string();
    let secret = agent_secret
        .map(str::to_string)
        .or_else(|| std::env::var("PROBIERZ_MODEL_AGENT_SECRET").ok())
        .unwrap_or_default()
        .trim()
        .to_string();
    if id.is_empty() != secret.is_empty() {
        return Err(Failure::config(
            "figure-evaluate",
            "agent identity needs both an agent ID and an agent secret",
        ));
    }
    let candidate_extension = candidate
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    let stem = candidate
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("figure")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let report = output.map(resolve).transpose()?.unwrap_or_else(|| {
        let stamp = chrono::Utc::now()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            .replace([':', '.'], "-");
        std::env::current_dir()
            .unwrap_or_else(|_| harness.to_path_buf())
            .join("test-results/figure-evaluations")
            .join(format!("{stamp}-{stem}.probierz.json"))
    });
    if report
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
        != Some("json")
    {
        return Err(Failure::invalid(
            "figure-evaluate",
            "figure evaluation --out must end in .json",
        ));
    }
    let output_dir = report.parent().unwrap_or_else(|| Path::new("."));
    let output_stem = report
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("figure");
    let reference_output = output_dir.join(format!("{output_stem}-reference.png"));
    let candidate_output = output_dir.join(format!("{output_stem}-candidate.png"));
    for file in [&report, &reference_output, &candidate_output] {
        if file.exists() {
            return Err(Failure::invalid(
                "figure-evaluate",
                format!(
                    "figure evaluation output already exists: {}",
                    file.display()
                ),
            ));
        }
    }
    fs::create_dir_all(output_dir)?;
    let work = std::env::temp_dir().join(format!(
        "probierz-figure-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| Failure::config("figure-evaluate", error.to_string()))?
            .as_nanos()
    ));
    fs::create_dir_all(&work)?;
    let evaluated = (|| -> Result<JsonValue, Failure> {
        let reference_render = render_figure(&reference, &work, "reference", tex_preamble)
            .map_err(|detail| Failure::config("figure-evaluate.render", detail))?;
        let reference_geometry = figure_geometry(&reference_render)
            .map_err(|detail| Failure::config("figure-evaluate.render", detail))?;
        let image_magick = figure_process("magick", &["-version".to_string()], None)
            .map_err(|detail| Failure::config("figure-evaluate.render", detail))?
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let pdf_latex = if reference.extension() == Some(OsStr::new("tex"))
            || candidate.extension() == Some(OsStr::new("tex"))
        {
            JsonValue::String(
                figure_process("pdflatex", &["--version".to_string()], None)
                    .map_err(|detail| Failure::config("figure-evaluate.render", detail))?
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            )
        } else {
            JsonValue::Null
        };
        let identity = json!({
            "schemaVersion": 1,
            "kind": "probierz-figure-evaluation",
            "createdAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "inputs": {
                "reference": { "path": reference.to_string_lossy(), "sha256": hex::encode(Sha256::digest(fs::read(&reference)?)), "type": reference.extension().and_then(OsStr::to_str).unwrap_or_default() },
                "candidate": { "path": candidate.to_string_lossy(), "sha256": hex::encode(Sha256::digest(fs::read(&candidate)?)), "type": candidate_extension }
            },
            "rubric": rubric,
            "renderer": { "imageMagick": image_magick, "pdfLaTeX": pdf_latex, "texPreamble": tex_preamble.map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|| "built-in".to_string()) }
        });
        let candidate_render = match render_figure(&candidate, &work, "candidate", tex_preamble) {
            Ok(file) => file,
            Err(detail) => {
                fs::copy(&reference_render, &reference_output)?;
                let mut report_value = identity;
                let object = report_value.as_object_mut().ok_or_else(|| {
                    Failure::config("figure-evaluate", "figure identity is invalid")
                })?;
                object.insert("renders".to_string(), json!({
                    "reference": { "path": reference_output.to_string_lossy(), "sha256": hex::encode(Sha256::digest(fs::read(&reference_output)?)), "width": reference_geometry["width"], "height": reference_geometry["height"], "aspectRatio": reference_geometry["aspectRatio"], "contentBounds": reference_geometry["contentBounds"], "margins": reference_geometry["margins"] },
                    "candidate": JsonValue::Null
                }));
                object.insert(
                    "deterministic".to_string(),
                    json!({ "blockers": [], "aspectRatioDrift": JsonValue::Null }),
                );
                object.insert("model".to_string(), JsonValue::Null);
                object.insert("evaluation".to_string(), json!({
                    "summary": "The candidate could not be rendered, so no visual comparison was possible.",
                    "dimensions": {}, "blockers": [], "fidelityLosses": [],
                    "recommendations": [{ "priority": "critical", "action": "Fix the renderer error reported below and return a candidate that builds." }]
                }));
                object.insert("verdict".to_string(), json!({ "pass": false, "overall": 0, "blockers": [{ "code": "candidate_render_failed", "artifact": "candidate", "evidence": detail.chars().take(4_000).collect::<String>() }] }));
                object.insert("reportPath".to_string(), json!(report.to_string_lossy()));
                return Ok(report_value);
            }
        };
        let candidate_geometry = figure_geometry(&candidate_render)
            .map_err(|detail| Failure::config("figure-evaluate.render", detail))?;
        let deterministic = deterministic_figure(&reference_geometry, &candidate_geometry);
        let encode = |file: &Path| -> Result<String, Failure> {
            Ok(base64::engine::general_purpose::STANDARD.encode(fs::read(file)?))
        };
        let instructions: Vec<&str> = rubric["modelInstructions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(JsonValue::as_str)
            .collect();
        let body = json!({
            "model": selected_model, "max_tokens": 3200, "temperature": 0,
            "messages": [
                { "role": "system", "content": format!("You are the release evaluator for scientific figures.\n{}\nCall record_figure_evaluation exactly once. If the tool is unavailable, return only its arguments object as raw JSON.", instructions.join("\n")) },
                { "role": "user", "content": [
                    { "type": "text", "text": json!({ "task": "Evaluate the candidate scientific figure against the reference and every rubric dimension.", "dimensionCriteria": rubric["dimensions"], "deterministicGeometry": { "reference": reference_geometry, "candidate": candidate_geometry, "comparison": deterministic } }).to_string() },
                    { "type": "text", "text": "REFERENCE / INTERMEDIATE ARTIFACT" },
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", encode(&reference_render)?) } },
                    { "type": "text", "text": "CANDIDATE / FINAL ARTIFACT" },
                    { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{}", encode(&candidate_render)?) } }
                ]}
            ],
            "tools": [figure_tool(&rubric)]
        }).to_string();
        let (status, raw) = post_router(&selected_url, &token, &id, &secret, &body, 180)
            .map_err(|detail| Failure::unavailable("figure-evaluate.model", detail))?;
        let payload: JsonValue = serde_json::from_str(&raw).map_err(|_| {
            Failure::unavailable(
                "figure-evaluate.model",
                format!("model router returned non-JSON ({status})"),
            )
        })?;
        if !(200..300).contains(&status) {
            return Err(Failure::unavailable(
                "figure-evaluate.model",
                format!(
                    "model router HTTP {status}: {}",
                    payload
                        .pointer("/error/message")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("request failed")
                        .chars()
                        .take(500)
                        .collect::<String>()
                ),
            ));
        }
        let message = payload.pointer("/choices/0/message").ok_or_else(|| {
            Failure::config(
                "figure-evaluate.model",
                "model router returned no figure evaluation",
            )
        })?;
        let calls: Vec<&JsonValue> = message
            .get("tool_calls")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter(|call| {
                call.pointer("/function/name").and_then(JsonValue::as_str)
                    == Some("record_figure_evaluation")
            })
            .collect();
        if calls.len() > 1 {
            return Err(Failure::config("figure-evaluate.model", format!("model router returned {} record_figure_evaluation calls; exactly one is required", calls.len())));
        }
        let raw_evaluation = if let Some(call) = calls.first() {
            call.pointer("/function/arguments")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_string()
        } else {
            message
                .get("content")
                .and_then(JsonValue::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let start = raw_evaluation.find('{').unwrap_or(0);
        let end = raw_evaluation
            .rfind('}')
            .map(|index| index + 1)
            .unwrap_or(raw_evaluation.len());
        let mut evaluation: JsonValue = serde_json::from_str(
            raw_evaluation.get(start..end).unwrap_or_default(),
        )
        .map_err(|_| {
            Failure::config(
                "figure-evaluate.model",
                "model router returned an unparseable figure evaluation",
            )
        })?;
        if evaluation
            .get("summary")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(Failure::config(
                "figure-evaluate.model",
                "figure model summary is required",
            ));
        }
        if let Some(value) = evaluation
            .as_object_mut()
            .and_then(|object| object.remove("fidelity_losses"))
        {
            evaluation
                .as_object_mut()
                .map(|object| object.insert("fidelityLosses".to_string(), value));
        }
        let mut threshold = Vec::new();
        let mut overall = 0.0;
        for (name, rule) in rubric["dimensions"].as_object().into_iter().flatten() {
            let score = evaluation["dimensions"][name]["score"]
                .as_f64()
                .ok_or_else(|| {
                    Failure::config(
                        "figure-evaluate.model",
                        format!("figure model {name}.score is invalid"),
                    )
                })?;
            let weight = rule["weight"].as_f64().unwrap_or(0.0);
            let minimum = rule["minimum"].as_f64().unwrap_or(0.0);
            overall += score * weight;
            if score < minimum {
                threshold.push(json!({ "code": format!("dimension_below_minimum:{name}"), "artifact": "comparison", "evidence": format!("{score:.3} < {minimum:.3}") }));
            }
        }
        overall = (overall * 10_000.0).round() / 10_000.0;
        let overall_minimum = rubric["overallMinimum"].as_f64().unwrap_or(0.0);
        if overall < overall_minimum {
            threshold.push(json!({ "code": "overall_below_minimum", "artifact": "comparison", "evidence": format!("{overall:.3} < {overall_minimum:.3}") }));
        }
        let mut blockers = deterministic["blockers"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        blockers.extend(
            evaluation["blockers"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        blockers.extend(threshold);
        fs::copy(&reference_render, &reference_output)?;
        fs::copy(&candidate_render, &candidate_output)?;
        let mut report_value = identity;
        let object = report_value
            .as_object_mut()
            .ok_or_else(|| Failure::config("figure-evaluate", "figure identity is invalid"))?;
        object.insert("renders".to_string(), json!({
            "reference": { "path": reference_output.to_string_lossy(), "sha256": hex::encode(Sha256::digest(fs::read(&reference_output)?)), "width": reference_geometry["width"], "height": reference_geometry["height"], "aspectRatio": reference_geometry["aspectRatio"], "contentBounds": reference_geometry["contentBounds"], "margins": reference_geometry["margins"] },
            "candidate": { "path": candidate_output.to_string_lossy(), "sha256": hex::encode(Sha256::digest(fs::read(&candidate_output)?)), "width": candidate_geometry["width"], "height": candidate_geometry["height"], "aspectRatio": candidate_geometry["aspectRatio"], "contentBounds": candidate_geometry["contentBounds"], "margins": candidate_geometry["margins"] }
        }));
        object.insert("deterministic".to_string(), deterministic);
        object.insert("model".to_string(), json!({ "name": payload.get("model").and_then(JsonValue::as_str).unwrap_or(&selected_model), "usage": payload.get("usage").cloned().unwrap_or(JsonValue::Null), "attempts": 1 }));
        object.insert("evaluation".to_string(), evaluation);
        object.insert(
            "verdict".to_string(),
            json!({ "pass": blockers.is_empty(), "overall": overall, "blockers": blockers }),
        );
        object.insert("reportPath".to_string(), json!(report.to_string_lossy()));
        Ok(report_value)
    })();
    let _ = fs::remove_dir_all(&work);
    let value = evaluated?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report)?;
    file.write_all(serde_json::to_string_pretty(&value)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(value)
}


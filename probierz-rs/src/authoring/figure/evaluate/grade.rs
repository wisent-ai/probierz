//! Asking a vision model to score the candidate against the reference,
//! and turning its one tool call into scores.
//!
//! The model is shown both renders and the geometry measured from them,
//! and must answer with exactly one `record_figure_evaluation` call.
//! More than one call is an error rather than a choice, and a missing
//! summary or an unscored dimension is an error too — a figure
//! evaluation with a hole in it is not evidence.

use super::*;

/// The tool call the model must make, once.
const TOOL: &str = "record_figure_evaluation";

/// Room for the evaluation, and no sampling: the same figures must
/// score the same way twice.
const MAX_TOKENS: u64 = 3200;
const TEMPERATURE: u64 = 0;

/// The router is asked exactly once. A retry would let a second
/// sampling of the same figures produce a different verdict, so the
/// report records the one attempt it made.
const ATTEMPTS: u64 = 1;

/// How long the router may take to answer, in seconds. A vision call
/// over two full-page renders is not fast.
const ROUTER_TIMEOUT_SECONDS: u64 = 180;

/// Scores are reported to four decimal places.
const SCORE_SCALE: f64 = 10_000.0;

/// How much of a long router error is quoted.
const ERROR_EXCERPT: usize = 500;

/// HTTP statuses the router may answer with and still have produced an
/// evaluation.
const ROUTER_OK: std::ops::Range<u16> = 200..300;

/// What the model said, and what its scores add up to.
pub(crate) struct Graded {
    pub(crate) evaluation: JsonValue,
    pub(crate) model: JsonValue,
    pub(crate) overall: f64,
    /// Blockers from the thresholds in the rubric.
    pub(crate) threshold_blockers: Vec<JsonValue>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn grade_figure(
    rubric: &JsonValue,
    router: &Router,
    deterministic: &JsonValue,
    reference_render: &Path,
    reference_geometry: &JsonValue,
    candidate_render: &Path,
    candidate_geometry: &JsonValue,
) -> Result<Graded, Failure> {
    let body = request_body(
        rubric,
        router,
        deterministic,
        reference_render,
        reference_geometry,
        candidate_render,
        candidate_geometry,
    )?;
    let (status, raw) = post_router(
        &router.url,
        &router.token,
        &router.agent_id,
        &router.agent_secret,
        &body,
        ROUTER_TIMEOUT_SECONDS,
    )
    .map_err(|detail| Failure::unavailable("figure-evaluate.model", detail))?;

    let payload: JsonValue = serde_json::from_str(&raw).map_err(|_| {
        Failure::unavailable(
            "figure-evaluate.model",
            format!("model router returned non-JSON ({status})"),
        )
    })?;
    if !ROUTER_OK.contains(&status) {
        return Err(Failure::unavailable(
            "figure-evaluate.model",
            format!(
                "model router HTTP {status}: {}",
                payload
                    .pointer("/error/message")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("request failed")
                    .chars()
                    .take(ERROR_EXCERPT)
                    .collect::<String>()
            ),
        ));
    }

    let evaluation = evaluation_from(&payload)?;
    let (overall, threshold_blockers) = scores(rubric, &evaluation)?;
    Ok(Graded {
        model: json!({
            "name": payload.get("model").and_then(JsonValue::as_str).unwrap_or(&router.model),
            "usage": payload.get("usage").cloned().unwrap_or(JsonValue::Null),
            "attempts": ATTEMPTS
        }),
        evaluation,
        overall,
        threshold_blockers,
    })
}

#[allow(clippy::too_many_arguments)]
fn request_body(
    rubric: &JsonValue,
    router: &Router,
    deterministic: &JsonValue,
    reference_render: &Path,
    reference_geometry: &JsonValue,
    candidate_render: &Path,
    candidate_geometry: &JsonValue,
) -> Result<String, Failure> {
    let instructions: Vec<&str> = rubric["modelInstructions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect();
    let task = json!({
        "task": "Evaluate the candidate scientific figure against the reference and every rubric dimension.",
        "dimensionCriteria": rubric["dimensions"],
        "deterministicGeometry": {
            "reference": reference_geometry,
            "candidate": candidate_geometry,
            "comparison": deterministic
        }
    });
    Ok(json!({
        "model": router.model, "max_tokens": MAX_TOKENS, "temperature": TEMPERATURE,
        "messages": [
            { "role": "system", "content": format!(
                "You are the release evaluator for scientific figures.\n{}\nCall {TOOL} exactly once. If the tool is unavailable, return only its arguments object as raw JSON.",
                instructions.join("\n")
            ) },
            { "role": "user", "content": [
                { "type": "text", "text": task.to_string() },
                { "type": "text", "text": "REFERENCE / INTERMEDIATE ARTIFACT" },
                { "type": "image_url", "image_url": { "url": data_url(reference_render)? } },
                { "type": "text", "text": "CANDIDATE / FINAL ARTIFACT" },
                { "type": "image_url", "image_url": { "url": data_url(candidate_render)? } }
            ]}
        ],
        "tools": [figure_tool(rubric)]
    })
    .to_string())
}

fn data_url(file: &Path) -> Result<String, Failure> {
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(fs::read(file)?)
    ))
}

/// The evaluation object: the model's single tool call, or the raw JSON
/// object it returned when the tool was unavailable to it.
fn evaluation_from(payload: &JsonValue) -> Result<JsonValue, Failure> {
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
        .filter(|call| call.pointer("/function/name").and_then(JsonValue::as_str) == Some(TOOL))
        .collect();
    if calls.len() > 1 {
        return Err(Failure::config(
            "figure-evaluate.model",
            format!(
                "model router returned {} {TOOL} calls; exactly one is required",
                calls.len()
            ),
        ));
    }
    let raw = match calls.first() {
        Some(call) => call
            .pointer("/function/arguments")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
        None => message
            .get("content")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string(),
    };

    // Models sometimes wrap the object in prose; take the object.
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').map(|index| index + 1).unwrap_or(raw.len());
    let mut evaluation: JsonValue = serde_json::from_str(raw.get(start..end).unwrap_or_default())
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
    // The tool declares fidelityLosses; accept the snake_case spelling
    // a model may answer with and report the declared one.
    if let Some(value) = evaluation
        .as_object_mut()
        .and_then(|object| object.remove("fidelity_losses"))
    {
        if let Some(object) = evaluation.as_object_mut() {
            object.insert("fidelityLosses".to_string(), value);
        }
    }
    Ok(evaluation)
}

/// The weighted overall score, and one blocker per dimension that came
/// in under its declared minimum.
fn scores(rubric: &JsonValue, evaluation: &JsonValue) -> Result<(f64, Vec<JsonValue>), Failure> {
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
            threshold.push(json!({
                "code": format!("dimension_below_minimum:{name}"),
                "artifact": "comparison",
                "evidence": format!("{score:.3} < {minimum:.3}")
            }));
        }
    }
    let overall = (overall * SCORE_SCALE).round() / SCORE_SCALE;
    let overall_minimum = rubric["overallMinimum"].as_f64().unwrap_or(0.0);
    if overall < overall_minimum {
        threshold.push(json!({
            "code": "overall_below_minimum",
            "artifact": "comparison",
            "evidence": format!("{overall:.3} < {overall_minimum:.3}")
        }));
    }
    Ok((overall, threshold))
}

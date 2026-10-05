//! Asking a vision model to judge the candidate against the reference,
//! and turning its one tool call into criterion blockers.
//!
//! The model is shown both renders and the geometry measured from them,
//! and must answer with exactly one `record_figure_evaluation` call that
//! says, for every rubric dimension, whether its criterion is met. More
//! than one call is an error rather than a choice, and a missing summary
//! or an unjudged dimension is an error too — a figure evaluation with a
//! hole in it is not evidence.

use super::*;

/// The tool call the model must make, once.
const TOOL: &str = "record_figure_evaluation";

/// No sampling: the same figures must score the same way twice. The
/// answer's length is the routed model's own limit, not a budget chosen here.
const TEMPERATURE: u64 = 0;

/// The router is asked exactly once. A retry would let a second
/// sampling of the same figures produce a different verdict, so the
/// report records the one attempt it made.
const ATTEMPTS: u64 = 1;

/// HTTP statuses the router may answer with and still have produced an
/// evaluation.
const ROUTER_OK: std::ops::Range<u16> = 200..300;

/// What the model said, and which criteria it found unmet.
pub(crate) struct Graded {
    pub(crate) evaluation: JsonValue,
    pub(crate) model: JsonValue,
    /// One blocker per rubric dimension the model judged unmet.
    pub(crate) criterion_blockers: Vec<JsonValue>,
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
    )
    .map_err(|detail| Failure::unavailable("figure-evaluate.model", detail))?;

    let payload: JsonValue = serde_json::from_str(&raw).map_err(|_| {
        Failure::unavailable(
            "figure-evaluate.model",
            format!("model router returned non-JSON ({status}): {}", raw.trim()),
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
            ),
        ));
    }

    let evaluation = evaluation_from(&payload)?;
    let criterion_blockers = unmet(rubric, &evaluation)?;
    Ok(Graded {
        model: json!({
            "name": payload.get("model").and_then(JsonValue::as_str).unwrap_or(&router.model),
            "usage": payload.get("usage").cloned().unwrap_or(JsonValue::Null),
            "attempts": ATTEMPTS
        }),
        evaluation,
        criterion_blockers,
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
        "model": router.model, "temperature": TEMPERATURE,
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

/// One blocker per rubric dimension the model judged unmet, carrying the
/// issues it named.
fn unmet(rubric: &JsonValue, evaluation: &JsonValue) -> Result<Vec<JsonValue>, Failure> {
    let mut blockers = Vec::new();
    for name in rubric["dimensions"]
        .as_object()
        .into_iter()
        .flat_map(Map::keys)
    {
        let judged = &evaluation["dimensions"][name];
        let met = judged["met"].as_bool().ok_or_else(|| {
            Failure::config(
                "figure-evaluate.model",
                format!("figure model {name}.met is missing or not a boolean"),
            )
        })?;
        if !met {
            let issues: Vec<&str> = judged["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(JsonValue::as_str)
                .collect();
            blockers.push(json!({
                "code": format!("criterion_unmet:{name}"),
                "artifact": "comparison",
                "evidence": if issues.is_empty() {
                    "the model judged this criterion unmet and named no issue".to_string()
                } else {
                    issues.join("; ")
                }
            }));
        }
    }
    Ok(blockers)
}

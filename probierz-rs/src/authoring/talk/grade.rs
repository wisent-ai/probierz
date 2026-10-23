//! Asking a vision model to judge the talk, and turning its one tool call
//! into unmet criteria.
//!
//! The model is shown the full text of every source document, the speaker
//! script, and every slide picture in order, and must answer with exactly one
//! `record_talk_evaluation` call that says, for every rubric dimension,
//! whether its criterion is met with the evidence for that answer. The router
//! is asked once: a second sampling could give a different verdict, and the
//! report records the one the evaluation got.

use super::*;

/// The tool call the model must make, once.
const TOOL: &str = "record_talk_evaluation";

/// What the model said, and which criteria it found unmet.
pub(crate) struct Graded {
    pub(crate) evaluation: JsonValue,
    pub(crate) model: JsonValue,
    pub(crate) unmet: Vec<JsonValue>,
}

pub(crate) fn grade_talk(
    talk: &Talk,
    rubric: &JsonValue,
    router: &TalkRouter,
) -> Result<Graded, Failure> {
    let body = request_body(talk, rubric, router)?;
    let (status, raw) = post_router(
        &router.url,
        &router.token,
        &router.agent_id,
        &router.agent_secret,
        &body,
    )
    .map_err(|detail| Failure::unavailable("evaluate.talk.model", detail))?;
    let payload: JsonValue = serde_json::from_str(&raw).map_err(|error| {
        Failure::unavailable(
            "evaluate.talk.model",
            format!(
                "model router answered {status} with a body that is not JSON ({error}): {}",
                raw.trim()
            ),
        )
    })?;
    let answered = http::StatusCode::from_u16(status).map_err(|error| {
        Failure::unavailable(
            "evaluate.talk.model",
            format!("model router answered status {status}: {error}"),
        )
    })?;
    if !answered.is_success() {
        return Err(Failure::unavailable(
            "evaluate.talk.model",
            router_refusal(status, &payload, &raw),
        ));
    }
    let evaluation = tool_call_arguments(&payload, TOOL)
        .map_err(|detail| Failure::config("evaluate.talk.model", detail))?;
    let summarized = evaluation
        .get("summary")
        .and_then(JsonValue::as_str)
        .is_some_and(|text| !text.trim().is_empty());
    if !summarized {
        return Err(Failure::config(
            "evaluate.talk.model",
            "the talk evaluation states no summary",
        ));
    }
    let unmet = unmet(rubric, &evaluation)?;
    // The router names the model that answered, or the report says it did not:
    // the requested model is not evidence of which one judged the talk.
    let answered = payload.get("model").and_then(JsonValue::as_str);
    let usage = payload.get("usage").cloned();
    Ok(Graded {
        model: json!({ "name": answered, "requested": router.model, "usage": usage }),
        evaluation,
        unmet,
    })
}

fn request_body(talk: &Talk, rubric: &JsonValue, router: &TalkRouter) -> Result<String, Failure> {
    let instructions: Vec<&str> = rubric["modelInstructions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(JsonValue::as_str)
        .collect();
    let mut content = vec![json!({ "type": "text", "text": json!({
        "task": "Evaluate this talk, given to a review panel, against every rubric dimension.",
        "dimensionCriteria": rubric["dimensions"],
        "slides": talk.slides.len()
    }).to_string() })];
    for source in &talk.sources {
        content.push(json!({ "type": "text", "text": format!(
            "SOURCE DOCUMENT {}\n{}", source.path.display(), source.text
        ) }));
    }
    content
        .push(json!({ "type": "text", "text": format!("SPEAKER SCRIPT\n{}", talk.script_text) }));
    for (number, slide) in (std::num::NonZeroUsize::MIN.get()..).zip(&talk.slides) {
        content.push(json!({ "type": "text", "text": format!("SLIDE {number}") }));
        content.push(json!({ "type": "image_url", "image_url": { "url": format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&slide.bytes)
        ) } }));
    }
    Ok(json!({
        "model": router.model,
        "messages": [
            { "role": "system", "content": format!(
                "You are the release evaluator for talks given to review panels.\n{}\nCall {TOOL} exactly once.",
                instructions.join("\n")
            ) },
            { "role": "user", "content": content }
        ],
        "tools": [tool(rubric)]
    })
    .to_string())
}

fn tool(rubric: &JsonValue) -> JsonValue {
    let names: Vec<&String> = rubric["dimensions"]
        .as_object()
        .into_iter()
        .flat_map(Map::keys)
        .collect();
    let judged = json!({ "type": "object", "properties": {
        "met": { "type": "boolean" },
        "evidence": { "type": "array", "items": { "type": "string" } },
        "issues": { "type": "array", "items": { "type": "string" } }
    }, "required": ["met", "evidence", "issues"], "additionalProperties": false });
    let properties: Map<String, JsonValue> = names
        .iter()
        .map(|name| ((*name).clone(), judged.clone()))
        .collect();
    json!({ "type": "function", "function": {
        "name": TOOL, "description": "Record one evidence-grounded evaluation of the talk.",
        "parameters": { "type": "object", "properties": {
            "summary": { "type": "string" },
            "dimensions": { "type": "object", "properties": properties, "required": names, "additionalProperties": false },
            "blockers": { "type": "array", "items": { "type": "object", "properties": {
                "code": { "type": "string" }, "slide": { "type": "string" }, "evidence": { "type": "string" }
            }, "required": ["code", "slide", "evidence"], "additionalProperties": false }},
            "recommendations": { "type": "array", "items": { "type": "object", "properties": {
                "slide": { "type": "string" }, "action": { "type": "string" }
            }, "required": ["slide", "action"], "additionalProperties": false }}
        }, "required": ["summary", "dimensions", "blockers", "recommendations"], "additionalProperties": false }
    }})
}

/// One blocker per rubric dimension the model judged unmet, carrying the
/// issues it named; a dimension the model did not judge is refused.
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
                "evaluate.talk.model",
                format!("the talk evaluation does not judge {name}.met"),
            )
        })?;
        if !met {
            let issues: Vec<&str> = judged["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(JsonValue::as_str)
                .collect();
            let evidence = if issues.is_empty() {
                "the model judged this criterion unmet and named no issue".to_string()
            } else {
                issues.join("; ")
            };
            blockers
                .push(json!({ "code": format!("criterion_unmet:{name}"), "evidence": evidence }));
        }
    }
    Ok(blockers)
}

//! Asking the routed model what it thinks of the page.
//!
//! The tool shape is built from the rubric, so a dimension added to the rubric
//! is a dimension the model must answer for, and a recommendation priority
//! the rubric declares is the only one it may use. The request goes through
//! the Stado model router with the captured screenshots attached, and the
//! reply is read strictly: an answer missing a graded dimension, or scoring
//! one outside its range, is a failed evaluation rather than a pass with a
//! hole in it.

use base64::Engine;
use serde_json::{json, Map};

use super::super::constants::{
    CAPTURE_TYPE, DIMENSION_SCHEMA, HTTP_SERVED, MAX_OUTPUT_TOKENS, MAX_OUTPUT_TOKENS_BOUNDS,
    ROUTER_BUDGET_SECONDS, TEMPERATURE,
};
use super::super::inputs::{non_empty, string_array};
use crate::authoring::{post_router, stado_model_router_url};
use crate::specs::*;

const TOOL: &str = "record_landing_page_evaluation";

fn tool_schema(rubric: &Value) -> Result<Value, String> {
    let dimension: Value = serde_json::from_str(DIMENSION_SCHEMA)
        .map_err(|error| format!("dimension schema: {error}"))?;
    let names: Vec<String> = rubric["dimensions"]
        .as_object()
        .map(|all| all.keys().cloned().collect())
        .unwrap_or_default();
    let properties: Map<String, Value> = names
        .iter()
        .map(|name| (name.clone(), dimension.clone()))
        .collect();
    Ok(json!({
        "type": "function",
        "function": {
            "name": TOOL,
            "description": "Record one evidence-grounded landing page evaluation.",
            "parameters": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "dimensions": { "type": "object", "properties": properties, "required": names, "additionalProperties": false },
                    "blocking_issues": { "type": "array", "items": {
                        "type": "object",
                        "properties": { "code": { "type": "string" }, "evidence": { "type": "string" } },
                        "required": ["code", "evidence"], "additionalProperties": false } },
                    "recommendations": { "type": "array", "items": {
                        "type": "object",
                        "properties": {
                            "priority": { "type": "string", "enum": rubric["recommendationPriorities"] },
                            "dimension": { "type": "string" },
                            "action": { "type": "string" } },
                        "required": ["priority", "dimension", "action"], "additionalProperties": false } },
                },
                "required": ["summary", "dimensions", "blocking_issues", "recommendations"],
                "additionalProperties": false,
            },
        },
    }))
}

/// The model's answer, checked against every rubric dimension.
fn parse(value: &Value, rubric: &Value) -> Result<Value, String> {
    let candidate = value
        .as_object()
        .ok_or("model evaluation must be an object")?;
    let summary = non_empty(&value["summary"], "model summary")?;
    let raw = candidate
        .get("dimensions")
        .and_then(Value::as_object)
        .ok_or("model evaluation dimensions must be an object")?;
    let mut dimensions = Map::new();
    for name in rubric["dimensions"]
        .as_object()
        .into_iter()
        .flat_map(Map::keys)
    {
        let answer = raw
            .get(name)
            .filter(|answer| answer.is_object())
            .ok_or_else(|| format!("model evaluation {name} must be an object"))?;
        let score = answer["score"]
            .as_f64()
            .filter(|score| score.is_finite() && (0.0..=1.0).contains(score))
            .ok_or_else(|| format!("model evaluation {name}.score must be between 0 and 1"))?;
        let evidence = string_array(
            &answer["evidence"],
            &format!("model evaluation {name}.evidence"),
        )?;
        let issues = answer["issues"]
            .as_array()
            .filter(|issues| issues.iter().all(Value::is_string))
            .ok_or_else(|| format!("model evaluation {name}.issues must be a string array"))?;
        dimensions.insert(
            name.clone(),
            json!({ "score": score, "evidence": evidence, "issues": issues }),
        );
    }
    let blocking = candidate
        .get("blocking_issues")
        .and_then(Value::as_array)
        .ok_or("model evaluation blocking_issues must be an array")?;
    let blocking_issues = blocking
        .iter()
        .enumerate()
        .map(|(index, issue)| {
            Ok(json!({
                "code": non_empty(&issue["code"], &format!("model blocking_issues.{index}.code"))?,
                "evidence": non_empty(&issue["evidence"], &format!("model blocking_issues.{index}.evidence"))?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let declared = rubric["recommendationPriorities"]
        .as_array()
        .ok_or("rubric.recommendationPriorities is missing")?;
    let advice = candidate
        .get("recommendations")
        .and_then(Value::as_array)
        .ok_or("model evaluation recommendations must be an array")?;
    let recommendations = advice
        .iter()
        .enumerate()
        .map(|(index, item)| {
            if !declared.contains(&item["priority"]) {
                return Err(format!("model recommendations.{index}.priority is invalid"));
            }
            Ok(json!({
                "priority": item["priority"],
                "dimension": non_empty(&item["dimension"], &format!("model recommendations.{index}.dimension"))?,
                "action": non_empty(&item["action"], &format!("model recommendations.{index}.action"))?,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(
        json!({ "summary": summary, "dimensions": dimensions, "blocking_issues": blocking_issues, "recommendations": recommendations }),
    )
}

/// Ask the routed vision model to grade the page; answers
/// { evaluation, routerModel, usage }.
pub(in super::super) fn evaluate(
    context: &Context,
    rubric: &Value,
    brief: &Value,
    audits: &Value,
    images: &[(String, PathBuf)],
) -> Result<Value, String> {
    let router = stado_model_router_url(context.optional("STADO_MODEL_ROUTER_URL").as_deref())?;
    let token = context.required("STADO_MODEL_ROUTER_TOKEN", "the Stado model router bearer")?;
    let model = context.required(
        "PROBIERZ_LANDING_VISION_MODEL",
        "the vision model the router serves",
    )?;
    let tokens = match context.optional("PROBIERZ_LANDING_MAX_OUTPUT_TOKENS") {
        None => MAX_OUTPUT_TOKENS,
        Some(raw) => raw
            .parse::<u64>()
            .ok()
            .filter(|tokens| MAX_OUTPUT_TOKENS_BOUNDS.contains(tokens))
            .ok_or_else(|| {
                format!(
                    "PROBIERZ_LANDING_MAX_OUTPUT_TOKENS must be an integer between {} and {}",
                    MAX_OUTPUT_TOKENS_BOUNDS.start(),
                    MAX_OUTPUT_TOKENS_BOUNDS.end()
                )
            })?,
    };
    let task = json!({
        "task": "Evaluate this landing page against the approved brief and every rubric dimension.",
        "approvedBrief": brief,
        "dimensionCriteria": rubric["dimensions"],
        "deterministicBrowserEvidence": audits,
    });
    let mut content = vec![json!({ "type": "text", "text": task.to_string() })];
    for (label, path) in images {
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        content.push(json!({ "type": "text", "text": label }));
        content.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{CAPTURE_TYPE};base64,{data}") } }));
    }
    let mut system = vec!["You are the release evaluator for a landing page.".to_string()];
    system.extend(string_array(
        &rubric["modelInstructions"],
        "rubric.modelInstructions",
    )?);
    system.push(format!(
        "Call {TOOL} exactly once. Return no prose outside the tool call."
    ));
    let body = json!({
        "model": model,
        "max_tokens": tokens,
        "temperature": TEMPERATURE,
        "messages": [{ "role": "system", "content": system.join("\n") }, { "role": "user", "content": content }],
        "tools": [tool_schema(rubric)?],
        "tool_choice": { "type": "function", "function": { "name": TOOL } },
    })
    .to_string();
    let agent = context.required(
        "PROBIERZ_MODEL_AGENT_ID",
        "the agent identity the router admits",
    )?;
    let secret = context.required("PROBIERZ_MODEL_AGENT_SECRET", "that agent's signing secret")?;
    let (status, raw) = post_router(
        &router,
        &token,
        &agent,
        &secret,
        &body,
        ROUTER_BUDGET_SECONDS,
    )?;
    let payload: Value = serde_json::from_str(&raw)
        .map_err(|_| format!("model router returned non-JSON ({status})"))?;
    if !payload.is_object() {
        return Err(format!(
            "model router returned an invalid response object ({status})"
        ));
    }
    if !HTTP_SERVED.contains(&u64::from(status)) {
        let detail = payload["error"]["message"]
            .as_str()
            .unwrap_or("request failed");
        return Err(format!(
            "model router HTTP {status}: {}",
            detail.chars().take(500).collect::<String>()
        ));
    }
    let calls = payload
        .pointer("/choices/0/message/tool_calls")
        .and_then(Value::as_array)
        .ok_or("model router must return exactly one landing evaluation tool call")?;
    let matching: Vec<&Value> = calls
        .iter()
        .filter(|call| {
            call["type"] == "function"
                && call["function"]["name"] == TOOL
                && call["function"]["arguments"].is_string()
        })
        .collect();
    let [call] = matching.as_slice() else {
        return Err("model router must return exactly one landing evaluation tool call".into());
    };
    let text = call["function"]["arguments"]
        .as_str()
        .ok_or("model router returned missing landing evaluation arguments")?;
    let arguments: Value = serde_json::from_str(text)
        .map_err(|_| "model router returned invalid landing evaluation arguments".to_string())?;
    Ok(json!({
        "evaluation": parse(&arguments, rubric)?,
        "routerModel": payload["model"].as_str(),
        "usage": payload["usage"],
    }))
}

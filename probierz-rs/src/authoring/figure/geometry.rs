use crate::authoring::*;
use serde_json::json;
pub(crate) fn figure_geometry(png: &Path) -> Result<JsonValue, String> {
    let dimensions = figure_process(
        "magick",
        &[
            "identify".to_string(),
            "-format".to_string(),
            "%w %h".to_string(),
            png.to_string_lossy().into_owned(),
        ],
        None,
    )?;
    let mut parts = dimensions.split_whitespace();
    let width = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| format!("could not read render dimensions: {}", png.display()))?;
    let height = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| format!("could not read render dimensions: {}", png.display()))?;
    // The content box is everything that differs from the corner pixel's
    // colour; no fuzz percentage is chosen, so any drawn pixel counts.
    let trimmed = figure_process(
        "magick",
        &[
            png.to_string_lossy().into_owned(),
            "-trim".to_string(),
            "-format".to_string(),
            "%w %h %X %Y".to_string(),
            "info:".to_string(),
        ],
        None,
    )?;
    let pieces: Vec<&str> = trimmed.split_whitespace().collect();
    let content_width = pieces
        .first()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(width);
    let content_height = pieces
        .get(1)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(height);
    let signed = |raw: Option<&&str>| {
        raw.copied()
            .unwrap_or("+0")
            .trim_start_matches('+')
            .parse::<i64>()
            .unwrap_or(0)
    };
    let x = signed(pieces.get(2));
    let y = signed(pieces.get(3));
    let aspect = width as f64 / height as f64;
    Ok(json!({
        "width": width, "height": height, "aspectRatio": aspect,
        "contentBounds": { "x": x, "y": y, "width": content_width, "height": content_height },
        "margins": {
            "left": x.max(0), "top": y.max(0),
            "right": (width - x - content_width).max(0), "bottom": (height - y - content_height).max(0)
        }
    }))
}

/// Facts measured without a model. Content that reaches a canvas edge (no
/// background pixel between it and that edge) blocks: it is clipped or flush
/// by definition. Size and aspect ratio are reported for the model to judge
/// against the rubric; no pixel minimum or drift percentage is chosen.
pub(crate) fn deterministic_figure(reference: &JsonValue, candidate: &JsonValue) -> JsonValue {
    let mut blockers = Vec::new();
    for (label, geometry) in [("reference", reference), ("candidate", candidate)] {
        let touching: Vec<&str> = geometry["margins"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, margin)| margin.as_i64() == Some(0))
            .map(|(edge, _)| edge.as_str())
            .collect();
        if !touching.is_empty() {
            blockers.push(json!({ "code": format!("{label}_content_at_canvas_edge"), "artifact": label, "evidence": format!("non-background content reaches: {}", touching.join(", ")) }));
        }
    }
    let drift = match (
        reference["aspectRatio"].as_f64(),
        candidate["aspectRatio"].as_f64(),
    ) {
        (Some(left), Some(right)) => json!((right - left).abs() / left),
        _ => JsonValue::Null,
    };
    json!({ "blockers": blockers, "aspectRatioDrift": drift })
}

pub(crate) fn figure_tool(rubric: &JsonValue) -> JsonValue {
    let names: Vec<String> = rubric["dimensions"]
        .as_object()
        .into_iter()
        .flat_map(|value| value.keys())
        .cloned()
        .collect();
    let mut properties = Map::new();
    // Each dimension is judged met or not, with the evidence for that
    // answer; every field is required.
    let fields = json!({
        "met": { "type": "boolean" },
        "evidence": { "type": "array", "minItems": 1, "items": { "type": "string" } },
        "issues": { "type": "array", "items": { "type": "string" } }
    });
    let required: Vec<&String> = fields.as_object().into_iter().flat_map(Map::keys).collect();
    for name in &names {
        properties.insert(
            name.clone(),
            json!({
                "type": "object",
                "properties": fields,
                "required": required, "additionalProperties": false
            }),
        );
    }
    json!({ "type": "function", "function": {
        "name": "record_figure_evaluation", "description": "Record one evidence-grounded scientific figure evaluation.",
        "parameters": { "type": "object", "properties": {
            "summary": { "type": "string" },
            "dimensions": { "type": "object", "properties": properties, "required": names, "additionalProperties": false },
            "blockers": { "type": "array", "items": { "type": "object", "properties": {
                "code": { "type": "string" }, "artifact": { "type": "string", "enum": ["reference", "candidate", "comparison"] }, "evidence": { "type": "string" }
            }, "required": ["code", "artifact", "evidence"], "additionalProperties": false }},
            "fidelity_losses": { "type": "array", "items": { "type": "string" } },
            "recommendations": { "type": "array", "items": { "type": "object", "properties": {
                "priority": { "type": "string", "enum": ["critical", "high", "medium", "low"] }, "action": { "type": "string" }
            }, "required": ["priority", "action"], "additionalProperties": false }}
        }, "required": ["summary", "dimensions", "blockers", "fidelity_losses", "recommendations"], "additionalProperties": false }
    }})
}

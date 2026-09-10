use serde_json::json;
use crate::authoring::*;
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
    let trimmed = figure_process(
        "magick",
        &[
            png.to_string_lossy().into_owned(),
            "-fuzz".to_string(),
            "4%".to_string(),
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
    let aspect = ((width as f64 / height as f64) * 10_000.0).round() / 10_000.0;
    Ok(json!({
        "width": width, "height": height, "aspectRatio": aspect,
        "contentBounds": { "x": x, "y": y, "width": content_width, "height": content_height },
        "margins": {
            "left": x.max(0), "top": y.max(0),
            "right": (width - x - content_width).max(0), "bottom": (height - y - content_height).max(0)
        }
    }))
}

pub(crate) fn deterministic_figure(reference: &JsonValue, candidate: &JsonValue) -> JsonValue {
    let mut blockers = Vec::new();
    for (label, geometry) in [("reference", reference), ("candidate", candidate)] {
        let width = geometry["width"].as_i64().unwrap_or(0);
        let height = geometry["height"].as_i64().unwrap_or(0);
        if width < 600 || height < 300 {
            blockers.push(json!({ "code": format!("{label}_render_too_small"), "artifact": label, "evidence": format!("{width}x{height} is below 600x300") }));
        }
        let touching: Vec<&str> = ["left", "top", "right", "bottom"]
            .into_iter()
            .filter(|edge| geometry["margins"][*edge].as_i64().unwrap_or(0) <= 2)
            .collect();
        if !touching.is_empty() {
            blockers.push(json!({ "code": format!("{label}_content_at_canvas_edge"), "artifact": label, "evidence": format!("non-background content reaches: {}", touching.join(", ")) }));
        }
    }
    let left = reference["aspectRatio"].as_f64().unwrap_or(1.0);
    let right = candidate["aspectRatio"].as_f64().unwrap_or(1.0);
    let drift = ((right - left).abs() / left * 10_000.0).round() / 10_000.0;
    if drift >= 0.25 {
        blockers.push(json!({ "code": "candidate_aspect_ratio_drift", "artifact": "candidate", "evidence": format!("reference {left}, candidate {right}, drift {:.1}%", drift * 100.0) }));
    }
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
    for name in &names {
        properties.insert(
            name.clone(),
            json!({
                "type": "object",
                "properties": {
                    "score": { "type": "number", "minimum": 0, "maximum": 1 },
                    "evidence": { "type": "array", "minItems": 1, "items": { "type": "string" } },
                    "issues": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["score", "evidence", "issues"], "additionalProperties": false
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


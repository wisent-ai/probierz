use serde_json::json;
use crate::authoring::*;
pub(crate) fn draft_structured_artifact(
    harness: &Path,
    app_id: &str,
    target: Option<&str>,
    brief: &str,
    tool_name: &str,
    description: &str,
) -> Result<RouterReply, String> {
    if brief.trim().is_empty() {
        return Err("model-router brief is required".to_string());
    }
    if tool_name.is_empty()
        || tool_name.len() > 64
        || !tool_name.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase()
            } else {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
            }
        })
    {
        return Err("model-router tool name is invalid".to_string());
    }
    if description.trim().is_empty() {
        return Err("model-router artifact description is required".to_string());
    }
    let loaded = manifest::load(harness, app_id).ok();
    let url = stado_model_router_url(
        selected_setting(loaded.as_ref(), target, "STADO_MODEL_ROUTER_URL", None).as_deref(),
    )?;
    let token = required_setting(
        selected_setting(loaded.as_ref(), target, "STADO_MODEL_ROUTER_TOKEN", None),
        "STADO_MODEL_ROUTER_TOKEN",
    )?;
    let agent_id = required_setting(
        selected_setting(loaded.as_ref(), target, "PROBIERZ_MODEL_AGENT_ID", None),
        "PROBIERZ_MODEL_AGENT_ID",
    )?;
    let agent_secret = required_setting(
        selected_setting(loaded.as_ref(), target, "PROBIERZ_MODEL_AGENT_SECRET", None),
        "PROBIERZ_MODEL_AGENT_SECRET",
    )?;
    let model = selected_setting(loaded.as_ref(), target, "PROBIERZ_AUTHOR_MODEL", None)
        .unwrap_or_else(|| "any".to_string());
    let body = json!({
        "model": model, "max_tokens": 12000, "temperature": 0.1,
        "messages": [
            { "role": "system", "content": format!("You are a Probierz authoring worker. Produce the requested artifact, then call {tool_name} exactly once with the complete file contents. Do not modify files or return prose.") },
            { "role": "user", "content": brief }
        ],
        "tools": [{ "type": "function", "function": { "name": tool_name, "description": description, "parameters": {
            "type": "object", "properties": { "content": { "type": "string", "description": "Complete artifact contents, without Markdown fences." } }, "required": ["content"], "additionalProperties": false
        }}}]
    }).to_string();
    let (status, raw) = post_router(&url, &token, &agent_id, &agent_secret, &body, 3600)?;
    let payload: JsonValue = serde_json::from_str(&raw)
        .map_err(|_| format!("Stado model router returned non-JSON ({status})"))?;
    if !(200..300).contains(&status) {
        let detail = payload
            .pointer("/error/message")
            .and_then(JsonValue::as_str)
            .unwrap_or("request failed")
            .chars()
            .take(500)
            .collect::<String>();
        return Err(format!(
            "Stado model router request failed ({status}): {detail}"
        ));
    }
    let calls: Vec<&JsonValue> = payload
        .pointer("/choices/0/message/tool_calls")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter(|call| {
            call.get("type").and_then(JsonValue::as_str) == Some("function")
                && call.pointer("/function/name").and_then(JsonValue::as_str) == Some(tool_name)
        })
        .collect();
    if calls.len() != 1 {
        return Err(format!(
            "Stado model router response must contain exactly one {tool_name} tool call"
        ));
    }
    let args: JsonValue = serde_json::from_str(
        calls[0]
            .pointer("/function/arguments")
            .and_then(JsonValue::as_str)
            .unwrap_or_default(),
    )
    .map_err(|_| format!("Stado model router returned invalid {tool_name} arguments"))?;
    let content = args
        .get("content")
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("Stado model router returned an empty {tool_name} artifact"))?;
    Ok(RouterReply {
        content,
        model: payload
            .get("model")
            .cloned()
            .filter(JsonValue::is_string)
            .unwrap_or(JsonValue::Null),
        usage: payload
            .get("usage")
            .cloned()
            .filter(JsonValue::is_object)
            .unwrap_or(JsonValue::Null),
    })
}

pub(crate) fn probe(target: &str, base_url: Option<&str>, app_path: Option<&str>) -> Result<String, String> {
    if matches!(target, "web" | "electron") {
        let url = base_url.ok_or_else(|| format!("{target} needs --base-url"))?;
        let output = command_output(
            "curl",
            &["--silent", "--show-error", "--location", url],
            None,
        )
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let html = String::from_utf8_lossy(&output.stdout);
        let title = html
            .split("<title")
            .nth(1)
            .and_then(|value| value.split('>').nth(1))
            .and_then(|value| value.split("</title>").next())
            .unwrap_or_default();
        let body: String = html
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(BODY_CHARS)
            .collect();
        return Ok(format!(
            "kind: web\nurl: {url}\ntitle: {title}\nbody text: {body}\ninteractive/headings:"
        )
        .chars()
        .take(PROBE_CHARS)
        .collect());
    }
    let app = app_path.ok_or_else(|| format!("{target} needs --app-path"))?;
    let label = if target == "tui" {
        "initial screen (pty frame, ANSI stripped):"
    } else {
        "accessibility tree (truncated):"
    };
    Ok(format!("kind: {target}\napp: {app}\n{label}")
        .chars()
        .take(PROBE_CHARS)
        .collect())
}

/// Where a spec file for this target lives, from the one inventory
/// `discovery` keeps. `tui` and `desktop:cua` have no answer: their journeys
/// are functions in this crate, not files a spec author writes.
pub(crate) fn target_spec_dir(harness: &Path, target: &str) -> Option<PathBuf> {
    crate::discovery::spec_dir(target).map(|relative| harness.join(relative))
}

pub(crate) fn spec_extension(target: &str) -> &'static str {
    if matches!(target, "web" | "electron") {
        ".spec.ts"
    } else {
        ".e2e.ts"
    }
}

/// The refusal an authoring command owes a registry surface. Writing a file
/// for it would produce a spec nothing runs.
pub(crate) fn registry_surface_refusal(point: &str, target: &str) -> Failure {
    Failure::invalid(
        point,
        format!(
            "{target} journeys are Rust functions in probierz-rs/src/specs, not spec files: \
add one there and register it, then `probierz specs {target}` lists it"
        ),
    )
}


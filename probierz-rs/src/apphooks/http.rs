use crate::apphooks::*;
use serde_json::json;
pub(crate) fn response_json(response: ureq::Response) -> Result<Value, Failure> {
    let status = response.status();
    let text = response
        .into_string()
        .map_err(|error| Failure::unavailable("apphook.http", error.to_string()))?;
    let body = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| json!({ "message": text }))
    };
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        // A plain message where the service gives one; a structured error
        // (wisent-integrations answers `{"error": {"code": …}}`) whole, so its
        // code and the provider's reason reach the run report.
        let detail = body
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| body.get("error").and_then(Value::as_str))
            .or_else(|| body.pointer("/error/message").and_then(Value::as_str))
            .or_else(|| body.get("hint").and_then(Value::as_str))
            .map(str::to_string)
            .or_else(|| body.get("error").map(Value::to_string))
            .unwrap_or_else(|| "request failed".to_string());
        Err(Failure::new(
            "apphook.http",
            Code::Refused,
            format!("HTTP {status}: {detail}"),
        ))
    }
}

pub(crate) fn request_json(
    method: &str,
    endpoint: &str,
    headers: &[(&str, String)],
    body: Option<Value>,
) -> Result<Value, Failure> {
    let mut request = ureq::request(method, endpoint);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    };
    match response {
        Ok(response) => response_json(response),
        Err(ureq::Error::Status(_, response)) => response_json(response),
        Err(error) => Err(Failure::unavailable("apphook.http", error.to_string())),
    }
}

pub(crate) fn supabase_headers(
    source: &BTreeMap<String, String>,
    prefer: Option<&str>,
) -> Vec<(&'static str, String)> {
    let key = source
        .get("OKO_E2E_SUPABASE_SERVICE_ROLE_KEY")
        .cloned()
        .unwrap_or_default();
    let mut headers = vec![
        ("apikey", key.clone()),
        ("Authorization", format!("Bearer {key}")),
        ("Content-Type", "application/json".into()),
    ];
    if let Some(prefer) = prefer {
        headers.push(("Prefer", prefer.into()));
    }
    headers
}

pub(crate) fn supabase(
    source: &BTreeMap<String, String>,
    path: &str,
    method: &str,
    prefer: Option<&str>,
    body: Option<Value>,
) -> Result<Value, Failure> {
    let base = source
        .get("OKO_E2E_SUPABASE_URL")
        .map(|value| value.trim_end_matches('/'))
        .unwrap_or_default();
    request_json(
        method,
        &format!("{base}{path}"),
        &supabase_headers(source, prefer),
        body,
    )
}

/// The Slack identities the Oko journey seeds its thread as: the bot that
/// opens it and the user who answers in it. Their tokens are wisent-integrations'
/// provider items `slack-oko-e2e-bot` and `slack-oko-e2e-user`.
pub(crate) const SLACK_BOT: &str = "oko-e2e-bot";
pub(crate) const SLACK_USER: &str = "oko-e2e-user";

/// One `slack/<action>` call on wisent-integrations as `identity`, with the
/// run's integration origin and bearer. Slack's own refusal comes back as the
/// boundary's `slack_refused` with Slack's error word.
pub(crate) fn slack(
    source: &BTreeMap<String, String>,
    identity: &str,
    action: &str,
    mut payload: Value,
) -> Result<Value, Failure> {
    let base = source
        .get("STADO_INTEGRATION_API_URL")
        .map(|value| value.trim_end_matches('/'))
        .unwrap_or_default();
    let token = source
        .get("PROBIERZ_STADO_INTEGRATION_TOKEN")
        .cloned()
        .unwrap_or_default();
    payload["identity"] = json!(identity);
    let envelope = request_json(
        "POST",
        &format!("{base}/api/integration/slack/{action}"),
        &[
            ("Authorization", format!("Bearer {token}")),
            ("Content-Type", "application/json".into()),
        ],
        Some(payload),
    )
    .map_err(|failure| {
        Failure::new(
            "apphook.oko.slack",
            Code::Refused,
            format!("wisent-integrations slack/{action} as {identity}: {failure}"),
        )
    })?;
    Ok(envelope.get("result").cloned().unwrap_or(Value::Null))
}

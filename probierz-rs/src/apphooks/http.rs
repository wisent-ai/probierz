use crate::apphooks::*;
use serde_json::json;
pub(crate) fn response_json(response: ureq::Response) -> Result<Value, Failure> {
    let status = response.status();
    let text = response
        .into_string()
        .map_err(|error| Failure::unavailable("apphook.http", error.to_string()))?;
    // A success is read as JSON, an empty body as null; a body that is not
    // JSON is refused, never read as a message.
    let body = || -> Result<Value, Failure> {
        if text.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).map_err(|error| {
            Failure::unavailable(
                "apphook.http",
                format!("HTTP {status} answered a body that is not JSON ({error}): {text}"),
            )
        })
    };
    if (200..300).contains(&status) {
        body()
    } else {
        // The refusal as the service sent it: a structured error (wisent-
        // integrations answers `{"error": {"code": …}}`) keeps its code and
        // the provider's reason; an empty body is said as empty.
        let detail = match text.trim() {
            "" => "the response body is empty",
            said => said,
        };
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
    // The integration origin and bearer are the run's; without either the
    // call went to '/api/…' or carried 'Bearer ' and failed as a refusal.
    let required = |name: &str| {
        source
            .get(name)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                Failure::config(
                    "apphook.oko.slack",
                    format!(
                        "{name} is not set, so wisent-integrations slack/{action} cannot be called"
                    ),
                )
            })
    };
    let base = required("STADO_INTEGRATION_API_URL")?.trim_end_matches('/');
    let token = required("PROBIERZ_STADO_INTEGRATION_TOKEN")?;
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

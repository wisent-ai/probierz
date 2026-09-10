use serde_json::json;
use crate::apphooks::*;
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
        let detail = body
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| body.get("error").and_then(Value::as_str))
            .or_else(|| body.pointer("/error/message").and_then(Value::as_str))
            .or_else(|| body.get("hint").and_then(Value::as_str))
            .unwrap_or("request failed");
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

pub(crate) fn slack(
    token: &str,
    method: &str,
    payload: Value,
    accepted_errors: &[&str],
) -> Result<Value, Failure> {
    let body = request_json(
        "POST",
        &format!("https://slack.com/api/{method}"),
        &[
            ("Authorization", format!("Bearer {token}")),
            ("Content-Type", "application/json; charset=utf-8".into()),
        ],
        Some(payload),
    )?;
    if body.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("request failed");
        if !accepted_errors.contains(&error) {
            return Err(Failure::new(
                "apphook.oko.slack",
                Code::Refused,
                format!("Slack {method}: {error}"),
            ));
        }
    }
    Ok(body)
}

